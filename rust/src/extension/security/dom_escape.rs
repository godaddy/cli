//! SEC012 — DOM escape in UI extension source (ported from TS SEC012-dom-escape.ts).
//!
//! Blocks page-level DOM, storage, and navigation APIs outside the host container.
//! Lexical shadowing uses oxc scope enter/leave hooks plus a binding stack. The host
//! `container` binding matches TS: it is not treated as a free-variable shadow.

use std::cell::Cell;
use std::collections::HashMap;

use oxc_ast::ast::{
    BindingPattern, CallExpression, Class, ClassType, ComputedMemberExpression, Expression,
    FormalParameter, Function, FunctionType, ImportDeclaration, ImportDeclarationSpecifier,
    PropertyKey, StaticMemberExpression, VariableDeclarator,
};
use oxc_ast_visit::{Visit, walk};
use oxc_span::Span;
use oxc_syntax::scope::{ScopeFlags, ScopeId};

use crate::extension::{Finding, Severity};

use super::util::{offset_to_line_col, snippet_at};

/// `(object, property)` pairs blocked as member access.
const BLOCKED_GLOBAL_PROPERTIES: &[(&str, &str)] = &[
    ("document", "body"),
    ("document", "documentElement"),
    ("document", "head"),
    ("document", "forms"),
    ("document", "images"),
    ("document", "links"),
    ("document", "scripts"),
    ("document", "cookie"),
    ("document", "activeElement"),
    ("document", "children"),
    ("document", "firstElementChild"),
    ("window", "document"),
    ("window", "location"),
    ("globalThis", "document"),
    ("globalThis", "location"),
    ("location", "href"),
    ("location", "assign"),
    ("location", "replace"),
    ("history", "pushState"),
    ("history", "replaceState"),
    ("top", "document"),
    ("top", "location"),
    ("parent", "document"),
    ("parent", "location"),
    ("Element", "prototype"),
    ("Node", "prototype"),
    ("container", "ownerDocument"),
    ("container", "parentElement"),
    ("container", "parentNode"),
    ("container", "closest"),
];

/// `(object, method)` pairs blocked as calls.
const BLOCKED_GLOBAL_CALLS: &[(&str, &str)] = &[
    ("document", "write"),
    ("document", "querySelector"),
    ("document", "querySelectorAll"),
    ("document", "getElementById"),
    ("document", "getElementsByClassName"),
    ("document", "getElementsByName"),
    ("document", "getElementsByTagName"),
    ("document", "getElementsByTagNameNS"),
    ("document", "createElement"),
    ("document", "createRange"),
    ("document", "evaluate"),
    ("window", "open"),
    ("location", "assign"),
    ("location", "replace"),
    ("history", "pushState"),
    ("history", "replaceState"),
    ("container", "closest"),
];

/// `(object, mid, method)` nested calls, e.g. `window.document.querySelector`.
const BLOCKED_NESTED_CALLS: &[(&str, &str, &str)] = &[
    ("window", "document", "querySelector"),
    ("window", "document", "querySelectorAll"),
    ("window", "document", "getElementById"),
    ("window", "document", "getElementsByClassName"),
    ("window", "document", "getElementsByName"),
    ("window", "document", "getElementsByTagName"),
    ("window", "document", "getElementsByTagNameNS"),
    ("window", "document", "write"),
    ("window", "location", "assign"),
    ("window", "location", "replace"),
    ("globalThis", "document", "querySelector"),
    ("globalThis", "document", "querySelectorAll"),
    ("globalThis", "document", "getElementById"),
    ("globalThis", "document", "write"),
    ("globalThis", "location", "assign"),
    ("globalThis", "location", "replace"),
    ("top", "document", "querySelector"),
    ("top", "document", "querySelectorAll"),
    ("top", "document", "getElementById"),
    ("parent", "document", "querySelector"),
    ("parent", "document", "querySelectorAll"),
    ("parent", "document", "getElementById"),
];

const STORAGE_ROOTS: &[&str] = &["localStorage", "sessionStorage"];
const BLOCKED_GLOBAL_FUNCTIONS: &[&str] = &["open"];
const DOCUMENT_OWNER_ROOTS: &[&str] = &["window", "globalThis", "top", "parent"];

const ALIASABLE_GLOBAL_ROOTS: &[&str] = &[
    "window",
    "globalThis",
    "document",
    "location",
    "history",
    "top",
    "parent",
    "Element",
    "Node",
    "container",
    "localStorage",
    "sessionStorage",
    "open",
];

const MSG_PROP: &str = "Blocked: UI extensions must render only inside the host-provided container and must not access page-level DOM, storage, or navigation APIs.";
const MSG_CALL: &str = "Blocked: UI extensions must render only inside the host-provided container and must not query, write, navigate, or escape checkout page DOM directly.";
const MSG_STORAGE: &str = "Blocked: UI extensions must not access page-global browser storage.";
const MSG_DESTRUCTURE: &str = "Blocked: UI extensions must not destructure page-level DOM, storage, or navigation APIs outside the host-provided container.";

#[derive(Debug, Clone)]
enum Binding {
    /// Local binding that hides the page global / outer alias.
    Local,
    /// `const doc = document` → references resolve to the page root.
    Alias(String),
}

pub fn scan_dom_escape(
    path: &str,
    source: &str,
    program: &oxc_ast::ast::Program<'_>,
) -> Vec<Finding> {
    let mut visitor = DomEscapeVisitor {
        source,
        file: path,
        scopes: Vec::new(),
        suppress_storage_ident: false,
        findings: Vec::new(),
    };
    visitor.visit_program(program);
    visitor.findings
}

struct DomEscapeVisitor<'a> {
    source: &'a str,
    file: &'a str,
    /// Innermost scope last. Populated via oxc `enter_scope` / `leave_scope`.
    scopes: Vec<HashMap<String, Binding>>,
    /// When walking a member-expression object, skip bare-storage checks.
    suppress_storage_ident: bool,
    findings: Vec<Finding>,
}

impl DomEscapeVisitor<'_> {
    fn report(&mut self, message: &str, span: Span) {
        let (line, col) = offset_to_line_col(self.source, span.start as usize);
        self.findings.push(Finding {
            rule_id: "SEC012",
            severity: Severity::Block,
            message: message.to_owned(),
            file: self.file.to_owned(),
            line,
            col,
            snippet: snippet_at(self.source, span),
        });
    }

    fn declare(&mut self, name: &str, binding: Binding) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_owned(), binding);
        }
    }

    fn declare_local(&mut self, name: &str) {
        self.declare(name, Binding::Local);
    }

    fn declare_alias(&mut self, name: &str, root: String) {
        self.declare(name, Binding::Alias(root));
    }

    fn declare_pattern_local(&mut self, pat: &BindingPattern<'_>) {
        match pat {
            BindingPattern::BindingIdentifier(id) => self.declare_local(id.name.as_str()),
            BindingPattern::ObjectPattern(obj) => {
                for prop in &obj.properties {
                    self.declare_pattern_local(&prop.value);
                }
                if let Some(rest) = &obj.rest {
                    self.declare_pattern_local(&rest.argument);
                }
            }
            BindingPattern::ArrayPattern(arr) => {
                for pat in arr.elements.iter().flatten() {
                    self.declare_pattern_local(pat);
                }
                if let Some(rest) = &arr.rest {
                    self.declare_pattern_local(&rest.argument);
                }
            }
            BindingPattern::AssignmentPattern(ap) => self.declare_pattern_local(&ap.left),
        }
    }

    fn walk_pattern_defaults(&mut self, pat: &BindingPattern<'_>) {
        match pat {
            BindingPattern::AssignmentPattern(ap) => {
                self.visit_expression(&ap.right);
                self.walk_pattern_defaults(&ap.left);
            }
            BindingPattern::ObjectPattern(obj) => {
                for prop in &obj.properties {
                    self.walk_pattern_defaults(&prop.value);
                }
                if let Some(rest) = &obj.rest {
                    self.walk_pattern_defaults(&rest.argument);
                }
            }
            BindingPattern::ArrayPattern(arr) => {
                for pat in arr.elements.iter().flatten() {
                    self.walk_pattern_defaults(pat);
                }
                if let Some(rest) = &arr.rest {
                    self.walk_pattern_defaults(&rest.argument);
                }
            }
            BindingPattern::BindingIdentifier(_) => {}
        }
    }

    fn resolve_identifier_root<'b>(&'b self, name: &'b str) -> Option<&'b str> {
        // Match TS SEC012: aliases win unless shadowed by a later local; the host
        // `container` binding is never treated as a free-variable shadow (params
        // like `mount({ container })` must still hit container.* blocklists).
        let mut saw_local = false;
        for scope in self.scopes.iter().rev() {
            match scope.get(name) {
                Some(Binding::Local) => saw_local = true,
                Some(Binding::Alias(root)) => {
                    return if saw_local { None } else { Some(root.as_str()) };
                }
                None => {}
            }
        }
        if saw_local {
            return (name == "container").then_some("container");
        }
        if ALIASABLE_GLOBAL_ROOTS.contains(&name) {
            Some(name)
        } else {
            None
        }
    }

    fn resolve_expression_root<'b>(&'b self, expr: &'b Expression<'_>) -> Option<&'b str> {
        match expr {
            Expression::Identifier(id) => self.resolve_identifier_root(id.name.as_str()),
            Expression::StaticMemberExpression(mem) => {
                let owner = self.resolve_expression_root(&mem.object)?;
                reroot_document_owner(owner, mem.property.name.as_str())
            }
            Expression::ComputedMemberExpression(mem) => {
                let member = static_string_key(&mem.expression)?;
                let owner = self.resolve_expression_root(&mem.object)?;
                reroot_document_owner(owner, member)
            }
            _ => None,
        }
    }

    fn is_member_access(&self, expr: &Expression<'_>, object: &str, prop: &str) -> bool {
        match expr {
            Expression::StaticMemberExpression(mem) => {
                mem.property.name.as_str() == prop
                    && self.resolve_expression_root(&mem.object) == Some(object)
            }
            Expression::ComputedMemberExpression(mem) => {
                static_string_key(&mem.expression) == Some(prop)
                    && self.resolve_expression_root(&mem.object) == Some(object)
            }
            _ => false,
        }
    }

    fn is_nested_member_access(
        &self,
        expr: &Expression<'_>,
        object: &str,
        first: &str,
        second: &str,
    ) -> bool {
        let (inner, second_name) = match expr {
            Expression::StaticMemberExpression(mem) => (&mem.object, mem.property.name.as_str()),
            Expression::ComputedMemberExpression(mem) => {
                let Some(name) = static_string_key(&mem.expression) else {
                    return false;
                };
                (&mem.object, name)
            }
            _ => return false,
        };
        second_name == second && self.is_member_access(inner, object, first)
    }

    fn is_blocked_call(&self, call: &CallExpression<'_>) -> bool {
        if let Expression::Identifier(id) = &call.callee {
            let root = self.resolve_identifier_root(id.name.as_str());
            if root.is_some_and(|r| BLOCKED_GLOBAL_FUNCTIONS.contains(&r)) {
                return true;
            }
        }

        BLOCKED_GLOBAL_CALLS
            .iter()
            .any(|(obj, method)| self.is_member_access(&call.callee, obj, method))
            || BLOCKED_NESTED_CALLS.iter().any(|(obj, mid, method)| {
                self.is_nested_member_access(&call.callee, obj, mid, method)
            })
    }

    fn is_blocked_destructure_prop(root: &str, prop: &str) -> bool {
        BLOCKED_GLOBAL_PROPERTIES
            .iter()
            .any(|(o, p)| *o == root && *p == prop)
            || BLOCKED_GLOBAL_CALLS
                .iter()
                .any(|(o, p)| *o == root && *p == prop)
    }

    fn walk_member_object(&mut self, object: &Expression<'_>) {
        self.suppress_storage_ident = true;
        self.visit_expression(object);
        self.suppress_storage_ident = false;
    }

    fn is_blocked_property_access(&self, object: &Expression<'_>, prop: &str) -> bool {
        if let Some(root) = self.resolve_expression_root(object)
            && STORAGE_ROOTS.contains(&root)
        {
            return true;
        }
        BLOCKED_GLOBAL_PROPERTIES
            .iter()
            .any(|(obj, p)| *p == prop && self.resolve_expression_root(object) == Some(*obj))
    }

    fn is_blocked_property_access_static(&self, mem: &StaticMemberExpression<'_>) -> bool {
        self.is_blocked_property_access(&mem.object, mem.property.name.as_str())
    }

    fn is_blocked_property_access_computed(&self, mem: &ComputedMemberExpression<'_>) -> bool {
        static_string_key(&mem.expression)
            .is_some_and(|prop| self.is_blocked_property_access(&mem.object, prop))
    }
}

impl<'a> Visit<'a> for DomEscapeVisitor<'_> {
    fn enter_scope(&mut self, _flags: ScopeFlags, _scope_id: &Cell<Option<ScopeId>>) {
        self.scopes.push(HashMap::new());
    }

    fn leave_scope(&mut self) {
        self.scopes.pop();
    }

    fn visit_function(&mut self, it: &Function<'a>, flags: ScopeFlags) {
        // Function declarations hoist their name into the enclosing scope.
        if it.r#type == FunctionType::FunctionDeclaration
            && let Some(id) = &it.id
        {
            self.declare_local(id.name.as_str());
        }
        walk::walk_function(self, it, flags);
    }

    fn visit_binding_identifier(&mut self, it: &oxc_ast::ast::BindingIdentifier<'a>) {
        // Function/class ids and any pattern walk that reaches here.
        // VariableDeclarators / params / catch use explicit declare_* instead
        // and do not walk binding identifiers before their initializers.
        self.declare_local(it.name.as_str());
        walk::walk_binding_identifier(self, it);
    }

    fn visit_class(&mut self, it: &Class<'a>) {
        if it.r#type == ClassType::ClassDeclaration
            && let Some(id) = &it.id
        {
            self.declare_local(id.name.as_str());
        }
        walk::walk_class(self, it);
    }

    fn visit_import_declaration(&mut self, it: &ImportDeclaration<'a>) {
        if let Some(specifiers) = &it.specifiers {
            for spec in specifiers {
                let local = match spec {
                    ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => s.local.name.as_str(),
                    ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                        s.local.name.as_str()
                    }
                    ImportDeclarationSpecifier::ImportSpecifier(s) => s.local.name.as_str(),
                };
                self.declare_local(local);
            }
        }
        walk::walk_import_declaration(self, it);
    }

    fn visit_formal_parameter(&mut self, it: &FormalParameter<'a>) {
        // Visit default value before the param binding shadows outer names.
        if let Some(init) = &it.initializer {
            self.visit_expression(init);
        }
        self.declare_pattern_local(&it.pattern);
    }

    fn visit_catch_parameter(&mut self, it: &oxc_ast::ast::CatchParameter<'a>) {
        self.declare_pattern_local(&it.pattern);
    }

    fn visit_variable_declarator(&mut self, it: &VariableDeclarator<'a>) {
        // Resolve / scan initializer before declaring bindings (TDZ / shadowing).
        let alias_root = it.init.as_ref().and_then(|init| {
            let root = self.resolve_expression_root(init).map(str::to_owned);

            if let (Some(root_name), BindingPattern::ObjectPattern(obj)) = (&root, &it.id) {
                for prop in &obj.properties {
                    if let Some(prop_name) = binding_property_name(prop)
                        && Self::is_blocked_destructure_prop(root_name, prop_name)
                    {
                        self.report(MSG_DESTRUCTURE, it.span);
                        break;
                    }
                }
            }

            self.visit_expression(init);
            root
        });

        self.walk_pattern_defaults(&it.id);

        if let (Some(root_name), BindingPattern::BindingIdentifier(id)) = (&alias_root, &it.id)
            && ALIASABLE_GLOBAL_ROOTS.contains(&root_name.as_str())
        {
            self.declare_alias(id.name.as_str(), root_name.clone());
        } else {
            self.declare_pattern_local(&it.id);
        }
    }

    fn visit_identifier_reference(&mut self, it: &oxc_ast::ast::IdentifierReference<'a>) {
        if !self.suppress_storage_ident {
            let root = self.resolve_identifier_root(it.name.as_str());
            if root.is_some_and(|r| STORAGE_ROOTS.contains(&r)) {
                self.report(MSG_STORAGE, it.span);
            }
        }
        walk::walk_identifier_reference(self, it);
    }

    fn visit_static_member_expression(&mut self, it: &StaticMemberExpression<'a>) {
        if self.is_blocked_property_access_static(it) {
            self.report(MSG_PROP, it.span);
        }
        self.walk_member_object(&it.object);
        walk::walk_identifier_name(self, &it.property);
    }

    fn visit_computed_member_expression(&mut self, it: &ComputedMemberExpression<'a>) {
        if self.is_blocked_property_access_computed(it) {
            self.report(MSG_PROP, it.span);
        }
        self.walk_member_object(&it.object);
        self.visit_expression(&it.expression);
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if self.is_blocked_call(it) {
            self.report(MSG_CALL, it.span);
        }
        walk::walk_call_expression(self, it);
    }
}

fn reroot_document_owner(owner: &str, member: &str) -> Option<&'static str> {
    if !DOCUMENT_OWNER_ROOTS.contains(&owner) {
        return None;
    }
    match member {
        "document" => Some("document"),
        "location" => Some("location"),
        _ => None,
    }
}

fn static_string_key<'a>(expr: &'a Expression<'_>) -> Option<&'a str> {
    match expr {
        Expression::StringLiteral(lit) => Some(lit.value.as_str()),
        Expression::TemplateLiteral(t) if t.expressions.is_empty() => t
            .quasis
            .first()
            .and_then(|q| q.value.cooked.as_ref().map(|s| s.as_str())),
        _ => None,
    }
}

fn binding_property_name<'a>(prop: &'a oxc_ast::ast::BindingProperty<'_>) -> Option<&'a str> {
    if prop.shorthand
        && let BindingPattern::BindingIdentifier(id) = &prop.value
    {
        return Some(id.name.as_str());
    }
    match &prop.key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.as_str()),
        PropertyKey::StringLiteral(lit) => Some(lit.value.as_str()),
        _ => None,
    }
}
