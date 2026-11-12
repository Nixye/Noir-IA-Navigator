//! Dynamic source stays inside the original interpreter and its current host.
use crate::{Declaration, Expr, Flow, ForBinding, Host, JsError, Runtime, Stmt, Value, parser};
use std::borrow::Cow;
use std::collections::BTreeSet;

const MAX_SOURCE: usize = 64 * 1024;
const MAX_EVAL_SOURCE: usize = 128 * 1024;
const MAX_TOTAL_SOURCE: usize = 256 * 1024;
const MAX_DYNAMIC_DEPTH: usize = 8;

impl Runtime {
    fn charge_dynamic_source(
        &mut self,
        bytes: usize,
        limit: usize,
        operation: &str,
    ) -> Result<(), JsError> {
        if bytes > limit {
            let mut error = JsError::resource("Dynamic source size limit exceeded");
            error.add_diagnostic_context(&format!(
                "dynamic operation={operation} input-bytes={bytes} limit={limit}"
            ));
            return Err(error);
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
            Some(Value::String(source)) => {
                // Arc-backed source can be parsed in place. Reserve source work
                // before parsing, without making an unbudgeted UTF-8 copy.
                self.charge_dynamic_source(source.len(), MAX_EVAL_SOURCE, "eval")?;
                self.string_copy_work(source.len())?;
                Cow::Borrowed(source.as_ref())
            }
            Some(Value::Utf16(source)) => {
                // Bound the measurement itself, then reserve before conversion.
                if source.len() > MAX_EVAL_SOURCE {
                    return Err(JsError::resource("Dynamic source unit limit exceeded"));
                }
                self.string_copy_work(source.len().saturating_mul(2))?;
                let mut bytes = 0usize;
                for scalar in char::decode_utf16(source.iter().copied()) {
                    let scalar = scalar
                        .map_err(|_| JsError::syntax(0, "Unpaired surrogate in dynamic source"))?;
                    bytes += scalar.len_utf8();
                }
                self.charge_dynamic_source(bytes, MAX_EVAL_SOURCE, "eval")?;
                let mut text = String::new();
                text.try_reserve_exact(bytes)
                    .map_err(|_| JsError::resource("Dynamic source storage unavailable"))?;
                for scalar in char::decode_utf16(source.iter().copied()) {
                    text.push(scalar.expect("already checked UTF-16"));
                }
                Cow::Owned(text)
            }
            _ => return Ok(args.first().cloned().unwrap_or(Value::Undefined)),
        };
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
        crate::call_storage::check_arguments(args.len())?;
        let mut parameters = String::new();
        let mut body = Cow::Borrowed("");
        for (index, value) in args.iter().enumerate() {
            let text = if let Value::String(text) = value {
                self.tick()?;
                Cow::Borrowed(text.as_ref())
            } else {
                // Preserve the existing bounded conversion behavior for other
                // values; ordinary string arguments require no source clone.
                Cow::Owned(self.text(value, 0)?)
            };
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
                let additional = text.len() + separator;
                let needed = parameters.len() + additional;
                if needed > parameters.capacity() {
                    let next = needed
                        .max(parameters.capacity().saturating_mul(2))
                        .min(MAX_SOURCE);
                    self.charge_string(next - parameters.capacity())?;
                    self.string_copy_work(parameters.len())?;
                    parameters
                        .try_reserve_exact(next - parameters.len())
                        .map_err(|_| JsError::resource("Dynamic parameter storage unavailable"))?;
                }
                self.string_copy_work(additional)?;
                if index > 0 {
                    parameters.push(',');
                }
                parameters.push_str(&text);
            }
        }
        self.charge_dynamic_source(parameters.len() + body.len(), MAX_SOURCE, "Function")?;
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
    let mut pending = vec![pattern];
    while let Some(pattern) = pending.pop() {
        match pattern {
            parser::Pattern::Name(name) => names.push(name.clone()),
            parser::Pattern::Default(pattern, _) => pending.push(pattern),
            parser::Pattern::Array(items, rest) => {
                if let Some(rest) = rest {
                    pending.push(rest);
                }
                pending.extend(items.iter().rev().flatten());
            }
            parser::Pattern::Object(items, rest) => {
                if let Some(rest) = rest {
                    pending.push(rest);
                }
                pending.extend(items.iter().rev().map(|(_, pattern)| pattern));
            }
        }
    }
    names
}

fn collect_vars(statements: &[Stmt], names: &mut BTreeSet<String>) {
    collect_var_statements(statements.iter().collect(), names);
}

fn collect_var_statements(mut pending: Vec<&Stmt>, names: &mut BTreeSet<String>) {
    while let Some(statement) = pending.pop() {
        match statement {
            Stmt::Variables(Declaration::Var, values) => {
                names.extend(values.iter().map(|(name, _)| name.clone()));
            }
            Stmt::PatternVariables(Declaration::Var, values) => {
                for (pattern, _) in values {
                    names.extend(pattern_names(pattern));
                }
            }
            Stmt::Block(body) => pending.extend(body),
            Stmt::If(_, yes, no) => {
                pending.push(yes);
                if let Some(no) = no {
                    pending.push(no);
                }
            }
            Stmt::IfChain(chain) => {
                // This collector visits a legacy if's else before its yes arm.
                pending.extend(chain.clauses().iter().map(|(_, body)| body));
                if let Some(fallback) = chain.fallback() {
                    pending.push(fallback);
                }
            }
            Stmt::While(_, body) | Stmt::DoWhile(body, _) | Stmt::Label(_, body) => {
                pending.push(body)
            }
            Stmt::For(init, _, _, body) => {
                if let Some(init) = init {
                    pending.push(init);
                }
                pending.push(body);
            }
            Stmt::ForIn(binding, _, body)
            | Stmt::ForOf(binding, _, body)
            | Stmt::ForAwaitOf(binding, _, body) => {
                match binding {
                    ForBinding::Declaration(Declaration::Var, name) => {
                        names.insert(name.clone());
                    }
                    ForBinding::Pattern(Declaration::Var, pattern) => {
                        names.extend(pattern_names(pattern))
                    }
                    _ => {}
                }
                pending.push(body);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                pending.extend(body);
                if let Some((_, body)) = catch {
                    pending.extend(body);
                }
                if let Some(body) = finally {
                    pending.extend(body);
                }
            }
            Stmt::Switch(_, clauses) => {
                for (_, body) in clauses {
                    pending.extend(body);
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

// Separate scope checks from tree traversal. The work stack holds references,
// including switch clause bodies, rather than recursively cloning whole trees.
// Reverse scheduling retains the original depth-first validation/error order.
enum Validation<'a> {
    List {
        statements: Vec<&'a Stmt>,
        params: Vec<String>,
        variable_scope: bool,
    },
    Statement(&'a Stmt),
    Expression(&'a Expr),
    ConditionalClauses(&'a [(Expr, Expr)]),
    Pattern(&'a parser::Pattern),
    FunctionBody(&'a Expr),
    LoopNames(Vec<String>, &'a Stmt),
}

pub(super) fn validate_list(
    statements: &[Stmt],
    params: &[String],
    variable_scope: bool,
) -> Result<(), JsError> {
    validate_pending(vec![Validation::List {
        statements: statements.iter().collect(),
        params: params.to_vec(),
        variable_scope,
    }])
}

fn validate_expression(expression: &Expr) -> Result<(), JsError> {
    validate_pending(vec![Validation::Expression(expression)])
}

fn validate_pending(mut pending: Vec<Validation<'_>>) -> Result<(), JsError> {
    while let Some(item) = pending.pop() {
        match item {
            Validation::List {
                statements,
                params,
                variable_scope,
            } => {
                validate_scope(&statements, &params, variable_scope)?;
                pending.extend(statements.into_iter().rev().map(Validation::Statement));
            }
            Validation::Statement(statement) => statement_children(statement, &mut pending),
            Validation::Expression(expression) => expression_children(expression, &mut pending),
            Validation::ConditionalClauses(clauses) => {
                if let Some(((condition, yes), rest)) = clauses.split_last() {
                    pending.push(Validation::ConditionalClauses(rest));
                    pending.push(Validation::Expression(yes));
                    pending.push(Validation::Expression(condition));
                }
            }
            Validation::Pattern(pattern) => pattern_children(pattern, &mut pending),
            Validation::FunctionBody(expression) => {
                let Expr::Function {
                    kind,
                    params,
                    parameters,
                    body,
                    ..
                } = expression
                else {
                    unreachable!()
                };
                let names = parameters.as_ref().map_or_else(
                    || params.as_ref().clone(),
                    |parameters| {
                        parameters
                            .iter()
                            .flat_map(|parameter| pattern_names(&parameter.pattern))
                            .collect()
                    },
                );
                crate::resumable::validate_body(body, *kind)?;
                pending.push(Validation::List {
                    statements: body.iter().collect(),
                    params: names,
                    variable_scope: true,
                });
            }
            Validation::LoopNames(names, body) => validate_loop_names(names, body)?,
        }
    }
    Ok(())
}

fn validate_scope(
    statements: &[&Stmt],
    params: &[String],
    variable_scope: bool,
) -> Result<(), JsError> {
    let mut vars = BTreeSet::new();
    collect_var_statements(statements.to_vec(), &mut vars);
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
    Ok(())
}

fn pattern_children<'a>(pattern: &'a parser::Pattern, pending: &mut Vec<Validation<'a>>) {
    match pattern {
        parser::Pattern::Name(_) => {}
        parser::Pattern::Default(pattern, value) => {
            pending.push(Validation::Expression(value));
            pending.push(Validation::Pattern(pattern));
        }
        parser::Pattern::Array(items, rest) => {
            if let Some(rest) = rest {
                pending.push(Validation::Pattern(rest));
            }
            pending.extend(items.iter().rev().flatten().map(Validation::Pattern));
        }
        parser::Pattern::Object(items, rest) => {
            if let Some(rest) = rest {
                pending.push(Validation::Pattern(rest));
            }
            pending.extend(
                items
                    .iter()
                    .rev()
                    .map(|(_, pattern)| Validation::Pattern(pattern)),
            );
        }
    }
}

fn class_children<'a>(definition: &'a parser::ClassDefinition, pending: &mut Vec<Validation<'a>>) {
    for member in definition.members.iter().rev() {
        if let Some(value) = &member.value {
            pending.push(Validation::Expression(value));
        }
    }
    if let Some(base) = &definition.base {
        pending.push(Validation::Expression(base));
    }
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

fn block<'a>(body: &'a [Stmt]) -> Validation<'a> {
    Validation::List {
        statements: body.iter().collect(),
        params: Vec::new(),
        variable_scope: false,
    }
}

fn statement_children<'a>(statement: &'a Stmt, pending: &mut Vec<Validation<'a>>) {
    match statement {
        Stmt::Expression(expr) | Stmt::Throw(expr) | Stmt::Return(Some(expr)) => {
            pending.push(Validation::Expression(expr))
        }
        Stmt::Variables(_, values) => {
            pending.extend(
                values
                    .iter()
                    .rev()
                    .filter_map(|(_, expr)| expr.as_ref())
                    .map(Validation::Expression),
            );
        }
        Stmt::PatternVariables(_, values) => {
            for (pattern, expr) in values.iter().rev() {
                if let Some(expr) = expr {
                    pending.push(Validation::Expression(expr));
                }
                pending.push(Validation::Pattern(pattern));
            }
        }
        Stmt::ClassDefinition(_, definition) => class_children(definition, pending),
        Stmt::FunctionDefinition(_, expression) => pending.push(Validation::Expression(expression)),
        Stmt::Block(body) => pending.push(block(body)),
        Stmt::If(expr, yes, no) => {
            if let Some(no) = no {
                pending.push(Validation::Statement(no));
            }
            pending.push(Validation::Statement(yes));
            pending.push(Validation::Expression(expr));
        }
        Stmt::IfChain(chain) => {
            if let Some(fallback) = chain.fallback() {
                pending.push(Validation::Statement(fallback));
            }
            for (condition, body) in chain.clauses().iter().rev() {
                pending.push(Validation::Statement(body));
                pending.push(Validation::Expression(condition));
            }
        }
        Stmt::While(expr, body) | Stmt::DoWhile(body, expr) => {
            pending.push(Validation::Statement(body));
            pending.push(Validation::Expression(expr));
        }
        Stmt::For(init, condition, update, body) => {
            pending.push(Validation::Statement(body));
            for expr in [update, condition].into_iter().flatten() {
                pending.push(Validation::Expression(expr));
            }
            if let Some(init) = init {
                pending.push(Validation::LoopNames(lexical_names(init), body));
                pending.push(Validation::Statement(init));
            }
        }
        Stmt::ForIn(binding, expr, body)
        | Stmt::ForOf(binding, expr, body)
        | Stmt::ForAwaitOf(binding, expr, body) => {
            pending.push(Validation::Statement(body));
            pending.push(Validation::Expression(expr));
            match binding {
                ForBinding::Reference(expr) => pending.push(Validation::Expression(expr)),
                ForBinding::Pattern(kind, pattern) => {
                    if *kind != Declaration::Var {
                        pending.push(Validation::LoopNames(pattern_names(pattern), body));
                    }
                    pending.push(Validation::Pattern(pattern));
                }
                ForBinding::Declaration(kind, name) if *kind != Declaration::Var => {
                    pending.push(Validation::LoopNames(vec![name.clone()], body))
                }
                _ => {}
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            if let Some(body) = finally {
                pending.push(block(body));
            }
            if let Some((name, body)) = catch {
                pending.push(Validation::List {
                    statements: body.iter().collect(),
                    params: name.iter().cloned().collect(),
                    variable_scope: false,
                });
            }
            pending.push(block(body));
        }
        Stmt::Switch(expr, clauses) => {
            for (expr, _) in clauses.iter().rev() {
                if let Some(expr) = expr {
                    pending.push(Validation::Expression(expr));
                }
            }
            pending.push(Validation::List {
                statements: clauses.iter().flat_map(|(_, body)| body).collect(),
                params: Vec::new(),
                variable_scope: false,
            });
            pending.push(Validation::Expression(expr));
        }
        Stmt::Label(_, body) => pending.push(Validation::Statement(body)),
        _ => {}
    }
}

fn expression_children<'a>(expr: &'a Expr, pending: &mut Vec<Validation<'a>>) {
    match expr {
        Expr::Function { parameters, .. } => {
            pending.push(Validation::FunctionBody(expr));
            if let Some(parameters) = parameters {
                pending.extend(
                    parameters
                        .iter()
                        .rev()
                        .map(|parameter| Validation::Pattern(&parameter.pattern)),
                );
            }
        }
        Expr::Class(definition) => class_children(definition, pending),
        Expr::Sequence(sequence) => {
            pending.extend(sequence.items().iter().rev().map(Validation::Expression));
        }
        Expr::Array(values) | Expr::Template(values) => {
            pending.extend(values.iter().rev().map(Validation::Expression))
        }
        Expr::Object(values) => pending.extend(
            values
                .iter()
                .rev()
                .map(|(_, expr)| Validation::Expression(expr)),
        ),
        Expr::ImportCall { specifier, options } => {
            if let Some(options) = options {
                pending.push(Validation::Expression(options));
            }
            pending.push(Validation::Expression(specifier));
        }
        Expr::Call(expr, args) | Expr::Construct(expr, args) => {
            pending.extend(args.iter().rev().map(Validation::Expression));
            pending.push(Validation::Expression(expr));
        }
        Expr::Member(left, right) | Expr::Binary(_, left, right) | Expr::Assign(_, left, right) => {
            pending.push(Validation::Expression(right));
            pending.push(Validation::Expression(left));
        }
        Expr::Await(expr)
        | Expr::YieldDelegate(expr)
        | Expr::Yield(Some(expr))
        | Expr::Unary(_, expr)
        | Expr::Update(expr, _, _)
        | Expr::Spread(expr)
        | Expr::PrivateMember(expr, _) => pending.push(Validation::Expression(expr)),
        Expr::ConditionalChain(chain) => {
            pending.push(Validation::Expression(chain.fallback()));
            pending.push(Validation::ConditionalClauses(chain.clauses_rev()));
        }
        Expr::Conditional(condition, yes, no) => {
            pending.push(Validation::Expression(no));
            pending.push(Validation::Expression(yes));
            pending.push(Validation::Expression(condition));
        }
        _ => {}
    }
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

#[cfg(test)]
mod traversal_order_tests {
    use super::*;
    use crate::ErrorKind;

    #[test]
    fn scope_defaults_loops_and_switches_keep_their_first_collision() {
        for (source, name) in [
            (
                "function f(a = function(){ let parameter; var parameter; }){ let body; var body; }",
                "parameter",
            ),
            (
                "{ let outer; var outer; { let inner; var inner; } }",
                "outer",
            ),
            (
                "for (let loop = function(){ let initial; var initial; }; false;) { var loop; }",
                "initial",
            ),
            (
                "for (let loop = 0; function(){ let condition; var condition; };) { var loop; }",
                "loop",
            ),
            (
                "switch (function(){ let discriminant; var discriminant; }) { case 0: let body; var body; }",
                "discriminant",
            ),
            (
                "switch (0) { case function(){ let caseTest; var caseTest; }: { let body; var body; } }",
                "body",
            ),
            (
                "switch (0) { case 0: let shared; break; default: var shared; }",
                "shared",
            ),
            (
                "try { let first; var first; } catch (error) { let error; } finally { let last; var last; }",
                "first",
            ),
            (
                "try {} catch (error) { let error; } finally { let last; var last; }",
                "error",
            ),
        ] {
            let statements = parser::parse(source).unwrap();
            let error = validate_list(&statements, &[], true).unwrap_err();
            assert_eq!(error.kind, ErrorKind::Syntax, "{source}");
            assert_eq!(error.message, collision(name).message, "{source}");
        }
    }

    #[test]
    fn deep_scope_and_pattern_errors_fit_one_mib_without_leaking_names() {
        std::thread::Builder::new().stack_size(1024 * 1024).spawn(|| {
            let mut source = "function inner(){let collision; var collision;}".to_string();
            for _ in 0..55 { source = format!("function outer(){{{source}}}"); }
            let statements = parser::parse(&source).unwrap();
            assert_eq!(validate_list(&statements, &[], true).unwrap_err().message, collision("collision").message);
            let pattern = format!("{}name{}", "[".repeat(55), "]".repeat(55));
            let statements = parser::parse(&format!("let name; var {pattern} = []; ")).unwrap();
            assert_eq!(validate_list(&statements, &[], true).unwrap_err().message, collision("name").message);
            let statements = parser::parse("let hidden; function f(){ var hidden; } switch (0) { case 0: {let x;} break; default: {let x;} }").unwrap();
            validate_list(&statements, &[], true).unwrap();
        }).unwrap().join().unwrap();
    }
}

#[cfg(test)]
mod if_chain_tests {
    use super::*;

    fn legacy(statement: &Stmt) -> Stmt {
        let Stmt::IfChain(chain) = statement else {
            panic!("expected flat if chain")
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
    fn flat_if_validation_keeps_condition_body_and_fallback_precedence() {
        for (source, name) in [
            (
                "if (function(){let first; var first;}) {let second; var second;} else if (true) {}",
                "first",
            ),
            (
                "if (true) {let first; var first;} else if (function(){let second; var second;}) {}",
                "first",
            ),
            (
                "if (true) {} else if (function(){let second; var second;}) {let third; var third;}",
                "second",
            ),
            (
                "if (true) {} else if (false) {} else {let last; var last;}",
                "last",
            ),
        ] {
            let statements = parser::parse(source).unwrap();
            let flat = validate_list(&statements, &[], true).unwrap_err();
            let nested = validate_list(&[legacy(&statements[0])], &[], true).unwrap_err();
            assert_eq!(flat.kind, nested.kind, "{source}");
            assert_eq!(flat.message, nested.message, "{source}");
            assert_eq!(flat.message, collision(name).message, "{source}");
        }
    }

    #[test]
    fn flat_if_collects_vars_without_crossing_functions_or_lexical_scopes() {
        let statements = parser::parse("if (function(){var hiddenCondition;}) {var first; let local;} else if (false) {var [second] = []; function f(){var hiddenBody;}} else {var last; let local;}").unwrap();
        let mut flat = BTreeSet::new();
        collect_vars(&statements, &mut flat);
        let mut nested = BTreeSet::new();
        collect_vars(&[legacy(&statements[0])], &mut nested);
        assert_eq!(flat, nested);
        assert_eq!(flat, ["first", "last", "second"].map(str::to_owned).into());
        validate_list(&statements, &[], true).unwrap();
        for source in [
            "let second; if (true) {} else if (false) {var second;}",
            "let last; if (true) {} else if (false) {} else {var last;}",
            "for (let loop = 0; false;) if (true) {} else if (false) {var loop;}",
        ] {
            assert!(
                validate_list(&parser::parse(source).unwrap(), &[], true).is_err(),
                "{source}"
            );
        }
    }
}
