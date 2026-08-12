//! AST scan engine: parse with oxc, build aliases, run SEC001–SEC010 (+ SEC012 via dom_escape).

use oxc_ast::ast::{
    CallExpression, Expression, ImportDeclaration, NewExpression, StaticMemberExpression,
    StringLiteral, TemplateLiteral,
};
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::SourceType;

use crate::extension::{Finding, Severity};

use super::alias_builder::build_alias_maps;
use super::config::{is_trusted_domain, security_config};
use super::dom_escape::scan_dom_escape;
use super::types::{AliasMaps, SecurityConfig};
use super::util::{first_string_arg, matches_http_url, offset_to_line_col, snippet_at};

const CP_METHODS: &[&str] = &[
    "exec",
    "spawn",
    "fork",
    "execFile",
    "execSync",
    "spawnSync",
    "execFileSync",
];

const VM_METHODS: &[&str] = &[
    "runInNewContext",
    "runInContext",
    "runInThisContext",
    "createContext",
];

const NATIVE_LIBS: &[&str] = &["node-gyp-build", "ffi-napi", "ref-napi", "bindings"];

const MODULE_PATCH_PROPS: &[&str] = &["_load", "_extensions", "_compile", "_resolveFilename"];

const SENSITIVE_PATHS: &[&str] = &["~/.ssh", "/etc/passwd", "/etc/shadow", "/var/run/secrets"];

pub fn scan_source_file(path: &str, source: &str) -> Vec<Finding> {
    let allocator = oxc_allocator::Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::tsx()).parse();
    let aliases = build_alias_maps(&parsed.program);
    let config = security_config();

    let mut visitor = RuleVisitor {
        source,
        file: path,
        aliases: &aliases,
        config: &config,
        findings: Vec::new(),
    };
    visitor.visit_program(&parsed.program);

    let mut findings = visitor.findings;
    findings.extend(scan_dom_escape(path, source, &parsed.program));
    findings
}

struct RuleVisitor<'a> {
    source: &'a str,
    file: &'a str,
    aliases: &'a AliasMaps,
    config: &'a SecurityConfig,
    findings: Vec<Finding>,
}

impl RuleVisitor<'_> {
    fn report(
        &mut self,
        rule_id: &'static str,
        severity: Severity,
        message: String,
        span: oxc_span::Span,
    ) {
        let (line, col) = offset_to_line_col(self.source, span.start as usize);
        self.findings.push(Finding {
            rule_id,
            severity,
            message,
            file: self.file.to_owned(),
            line,
            col,
            snippet: snippet_at(self.source, span),
        });
    }

    fn is_aliased_method(
        &self,
        method: &str,
        callee_local: &str,
        module: &str,
        methods: &[&str],
    ) -> bool {
        methods.contains(&method) && self.aliases.is_alias_of(callee_local, module)
    }

    fn check_url_string(&mut self, text: &str, span: oxc_span::Span) {
        if matches_http_url(text) && !is_trusted_domain(text, self.config) {
            self.report(
                "SEC008",
                Severity::Warn,
                format!(
                    "Warning: External URL '{text}' detected. Review if this is necessary or use GoDaddy APIs instead."
                ),
                span,
            );
        }
    }

    fn check_sensitive_path(&mut self, text: &str, span: oxc_span::Span) {
        if SENSITIVE_PATHS.iter().any(|p| text.contains(p)) {
            self.report(
                "SEC010",
                Severity::Warn,
                format!("Sensitive path literal detected: {text}"),
                span,
            );
        }
    }
}

impl<'a> Visit<'a> for RuleVisitor<'_> {
    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if let Expression::Identifier(id) = &it.callee {
            let name = id.name.as_str();
            if name == "eval" {
                self.report(
                    "SEC001",
                    Severity::Block,
                    "Blocked: eval() allows arbitrary code execution. Use JSON.parse() for data or refactor code.".to_owned(),
                    it.span,
                );
            }
            if self.is_aliased_method(name, name, "child_process", CP_METHODS) {
                self.report(
                    "SEC002",
                    Severity::Block,
                    format!(
                        "Blocked: child_process.{name}() can execute arbitrary system commands. Use platform APIs instead."
                    ),
                    it.span,
                );
            }
            if self.is_aliased_method(name, name, "vm", VM_METHODS) {
                self.report(
                    "SEC003",
                    Severity::Block,
                    format!(
                        "Blocked: vm.{name}() enables arbitrary code execution. Contact platform team if you need sandboxing."
                    ),
                    it.span,
                );
            }
        }

        if let Some((obj, method)) = static_member_call(it) {
            if self.is_aliased_method(method, obj, "child_process", CP_METHODS) {
                self.report(
                    "SEC002",
                    Severity::Block,
                    format!(
                        "Blocked: child_process.{method}() can execute arbitrary system commands. Use platform APIs instead."
                    ),
                    it.span,
                );
            }
            if self.is_aliased_method(method, obj, "vm", VM_METHODS) {
                self.report(
                    "SEC003",
                    Severity::Block,
                    format!(
                        "Blocked: vm.{method}() enables arbitrary code execution. Contact platform team if you need sandboxing."
                    ),
                    it.span,
                );
            }
        }

        if is_require_call(it)
            && let Some(module) = first_string_arg(it)
        {
            if module.ends_with(".node") || NATIVE_LIBS.contains(&module) {
                self.report(
                    "SEC005",
                    Severity::Block,
                    format!(
                        "Blocked: require('{module}') loads a native binding. Extensions must use pure JavaScript/TypeScript."
                    ),
                    it.span,
                );
            }
            if module == "inspector" || module == "node:inspector" {
                self.report(
                    "SEC007",
                    Severity::Block,
                    "Blocked: require('inspector') provides programmatic debugging. Use standard debugging tools instead.".to_owned(),
                    it.span,
                );
            }
        }

        if is_buffer_from(it)
            && let Some((data, encoding)) = buffer_from_args(it)
            && matches!(encoding, "base64" | "hex")
            && data.len() > 200
        {
            self.report(
                "SEC009",
                Severity::Warn,
                format!(
                    "Large {encoding} blob ({}) chars in Buffer.from() may hide payloads. Load binary from files when possible.",
                    data.len()
                ),
                it.span,
            );
        }

        walk::walk_call_expression(self, it);
    }

    fn visit_new_expression(&mut self, it: &NewExpression<'a>) {
        if let Expression::Identifier(id) = &it.callee
            && id.name.as_str() == "Function"
        {
            self.report(
                "SEC001",
                Severity::Block,
                "Blocked: new Function() allows arbitrary code execution. Use regular function declarations instead.".to_owned(),
                it.span,
            );
        }
        if let Some((obj, "Script")) = static_member_callee_new(it)
            && self.aliases.is_alias_of(obj, "vm")
        {
            self.report(
                "SEC003",
                Severity::Block,
                "Blocked: new vm.Script() enables arbitrary code execution.".to_owned(),
                it.span,
            );
        }
        walk::walk_new_expression(self, it);
    }

    fn visit_static_member_expression(&mut self, it: &StaticMemberExpression<'a>) {
        if let Expression::Identifier(obj) = &it.object {
            let obj_name = obj.name.as_str();
            let prop = it.property.name.as_str();
            if obj_name == "process" && (prop == "binding" || prop == "dlopen") {
                self.report(
                    "SEC004",
                    Severity::Block,
                    format!("Blocked: process.{prop}() accesses low-level process internals."),
                    it.span,
                );
            }
            if obj_name == "Module" && MODULE_PATCH_PROPS.contains(&prop) {
                self.report(
                    "SEC006",
                    Severity::Block,
                    format!("Blocked: Module.{prop} patches module loading."),
                    it.span,
                );
            }
            if obj_name == "require" && prop == "extensions" {
                self.report(
                    "SEC006",
                    Severity::Block,
                    "Blocked: require.extensions patches module loading.".to_owned(),
                    it.span,
                );
            }
        }
        walk::walk_static_member_expression(self, it);
    }

    fn visit_import_declaration(&mut self, it: &ImportDeclaration<'a>) {
        let module = it.source.value.as_str();
        if module == "inspector" || module == "node:inspector" {
            self.report(
                "SEC007",
                Severity::Block,
                "Blocked: import of 'inspector' provides programmatic debugging.".to_owned(),
                it.span,
            );
        }
        if NATIVE_LIBS.contains(&module) || module.ends_with(".node") {
            self.report(
                "SEC005",
                Severity::Block,
                format!(
                    "Blocked: import of '{module}' loads a native binding. Extensions must use pure JavaScript/TypeScript."
                ),
                it.span,
            );
        }
        walk::walk_import_declaration(self, it);
    }

    fn visit_string_literal(&mut self, it: &StringLiteral<'a>) {
        let text = it.value.as_str();
        self.check_url_string(text, it.span);
        self.check_sensitive_path(text, it.span);
        walk::walk_string_literal(self, it);
    }

    fn visit_template_literal(&mut self, it: &TemplateLiteral<'a>) {
        // Parity with TS: no-sub templates + template heads (not interpolated spans).
        if it.expressions.is_empty() {
            if let Some(quasi) = it.quasis.first()
                && let Some(cooked) = &quasi.value.cooked
            {
                let text = cooked.as_str();
                self.check_url_string(text, it.span);
                self.check_sensitive_path(text, it.span);
            }
        } else if let Some(head) = it.quasis.first()
            && let Some(cooked) = &head.value.cooked
        {
            self.check_url_string(cooked.as_str(), head.span);
        }
        walk::walk_template_literal(self, it);
    }
}

fn static_member_parts<'a>(expr: &Expression<'a>) -> Option<(&'a str, &'a str)> {
    let Expression::StaticMemberExpression(mem) = expr else {
        return None;
    };
    let Expression::Identifier(obj) = &mem.object else {
        return None;
    };
    Some((obj.name.as_str(), mem.property.name.as_str()))
}

fn static_member_call<'a>(call: &CallExpression<'a>) -> Option<(&'a str, &'a str)> {
    static_member_parts(&call.callee)
}

fn static_member_callee_new<'a>(expr: &NewExpression<'a>) -> Option<(&'a str, &'a str)> {
    static_member_parts(&expr.callee)
}

fn is_require_call(call: &CallExpression<'_>) -> bool {
    matches!(&call.callee, Expression::Identifier(id) if id.name.as_str() == "require")
}

fn is_buffer_from(call: &CallExpression<'_>) -> bool {
    match &call.callee {
        Expression::StaticMemberExpression(mem) => {
            matches!(&mem.object, Expression::Identifier(id) if id.name.as_str() == "Buffer")
                && mem.property.name.as_str() == "from"
        }
        _ => false,
    }
}

fn buffer_from_args<'a>(call: &CallExpression<'a>) -> Option<(&'a str, &'a str)> {
    let data = first_string_arg(call)?;
    let enc_expr = call.arguments.get(1)?.as_expression()?;
    let Expression::StringLiteral(enc) = enc_expr else {
        return None;
    };
    Some((data, enc.value.as_str()))
}
