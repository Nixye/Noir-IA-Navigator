//! Static restrictions are checked before any script expression can run.
use crate::{
    JsError, Value,
    parser::{ClassDefinition, Expr, ForBinding, Pattern, Stmt},
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
// Work items borrow the parsed tree, so nested declarations and defaults do not
// retain one native frame per AST node. Push children in reverse source order:
// the first reported syntax error must remain the same as the recursive walk.
enum Visit<'a> {
    Statement(&'a Stmt, bool),
    Expression(&'a Expr, bool),
    ConditionalClauses(&'a [(Expr, Expr)], bool),
    Pattern(&'a Pattern, bool),
    Class(&'a ClassDefinition),
    BindingName(&'a str, bool),
    AssignmentNames(&'a Expr, bool),
    ParameterNames(&'a [String]),
}

pub(super) fn validate(statements: &[Stmt], inherited: bool) -> Result<(), JsError> {
    let strict = inherited || strict_directive(statements);
    let mut pending = Vec::new();
    push_statements(&mut pending, statements, strict);
    while let Some(visit) = pending.pop() {
        match visit {
            Visit::Statement(statement, strict) => {
                statement_children(statement, strict, &mut pending)
            }
            Visit::Expression(expression, strict) => {
                expression_children(expression, strict, &mut pending)?
            }
            Visit::ConditionalClauses(clauses, strict) => {
                if let Some(((condition, yes), rest)) = clauses.split_last() {
                    pending.push(Visit::ConditionalClauses(rest, strict));
                    pending.push(Visit::Expression(yes, strict));
                    pending.push(Visit::Expression(condition, strict));
                }
            }
            Visit::Pattern(pattern, strict) => pattern_children(pattern, strict, &mut pending),
            Visit::Class(class) => {
                if class.name.as_ref().is_some_and(|name| restricted(name)) {
                    return Err(invalid());
                }
                for member in class.members.iter().rev() {
                    if let Some(value) = &member.value {
                        pending.push(Visit::Expression(value, true));
                    }
                }
                if let Some(base) = &class.base {
                    pending.push(Visit::Expression(base, true));
                }
            }
            Visit::BindingName(name, strict) => {
                if strict && restricted(name) {
                    return Err(invalid());
                }
            }
            Visit::AssignmentNames(expression, strict) => assignment_names(expression, strict)?,
            Visit::ParameterNames(params) => {
                let mut names = std::collections::BTreeSet::new();
                for name in params {
                    if restricted(name) || !names.insert(name) {
                        return Err(invalid());
                    }
                }
            }
        }
    }
    Ok(())
}

fn push_statements<'a>(pending: &mut Vec<Visit<'a>>, statements: &'a [Stmt], strict: bool) {
    pending.extend(
        statements
            .iter()
            .rev()
            .map(|statement| Visit::Statement(statement, strict)),
    );
}

fn pattern_children<'a>(pattern: &'a Pattern, strict: bool, pending: &mut Vec<Visit<'a>>) {
    match pattern {
        Pattern::Name(name) => pending.push(Visit::BindingName(name, strict)),
        Pattern::Array(items, rest) => {
            if let Some(rest) = rest {
                pending.push(Visit::Pattern(rest, strict));
            }
            pending.extend(
                items
                    .iter()
                    .rev()
                    .flatten()
                    .map(|item| Visit::Pattern(item, strict)),
            );
        }
        Pattern::Object(items, rest) => {
            if let Some(rest) = rest {
                pending.push(Visit::Pattern(rest, strict));
            }
            pending.extend(
                items
                    .iter()
                    .rev()
                    .map(|(_, item)| Visit::Pattern(item, strict)),
            );
        }
        Pattern::Default(item, value) => {
            pending.push(Visit::Expression(value, strict));
            pending.push(Visit::Pattern(item, strict));
        }
    }
}

fn statement_children<'a>(statement: &'a Stmt, strict: bool, pending: &mut Vec<Visit<'a>>) {
    match statement {
        Stmt::Expression(value) | Stmt::Throw(value) | Stmt::Return(Some(value)) => {
            pending.push(Visit::Expression(value, strict));
        }
        Stmt::Variables(_, values) => {
            for (name, value) in values.iter().rev() {
                if let Some(value) = value {
                    pending.push(Visit::Expression(value, strict));
                }
                pending.push(Visit::BindingName(name, strict));
            }
        }
        Stmt::PatternVariables(_, values) => {
            for (pattern, value) in values.iter().rev() {
                if let Some(value) = value {
                    pending.push(Visit::Expression(value, strict));
                }
                pending.push(Visit::Pattern(pattern, strict));
            }
        }
        Stmt::FunctionDefinition(name, value) => {
            pending.push(Visit::Expression(value, strict));
            pending.push(Visit::BindingName(name, strict));
        }
        Stmt::ClassDefinition(name, value) => {
            pending.push(Visit::Class(value));
            pending.push(Visit::BindingName(name, strict));
        }
        Stmt::Block(body) => push_statements(pending, body, strict),
        Stmt::If(condition, yes, no) => {
            if let Some(no) = no {
                pending.push(Visit::Statement(no, strict));
            }
            pending.push(Visit::Statement(yes, strict));
            pending.push(Visit::Expression(condition, strict));
        }
        Stmt::IfChain(chain) => {
            if let Some(fallback) = chain.fallback() {
                pending.push(Visit::Statement(fallback, strict));
            }
            for (condition, body) in chain.clauses().iter().rev() {
                pending.push(Visit::Statement(body, strict));
                pending.push(Visit::Expression(condition, strict));
            }
        }
        Stmt::While(condition, body) | Stmt::DoWhile(body, condition) => {
            pending.push(Visit::Statement(body, strict));
            pending.push(Visit::Expression(condition, strict));
        }
        Stmt::For(initial, condition, update, body) => {
            pending.push(Visit::Statement(body, strict));
            for value in [update, condition].into_iter().flatten() {
                pending.push(Visit::Expression(value, strict));
            }
            if let Some(initial) = initial {
                pending.push(Visit::Statement(initial, strict));
            }
        }
        Stmt::ForIn(binding, value, body)
        | Stmt::ForOf(binding, value, body)
        | Stmt::ForAwaitOf(binding, value, body) => {
            pending.push(Visit::Statement(body, strict));
            pending.push(Visit::Expression(value, strict));
            match binding {
                ForBinding::Declaration(_, name) => pending.push(Visit::BindingName(name, strict)),
                ForBinding::Pattern(_, pattern) => pending.push(Visit::Pattern(pattern, strict)),
                ForBinding::Reference(value) => {
                    pending.push(Visit::Expression(value, strict));
                    // Assignment-target restrictions precede operand checks.
                    pending.push(Visit::AssignmentNames(value, strict));
                }
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            if let Some(body) = finally {
                push_statements(pending, body, strict);
            }
            if let Some((name, body)) = catch {
                push_statements(pending, body, strict);
                if let Some(name) = name {
                    pending.push(Visit::BindingName(name, strict));
                }
            }
            push_statements(pending, body, strict);
        }
        Stmt::Switch(value, clauses) => {
            for (value, body) in clauses.iter().rev() {
                push_statements(pending, body, strict);
                if let Some(value) = value {
                    pending.push(Visit::Expression(value, strict));
                }
            }
            pending.push(Visit::Expression(value, strict));
        }
        Stmt::Label(_, body) => pending.push(Visit::Statement(body, strict)),
        _ => {}
    }
}

fn assignment_names(expr: &Expr, strict: bool) -> Result<(), JsError> {
    if !strict {
        return Ok(());
    }
    let mut pending = vec![expr];
    while let Some(expr) = pending.pop() {
        match expr {
            Expr::Name(name) if matches!(name.as_str(), "eval" | "arguments") => {
                return Err(invalid());
            }
            Expr::Array(items) => pending.extend(items.iter().rev()),
            Expr::Object(items) => pending.extend(items.iter().rev().map(|(_, item)| item)),
            Expr::Assign(_, target, _) | Expr::Spread(target) => pending.push(target),
            _ => {}
        }
    }
    Ok(())
}

fn expression_children<'a>(
    expr: &'a Expr,
    strict: bool,
    pending: &mut Vec<Visit<'a>>,
) -> Result<(), JsError> {
    match expr {
        Expr::Function {
            kind,
            name,
            params,
            body,
            parameters: extra,
            ..
        } => {
            let own_directive = strict_directive(body);
            let strict = strict || own_directive;
            if strict && name.as_ref().is_some_and(|name| restricted(name)) {
                return Err(invalid());
            }
            crate::resumable::validate_body(body, *kind)?;
            if let Some(extra) = extra
                && own_directive
                && extra.iter().any(|parameter| {
                    parameter.rest || !matches!(parameter.pattern, Pattern::Name(_))
                })
            {
                return Err(JsError::syntax(
                    0,
                    "Strict directive with non-simple parameters",
                ));
            }
            push_statements(pending, body, strict);
            if strict {
                pending.push(Visit::ParameterNames(params));
            }
            if let Some(extra) = extra {
                pending.extend(
                    extra
                        .iter()
                        .rev()
                        .map(|parameter| Visit::Pattern(&parameter.pattern, strict)),
                );
            }
        }
        Expr::Class(definition) => pending.push(Visit::Class(definition)),
        Expr::Sequence(sequence) => {
            pending.extend(
                sequence
                    .items()
                    .iter()
                    .rev()
                    .map(|value| Visit::Expression(value, strict)),
            );
        }
        Expr::Array(values) | Expr::Template(values) => {
            pending.extend(
                values
                    .iter()
                    .rev()
                    .map(|value| Visit::Expression(value, strict)),
            );
        }
        Expr::Object(values) => {
            pending.extend(
                values
                    .iter()
                    .rev()
                    .map(|(_, value)| Visit::Expression(value, strict)),
            );
        }
        Expr::Member(left, right) | Expr::Binary(_, left, right) | Expr::Assign(_, left, right) => {
            if matches!(expr, Expr::Assign(..)) {
                assignment_names(left, strict)?;
            }
            pending.push(Visit::Expression(right, strict));
            pending.push(Visit::Expression(left, strict));
        }
        Expr::Unary(op, value) => {
            if op == "delete" && matches!(value.as_ref(), Expr::PrivateMember(..)) {
                return Err(JsError::syntax(0, "Cannot delete private class fields"));
            }
            if strict && op == "delete" && matches!(value.as_ref(), Expr::Name(_)) {
                return Err(invalid());
            }
            pending.push(Visit::Expression(value, strict));
        }
        Expr::Update(value, _, _) => {
            if strict
                && matches!(value.as_ref(), Expr::Name(name) if matches!(name.as_str(), "eval" | "arguments"))
            {
                return Err(invalid());
            }
            pending.push(Visit::Expression(value, strict));
        }
        Expr::Await(value)
        | Expr::YieldDelegate(value)
        | Expr::Yield(Some(value))
        | Expr::Spread(value)
        | Expr::PrivateMember(value, _) => pending.push(Visit::Expression(value, strict)),
        Expr::ImportCall { specifier, options } => {
            if let Some(options) = options {
                pending.push(Visit::Expression(options, strict));
            }
            pending.push(Visit::Expression(specifier, strict));
        }
        Expr::Call(target, args) | Expr::Construct(target, args) => {
            pending.extend(
                args.iter()
                    .rev()
                    .map(|value| Visit::Expression(value, strict)),
            );
            pending.push(Visit::Expression(target, strict));
        }
        Expr::ConditionalChain(chain) => {
            pending.push(Visit::Expression(chain.fallback(), strict));
            pending.push(Visit::ConditionalClauses(chain.clauses_rev(), strict));
        }
        Expr::Conditional(condition, yes, no) => {
            pending.push(Visit::Expression(no, strict));
            pending.push(Visit::Expression(yes, strict));
            pending.push(Visit::Expression(condition, strict));
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

#[cfg(test)]
mod traversal_order_tests {
    use super::*;
    use crate::{Declaration, ErrorKind, parser::FunctionKind, parser::Parameter};

    fn private_delete() -> Expr {
        Expr::Unary(
            "delete".into(),
            Box::new(Expr::PrivateMember(
                Box::new(Expr::Name("object".into())),
                "field".into(),
            )),
        )
    }

    fn function(name: Option<&str>, body: Vec<Stmt>) -> Expr {
        Expr::Function {
            kind: FunctionKind::Ordinary,
            method: false,
            name: name.map(str::to_owned),
            params: vec!["a".into(), "a".into()].into(),
            body: body.into(),
            arrow: false,
            parameters: Some(
                vec![Parameter {
                    pattern: Pattern::Default(
                        Box::new(Pattern::Name("a".into())),
                        Box::new(private_delete()),
                    ),
                    rest: false,
                }]
                .into(),
            ),
        }
    }

    fn error(statement: Stmt) -> String {
        let error = validate(&[statement], true).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Syntax);
        error.message
    }

    #[test]
    fn function_checks_keep_their_original_first_error() {
        let suspension = || {
            Stmt::Return(Some(Expr::Await(Box::new(Expr::Literal(Value::Number(
                1.0,
            ))))))
        };
        assert_eq!(
            error(Stmt::Expression(function(Some("eval"), vec![suspension()]))),
            invalid().message
        );
        assert!(
            error(Stmt::Expression(function(None, vec![suspension()])))
                .starts_with("Unsupported suspension position:")
        );
        let directive = Stmt::Expression(Expr::Literal(Value::String("use strict".into())));
        assert_eq!(
            error(Stmt::Expression(function(None, vec![directive]))),
            "Strict directive with non-simple parameters"
        );
        // Defaults are checked before duplicate parameter names, and those names
        // are checked before the body. Inherited strictness allows this default.
        assert_eq!(
            error(Stmt::Expression(function(None, vec![]))),
            "Cannot delete private class fields"
        );
        let mut expression = function(None, vec![Stmt::Expression(private_delete())]);
        if let Expr::Function { parameters, .. } = &mut expression {
            *parameters = None;
        }
        assert_eq!(error(Stmt::Expression(expression)), invalid().message);
    }

    #[test]
    fn statements_bindings_and_operands_keep_source_order() {
        assert_eq!(
            error(Stmt::Variables(
                Declaration::Let,
                vec![("eval".into(), Some(private_delete()))]
            )),
            invalid().message
        );
        assert_eq!(
            error(Stmt::Variables(
                Declaration::Let,
                vec![("x".into(), Some(private_delete())), ("eval".into(), None)]
            )),
            "Cannot delete private class fields"
        );
        assert_eq!(
            error(Stmt::Try {
                body: vec![Stmt::Expression(private_delete())],
                catch: Some((Some("eval".into()), Vec::new())),
                finally: None,
            }),
            "Cannot delete private class fields"
        );
        assert_eq!(
            error(Stmt::ForOf(
                ForBinding::Reference(Expr::Array(vec![
                    Expr::Name("eval".into()),
                    private_delete()
                ])),
                private_delete(),
                Box::new(Stmt::Empty),
            )),
            invalid().message
        );
    }

    #[test]
    fn inherited_directive_and_class_strictness_reach_deep_descendants() {
        std::thread::Builder::new()
            .stack_size(1024 * 1024)
            .spawn(|| {
                let mut source = "function inner(){'use strict'; eval = 1;}".to_string();
                for _ in 0..55 {
                    source = format!("function outer(){{{source}}}");
                }
                let statements = crate::parser::parse(&source).unwrap();
                assert_eq!(
                    validate(&statements, false).unwrap_err().message,
                    invalid().message
                );
                for source in [
                    "'use strict'; function f(){ function g(){ arguments = 1; } }",
                    "class C { field = function(){ eval = 1; }; }",
                    "let value = class { method(){ arguments = 1; } };",
                ] {
                    let statements = crate::parser::parse(source).unwrap();
                    assert_eq!(
                        validate(&statements, false).unwrap_err().message,
                        invalid().message
                    );
                }
                let statements = crate::parser::parse(
                    "function strict(){'use strict';} function sloppy(eval){}",
                )
                .unwrap();
                validate(&statements, false).unwrap();
            })
            .unwrap()
            .join()
            .unwrap();
    }
}

#[cfg(test)]
mod if_chain_tests {
    use super::*;

    fn legacy(statement: &Stmt) -> Stmt {
        let Stmt::IfChain(chain) = statement else {
            panic!("expected flat if chain");
        };
        let mut tail = chain.fallback().cloned();
        for (condition, body) in chain.clauses().iter().rev() {
            tail = Some(Stmt::If(
                condition.clone(),
                Box::new(body.clone()),
                tail.map(Box::new),
            ));
        }
        tail.unwrap()
    }

    #[test]
    fn flat_if_conditions_bodies_and_fallback_keep_strict_error_order() {
        for (source, expected) in [
            (
                "if (eval = 1) {} else if (true) { function f(a = 1) { 'use strict'; } }",
                invalid().message,
            ),
            (
                "if (true) { function f(a = 1) { 'use strict'; } } else if (arguments = 1) {}",
                "Strict directive with non-simple parameters".into(),
            ),
            (
                "if (true) {} else if (arguments = 1) {} else { function f(a = 1) { 'use strict'; } }",
                invalid().message,
            ),
            (
                "if (true) {} else if (false) {} else { eval = 1; }",
                invalid().message,
            ),
        ] {
            let statements = crate::parser::parse(source).unwrap();
            let flat = validate(&statements, true).unwrap_err();
            let nested = validate(&[legacy(&statements[0])], true).unwrap_err();
            assert_eq!(flat.kind, nested.kind, "{source}");
            assert_eq!(flat.message, nested.message, "{source}");
            assert_eq!(flat.message, expected, "{source}");
        }
    }

    #[test]
    fn flat_if_preserves_inherited_and_local_strictness() {
        let statements = crate::parser::parse(
            "if (eval = 1) {} else if (false) { arguments = 2; } else { var eval; }",
        )
        .unwrap();
        validate(&statements, false).unwrap();
        assert!(validate(&statements, true).is_err());
        let statements = crate::parser::parse(
            "if (true) {} else if (false) { function f() { 'use strict'; eval = 1; } }",
        )
        .unwrap();
        assert!(validate(&statements, false).is_err());
    }
}
