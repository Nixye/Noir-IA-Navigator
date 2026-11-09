//! Dynamic source stays inside the original interpreter and its current host.
use crate::{Declaration, Expr, Flow, ForBinding, Host, JsError, Runtime, Stmt, Value, parser};
use std::collections::BTreeSet;

const MAX_SOURCE: usize = 64 * 1024;
const MAX_TOTAL_SOURCE: usize = 256 * 1024;
const MAX_DYNAMIC_DEPTH: usize = 8;

impl Runtime {
    fn charge_dynamic_source(&mut self, bytes: usize) -> Result<(), JsError> {
        if bytes > MAX_SOURCE {
            return Err(JsError::resource("Dynamic source exceeds 64 KiB"));
        }
        let total = self.dynamic_source_bytes.saturating_add(bytes);
        if total > MAX_TOTAL_SOURCE {
            return Err(JsError::resource("Dynamic source budget exceeds 256 KiB"));
        }
        if self.dynamic_depth >= MAX_DYNAMIC_DEPTH {
            return Err(JsError::resource("Dynamic source nesting exceeds 8"));
        }
        self.tick()?;
        self.charge_string(bytes)?;
        self.dynamic_source_bytes = total;
        Ok(())
    }

    pub(super) fn dynamic_eval(
        &mut self,
        args: &[Value],
        caller: usize,
        direct: bool,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let source = match args.first() {
            Some(Value::String(source)) => source.to_string(),
            Some(Value::Utf16(source)) => String::from_utf16(source)
                .map_err(|_| JsError::syntax(0, "Unpaired surrogate in dynamic source"))?,
            _ => return Ok(args.first().cloned().unwrap_or(Value::Undefined)),
        };
        self.charge_dynamic_source(source.len())?;
        let statements = parser::parse(&source)?;
        let inherited = direct && self.strict;
        let strict = inherited || crate::strict_validation::strict_directive(&statements);
        crate::strict_validation::validate(&statements, inherited)?;
        validate_list(&statements, &[], true)?;
        let caller = if direct { caller } else { 0 };
        let outer_target = self.function_scope(caller);
        let mut vars = BTreeSet::new();
        collect_vars(&statements, &mut vars);
        for statement in &statements {
            if let Stmt::FunctionDefinition(name, _) = statement {
                vars.insert(name.clone());
            }
        }
        // Sloppy eval can introduce vars into its caller. Check the complete
        // intervening lexical chain before creating or updating any binding.
        if !strict {
            for name in &vars {
                let mut scope = caller;
                loop {
                    if self.scopes[scope]
                        .bindings
                        .get(name)
                        .is_some_and(|binding| binding.lexical)
                    {
                        return Err(collision(name));
                    }
                    if scope == outer_target {
                        break;
                    }
                    scope = self.scopes[scope].parent.unwrap_or(outer_target);
                }
                if outer_target == 0
                    && self.scopes[0]
                        .bindings
                        .get(name)
                        .is_some_and(|binding| !binding.global_property)
                {
                    return Err(collision(name));
                }
            }
        }
        self.dynamic_depth += 1;
        let previous_strict = self.strict;
        let previous_deletable = self.global_declaration_deletable;
        self.global_declaration_deletable = !strict && outer_target == 0;
        self.strict = strict;
        let result = (|| {
            // A strict eval owns a var environment. Sloppy eval only owns a
            // lexical environment; its top-level functions use the var target.
            let scope = self.scope(caller, strict)?;
            let target = if strict { scope } else { outer_target };
            self.collect_vars(&statements, scope, host)?;
            for statement in &statements {
                match statement {
                    Stmt::Variables(kind, values) if *kind != Declaration::Var => {
                        for (name, _) in values {
                            self.declare(scope, name, *kind == Declaration::Let, false)?;
                        }
                    }
                    Stmt::PatternVariables(kind, values) if *kind != Declaration::Var => {
                        for (pattern, _) in values {
                            self.declare_pattern(pattern, *kind, scope)?;
                        }
                    }
                    Stmt::ClassDefinition(name, _) => {
                        self.declare(scope, name, true, false)?;
                    }
                    _ => {}
                }
            }
            for statement in &statements {
                let (name, value) = match statement {
                    Stmt::FunctionDefinition(name, expression) => {
                        (name, self.eval(expression, scope, host)?)
                    }
                    _ => continue,
                };
                if target == 0 {
                    self.define_global_function(name, value.clone(), host)?;
                }
                if !self.scopes[target].bindings.contains_key(name) {
                    self.declare(target, name, true, true)?;
                }
                let binding = self.scopes[target].bindings.get_mut(name).unwrap();
                binding.value = value;
                binding.initialized = true;
                binding.global_property = target == 0;
                binding.global_configurable = target == 0;
                if target == 0
                    && let Some(id) = self.global_properties
                    && let Some(deleted) = self.deleted_synthetics.get_mut(&id)
                {
                    deleted.remove(name);
                }
            }
            let mut result = Value::Undefined;
            for statement in &statements {
                match self.statement(statement, scope, host)? {
                    Flow::Normal(value) => result = value,
                    Flow::Empty => {}
                    _ => return Err(JsError::runtime("Invalid eval completion")),
                }
            }
            Ok(result)
        })();
        self.strict = previous_strict;
        self.global_declaration_deletable = previous_deletable;
        self.dynamic_depth -= 1;
        result
    }

    pub(super) fn dynamic_function(
        &mut self,
        args: &[Value],
        _host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let mut parameters = String::new();
        let mut body = String::new();
        for (index, value) in args.iter().enumerate() {
            let text = self.text(value, 0)?;
            let separator = usize::from(index > 0 && index + 1 < args.len());
            if parameters
                .len()
                .saturating_add(body.len())
                .saturating_add(text.len())
                .saturating_add(separator)
                > MAX_SOURCE
            {
                return Err(JsError::resource("Dynamic source exceeds 64 KiB"));
            }
            if index + 1 == args.len() {
                body = text;
            } else {
                if index > 0 {
                    parameters.push(',');
                }
                parameters.push_str(&text);
            }
        }
        self.charge_dynamic_source(parameters.len() + body.len())?;
        let (params, statements, parameters) = parser::parse_dynamic_function(&parameters, &body)?;
        // Validate the complete function expression, not just the body: strict
        // parameter names and directives with non-simple parameters are errors.
        let expression = Expr::Function {
            kind: parser::FunctionKind::Ordinary,
            method: false,
            name: None,
            params: params.clone(),
            body: statements.clone(),
            arrow: false,
            parameters: Some(parameters.clone()),
        };
        validate_expression(&expression)?;
        crate::strict_validation::validate(&[Stmt::Expression(expression)], false)?;
        // A Function constructor never captures the caller's lexical environment.
        let previous_strict = self.strict;
        self.strict = false;
        let result = self.make_function(params, statements, 0, false);
        self.strict = previous_strict;
        let function = result?;
        self.attach_parameters(&function, Some(parameters))?;
        Ok(function)
    }
}

fn collision(name: &str) -> JsError {
    JsError::syntax(0, format!("Dynamic declaration conflicts with {name}"))
}

fn pattern_names(pattern: &parser::Pattern) -> Vec<String> {
    let mut names = Vec::new();
    pattern.names(&mut names);
    names
}

fn collect_vars(statements: &[Stmt], names: &mut BTreeSet<String>) {
    for statement in statements {
        match statement {
            Stmt::Variables(Declaration::Var, values) => {
                names.extend(values.iter().map(|(name, _)| name.clone()));
            }
            Stmt::PatternVariables(Declaration::Var, values) => {
                for (pattern, _) in values {
                    names.extend(pattern_names(pattern));
                }
            }
            Stmt::Block(body) => collect_vars(body, names),
            Stmt::If(_, yes, no) => {
                collect_vars(std::slice::from_ref(yes.as_ref()), names);
                if let Some(no) = no {
                    collect_vars(std::slice::from_ref(no.as_ref()), names);
                }
            }
            Stmt::While(_, body) | Stmt::DoWhile(body, _) | Stmt::Label(_, body) => {
                collect_vars(std::slice::from_ref(body.as_ref()), names);
            }
            Stmt::For(init, _, _, body) => {
                if let Some(init) = init {
                    collect_vars(std::slice::from_ref(init.as_ref()), names);
                }
                collect_vars(std::slice::from_ref(body.as_ref()), names);
            }
            Stmt::ForIn(binding, _, body)
            | Stmt::ForOf(binding, _, body)
            | Stmt::ForAwaitOf(binding, _, body) => {
                match binding {
                    ForBinding::Declaration(Declaration::Var, name) => {
                        names.insert(name.clone());
                    }
                    ForBinding::Pattern(Declaration::Var, pattern) => {
                        names.extend(pattern_names(pattern));
                    }
                    _ => {}
                }
                collect_vars(std::slice::from_ref(body.as_ref()), names);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_vars(body, names);
                if let Some((_, body)) = catch {
                    collect_vars(body, names);
                }
                if let Some(body) = finally {
                    collect_vars(body, names);
                }
            }
            Stmt::Switch(_, clauses) => {
                for (_, body) in clauses {
                    collect_vars(body, names);
                }
            }
            _ => {}
        }
    }
}

fn lexical_names(statement: &Stmt) -> Vec<String> {
    match statement {
        Stmt::Variables(kind, values) if *kind != Declaration::Var => {
            values.iter().map(|(name, _)| name.clone()).collect()
        }
        Stmt::PatternVariables(kind, values) if *kind != Declaration::Var => values
            .iter()
            .flat_map(|(pattern, _)| pattern_names(pattern))
            .collect(),
        Stmt::ClassDefinition(name, _) => vec![name.clone()],
        _ => Vec::new(),
    }
}

pub(super) fn validate_list(
    statements: &[Stmt],
    params: &[String],
    variable_scope: bool,
) -> Result<(), JsError> {
    let mut vars = BTreeSet::new();
    collect_vars(statements, &mut vars);
    let mut lexical = BTreeSet::new();
    let mut functions = BTreeSet::new();
    for statement in statements {
        let mut names = lexical_names(statement);
        if let Stmt::FunctionDefinition(name, _) = statement {
            if variable_scope {
                functions.insert(name.clone());
            } else {
                names.push(name.clone());
            }
        }
        for name in names {
            if !lexical.insert(name.clone()) || vars.contains(&name) || params.contains(&name) {
                return Err(collision(&name));
            }
        }
    }
    if let Some(name) = lexical.intersection(&functions).next() {
        return Err(collision(name));
    }
    for statement in statements {
        validate_statement(statement)?;
    }
    Ok(())
}

fn validate_pattern(pattern: &parser::Pattern) -> Result<(), JsError> {
    match pattern {
        parser::Pattern::Name(_) => {}
        parser::Pattern::Default(pattern, value) => {
            validate_pattern(pattern)?;
            validate_expression(value)?;
        }
        parser::Pattern::Array(items, rest) => {
            for pattern in items.iter().flatten() {
                validate_pattern(pattern)?;
            }
            if let Some(rest) = rest {
                validate_pattern(rest)?;
            }
        }
        parser::Pattern::Object(items, rest) => {
            for (_, pattern) in items {
                validate_pattern(pattern)?;
            }
            if let Some(rest) = rest {
                validate_pattern(rest)?;
            }
        }
    }
    Ok(())
}

fn validate_class(definition: &parser::ClassDefinition) -> Result<(), JsError> {
    if let Some(base) = &definition.base {
        validate_expression(base)?;
    }
    for member in &definition.members {
        if let Some(value) = &member.value {
            validate_expression(value)?;
        }
    }
    Ok(())
}

fn validate_loop_names(names: Vec<String>, body: &Stmt) -> Result<(), JsError> {
    let mut vars = BTreeSet::new();
    collect_vars(std::slice::from_ref(body), &mut vars);
    let mut unique = BTreeSet::new();
    for name in names {
        if !unique.insert(name.clone()) || vars.contains(&name) {
            return Err(collision(&name));
        }
    }
    Ok(())
}

fn validate_statement(statement: &Stmt) -> Result<(), JsError> {
    match statement {
        Stmt::Expression(expr) | Stmt::Throw(expr) => validate_expression(expr)?,
        Stmt::Variables(_, values) => {
            for (_, expr) in values {
                if let Some(expr) = expr {
                    validate_expression(expr)?;
                }
            }
        }
        Stmt::PatternVariables(_, values) => {
            for (pattern, expr) in values {
                validate_pattern(pattern)?;
                if let Some(expr) = expr {
                    validate_expression(expr)?;
                }
            }
        }
        Stmt::ClassDefinition(_, definition) => validate_class(definition)?,
        Stmt::FunctionDefinition(_, expression) => validate_expression(expression)?,
        Stmt::Block(body) => validate_list(body, &[], false)?,
        Stmt::If(expr, yes, no) => {
            validate_expression(expr)?;
            validate_statement(yes)?;
            if let Some(no) = no {
                validate_statement(no)?;
            }
        }
        Stmt::While(expr, body) | Stmt::DoWhile(body, expr) => {
            validate_expression(expr)?;
            validate_statement(body)?;
        }
        Stmt::For(init, condition, update, body) => {
            if let Some(init) = init {
                validate_statement(init)?;
                validate_loop_names(lexical_names(init), body)?;
            }
            if let Some(expr) = condition {
                validate_expression(expr)?;
            }
            if let Some(expr) = update {
                validate_expression(expr)?;
            }
            validate_statement(body)?;
        }
        Stmt::ForIn(binding, expr, body)
        | Stmt::ForOf(binding, expr, body)
        | Stmt::ForAwaitOf(binding, expr, body) => {
            match binding {
                ForBinding::Reference(expr) => validate_expression(expr)?,
                ForBinding::Pattern(kind, pattern) => {
                    validate_pattern(pattern)?;
                    if *kind != Declaration::Var {
                        validate_loop_names(pattern_names(pattern), body)?;
                    }
                }
                ForBinding::Declaration(kind, name) if *kind != Declaration::Var => {
                    validate_loop_names(vec![name.clone()], body)?
                }
                _ => {}
            }
            validate_expression(expr)?;
            validate_statement(body)?;
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            validate_list(body, &[], false)?;
            if let Some((name, body)) = catch {
                validate_list(body, &name.iter().cloned().collect::<Vec<_>>(), false)?;
            }
            if let Some(body) = finally {
                validate_list(body, &[], false)?;
            }
        }
        Stmt::Switch(expr, clauses) => {
            validate_expression(expr)?;
            let combined = clauses
                .iter()
                .flat_map(|(_, body)| body.iter().cloned())
                .collect::<Vec<_>>();
            validate_list(&combined, &[], false)?;
            for (expr, _) in clauses {
                if let Some(expr) = expr {
                    validate_expression(expr)?;
                }
            }
        }
        Stmt::Label(_, body) => validate_statement(body)?,
        Stmt::Return(Some(expr)) => validate_expression(expr)?,
        _ => {}
    }
    Ok(())
}

fn validate_expression(expr: &Expr) -> Result<(), JsError> {
    match expr {
        Expr::ImportMeta => {}
        Expr::Function {
            kind,
            params,
            parameters,
            body,
            ..
        } => {
            let names = if let Some(parameters) = parameters {
                let mut names = Vec::new();
                for parameter in parameters.iter() {
                    validate_pattern(&parameter.pattern)?;
                    names.extend(pattern_names(&parameter.pattern));
                }
                names
            } else {
                params.as_ref().clone()
            };
            crate::resumable::validate_body(body, *kind)?;
            validate_list(body, &names, true)?;
        }
        Expr::Class(definition) => validate_class(definition)?,
        Expr::Array(values) | Expr::Template(values) => {
            for expr in values {
                validate_expression(expr)?;
            }
        }
        Expr::Object(values) => {
            for (_, expr) in values {
                validate_expression(expr)?;
            }
        }
        Expr::ImportCall { specifier, options } => {
            validate_expression(specifier)?;
            if let Some(options) = options {
                validate_expression(options)?;
            }
        }
        Expr::Call(expr, args) | Expr::Construct(expr, args) => {
            validate_expression(expr)?;
            for expr in args {
                validate_expression(expr)?;
            }
        }
        Expr::Member(left, right) | Expr::Binary(_, left, right) | Expr::Assign(_, left, right) => {
            validate_expression(left)?;
            validate_expression(right)?;
        }
        Expr::Await(expr)
        | Expr::YieldDelegate(expr)
        | Expr::Yield(Some(expr))
        | Expr::Unary(_, expr)
        | Expr::Update(expr, _, _)
        | Expr::Spread(expr)
        | Expr::PrivateMember(expr, _) => validate_expression(expr)?,
        Expr::Conditional(condition, yes, no) => {
            validate_expression(condition)?;
            validate_expression(yes)?;
            validate_expression(no)?;
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod for_await_declaration_tests {
    use super::*;

    #[test]
    fn collects_for_await_vars_without_escaping_nested_functions() {
        let statements = parser::parse("async function f() { for await (var [x] of source) { var y; } async function g() { for await (var z of source) {} } }").unwrap();
        let Stmt::FunctionDefinition(_, Expr::Function { body, .. }) = &statements[0] else {
            panic!("expected function");
        };
        let mut names = BTreeSet::new();
        collect_vars(body, &mut names);
        assert!(names.contains("x"));
        assert!(names.contains("y"));
        assert!(!names.contains("z"));
    }

    #[test]
    fn rejects_lexical_conflicts_with_for_await_var_bindings() {
        let statements =
            parser::parse("async function f() { let x; for await (var x of source) {} }").unwrap();
        assert!(validate_list(&statements, &[], true).is_err());
    }
}

#[cfg(test)]
mod async_generator_declaration_tests {
    use super::*;

    #[test]
    fn validates_async_generator_scopes_and_suspension_operands() {
        for source in [
            "async function* f() { let x; for await (var x of source) {} }",
            "async function* f() { yield function() { let x; var x; }; }",
            "async function* f() { await function() { let x; var x; }; }",
        ] {
            let statements = parser::parse(source).unwrap();
            assert!(
                validate_list(&statements, &[], true).is_err(),
                "accepted {source}"
            );
        }
        let statements = parser::parse("async function* f() { for await (var x of source) { yield x; } async function* g() { var hidden; yield 1; } }").unwrap();
        let Stmt::FunctionDefinition(_, Expr::Function { body, .. }) = &statements[0] else {
            panic!("expected async generator");
        };
        let mut names = BTreeSet::new();
        collect_vars(body, &mut names);
        assert!(names.contains("x"));
        assert!(!names.contains("hidden"));
        assert!(validate_list(&statements, &[], true).is_ok());
    }
}
