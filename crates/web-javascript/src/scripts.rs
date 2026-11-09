//! Global script entry points. Reentrant evaluation is not a job boundary.
use crate::{ErrorKind, Flow, Host, JsError, Runtime, Stmt, Value, parser};

const MAX_NESTED_SCRIPTS: usize = 8;

impl Runtime {
    /// Callbacks entered from a setter still have an expression on the stack,
    /// even when no ordinary JS function or host method is currently active.
    pub(super) fn execution_job_boundary(&self) -> bool {
        self.call_depth == 0
            && self.eval_depth == 0
            && self.dynamic_depth == 0
            && self.nested_script_depth == 0
            && !self.resumable_execution_active()
    }

    /// Parsing completes before any script effect. Errors never skip syntax.
    pub fn execute(&mut self, source: &str, host: &mut impl Host) -> Result<Value, JsError> {
        let input_offset = self.execution_job_boundary();
        let statements = self
            .collect_garbage(&[], host)
            .and_then(|_| self.prepare_script(source, input_offset, false, host));
        let evaluation_started = statements.is_ok();
        let result = statements.and_then(|statements| self.evaluate_script(&statements, host));
        host.script_evaluation_finished();
        // Preserve the outer entry's checkpoint behavior: failed preparation
        // never starts a job, and fatal resource errors never run page cleanup.
        if evaluation_started
            && !result
                .as_ref()
                .is_err_and(|error| error.kind == ErrorKind::Resource)
        {
            self.drain_execution_jobs(host)?;
        }
        result
    }

    /// Synchronously evaluates a host-inserted global script in the current VM.
    /// This is neither eval nor Function: declarations use the global scope,
    /// strictness belongs to this script, and caller lexical scopes are hidden.
    /// The host owns saving/restoring its current-script metadata. This entry
    /// does not collect garbage, drain jobs, or finish the outer host script.
    /// Existing instruction, allocation, call and expression budgets are shared.
    pub fn execute_nested_script(
        &mut self,
        source: &str,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        if self.nested_script_depth >= MAX_NESTED_SCRIPTS {
            return Err(JsError::resource("Nested script depth exceeds 8"));
        }
        if self.call_depth >= self.limits.call_depth {
            return Err(JsError::resource("JavaScript call stack budget exceeded"));
        }
        self.tick()?;
        // A host may report syntax errors and continue. Failed compilation
        // still consumes the shared source budget, preventing parse-only loops.
        self.charge_string(source.len())?;
        self.nested_script_depth += 1;
        self.call_depth += 1;
        let result = self
            .prepare_script(source, false, true, host)
            .and_then(|statements| self.evaluate_script(&statements, host));
        self.call_depth -= 1;
        self.nested_script_depth -= 1;
        result
    }

    fn prepare_script(
        &mut self,
        source: &str,
        input_offset: bool,
        source_precharged: bool,
        host: &mut impl Host,
    ) -> Result<Vec<Stmt>, JsError> {
        let statements = parser::parse(source).map_err(|mut error| {
            if input_offset {
                error.mark_input_source_offset();
            }
            error
        })?;
        crate::dynamic_code::validate_list(&statements, &[], true)?;
        crate::strict_validation::validate(&statements, false)?;
        if !source_precharged {
            self.charge_string(source.len())?;
        }
        if !self.scopes[0].bindings.contains_key("this")
            && let Some(value) = host.global("this")
        {
            self.declare(0, "this", false, true)?;
            self.scopes[0].bindings.get_mut("this").unwrap().value = value;
        }
        Ok(statements)
    }

    fn evaluate_script(
        &mut self,
        statements: &[Stmt],
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let previous = self.strict;
        self.strict = crate::strict_validation::strict_directive(statements);
        let result = self.block(statements, 0, host);
        self.strict = previous;
        result.and_then(|flow| match flow {
            Flow::Normal(v) => Ok(v),
            Flow::Empty => Ok(Value::Undefined),
            _ => Err(JsError::runtime("Invalid top-level completion")),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Limits;

    #[derive(Default)]
    struct ScriptHost {
        current_script: usize,
        finished: usize,
        nested_calls: usize,
        callback_after_script: Option<Value>,
        callback_with_this: bool,
        collected_inside_setter: usize,
    }

    impl ScriptHost {
        fn nested(&mut self, source: &str, runtime: &mut Runtime) -> Result<Value, JsError> {
            self.nested_calls += 1;
            let previous = self.current_script;
            self.current_script += 1;
            let result = runtime.execute_nested_script(source, self);
            self.current_script = previous;
            result
        }
    }

    impl Host for ScriptHost {
        fn global(&mut self, name: &str) -> Option<Value> {
            match name {
                "this" | "window" => Some(Value::Host(0)),
                "bridge" => Some(Value::Host(1)),
                "runScript" => Some(Value::Host(2)),
                _ => None,
            }
        }
        fn global_object(&self) -> Option<u64> {
            Some(0)
        }
        fn global_accessor(&self, name: &str) -> bool {
            name == "scriptSource"
        }
        fn get(&mut self, _: u64, name: &str) -> Result<Value, JsError> {
            Ok(match name {
                "currentScript" => Value::Number(self.current_script as f64),
                _ => Value::Undefined,
            })
        }
        fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
            Err(JsError::runtime("Expected runtime-aware setter"))
        }
        fn set_with_runtime(
            &mut self,
            _: u64,
            property: &str,
            value: Value,
            runtime: &mut Runtime,
        ) -> Result<(), JsError> {
            if property == "callback" {
                self.callback_after_script = Some(value);
                return Ok(());
            }
            let source = value.primitive_text()?;
            let result = self.nested(&source, runtime);
            // A host may inspect/retain VM values during a setter; collection
            // and callback checkpoints must both remain suppressed here.
            self.collected_inside_setter += runtime.collect_garbage(&[], self)?.reclaimed_objects;
            if let Some(callback) = self.callback_after_script.clone() {
                if self.callback_with_this {
                    runtime.invoke_with_this(callback, vec![], Value::Host(0), self)?;
                } else {
                    runtime.invoke(callback, vec![], self)?;
                }
            }
            result.map(|_| ())
        }
        fn call(&mut self, _: u64, _: &str, _: &[Value]) -> Result<Value, JsError> {
            Err(JsError::runtime("Expected runtime-aware call"))
        }
        fn is_callable(&self, object: u64) -> bool {
            object == 2
        }
        fn call_with_runtime(
            &mut self,
            _: u64,
            _: &str,
            args: &[Value],
            _: &Value,
            runtime: &mut Runtime,
        ) -> Result<Value, JsError> {
            self.nested(&args[0].primitive_text()?, runtime)
        }
        fn script_evaluation_finished(&mut self) {
            self.finished += 1;
            self.current_script = 0;
        }
        fn trace_roots(&self) -> Vec<Value> {
            self.callback_after_script.iter().cloned().collect()
        }
    }

    fn assert_idle(runtime: &Runtime) {
        assert_eq!(runtime.nested_script_depth, 0);
        assert_eq!(runtime.call_depth, 0);
        assert_eq!(runtime.eval_depth, 0);
        assert!(!runtime.strict);
    }

    #[test]
    fn nested_scripts_declare_globals_without_capturing_caller_scope() {
        let mut runtime = Runtime::default();
        let mut host = ScriptHost::default();
        assert_eq!(
            runtime.execute(
                r#"
                var name='global'; var localKept;
                (function(){
                    var name='local'; let callerOnly=3;
                    bridge.script="var inserted=7; let lexical=9; function declared(){return name;} var seen=typeof callerOnly; var scriptThis=this;";
                    localKept=name;
                })();
                inserted===7 && window.inserted===7 && lexical===9 && window.lexical===undefined &&
                declared()==='global' && seen==='undefined' && scriptThis===window && localKept==='local';
                "#,
                &mut host,
            ).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(host.nested_calls, 1);
        assert_eq!(host.finished, 1);
        assert_idle(&runtime);
    }

    #[test]
    fn script_strictness_is_isolated_and_restored_after_errors() {
        for source in [
            r#"'use strict'; var restored=false; bridge.script='looseCreated=3;'; try{strictAfter=1;}catch(e){restored=true;} restored && window.looseCreated===3;"#,
            r#"var caught=false; try{bridge.script="'use strict'; strictNested=1;";}catch(e){caught=true;} looseAfter=2; caught && window.looseAfter===2 && typeof strictNested==='undefined';"#,
            r#"'use strict'; var caught=false; try{runScript('throw 17;');}catch(e){caught=e===17;} var strictAfter=false; try{undeclared=1;}catch(e){strictAfter=true;} caught && strictAfter;"#,
        ] {
            let mut runtime = Runtime::default();
            let mut host = ScriptHost::default();
            assert_eq!(
                runtime.execute(source, &mut host).unwrap(),
                Value::Bool(true),
                "{source}"
            );
            assert_idle(&runtime);
            assert_eq!(
                runtime.execute("runScript('3;');", &mut host).unwrap(),
                Value::Number(3.)
            );
        }
    }

    #[test]
    fn nested_scripts_and_setter_callbacks_wait_for_the_outer_checkpoint() {
        for callback_with_this in [false, true] {
            for target in ["bridge.script", "window.scriptSource"] {
                let mut runtime = Runtime::default();
                let mut host = ScriptHost {
                    current_script: 1,
                    callback_with_this,
                    ..ScriptHost::default()
                };
                let source = format!(
                    r#"
                    var order=''; var during; var microtaskScript;
                    bridge.callback=function(){{order+='callback,';}};
                    {target}="order+='nested,'; during=bridge.currentScript; queueMicrotask(()=>{{order+='job,';microtaskScript=bridge.currentScript;}});";
                    order+='parent,';
                "#
                );
                runtime.execute(&source, &mut host).unwrap();
                assert_eq!(
                    runtime.global_value("order"),
                    Some(Value::string("nested,callback,parent,job,"))
                );
                assert_eq!(runtime.global_value("during"), Some(Value::Number(2.)));
                assert_eq!(
                    runtime.global_value("microtaskScript"),
                    Some(Value::Number(0.))
                );
                assert_eq!(host.finished, 1);
                assert_idle(&runtime);
            }
        }
    }

    #[test]
    fn no_collection_occurs_with_live_caller_temporaries() {
        let mut runtime = Runtime::default();
        let mut host = ScriptHost::default();
        assert_eq!(runtime.execute(
            r#"function combine(a,b){return a.answer+b;} combine({answer:40}, (bridge.script='var allocated={unused:1};', 2));"#,
            &mut host,
        ).unwrap(), Value::Number(42.));
        assert_eq!(host.collected_inside_setter, 0);
        assert_idle(&runtime);
    }

    #[test]
    fn malformed_nested_source_has_no_script_effects_or_outer_offsets() {
        for source in [
            "var changed=1; let broken=;",
            "var changed=1; let duplicate; let duplicate;",
            "var changed=1; 'ignored'; return 1;",
        ] {
            let mut runtime = Runtime::default();
            let mut host = ScriptHost::default();
            let error = runtime
                .execute_nested_script(source, &mut host)
                .unwrap_err();
            assert_eq!(error.kind, ErrorKind::Syntax, "{source}");
            assert_eq!(error.input_source_offset(), None);
            assert_eq!(runtime.global_value("changed"), None);
            assert_eq!(host.finished, 0);
            assert_idle(&runtime);
        }

        let mut runtime = Runtime::new(Limits {
            string_bytes: 12,
            ..Limits::default()
        });
        let mut host = ScriptHost::default();
        for _ in 0..2 {
            assert_eq!(
                runtime
                    .execute_nested_script("var =;", &mut host)
                    .unwrap_err()
                    .kind,
                ErrorKind::Syntax
            );
        }
        assert_eq!(
            runtime
                .execute_nested_script("var =;", &mut host)
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
        assert_idle(&runtime);
    }

    #[test]
    fn nested_script_depth_is_bounded_and_resource_errors_are_fatal() {
        let mut runtime = Runtime::default();
        let mut host = ScriptHost::default();
        let error = runtime.execute(
            r#"var caught=false; var cleaned=false; var after=false; var source='bridge.script=source;'; try{bridge.script=source;}catch(e){caught=true;}finally{cleaned=true;} after=true;"#,
            &mut host,
        ).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert_eq!(error.message, "Nested script depth exceeds 8");
        assert_eq!(host.nested_calls, 9);
        for name in ["caught", "cleaned", "after"] {
            assert_eq!(
                runtime.global_value(name),
                Some(Value::Bool(false)),
                "{name}"
            );
        }
        assert_idle(&runtime);
        assert_eq!(
            runtime
                .execute("bridge.script='var recovered=42;'; recovered;", &mut host)
                .unwrap(),
            Value::Number(42.)
        );
    }

    #[test]
    fn nested_scripts_share_the_call_budget_and_instruction_budget() {
        let mut runtime = Runtime::new(Limits {
            call_depth: 1,
            ..Limits::default()
        });
        let mut host = ScriptHost::default();
        let error = runtime.execute("runScript('1;');", &mut host).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert_eq!(error.message, "JavaScript call stack budget exceeded");
        assert_idle(&runtime);

        let mut runtime = Runtime::new(Limits {
            instructions: 100,
            ..Limits::default()
        });
        let error = runtime.execute(
            "var caught=false; var jobs=0; queueMicrotask(()=>jobs++); try{bridge.script='while(true){}';}catch(e){caught=true;}",
            &mut host,
        ).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert_eq!(runtime.remaining_instructions(), 0);
        assert_eq!(runtime.global_value("caught"), Some(Value::Bool(false)));
        assert_eq!(runtime.global_value("jobs"), Some(Value::Number(0.)));
        assert_idle(&runtime);
    }

    #[test]
    fn nested_scripts_preserve_expression_and_allocation_limits() {
        let mut host = ScriptHost::default();
        let mut runtime = Runtime {
            call_depth: 2,
            eval_depth: 64,
            dynamic_depth: 3,
            strict: true,
            ..Runtime::default()
        };
        let error = runtime.execute_nested_script("1;", &mut host).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert_eq!(error.message, "Expression evaluation depth exceeded");
        assert_eq!(runtime.call_depth, 2);
        assert_eq!(runtime.eval_depth, 64);
        assert_eq!(runtime.dynamic_depth, 3);
        assert_eq!(runtime.nested_script_depth, 0);
        assert!(runtime.strict);

        for (limits, first, second, message) in [
            (
                Limits {
                    string_bytes: 3,
                    ..Limits::default()
                },
                "1;",
                "2;",
                "JavaScript string allocation budget exceeded",
            ),
            (
                Limits {
                    heap_objects: 1,
                    ..Limits::default()
                },
                "var first=[];",
                "var second=[];",
                "JavaScript heap object budget exceeded",
            ),
        ] {
            let mut runtime = Runtime::new(limits);
            runtime.execute_nested_script(first, &mut host).unwrap();
            let spent = runtime.remaining_instructions();
            let error = runtime
                .execute_nested_script(second, &mut host)
                .unwrap_err();
            assert_eq!(error.kind, ErrorKind::Resource);
            assert_eq!(error.message, message);
            assert!(runtime.remaining_instructions() < spent);
            assert_idle(&runtime);
        }
    }

    #[test]
    fn standalone_nested_entry_never_drains_microtasks_or_finalization() {
        let mut runtime = Runtime::default();
        let mut host = ScriptHost::default();
        runtime.execute(
            "var order=''; var registry=new FinalizationRegistry(()=>order+='cleanup,'); var target={}; registry.register(target,1); target=null;",
            &mut host,
        ).unwrap();
        runtime.collect_garbage(&[], &host).unwrap();
        runtime
            .execute_nested_script(
                "order+='nested,'; queueMicrotask(()=>order+='job,');",
                &mut host,
            )
            .unwrap();
        assert_eq!(
            runtime.global_value("order"),
            Some(Value::string("nested,"))
        );
        assert_eq!(host.finished, 1);
        runtime.execute("order+='outer,';", &mut host).unwrap();
        assert_eq!(
            runtime.global_value("order"),
            Some(Value::string("nested,outer,job,cleanup,"))
        );
        assert_eq!(host.finished, 2);
        assert_idle(&runtime);
    }

    #[test]
    fn finishing_outer_script_also_runs_on_parse_and_execution_errors() {
        for source in ["let broken=;", "throw 1;", "while(true){}"] {
            let mut runtime = Runtime::default();
            let mut host = ScriptHost {
                current_script: 1,
                ..ScriptHost::default()
            };
            assert!(runtime.execute(source, &mut host).is_err());
            assert_eq!(host.finished, 1);
            assert_eq!(host.current_script, 0);
            assert_idle(&runtime);
        }
    }
}
