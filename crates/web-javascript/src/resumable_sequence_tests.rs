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
fn expand(expression: Expr) -> Expr {
    match expression {
        Expr::Sequence(sequence) => {
            let mut operands = sequence.items().iter().cloned();
            let head = operands.next().unwrap();
            operands.fold(head, |left, right| {
                Expr::Binary(",".into(), Box::new(left), Box::new(right))
            })
        }
        expr => expr,
    }
}
fn expand_statement(statement: Stmt) -> Stmt {
    match statement {
        Stmt::Return(Some(expr)) => Stmt::Return(Some(expand(expr))),
        Stmt::Expression(expr) => Stmt::Expression(expand(expr)),
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
fn sequence_resumable_results_ticks_and_effects_match_manual_binary_all_kinds() {
    for (source, kind) in [
        (
            "function* f(){let n=0;return(n=1,yield n,n=3,yield n,n=5);}",
            FunctionKind::Generator,
        ),
        (
            "async function f(){let n=0;return(n=1,await n,n=3,await n,n=5);}",
            FunctionKind::Async,
        ),
        (
            "async function* f(){let n=0;return(n=1,yield n,n=3,await n,n=5);}",
            FunctionKind::AsyncGenerator,
        ),
    ] {
        let (flat, binary) = bodies(source);
        let trace = |body| {
            let mut runtime = machine(body, kind);
            let mut input = ResumeInput::Normal(Value::Undefined);
            let mut output = Vec::new();
            for _ in 0..10 {
                let outcome = runtime.resumable_resume(0, input, &mut Empty).unwrap();
                let done = matches!(outcome, Outcome::Completed(_));
                input = ResumeInput::Normal(match &outcome {
                    Outcome::Await(value) => value.clone(),
                    _ => Value::Number(9.),
                });
                output.push((format!("{outcome:?}"), runtime.remaining, binding(&runtime)));
                if done {
                    return output;
                }
            }
            panic!("Unexpected continuation loop")
        };
        assert_eq!(trace(flat), trace(binary), "{source}");
    }
}
#[test]
fn sequence_resumable_resource_effect_prefix_and_abrupt_resume_match_binary() {
    let source = "function* f(){let n=0;try{return(n=1,yield n,n=3,yield n,n=5);}finally{n+=10;}}";
    let (flat, binary) = bodies(source);
    for phase in 0..3 {
        for budget in 0..100 {
            let run = |body| {
                let mut runtime = machine(body, FunctionKind::Generator);
                for _ in 0..phase {
                    assert!(matches!(
                        runtime
                            .resumable_resume(0, ResumeInput::Normal(Value::Undefined), &mut Empty)
                            .unwrap(),
                        Outcome::Yield(_)
                    ));
                }
                runtime.remaining = budget;
                let result = normalized(runtime.resumable_resume(
                    0,
                    ResumeInput::Normal(Value::Number(9.)),
                    &mut Empty,
                ));
                (result, runtime.remaining, binding(&runtime))
            };
            assert_eq!(
                run(flat.clone()),
                run(binary.clone()),
                "phase={phase}, budget={budget}"
            );
        }
    }
    for input in [
        ResumeInput::Return(Value::Number(99.)),
        ResumeInput::Throw(Value::Number(9.)),
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
        assert_eq!(run(flat.clone()), run(binary.clone()));
    }
}
#[test]
fn sequence_validation_preserves_logical_work_and_failure_phase() {
    let (flat, binary) = bodies("function* f(){return(1,yield 2,3,4);}");
    let Stmt::Return(Some(flat)) = &flat[0] else {
        panic!()
    };
    let Stmt::Return(Some(binary)) = &binary[0] else {
        panic!()
    };
    for allowance in 0..20 {
        let run = |expr| {
            let mut validator = Validator {
                kind: FunctionKind::Generator,
                work: 65_536 - allowance,
            };
            let result = validator
                .expression(expr, true, 0)
                .map_err(|error| (error.kind, error.message));
            (result, validator.work)
        };
        assert_eq!(run(flat), run(binary), "allowance={allowance}");
    }
}
#[test]
fn sequence_cursor_traces_enclosing_values_and_call_receivers_across_collection() {
    for source in [
        "function* f(){return ({valueOf(){return 7;}})+(0,yield 1,2);}",
        "function* f(){return ({x:7,m(a){return this.x+a;}}).m((0,yield 1,2));}",
    ] {
        let (body, _) = bodies(source);
        let mut runtime = machine(body, FunctionKind::Generator);
        assert!(matches!(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Undefined), &mut Empty)
                .unwrap(),
            Outcome::Yield(Value::Number(1.))
        ));
        runtime
            .collect_garbage(&[Value::Object(0)], &Empty)
            .unwrap();
        assert!(
            matches!(
                runtime
                    .resumable_resume(0, ResumeInput::Normal(Value::Number(99.)), &mut Empty)
                    .unwrap(),
                Outcome::Completed(Value::Number(9.))
            ),
            "{source}"
        );
    }
}
#[test]
fn sequence_cursor_counts_one_action_and_does_not_retain_discarded_value() {
    let (body, _) = bodies("function* f(){return(1,yield 2,3);}");
    let Stmt::Return(Some(Expr::Sequence(sequence))) = &body[0] else {
        panic!()
    };
    let action = Action::Sequence {
        sequence: sequence.clone(),
        index: 1,
        next: Box::new(Action::Return),
    };
    assert_eq!(action.depth(), 2);
    let mut values = Vec::new();
    let mut scopes = Vec::new();
    action.trace(&mut values, &mut scopes);
    assert!(values.is_empty() && scopes.is_empty());
    let mut runtime = machine(body, FunctionKind::Generator);
    runtime.remaining = 3; // loop + expression + first removed-ancestor frame tick
    let error = runtime
        .resumable_resume(0, ResumeInput::Normal(Value::Undefined), &mut Empty)
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
}
