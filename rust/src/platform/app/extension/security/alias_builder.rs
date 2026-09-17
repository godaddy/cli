//! Build module alias maps from an oxc Program (imports + requires).

use oxc_ast::ast::{
    BindingPattern, Expression, ImportDeclaration, ImportDeclarationSpecifier, Program,
    PropertyKey, VariableDeclarator,
};
use oxc_ast_visit::{Visit, walk};

use super::types::AliasMaps;
use super::util::first_string_arg;

pub fn build_alias_maps<'a>(program: &Program<'a>) -> AliasMaps {
    let mut maps = AliasMaps::default();
    let mut visitor = AliasVisitor { maps: &mut maps };
    visitor.visit_program(program);
    maps
}

struct AliasVisitor<'b> {
    maps: &'b mut AliasMaps,
}

impl<'a> Visit<'a> for AliasVisitor<'_> {
    fn visit_import_declaration(&mut self, it: &ImportDeclaration<'a>) {
        let module = it.source.value.as_str();
        if let Some(specifiers) = &it.specifiers {
            for spec in specifiers {
                match spec {
                    ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                        self.maps
                            .module_aliases
                            .entry(module.to_owned())
                            .or_default()
                            .insert(s.local.name.as_str().to_owned());
                    }
                    ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                        self.maps
                            .namespace_aliases
                            .insert(module.to_owned(), s.local.name.as_str().to_owned());
                    }
                    ImportDeclarationSpecifier::ImportSpecifier(s) => {
                        let imported = match &s.imported {
                            oxc_ast::ast::ModuleExportName::IdentifierName(id) => {
                                id.name.as_str().to_owned()
                            }
                            oxc_ast::ast::ModuleExportName::IdentifierReference(id) => {
                                id.name.as_str().to_owned()
                            }
                            oxc_ast::ast::ModuleExportName::StringLiteral(lit) => {
                                lit.value.as_str().to_owned()
                            }
                        };
                        let local = s.local.name.as_str().to_owned();
                        self.maps
                            .named_imports
                            .entry(module.to_owned())
                            .or_default()
                            .insert(imported, local);
                    }
                }
            }
        }
        walk::walk_import_declaration(self, it);
    }

    fn visit_variable_declarator(&mut self, it: &VariableDeclarator<'a>) {
        if let Some(init) = &it.init
            && let Some(module) = require_module_name(init)
        {
            match &it.id {
                BindingPattern::BindingIdentifier(id) => {
                    self.maps
                        .module_aliases
                        .entry(module.to_owned())
                        .or_default()
                        .insert(id.name.as_str().to_owned());
                }
                BindingPattern::ObjectPattern(obj) => {
                    for prop in &obj.properties {
                        let imported = match &prop.key {
                            PropertyKey::StaticIdentifier(id) => id.name.as_str().to_owned(),
                            PropertyKey::StringLiteral(lit) => lit.value.as_str().to_owned(),
                            _ => continue,
                        };
                        let BindingPattern::BindingIdentifier(local) = &prop.value else {
                            continue;
                        };
                        self.maps
                            .named_imports
                            .entry(module.to_owned())
                            .or_default()
                            .insert(imported, local.name.as_str().to_owned());
                    }
                }
                _ => {}
            }
        }
        walk::walk_variable_declarator(self, it);
    }

    fn visit_import_expression(&mut self, it: &oxc_ast::ast::ImportExpression<'a>) {
        if let Expression::StringLiteral(lit) = &it.source {
            self.maps
                .module_aliases
                .entry(lit.value.as_str().to_owned())
                .or_default()
                .insert("__dynamic__".to_owned());
        }
        walk::walk_import_expression(self, it);
    }
}

fn require_module_name<'a>(expr: &Expression<'a>) -> Option<&'a str> {
    let Expression::CallExpression(call) = expr else {
        return None;
    };
    let Expression::Identifier(id) = &call.callee else {
        return None;
    };
    if id.name.as_str() != "require" {
        return None;
    }
    first_string_arg(call)
}
