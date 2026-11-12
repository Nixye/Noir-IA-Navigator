//! Keep statement-specific temporaries off the recursive dispatch stack.
//! These helpers preserve the dispatcher's single tick and all existing limits.
use crate::{
    Declaration, ErrorKind, Expr, Flow, ForBinding, Host, JsError, Runtime, Stmt, Value,
    exception_diagnostics, intrinsic_global, strict_equal,
};

// Statement nesting accumulates across ordinary calls and host reentry.
pub(super) const MAX_STATEMENT_DEPTH: usize = 64;

// No valid parsed tree has more pending statements than its syntax-node limit.
const MAX_PENDING_HOIST_STATEMENTS: usize = 16_384;

pub(super) fn push_hoist_statement<'a>(
    pending: &mut Vec<&'a Stmt>,
    statement: &'a Stmt,
) -> Result<(), JsError> {
    if pending.len() >= MAX_PENDING_HOIST_STATEMENTS {
        return Err(JsError::resource(
            "Variable hoisting worklist limit exceeded",
        ));
    }
    pending.push(statement);
    Ok(())
}

pub(super) fn push_hoist_statements<'a>(
    pending: &mut Vec<&'a Stmt>,
    statements: &'a [Stmt],
) -> Result<(), JsError> {
    for statement in statements.iter().rev() {
        push_hoist_statement(pending, statement)?;
    }
    Ok(())
}

// A marker stands for the removed If node, preserving its interleaved
// hoisting tick without allocating recursive suffixes or pre-debiting work.
static VIRTUAL_IF: Stmt = Stmt::Empty;
pub(super) fn push_hoist_if_chain<'a>(
    pending: &mut Vec<&'a Stmt>,
    chain: &'a crate::parser::IfChain,
) -> Result<(), JsError> {
    if let Some(body) = chain.fallback() {
        push_hoist_statement(pending, body)?;
    }
    for (index, (_, body)) in chain.clauses().iter().enumerate().rev() {
        push_hoist_statement(pending, body)?;
        if index != 0 {
            push_hoist_statement(pending, &VIRTUAL_IF)?;
        }
    }
    Ok(())
}

impl Runtime {
    #[inline(never)]
    pub(super) fn statement_if_chain(
        &mut self,
        chain: &crate::parser::IfChain,
        scope: usize,
        host: &mut impl Host,
    ) -> Result<Flow, JsError> {
        for (index, (condition, body)) in chain.clauses().iter().enumerate() {
            // statement_labeled charged the first If. The others become
            // reachable only after the preceding condition returned false.
            if index != 0 {
                self.tick()?;
            }
            if self.eval(condition, scope, host)?.truthy() {
                return self.statement(body, scope, host);
            }
        }
        match chain.fallback() {
            Some(body) => self.statement(body, scope, host),
            None => Ok(Flow::Normal(Value::Undefined)),
        }
    }

    #[inline(never)]
    pub(super) fn statement_class(
        &mut self,
        name: &str,
        definition: &crate::parser::ClassDefinition,
        scope: usize,
        host: &mut impl Host,
    ) -> Result<Flow, JsError> {
        let value = self.eval_class(definition, scope, host)?;
        let binding = self.scopes[scope]
            .bindings
            .get_mut(name)
            .ok_or_else(|| JsError::runtime("Missing class binding"))?;
        binding.value = value;
        binding.initialized = true;
        Ok(Flow::Empty)
    }

    #[inline(never)]
    pub(super) fn statement_patterns(
        &mut self,
        kind: &Declaration,
        values: &[(crate::parser::Pattern, Option<Expr>)],
        scope: usize,
        host: &mut impl Host,
    ) -> Result<Flow, JsError> {
        for (pattern, initial) in values {
            if *kind == Declaration::Var && initial.is_none() {
                continue;
            }
            let value = match initial {
                Some(expr) => self.eval(expr, scope, host)?,
                None => Value::Undefined,
            };
            self.bind_declared_pattern(pattern, value, *kind, scope, host)?;
        }
        Ok(Flow::Empty)
    }

    #[inline(never)]
    pub(super) fn statement_variables(
        &mut self,
        kind: &Declaration,
        values: &[(String, Option<Expr>)],
        scope: usize,
        host: &mut impl Host,
    ) -> Result<Flow, JsError> {
        for (name, initial) in values {
            let target = if *kind == Declaration::Var {
                self.function_scope(scope)
            } else {
                scope
            };
            if !self.scopes[target].bindings.contains_key(name) {
                self.declare(
                    target,
                    name,
                    *kind != Declaration::Const,
                    *kind == Declaration::Var,
                )?;
            }
            let value = match initial {
                Some(expr) => self.eval(expr, scope, host)?,
                None => Value::Undefined,
            };
            if initial.is_some() || *kind != Declaration::Var {
                self.initialize_binding_value(target, name, value, host)?;
            }
            self.scopes[target]
                .bindings
                .get_mut(name)
                .unwrap()
                .initialized = true;
        }
        Ok(Flow::Empty)
    }

    #[inline(never)]
    pub(super) fn statement_while(
        &mut self,
        condition: &Expr,
        body: &Stmt,
        scope: usize,
        host: &mut impl Host,
        labels: &[String],
    ) -> Result<Flow, JsError> {
        while self.eval(condition, scope, host)?.truthy() {
            match self.statement(body, scope, host)? {
                Flow::Break(None) => break,
                Flow::Continue(Some(label)) if labels.contains(&label) => {}
                other @ (Flow::Break(Some(_)) | Flow::Continue(Some(_))) => {
                    return Ok(other);
                }
                Flow::Return(value) => return Ok(Flow::Return(value)),
                _ => {}
            }
        }
        Ok(Flow::Normal(Value::Undefined))
    }

    #[inline(never)]
    pub(super) fn statement_do_while(
        &mut self,
        body: &Stmt,
        condition: &Expr,
        scope: usize,
        host: &mut impl Host,
        labels: &[String],
    ) -> Result<Flow, JsError> {
        loop {
            match self.statement(body, scope, host)? {
                Flow::Break(None) => break,
                Flow::Continue(Some(label)) if labels.contains(&label) => {}
                other @ (Flow::Break(Some(_)) | Flow::Continue(Some(_))) => {
                    return Ok(other);
                }
                Flow::Return(value) => return Ok(Flow::Return(value)),
                _ => {}
            }
            if !self.eval(condition, scope, host)?.truthy() {
                break;
            }
        }
        Ok(Flow::Normal(Value::Undefined))
    }

    #[inline(never)]
    pub(super) fn statement_try(
        &mut self,
        body: &[Stmt],
        catch: &Option<(Option<String>, Vec<Stmt>)>,
        finally: &Option<Vec<Stmt>>,
        scope: usize,
        host: &mut impl Host,
    ) -> Result<Flow, JsError> {
        let child = self.scope(scope, false)?;
        let mut result = self.block(body, child, host);
        // Exhaustion is a host boundary, not an exception a page can
        // swallow or override by returning from finally.
        if result
            .as_ref()
            .is_err_and(|error| error.kind == ErrorKind::Resource)
        {
            return result;
        }
        if let Err(error) = result.as_ref()
            && error.kind != ErrorKind::Resource
            && let Some((binding, body)) = catch
        {
            let value = self.exception_value(error)?;
            let diagnostic = exception_diagnostics::CatchDiagnostic::capture(error, &value);
            let child = self.scope(scope, false)?;
            if let Some(name) = binding {
                self.declare(child, name, true, true)?;
                self.scopes[child].bindings.get_mut(name).unwrap().value = value;
            }
            result = self.block(body, child, host);
            if let Err(error) = &mut result {
                diagnostic.attach(error);
            }
        }
        if result
            .as_ref()
            .is_err_and(|error| error.kind == ErrorKind::Resource)
        {
            return result;
        }
        if let Some(body) = finally {
            let child = self.scope(scope, false)?;
            let final_result = self.block(body, child, host);
            if !matches!(final_result, Ok(Flow::Normal(_) | Flow::Empty)) {
                return final_result;
            }
        }
        result
    }

    #[inline(never)]
    pub(super) fn statement_switch(
        &mut self,
        discriminant: &Expr,
        clauses: &[(Option<Expr>, Vec<Stmt>)],
        scope: usize,
        host: &mut impl Host,
    ) -> Result<Flow, JsError> {
        let value = self.eval(discriminant, scope, host)?;
        let child = self.scope(scope, false)?;
        let statements = clauses
            .iter()
            .flat_map(|(_, body)| body.iter().cloned())
            .collect::<Vec<_>>();
        self.prepare(&statements, child, host)?;
        let mut start = None;
        let mut default = None;
        for (index, (test, _)) in clauses.iter().enumerate() {
            self.tick()?;
            if let Some(test) = test {
                if strict_equal(&value, &self.eval(test, child, host)?) {
                    start = Some(index);
                    break;
                }
            } else {
                default = Some(index);
            }
        }
        let mut last = Value::Undefined;
        if let Some(start) = start.or(default) {
            for (_, statements) in &clauses[start..] {
                for statement in statements {
                    match self.statement(statement, child, host)? {
                        Flow::Normal(value) => last = value,
                        Flow::Break(None) => return Ok(Flow::Normal(last)),
                        Flow::Empty => {}
                        flow => return Ok(flow),
                    }
                }
            }
        }
        Ok(Flow::Normal(last))
    }

    #[inline(never)]
    pub(super) fn statement_for(
        &mut self,
        statement: &Stmt,
        scope: usize,
        host: &mut impl Host,
        labels: &[String],
    ) -> Result<Flow, JsError> {
        let Stmt::For(initial, condition, update, body) = statement else {
            unreachable!()
        };
        let mut child = self.scope(scope, false)?;
        let mut per_iteration = Vec::new();
        if let Some(initial) = initial {
            match initial.as_ref() {
                Stmt::Variables(Declaration::Let, values) => {
                    per_iteration.extend(values.iter().map(|(name, _)| name.clone()))
                }
                Stmt::PatternVariables(Declaration::Let, values) => {
                    for (pattern, _) in values {
                        pattern.names(&mut per_iteration);
                    }
                }
                _ => {}
            }
            self.prepare(std::slice::from_ref(initial.as_ref()), child, host)?;
            self.statement(initial, child, host)?;
        }
        child = self.copy_iteration_scope(child, &per_iteration)?;
        loop {
            self.tick()?;
            if let Some(condition) = condition
                && !self.eval(condition, child, host)?.truthy()
            {
                break;
            }
            match self.statement(body, child, host)? {
                Flow::Break(None) => break,
                Flow::Continue(Some(label)) if labels.contains(&label) => {}
                other @ (Flow::Break(Some(_)) | Flow::Continue(Some(_))) => {
                    return Ok(other);
                }
                Flow::Return(v) => return Ok(Flow::Return(v)),
                _ => {}
            }
            child = self.copy_iteration_scope(child, &per_iteration)?;
            if let Some(update) = update {
                self.eval(update, child, host)?;
            }
        }
        Ok(Flow::Normal(Value::Undefined))
    }

    #[inline(never)]
    pub(super) fn statement_for_in(
        &mut self,
        binding: &ForBinding,
        source: &Expr,
        body: &Stmt,
        scope: usize,
        host: &mut impl Host,
        labels: &[String],
    ) -> Result<Flow, JsError> {
        let source_scope = if let ForBinding::Declaration(kind, name) = binding
            && *kind != Declaration::Var
        {
            let child = self.scope(scope, false)?;
            self.declare(child, name, *kind == Declaration::Let, false)?;
            child
        } else if let ForBinding::Pattern(kind, pattern) = binding
            && *kind != Declaration::Var
        {
            let child = self.scope(scope, false)?;
            self.declare_pattern(pattern, *kind, child)?;
            child
        } else {
            scope
        };
        let object = self.eval(source, source_scope, host)?;
        let keys = self.enumerable_keys(&object)?;
        for key in keys {
            self.tick()?;
            // Deleted properties are skipped. New properties are not
            // appended to this bounded enumeration snapshot.
            if !matches!(object, Value::String(_) | Value::Utf16(_))
                && !self.has_property(&object, &key, host)?
            {
                continue;
            }
            let value = self.string(key)?;
            let iteration = self.iteration_scope(binding, value, scope, host)?;
            match self.statement(body, iteration, host)? {
                Flow::Break(None) => break,
                Flow::Continue(Some(label)) if labels.contains(&label) => {}
                other @ (Flow::Break(Some(_)) | Flow::Continue(Some(_))) => {
                    return Ok(other);
                }
                Flow::Return(value) => return Ok(Flow::Return(value)),
                _ => {}
            }
        }
        Ok(Flow::Normal(Value::Undefined))
    }

    #[inline(never)]
    pub(super) fn statement_for_of(
        &mut self,
        binding: &ForBinding,
        source: &Expr,
        body: &Stmt,
        scope: usize,
        host: &mut impl Host,
        labels: &[String],
    ) -> Result<Flow, JsError> {
        let source_scope = match binding {
            ForBinding::Declaration(kind, name) if *kind != Declaration::Var => {
                let child = self.scope(scope, false)?;
                self.declare(child, name, *kind == Declaration::Let, false)?;
                child
            }
            ForBinding::Pattern(kind, pattern) if *kind != Declaration::Var => {
                let child = self.scope(scope, false)?;
                self.declare_pattern(pattern, *kind, child)?;
                child
            }
            _ => scope,
        };
        let value = self.eval(source, source_scope, host)?;
        let mut iterator = self.iterator(value, host)?;
        let mut count = 0usize;
        let mut last = Value::Undefined;
        let result = (|| {
            while let Some(value) = self.iterator_next(&mut iterator, host)? {
                count += 1;
                if count > 4096 {
                    return Err(JsError::resource("For-of iteration item limit exceeded"));
                }
                let iteration = self.iteration_scope(binding, value, scope, host)?;
                match self.statement(body, iteration, host)? {
                    Flow::Break(None) => {
                        self.iterator_close(&mut iterator, host)?;
                        return Ok(Flow::Normal(last));
                    }
                    Flow::Continue(None) => {}
                    Flow::Continue(Some(label)) if labels.contains(&label) => {}
                    Flow::Normal(value) => last = value,
                    Flow::Empty => {}
                    other => {
                        self.iterator_close(&mut iterator, host)?;
                        return Ok(other);
                    }
                }
            }
            Ok(Flow::Normal(last))
        })();
        if result
            .as_ref()
            .is_err_and(|error| error.kind != ErrorKind::Resource)
        {
            self.iterator_close(&mut iterator, host)?;
        }
        result
    }

    #[inline(never)]
    pub(super) fn statement_throw(
        &mut self,
        expr: &Expr,
        scope: usize,
        host: &mut impl Host,
    ) -> Result<Flow, JsError> {
        let value = self.eval(expr, scope, host)?;
        let mut error = JsError::runtime(format!(
            "Uncaught {}",
            value
                .primitive_text()
                .unwrap_or_else(|_| "[object Object]".into())
        ));
        error.thrown = Some(value);
        Err(error)
    }

    #[inline(never)]
    pub(super) fn statement_label(
        &mut self,
        name: &str,
        body: &Stmt,
        scope: usize,
        host: &mut impl Host,
        labels: &[String],
    ) -> Result<Flow, JsError> {
        let mut labels = labels.to_vec();
        labels.push(name.to_owned());
        match self.statement_labeled(body, scope, host, &labels)? {
            Flow::Break(Some(label)) if label == name => Ok(Flow::Normal(Value::Undefined)),
            other => Ok(other),
        }
    }
    // Preserve the original declaration order and lazy global materialization.
    #[inline(never)]
    pub(super) fn hoist_var_names<'a>(
        &mut self,
        names: impl IntoIterator<Item = &'a str>,
        scope: usize,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        for name in names {
            let target = self.function_scope(scope);
            if target == 0
                && host.global_object().is_some()
                && intrinsic_global(name).is_some()
                && !self.global_deleted(name)
            {
                self.materialize_global_property(name, host)?;
            }
            let new = !self.scopes[target].bindings.contains_key(name);
            if new {
                self.declare(target, name, true, true)?;
            }
            if target == 0 {
                if !new && !self.scopes[0].bindings[name].global_property {
                    return Err(JsError::runtime(format!(
                        "Global lexical binding conflicts with var {name}"
                    )));
                }
                // Namespace markers only recognize names; global var
                // bindings must retain the actual lazy namespace object.
                let initial = match self.global_own_property(name) {
                    Some(value) => Some(value),
                    None if self.global_deleted(name) => None,
                    None if name == "crypto" => Some(self.crypto_namespace(host)?),
                    None if name == "performance" => Some(self.performance_namespace(host)?),
                    None if name == "WebAssembly" => Some(self.wasm_namespace()?),
                    None if name == "JSON" => Some(self.json_namespace()?),
                    None if name == "Math" => Some(self.math_namespace()?),
                    None if name == "Reflect" => Some(self.reflect_namespace()?),
                    None => intrinsic_global(name).or_else(|| host.global(name)),
                };
                let binding = self.scopes[0].bindings.get_mut(name).unwrap();
                binding.global_property = true;
                if new {
                    binding.global_configurable = self.global_declaration_deletable;
                }
                if new && let Some(value) = initial {
                    binding.value = value;
                }
                if let Some(id) = self.global_properties
                    && let Some(deleted) = self.deleted_synthetics.get_mut(&id)
                {
                    deleted.remove(name);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Empty;
    impl Host for Empty {
        fn global(&mut self, _: &str) -> Option<Value> {
            None
        }
        fn get(&mut self, _: u64, _: &str) -> Result<Value, JsError> {
            Ok(Value::Undefined)
        }
        fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
            Ok(())
        }
        fn call(&mut self, _: u64, _: &str, _: &[Value]) -> Result<Value, JsError> {
            Ok(Value::Undefined)
        }
    }

    #[test]
    fn nested_statement_dispatch_fits_one_mib_native_stack() {
        // Construct an already-parsed, valid tree to isolate the interpreter's
        // recursive dispatch from the parser's separate stack budget.
        let mut statement = Stmt::Expression(Expr::Literal(Value::Number(7.)));
        for _ in 0..50 {
            statement = Stmt::Block(vec![statement]);
        }
        std::thread::Builder::new()
            .stack_size(1024 * 1024)
            .spawn(move || {
                let mut runtime = Runtime::default();
                let before = runtime.remaining;
                let result = runtime.block(&[statement], 0, &mut Empty).unwrap();
                assert!(matches!(result, Flow::Normal(Value::Number(7.))));
                assert!(runtime.remaining < before);
                assert_eq!(
                    (
                        runtime.statement_depth,
                        runtime.eval_depth,
                        runtime.call_depth
                    ),
                    (0, 0, 0)
                );
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn statement_depth_is_shared_by_calls_and_released_after_errors() {
        std::thread::Builder::new()
            .stack_size(1024 * 1024)
            .spawn(|| {
                let mut runtime = Runtime::default();
                let source = format!(
                    "let caught=0;let finalized=0;function f(){{{}return f();{}}}",
                    "{".repeat(20),
                    "}".repeat(20),
                );
                runtime.execute(&source, &mut Empty).unwrap();
                let before = runtime.clone();
                let remaining = runtime.remaining;
                let error = runtime
                    .execute("try{f()}catch(e){caught++}finally{finalized++}", &mut Empty)
                    .unwrap_err();
                assert_eq!(error.kind, ErrorKind::Resource);
                assert_eq!(error.message, "Statement evaluation depth exceeded");
                assert!(error.thrown.is_none());
                assert_eq!(runtime.global_value("caught"), Some(Value::Number(0.)));
                assert_eq!(runtime.global_value("finalized"), Some(Value::Number(0.)));
                assert_eq!(
                    (
                        runtime.statement_depth,
                        runtime.eval_depth,
                        runtime.call_depth
                    ),
                    (0, 0, 0),
                );
                assert!(runtime.remaining < remaining);
                let consumed = runtime.remaining;
                runtime.rollback_to(before);
                assert_eq!(runtime.remaining, consumed);
                assert_eq!(runtime.statement_depth, 0);
                assert_eq!(
                    runtime.execute("1+1", &mut Empty).unwrap(),
                    Value::Number(2.)
                );
                runtime
                    .execute("try{throw 1}catch(e){}", &mut Empty)
                    .unwrap();
                assert_eq!(runtime.statement_depth, 0);
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn iterative_hoisting_keeps_order_ticks_and_function_boundaries() {
        #[derive(Default)]
        struct Names(Vec<String>);
        impl Host for Names {
            fn global(&mut self, name: &str) -> Option<Value> {
                self.0.push(name.to_owned());
                None
            }
            fn get(&mut self, _: u64, _: &str) -> Result<Value, JsError> {
                Ok(Value::Undefined)
            }
            fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
                Ok(())
            }
            fn call(&mut self, _: u64, _: &str, _: &[Value]) -> Result<Value, JsError> {
                Ok(Value::Undefined)
            }
        }
        let source = "var a;{var b}if(0){var c}else var d;while(0)var e;do var f;while(0);label:{var g}try{var h}catch(e){var i}finally{var j}switch(0){case 1:var k;default:var l}for(var m;false;)var n;for(var o in [])var p;for(var [q] of [])var r;var {s,t}={};function ignored(){var hidden}";
        let statements = crate::parser::parse(source).unwrap();
        let mut host = Names::default();
        let mut runtime = Runtime::default();
        let before = runtime.remaining;
        runtime.collect_vars(&statements, 0, &mut host).unwrap();
        // The var pattern in for-of uses declare_pattern, just as before, and
        // therefore does not ask the host for q's initial global value.
        let expected = ('a'..='t')
            .filter(|name| *name != 'q')
            .map(|name| name.to_string())
            .collect::<Vec<_>>();
        assert_eq!(host.0, expected);
        // Hoisting still takes 32 visits; twenty one-byte binding keys now
        // reserve their two units of copy work before allocation.
        assert_eq!(before - runtime.remaining, 32 + 20 * 2);
        assert_eq!(runtime.global_value("q"), Some(Value::Undefined));
        assert!(runtime.global_value("hidden").is_none());
        assert!(runtime.global_value("ignored").is_none());

        let mut host = Names::default();
        let mut runtime = Runtime::new(crate::Limits {
            instructions: 4 + 2 * 2,
            ..crate::Limits::default()
        });
        let error = runtime.collect_vars(&statements, 0, &mut host).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert_eq!(host.0, ["a", "b"]);
        assert!(runtime.global_value("c").is_none());
    }

    #[test]
    fn iterative_hoisting_handles_nested_function_body_on_one_mib_stack() {
        let mut statement = Stmt::Variables(Declaration::Var, vec![("deep".into(), None)]);
        for _ in 0..64 {
            statement = Stmt::Block(vec![statement]);
        }
        std::thread::Builder::new()
            .stack_size(1024 * 1024)
            .spawn(move || {
                let mut runtime = Runtime::default();
                let function = runtime
                    .make_function(
                        std::sync::Arc::new(Vec::new()),
                        std::sync::Arc::new(vec![statement]),
                        0,
                        false,
                    )
                    .unwrap();
                let error = runtime
                    .invoke(function, Vec::new(), &mut Empty)
                    .unwrap_err();
                assert_eq!(error.kind, ErrorKind::Resource);
                assert_eq!(error.message, "Statement evaluation depth exceeded");
                assert_eq!(
                    (
                        runtime.statement_depth,
                        runtime.call_depth,
                        runtime.eval_depth
                    ),
                    (0, 0, 0),
                );
                assert_eq!(
                    runtime.execute("1+1", &mut Empty).unwrap(),
                    Value::Number(2.)
                );
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn hoisting_worklist_has_a_hard_bound() {
        let statement = Stmt::Empty;
        let mut pending = vec![&statement; MAX_PENDING_HOIST_STATEMENTS];
        assert_eq!(
            push_hoist_statement(&mut pending, &statement)
                .unwrap_err()
                .kind,
            ErrorKind::Resource,
        );
        assert_eq!(pending.len(), MAX_PENDING_HOIST_STATEMENTS);
    }

    #[test]
    fn extracted_statement_helpers_preserve_abrupt_completions_and_scopes() {
        let cases = [
            (
                "let n=0;outer:for(let i=0;i<3;i++){for(let j=0;j<3;j++){if(j===1)continue outer;n++}}n",
                3.,
            ),
            (
                "let n=0;let i=0;outer:while(i++<3){switch(i){case 1:n+=2;break;case 2:continue outer;default:n+=5}}n",
                7.,
            ),
            (
                "let n=0;do{try{n++;if(n===2)break}finally{n+=2}}while(n<5);n",
                6.,
            ),
            (
                "let n=0;for(const key in {a:2,b:3}){n++}for(const value of [2,3]){n+=value}n",
                7.,
            ),
            (
                "function f(){try{throw 3}catch(e){return e+4}finally{let x=1}}f()",
                7.,
            ),
            ("class C{constructor(){this.n=7}}const {n}=new C();n", 7.),
        ];
        for (source, expected) in cases {
            let mut runtime = Runtime::default();
            assert_eq!(
                runtime.execute(source, &mut Empty).unwrap(),
                Value::Number(expected),
                "{source}"
            );
        }
    }
}
