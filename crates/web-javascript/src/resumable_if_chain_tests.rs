use super::*;

struct Empty;
impl Host for Empty {
    fn global(&mut self, _: &str) -> Option<Value> {
        None
    }
    fn get(&mut self, _: u64, _: &str) -> Result<Value, JsError> {
        Err(JsError::runtime("No host"))
    }
    fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
        Err(JsError::runtime("No host"))
    }
    fn call(&mut self, _: u64, _: &str, _: &[Value]) -> Result<Value, JsError> {
        Err(JsError::runtime("No host"))
    }
}
fn expand_statement(statement: Stmt) -> Stmt {
    match statement {
        Stmt::IfChain(chain) => {
            let mut tail = chain
                .fallback()
                .cloned()
                .map(expand_statement)
                .map(Box::new);
            for (condition, body) in chain.clauses().iter().rev() {
                tail = Some(Box::new(Stmt::If(
                    condition.clone(),
                    Box::new(expand_statement(body.clone())),
                    tail,
                )));
            }
            *tail.unwrap()
        }
        Stmt::Block(body) => Stmt::Block(body.into_iter().map(expand_statement).collect()),
        Stmt::Try {
            body,
            catch,
            finally,
        } => Stmt::Try {
            body: body.into_iter().map(expand_statement).collect(),
            catch: catch
                .map(|(name, body)| (name, body.into_iter().map(expand_statement).collect())),
            finally: finally.map(|body| body.into_iter().map(expand_statement).collect()),
        },
        statement => statement,
    }
}
fn bodies(source: &str) -> (Arc<Vec<Stmt>>, Arc<Vec<Stmt>>) {
    let Stmt::FunctionDefinition(_, Expr::Function { body, .. }) =
        crate::parser::parse(source).unwrap().remove(0)
    else {
        panic!()
    };
    let binary = Arc::new(body.iter().cloned().map(expand_statement).collect());
    (body, binary)
}
fn machine(body: Arc<Vec<Stmt>>, kind: FunctionKind) -> Runtime {
    let mut runtime = Runtime::default();
    let owner = runtime
        .allocate(crate::Heap::Object(crate::Object {
            properties: BTreeMap::new(),
            non_enumerable: BTreeSet::new(),
            prototype: None,
            regexp: None,
        }))
        .unwrap();
    assert_eq!(owner, 0);
    runtime
        .resumable_start(owner, body, 0, false, kind)
        .unwrap();
    runtime
}
fn normalized(result: Result<Outcome, JsError>) -> Result<String, (ErrorKind, String)> {
    result
        .map(|outcome| format!("{outcome:?}"))
        .map_err(|error| (error.kind, error.message))
}
fn binding(runtime: &Runtime) -> Option<Value> {
    runtime.scopes[0]
        .bindings
        .get("n")
        .map(|binding| binding.value.clone())
}

#[test]
fn if_chain_resumable_ticks_effects_and_suspension_match_legacy() {
    for (prefix, suspension, kind) in [
        ("function*", "yield", FunctionKind::Generator),
        ("async function", "await", FunctionKind::Async),
        ("async function*", "yield", FunctionKind::AsyncGenerator),
    ] {
        for selected in 0..4 {
            let source = format!(
                "{prefix} f(){{let n=0;if((n+=1,{suspension} 0)){{n+=10;}}else if((n+=2,{suspension} 1)){{n+=20;}}else if((n+=3,{suspension} 2)){{n+=30;}}else{{n+=40;}}return n;}}"
            );
            let (flat, legacy) = bodies(&source);
            let run = |body| {
                let mut runtime = machine(body, kind);
                let mut input = ResumeInput::Normal(Value::Undefined);
                let mut results = Vec::new();
                for phase in 0..10 {
                    let outcome = runtime.resumable_resume(0, input, &mut Empty).unwrap();
                    let done = matches!(outcome, Outcome::Completed(_));
                    input = ResumeInput::Normal(Value::Bool(phase == selected));
                    results.push((format!("{outcome:?}"), runtime.remaining, binding(&runtime)));
                    if done {
                        return results;
                    }
                    runtime
                        .collect_garbage(&[Value::Object(0)], &Empty)
                        .unwrap();
                }
                panic!("Unexpected continuation loop")
            };
            assert_eq!(run(flat), run(legacy), "{source}: selected={selected}");
        }
    }
}
#[test]
fn if_chain_resource_is_fatal_before_and_after_suspension() {
    for (prefix, suspension, kind) in [
        ("function*", "yield", FunctionKind::Generator),
        ("async function", "await", FunctionKind::Async),
    ] {
        let source = format!(
            "{prefix} f(){{let n=0;try{{if((n+=1,{suspension} 0))n+=10;else if((n+=2,{suspension} 0))n+=20;else n+=30;}}catch(e){{n=1000;}}finally{{n+=10000;}}return n;}}"
        );
        let (flat, legacy) = bodies(&source);
        for phase in 0..3 {
            for budget in 0..100 {
                let run = |body| {
                    let mut runtime = machine(body, kind);
                    for _ in 0..phase {
                        let outcome = runtime
                            .resumable_resume(
                                0,
                                ResumeInput::Normal(Value::Bool(false)),
                                &mut Empty,
                            )
                            .unwrap();
                        assert!(matches!(outcome, Outcome::Yield(_) | Outcome::Await(_)));
                    }
                    runtime.remaining = budget;
                    let result = normalized(runtime.resumable_resume(
                        0,
                        ResumeInput::Normal(Value::Bool(false)),
                        &mut Empty,
                    ));
                    if matches!(&result, Err((ErrorKind::Resource, _))) {
                        assert!(!runtime.resumable_exists(0));
                    }
                    (result, runtime.remaining, binding(&runtime))
                };
                assert_eq!(
                    run(flat.clone()),
                    run(legacy.clone()),
                    "phase={phase}, budget={budget}"
                );
            }
        }
    }
}
#[test]
fn if_chain_validator_preserves_virtual_work_and_failure_order() {
    let (flat, legacy) =
        bodies("function* f(){if(yield 0)yield 1;else if(yield 2)yield 3;else yield 4;}");
    for allowance in 0..30 {
        let run = |statement| {
            let mut validator = Validator {
                kind: FunctionKind::Generator,
                work: 65_536 - allowance,
            };
            let result = validator
                .statement(statement, true, 0)
                .map_err(|error| (error.kind, error.message));
            (result, validator.work)
        };
        assert_eq!(run(&flat[0]), run(&legacy[0]), "allowance={allowance}");
    }
}
#[test]
fn if_chain_validator_accepts_long_flat_chain_but_keeps_real_depth_guard() {
    let source = format!(
        "function* f(){{{}return 1;}}",
        "if(false){}else ".repeat(1200) + "{}"
    );
    let Stmt::FunctionDefinition(_, Expr::Function { body: flat, .. }) =
        crate::parser::parse(&source).unwrap().remove(0)
    else {
        panic!()
    };
    assert!(validate_body(&flat, FunctionKind::Generator).is_ok());
    let mut validator = Validator {
        kind: FunctionKind::Generator,
        work: 0,
    };
    assert_eq!(
        validator.statement(&flat[0], true, LIMIT).unwrap_err().kind,
        ErrorKind::Resource
    );
}

#[test]
fn if_chain_zero_budget_does_not_run_catch_or_finally_after_suspend() {
    let (body, _) = bodies(
        "function* f(){let n=0;try{if(yield 0)n=1;else if(yield 0)n=2;else n=3;}catch(e){n=100;}finally{n=200;}}",
    );
    let mut runtime = machine(body, FunctionKind::Generator);
    assert!(matches!(
        runtime
            .resumable_resume(0, ResumeInput::Normal(Value::Undefined), &mut Empty)
            .unwrap(),
        Outcome::Yield(_)
    ));
    runtime.remaining = 0;
    let error = runtime
        .resumable_resume(0, ResumeInput::Normal(Value::Bool(false)), &mut Empty)
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert_eq!(binding(&runtime), Some(Value::Number(0.)));
    assert!(!runtime.resumable_exists(0));
}

#[test]
fn if_chain_errors_and_abrupt_resume_match_legacy() {
    for source in [
        "function* f(){let n=0;if(false)n=1;else if(false)n=2;return n;}",
        "function* f(){let n=0;if(true)n=1;else if(false)n=2;return n;}",
        "function* f(){let n=0;if(false)n=1;else if(true)n=2;return n;}",
        "function* f(){let n=0;if(false)n=1;else if(missing)n=2;else n=3;}",
        "function* f(){let n=0;if(false)n=1;else if(true)throw 12;else n=3;}",
        "function* f(){let n=0;if(false)n=1;else if(false)n=2;else throw 12;}",
    ] {
        let (flat, legacy) = bodies(source);
        let run = |body| {
            let mut runtime = machine(body, FunctionKind::Generator);
            let result = normalized(runtime.resumable_resume(
                0,
                ResumeInput::Normal(Value::Undefined),
                &mut Empty,
            ));
            (result, runtime.remaining, binding(&runtime))
        };
        assert_eq!(run(flat), run(legacy));
    }
    let (flat, legacy) = bodies(
        "function* f(){let n=0;try{if(false)n=1;else if(yield 0)n=2;else n=3;}finally{n+=10;}return n;}",
    );
    for input in [
        ResumeInput::Throw(Value::Number(12.)),
        ResumeInput::Return(Value::Number(99.)),
    ] {
        let run = |body| {
            let mut runtime = machine(body, FunctionKind::Generator);
            assert!(matches!(
                runtime
                    .resumable_resume(0, ResumeInput::Normal(Value::Undefined), &mut Empty)
                    .unwrap(),
                Outcome::Yield(_)
            ));
            let result = normalized(runtime.resumable_resume(0, input.clone(), &mut Empty));
            (result, runtime.remaining, binding(&runtime))
        };
        assert_eq!(run(flat.clone()), run(legacy.clone()));
    }
}

#[test]
fn if_chain_pending_condition_keeps_scope_objects_alive() {
    let (body, _) = bodies(
        "function* f(){let n={value:7};if(yield 0)return n.value;else if(yield 1)return n.value+1;else return n.value+2;}",
    );
    for selected in [true, false] {
        let mut runtime = machine(body.clone(), FunctionKind::Generator);
        assert!(matches!(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Undefined), &mut Empty)
                .unwrap(),
            Outcome::Yield(Value::Number(0.))
        ));
        runtime
            .collect_garbage(&[Value::Object(0)], &Empty)
            .unwrap();
        assert!(matches!(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Bool(false)), &mut Empty)
                .unwrap(),
            Outcome::Yield(Value::Number(1.))
        ));
        runtime
            .collect_garbage(&[Value::Object(0)], &Empty)
            .unwrap();
        let result = runtime
            .resumable_resume(0, ResumeInput::Normal(Value::Bool(selected)), &mut Empty)
            .unwrap();
        assert!(
            matches!(result, Outcome::Completed(Value::Number(n)) if n == if selected { 8. } else { 9. })
        );
    }
}

#[test]
fn if_chain_long_machine_runs_on_small_stack() {
    std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(|| {
            let source = format!(
                "function* f(){{{}return 17;}}",
                "if(false){}else ".repeat(1200) + "{}"
            );
            let Stmt::FunctionDefinition(_, Expr::Function { body, .. }) =
                crate::parser::parse(&source).unwrap().remove(0)
            else {
                panic!()
            };
            let mut runtime = machine(body, FunctionKind::Generator);
            assert!(matches!(
                runtime
                    .resumable_resume(0, ResumeInput::Normal(Value::Undefined), &mut Empty)
                    .unwrap(),
                Outcome::Completed(Value::Number(17.))
            ));
        })
        .unwrap()
        .join()
        .unwrap();
}
