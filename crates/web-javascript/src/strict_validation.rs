//! Static restrictions are checked before any script expression can run.
use crate::{
    JsError, Value,
    parser::{ClassDefinition, Expr, ForBinding, Parameter, Pattern, Stmt},
};
fn restricted(name: &str) -> bool {
    matches!(
        name,
        "eval"
            | "arguments"
            | "implements"
            | "interface"
            | "package"
            | "private"
            | "protected"
            | "public"
            | "static"
            | "yield"
            | "let"
    )
}
fn invalid() -> JsError {
    JsError::syntax(0, "Invalid binding, assignment or deletion in strict mode")
}
pub(super) fn strict_directive(statements: &[Stmt]) -> bool {
    statements.iter().take_while(|statement|matches!(statement,Stmt::Expression(Expr::Literal(Value::String(_))))).any(|statement|matches!(statement,Stmt::Expression(Expr::Literal(Value::String(value))) if value.as_ref()=="use strict"))
}
pub(super) fn validate(statements: &[Stmt], inherited: bool) -> Result<(), JsError> {
    let strict = inherited || strict_directive(statements);
    visit_statements(statements, strict)
}
fn visit_statements(statements: &[Stmt], strict: bool) -> Result<(), JsError> {
    for statement in statements {
        visit_statement(statement, strict)?;
    }
    Ok(())
}
fn bind(pattern: &Pattern, strict: bool) -> Result<(), JsError> {
    match pattern {
        Pattern::Name(name) => {
            if strict && restricted(name) {
                return Err(invalid());
            }
        }
        Pattern::Array(items, rest) => {
            for item in items.iter().flatten() {
                bind(item, strict)?;
            }
            if let Some(rest) = rest {
                bind(rest, strict)?;
            }
        }
        Pattern::Object(items, rest) => {
            for (_, item) in items {
                bind(item, strict)?;
            }
            if let Some(rest) = rest {
                bind(rest, strict)?;
            }
        }
        Pattern::Default(item, value) => {
            bind(item, strict)?;
            visit_expr(value, strict)?;
        }
    }
    Ok(())
}
fn parameters(
    params: &[String],
    extra: Option<&[Parameter]>,
    body: &[Stmt],
    inherited: bool,
) -> Result<(), JsError> {
    let strict = inherited || strict_directive(body);
    if let Some(extra) = extra {
        if strict_directive(body)
            && extra
                .iter()
                .any(|parameter| parameter.rest || !matches!(parameter.pattern, Pattern::Name(_)))
        {
            return Err(JsError::syntax(
                0,
                "Strict directive with non-simple parameters",
            ));
        }
        for parameter in extra {
            bind(&parameter.pattern, strict)?;
        }
    }
    if strict {
        let mut names = std::collections::BTreeSet::new();
        for name in params {
            if restricted(name) || !names.insert(name) {
                return Err(invalid());
            }
        }
    }
    validate(body, strict)
}
fn class(class: &ClassDefinition, _strict: bool) -> Result<(), JsError> {
    if class.name.as_ref().is_some_and(|name| restricted(name)) {
        return Err(invalid());
    }
    if let Some(base) = &class.base {
        visit_expr(base, true)?;
    }
    for member in &class.members {
        if let Some(value) = &member.value {
            visit_expr(value, true)?;
        }
    }
    Ok(())
}
fn visit_statement(statement: &Stmt, strict: bool) -> Result<(), JsError> {
    match statement {
        Stmt::Expression(value) | Stmt::Throw(value) => visit_expr(value, strict)?,
        Stmt::Variables(_, values) => {
            for (name, value) in values {
                if strict && restricted(name) {
                    return Err(invalid());
                }
                if let Some(value) = value {
                    visit_expr(value, strict)?;
                }
            }
        }
        Stmt::PatternVariables(_, values) => {
            for (pattern, value) in values {
                bind(pattern, strict)?;
                if let Some(value) = value {
                    visit_expr(value, strict)?;
                }
            }
        }
        Stmt::FunctionDefinition(name, value) => {
            if strict && restricted(name) {
                return Err(invalid());
            }
            visit_expr(value, strict)?;
        }
        Stmt::ClassDefinition(name, value) => {
            if strict && restricted(name) {
                return Err(invalid());
            }
            class(value, strict)?;
        }
        Stmt::Block(body) => visit_statements(body, strict)?,
        Stmt::If(condition, yes, no) => {
            visit_expr(condition, strict)?;
            visit_statement(yes, strict)?;
            if let Some(no) = no {
                visit_statement(no, strict)?;
            }
        }
        Stmt::While(condition, body) | Stmt::DoWhile(body, condition) => {
            visit_expr(condition, strict)?;
            visit_statement(body, strict)?;
        }
        Stmt::For(initial, condition, update, body) => {
            if let Some(initial) = initial {
                visit_statement(initial, strict)?;
            }
            for value in [condition, update].into_iter().flatten() {
                visit_expr(value, strict)?;
            }
            visit_statement(body, strict)?;
        }
        Stmt::ForIn(binding, value, body)
        | Stmt::ForOf(binding, value, body)
        | Stmt::ForAwaitOf(binding, value, body) => {
            match binding {
                ForBinding::Declaration(_, name) => {
                    if strict && restricted(name) {
                        return Err(invalid());
                    }
                }
                ForBinding::Pattern(_, pattern) => bind(pattern, strict)?,
                ForBinding::Reference(value) => {
                    assignment_names(value, strict)?;
                    visit_expr(value, strict)?;
                }
            }
            visit_expr(value, strict)?;
            visit_statement(body, strict)?;
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            visit_statements(body, strict)?;
            if let Some((name, body)) = catch {
                if strict && name.as_ref().is_some_and(|name| restricted(name)) {
                    return Err(invalid());
                }
                visit_statements(body, strict)?;
            }
            if let Some(body) = finally {
                visit_statements(body, strict)?;
            }
        }
        Stmt::Switch(value, clauses) => {
            visit_expr(value, strict)?;
            for (value, body) in clauses {
                if let Some(value) = value {
                    visit_expr(value, strict)?;
                }
                visit_statements(body, strict)?;
            }
        }
        Stmt::Return(Some(value)) => visit_expr(value, strict)?,
        Stmt::Label(_, body) => visit_statement(body, strict)?,
        _ => {}
    }
    Ok(())
}
fn assignment_names(expr: &Expr, strict: bool) -> Result<(), JsError> {
    match expr {
        Expr::Name(name) if strict && matches!(name.as_str(), "eval" | "arguments") => {
            return Err(invalid());
        }
        Expr::Array(items) => {
            for item in items {
                assignment_names(item, strict)?;
            }
        }
        Expr::Object(items) => {
            for (_, item) in items {
                assignment_names(item, strict)?;
            }
        }
        Expr::Assign(_, target, _) | Expr::Spread(target) => assignment_names(target, strict)?,
        _ => {}
    }
    Ok(())
}
fn visit_expr(expr: &Expr, strict: bool) -> Result<(), JsError> {
    match expr {
        Expr::Function {
            kind,
            name,
            params,
            body,
            parameters: extra,
            ..
        } => {
            if (strict || strict_directive(body))
                && name.as_ref().is_some_and(|name| restricted(name))
            {
                return Err(invalid());
            }
            crate::resumable::validate_body(body, *kind)?;
            parameters(params, extra.as_deref().map(Vec::as_slice), body, strict)?;
        }
        Expr::Class(definition) => class(definition, strict)?,
        Expr::Array(values) | Expr::Template(values) => {
            for value in values {
                visit_expr(value, strict)?;
            }
        }
        Expr::Object(values) => {
            for (_, value) in values {
                visit_expr(value, strict)?;
            }
        }
        Expr::Member(left, right) | Expr::Binary(_, left, right) => {
            visit_expr(left, strict)?;
            visit_expr(right, strict)?;
        }
        Expr::Assign(_, left, right) => {
            assignment_names(left, strict)?;
            if strict
                && matches!(left.as_ref(),Expr::Name(name) if matches!(name.as_str(),"eval"|"arguments"))
            {
                return Err(invalid());
            }
            visit_expr(left, strict)?;
            visit_expr(right, strict)?;
        }
        Expr::Unary(op, value) => {
            if op == "delete" && matches!(value.as_ref(), Expr::PrivateMember(..)) {
                return Err(JsError::syntax(0, "Cannot delete private class fields"));
            }
            if strict && op == "delete" && matches!(value.as_ref(), Expr::Name(_)) {
                return Err(invalid());
            }
            visit_expr(value, strict)?;
        }
        Expr::Update(value, _, _) => {
            if strict
                && matches!(value.as_ref(),Expr::Name(name) if matches!(name.as_str(),"eval"|"arguments"))
            {
                return Err(invalid());
            }
            visit_expr(value, strict)?;
        }
        Expr::Await(value)
        | Expr::YieldDelegate(value)
        | Expr::Yield(Some(value))
        | Expr::Spread(value)
        | Expr::PrivateMember(value, _) => visit_expr(value, strict)?,
        Expr::ImportCall { specifier, options } => {
            visit_expr(specifier, strict)?;
            if let Some(options) = options {
                visit_expr(options, strict)?;
            }
        }
        Expr::ImportMeta => {}
        Expr::Call(target, args) | Expr::Construct(target, args) => {
            visit_expr(target, strict)?;
            for value in args {
                visit_expr(value, strict)?;
            }
        }
        Expr::Conditional(condition, yes, no) => {
            visit_expr(condition, strict)?;
            visit_expr(yes, strict)?;
            visit_expr(no, strict)?;
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod for_await_strict_tests {
    use super::*;

    #[test]
    fn visits_for_await_bindings_sources_and_bodies() {
        for source in [
            "async function f() { 'use strict'; for await (eval of source) {} }",
            "async function f() { 'use strict'; for await (var arguments of source) {} }",
            "async function f() { 'use strict'; for await (const {x: eval} of source) {} }",
            "async function f() { 'use strict'; for await (const x of (eval = source)) {} }",
            "async function f() { 'use strict'; for await (const x of source) { arguments = x; } }",
        ] {
            let statements = crate::parser::parse(source).unwrap();
            assert!(validate(&statements, false).is_err(), "accepted {source}");
        }
    }
}

#[cfg(test)]
mod async_generator_strict_tests {
    use super::*;

    #[test]
    fn visits_async_generator_parameters_await_yield_and_for_await() {
        for source in [
            "async function* f(eval) { 'use strict'; yield 1; }",
            "async function* f() { 'use strict'; yield (eval = 1); }",
            "async function* f() { 'use strict'; await (arguments = 1); }",
            "async function* f() { 'use strict'; for await (eval of source) { yield 1; } }",
            "class C { async *f() { yield (eval = 1); } }",
            "let o = { async *f() { 'use strict'; await (arguments = 1); } };",
        ] {
            let statements = crate::parser::parse(source).unwrap();
            assert!(validate(&statements, false).is_err(), "accepted {source}");
        }
        let statements = crate::parser::parse(
            "async function* f() { 'use strict'; let x = await work(); yield x; }",
        )
        .unwrap();
        assert!(validate(&statements, false).is_ok());
    }
}
