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
// Only shallow fixtures are expanded: the production evaluator never rebuilds tails.
fn expand(expression: Expr) -> Expr {
    match expression {
        Expr::ConditionalChain(chain) => chain.clauses_rev().iter().fold(
            expand(chain.fallback().clone()),
            |tail, (test, yes)| {
                Expr::Conditional(
                    Box::new(expand(test.clone())),
                    Box::new(expand(yes.clone())),
                    Box::new(tail),
                )
            },
        ),
        Expr::Conditional(a, b, c) => Expr::Conditional(
            Box::new(expand(*a)),
            Box::new(expand(*b)),
            Box::new(expand(*c)),
        ),
        Expr::Binary(op, a, b) => Expr::Binary(op, Box::new(expand(*a)), Box::new(expand(*b))),
        Expr::Call(callee, args) => Expr::Call(
            Box::new(expand(*callee)),
            args.into_iter().map(expand).collect(),
        ),
        Expr::Await(value) => Expr::Await(Box::new(expand(*value))),
        Expr::Yield(value) => Expr::Yield(value.map(|value| Box::new(expand(*value)))),
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
fn conditional_chain_resumable_values_ticks_effects_and_gc_match_legacy() {
    for (prefix, suspend, kind) in [
        ("function*", "yield", FunctionKind::Generator),
        ("async function", "await", FunctionKind::Async),
        ("async function*", "yield", FunctionKind::AsyncGenerator),
        ("async function*", "await", FunctionKind::AsyncGenerator),
    ] {
        for selected in 0..3 {
            let source = format!(
                "{prefix} f(){{let n=0;return (n+=1,{suspend} 0)?(n+=10,{suspend} 10):(n+=2,{suspend} 1)?(n+=20,{suspend} 20):(n+=3,{suspend} 30);}}"
            );
            let (flat, legacy) = bodies(&source);
            let run = |body| {
                let mut runtime = machine(body, kind);
                let mut input = ResumeInput::Normal(Value::Undefined);
                let mut results = Vec::new();
                for _ in 0..12 {
                    let outcome = runtime.resumable_resume(0, input, &mut Empty).unwrap();
                    let done = matches!(outcome, Outcome::Completed(_));
                    input = ResumeInput::Normal(match &outcome {
                        // Async generators await their yield operand before yielding it.
                        Outcome::Await(value) if suspend == "yield" => value.clone(),
                        Outcome::Await(Value::Number(n)) | Outcome::Yield(Value::Number(n)) => {
                            if *n < 2. {
                                Value::Bool(*n == selected as f64)
                            } else {
                                Value::Number(99.)
                            }
                        }
                        _ => Value::Undefined,
                    });
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
            assert_eq!(run(flat), run(legacy), "{source}; selected={selected}");
        }
    }
}

#[test]
fn conditional_chain_resource_phases_and_abrupt_resume_match_legacy() {
    for (prefix, suspend, kind) in [
        ("function*", "yield", FunctionKind::Generator),
        ("async function", "await", FunctionKind::Async),
    ] {
        let source = format!(
            "{prefix} f(){{let n=0;try{{return (n+=1,{suspend} 0)?(n+=10):(n+=2,{suspend} 0)?(n+=20):(n+=3,{suspend} 0);}}catch(e){{n=1000;}}finally{{n+=10000;}}}}"
        );
        let (flat, legacy) = bodies(&source);
        for phase in 0..4 {
            for budget in 0..120 {
                let run = |body| {
                    let mut runtime = machine(body, kind);
                    for _ in 0..phase {
                        assert!(matches!(
                            runtime
                                .resumable_resume(
                                    0,
                                    ResumeInput::Normal(Value::Bool(false)),
                                    &mut Empty
                                )
                                .unwrap(),
                            Outcome::Yield(_) | Outcome::Await(_)
                        ));
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
                    "phase={phase}; budget={budget}"
                );
            }
        }
        for input in [
            ResumeInput::Return(Value::Number(99.)),
            ResumeInput::Throw(Value::Number(9.)),
        ] {
            for phase in 1..4 {
                let run = |body| {
                    let mut runtime = machine(body, kind);
                    for _ in 0..phase {
                        runtime
                            .resumable_resume(
                                0,
                                ResumeInput::Normal(Value::Bool(false)),
                                &mut Empty,
                            )
                            .unwrap();
                    }
                    let result = normalized(runtime.resumable_resume(0, input.clone(), &mut Empty));
                    (result, runtime.remaining, binding(&runtime))
                };
                assert_eq!(run(flat.clone()), run(legacy.clone()));
            }
        }
    }
}

#[test]
fn conditional_chain_errors_and_unselected_errors_match_legacy_for_every_budget() {
    for expression in [
        "true?7:missing?8:other",
        "false?missing:true?9:other",
        "false?missing:false?other:11",
        "false?1:missing?2:3",
        "false?1:true?missing:3",
        "false?1:false?2:missing",
    ] {
        let (flat, legacy) = bodies(&format!("function* f(){{return {expression};}}"));
        for budget in 0..80 {
            let run = |body| {
                let mut runtime = machine(body, FunctionKind::Generator);
                runtime.remaining = budget;
                let result = normalized(runtime.resumable_resume(
                    0,
                    ResumeInput::Normal(Value::Undefined),
                    &mut Empty,
                ));
                (result, runtime.remaining)
            };
            assert_eq!(
                run(flat.clone()),
                run(legacy.clone()),
                "{expression}; budget={budget}"
            );
        }
    }
}

#[test]
fn conditional_chain_validator_interleaves_virtual_work_and_keeps_direct_boundary() {
    for source in [
        "function* f(){return (yield 0)?(yield 1):(yield 2)?(yield 3):(yield 4);}",
        "async function f(){return false?1:false?(await 2):3;}",
    ] {
        let (flat, legacy) = bodies(source);
        let Stmt::Return(Some(flat)) = &flat[0] else {
            panic!()
        };
        let Stmt::Return(Some(legacy)) = &legacy[0] else {
            panic!()
        };
        for direct in [false, true] {
            for allowance in 0..30 {
                let run = |expr| {
                    let mut validator = Validator {
                        kind: FunctionKind::Generator,
                        work: 65_536 - allowance,
                    };
                    let result = validator
                        .expression(expr, direct, 0)
                        .map_err(|e| (e.kind, e.message));
                    (result, validator.work)
                };
                assert_eq!(
                    run(flat),
                    run(legacy),
                    "{source}; direct={direct}; allowance={allowance}"
                );
            }
        }
    }
}

#[test]
fn conditional_chain_cursor_traces_enclosing_temporaries_across_gc() {
    for source in [
        "function* f(){return ({valueOf(){return 7;}})+(false?100:(yield 1)?100:2);}",
        "function* f(){return ({x:7,m(a){return this.x+a;}}).m(false?100:(yield 1)?100:2);}",
    ] {
        let (flat, legacy) = bodies(source);
        for body in [flat, legacy] {
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
                        .resumable_resume(0, ResumeInput::Normal(Value::Bool(false)), &mut Empty)
                        .unwrap(),
                    Outcome::Completed(Value::Number(9.))
                ),
                "{source}"
            );
        }
    }
}

#[test]
fn conditional_chain_cursor_depth_and_trace_match_evaluate_frame() {
    let (body, _) = bodies("function* f(){return false?1:false?2:3;}");
    let Stmt::Return(Some(Expr::ConditionalChain(chain))) = &body[0] else {
        panic!()
    };
    let next = Action::BinaryRight {
        operator: "+".into(),
        left: Value::Object(17),
        next: Box::new(Action::Return),
    };
    let action = Action::ConditionalChain {
        chain: chain.clone(),
        cursor: 0,
        next: Box::new(next.clone()),
    };
    assert_eq!(action.depth(), 3);
    assert_eq!(
        Frame::ConditionalChain(chain.clone(), 0, 0, next.clone()).depth(),
        Frame::Evaluate(chain.fallback().clone(), 0, next).depth()
    );
    let mut values = Vec::new();
    let mut scopes = Vec::new();
    action.trace(&mut values, &mut scopes);
    assert_eq!(values, [Value::Object(17)]);
    assert!(scopes.is_empty());
}

#[test]
fn conditional_chain_resource_never_runs_handlers_after_suspension() {
    let (body, _) = bodies(
        "function* f(){let n=0;try{return (yield 0)?(n=1):false?(n=2):(n=3);}catch(e){n=100;}finally{n=200;}}",
    );
    let mut runtime = machine(body, FunctionKind::Generator);
    runtime
        .resumable_resume(0, ResumeInput::Normal(Value::Undefined), &mut Empty)
        .unwrap();
    runtime.remaining = 0;
    let error = runtime
        .resumable_resume(0, ResumeInput::Normal(Value::Bool(false)), &mut Empty)
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert_eq!(binding(&runtime), Some(Value::Number(0.)));
    assert!(!runtime.resumable_exists(0));
}

#[test]
fn conditional_chain_host_effect_prefix_matches_legacy_at_every_budget() {
    #[derive(Default)]
    struct Effects(Vec<String>);
    impl Host for Effects {
        fn global(&mut self, name: &str) -> Option<Value> {
            (name == "bridge").then_some(Value::Host(1))
        }
        fn get(&mut self, _: u64, _: &str) -> Result<Value, JsError> {
            Ok(Value::Undefined)
        }
        fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
            panic!("Unexpected write")
        }
        fn call(&mut self, _: u64, method: &str, _: &[Value]) -> Result<Value, JsError> {
            self.0.push(method.to_owned());
            Ok(Value::Bool(method == "yes"))
        }
    }
    for selected in ["yes", "no"] {
        let (flat, legacy) = bodies(&format!(
            "function* f(){{return bridge.no()?bridge.bad():bridge.{selected}()?bridge.chosen():bridge.fallback();}}"
        ));
        for budget in 0..150 {
            let run = |body| {
                let mut runtime = machine(body, FunctionKind::Generator);
                let mut host = Effects::default();
                runtime.remaining = budget;
                let result = normalized(runtime.resumable_resume(
                    0,
                    ResumeInput::Normal(Value::Undefined),
                    &mut host,
                ));
                (result, runtime.remaining, host.0)
            };
            assert_eq!(
                run(flat.clone()),
                run(legacy.clone()),
                "{selected}; budget={budget}"
            );
        }
    }
}

#[test]
fn conditional_chain_resource_does_not_close_iterator_or_run_jobs() {
    let (body, _) = bodies(
        "function* f(){let n=0;let closed=0;let job=0;let it={next(){return {value:1,done:false};},return(){closed++;return {};}};it[Symbol.iterator]=function(){return this;};for(let x of it){try{Promise.resolve().then(function(){job++;});return (yield 0)?(n=1):false?(n=2):(n=3);}catch(e){n=100;}finally{n=200;}}}",
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
    for name in ["n", "closed", "job"] {
        assert_eq!(
            runtime.scopes[0].bindings[name].value,
            Value::Number(0.),
            "{name}"
        );
    }
    assert!(!runtime.resumable_exists(0));
}

// Small recursive oracle retained only for shallow differential fixtures.
struct LegacyValidator {
    kind: FunctionKind,
    work: usize,
}
impl LegacyValidator {
    fn step(&mut self, depth: usize) -> Result<(), JsError> {
        self.work += 1;
        if depth > LIMIT || self.work > 65_536 {
            return Err(JsError::resource("Continuation validation budget exceeded"));
        }
        Ok(())
    }
    // Dispatch compact owners before entering the larger legacy match frame.
    // Nested residual chains must not keep that frame alive at every level.
    #[inline(never)]
    fn expression(&mut self, expr: &Expr, direct: bool, depth: usize) -> Result<(), JsError> {
        self.step(depth)?;
        if let Expr::ConditionalChain(chain) = expr {
            self.conditional_chain(chain, direct, depth)
        } else {
            self.expression_inner(expr, direct, depth)
        }
    }
    #[inline(never)]
    fn expression_inner(&mut self, expr: &Expr, direct: bool, depth: usize) -> Result<(), JsError> {
        let d = depth + 1;
        match expr {
            Expr::Await(value) => {
                if !direct
                    || !matches!(
                        self.kind,
                        FunctionKind::Async | FunctionKind::AsyncGenerator
                    )
                {
                    return Err(unsupported());
                }
                self.expression(value, true, d)?;
            }
            Expr::YieldDelegate(value) => {
                if !direct
                    || !matches!(
                        self.kind,
                        FunctionKind::Generator | FunctionKind::AsyncGenerator
                    )
                {
                    return Err(unsupported());
                }
                self.expression(value, true, d)?;
            }
            Expr::Yield(value) => {
                if !direct
                    || !matches!(
                        self.kind,
                        FunctionKind::Generator | FunctionKind::AsyncGenerator
                    )
                {
                    return Err(unsupported());
                }
                if let Some(value) = value {
                    self.expression(value, true, d)?;
                }
            }
            Expr::Function { .. } => {}
            Expr::Class(class) => self.class(class, d)?,
            Expr::Array(values) => {
                for value in values {
                    if let Expr::Spread(operand) = value {
                        self.step(d)?;
                        self.expression(operand, direct, d + 1)?;
                    } else {
                        self.expression(value, direct, d)?;
                    }
                }
            }
            Expr::Template(values) => {
                for value in values {
                    self.expression(value, direct, d)?;
                }
            }
            Expr::Object(values) => {
                for (_, value) in values {
                    self.expression(value, false, d)?;
                }
            }
            Expr::Assign(operator, target, right) => {
                let allowed = direct && resumable_assignment(operator, target);
                self.expression(target, allowed && ordinary_member_reference(target), d)?;
                self.expression(right, allowed, d)?;
            }
            Expr::Sequence(sequence) => {
                // Preserve logical validation work even though physical
                // ownership no longer retains the left Binary ancestors.
                for _ in 0..sequence.removed_binary_nodes() {
                    self.step(depth)?;
                }
                for operand in sequence.items() {
                    self.expression(operand, direct, d)?;
                }
            }
            Expr::Binary(operator, a, b) => {
                let allowed = direct && resumable_binary(operator);
                self.expression(a, allowed, d)?;
                self.expression(b, allowed, d)?;
            }
            Expr::Member(a, b) => {
                let allow = direct && !matches!(a.as_ref(), Expr::Super);
                self.expression(a, allow, d)?;
                self.expression(b, allow, d)?;
            }
            Expr::ImportCall { specifier, options } => {
                self.expression(specifier, false, d)?;
                if let Some(options) = options {
                    self.expression(options, false, d)?;
                }
            }
            Expr::Call(callee, args) => {
                let allow = direct && resumable_call(callee, args);
                self.expression(callee, allow, d)?;
                for arg in args {
                    self.expression(arg, allow, d)?;
                }
            }
            Expr::Construct(callee, args) => {
                self.expression(callee, false, d)?;
                for arg in args {
                    self.expression(arg, false, d)?;
                }
            }
            Expr::Unary(operator, value) => {
                let allow = resumable_unary(operator)
                    || (operator == "delete" && ordinary_member_reference(value));
                self.expression(value, direct && allow, d)?
            }
            Expr::Update(value, _, _) => {
                self.expression(value, direct && ordinary_member_reference(value), d)?
            }
            Expr::Spread(value) | Expr::PrivateMember(value, _) => {
                self.expression(value, false, d)?
            }
            Expr::ConditionalChain(_) => unreachable!("Compact chains dispatch in expression"),
            Expr::Conditional(a, b, c) => {
                self.expression(a, direct, d)?;
                self.expression(b, direct, d)?;
                self.expression(c, direct, d)?;
            }
            Expr::ImportMeta
            | Expr::Literal(_)
            | Expr::RegExp(..)
            | Expr::Name(_)
            | Expr::Hole
            | Expr::Super => {}
        }
        Ok(())
    }
    // Keep chain iteration state out of every recursive legacy expression frame.
    #[inline(never)]
    fn conditional_chain(
        &mut self,
        chain: &ConditionalChain,
        direct: bool,
        depth: usize,
    ) -> Result<(), JsError> {
        let d = depth + 1;
        for (index, (condition, yes)) in chain.clauses_rev().iter().rev().enumerate() {
            // expression already charged the outer entry. Visit each virtual
            // no Conditional only after the preceding source children.
            if index != 0 {
                self.step(depth)?;
            }
            self.expression(condition, direct, d)?;
            self.expression(yes, direct, d)?;
        }
        self.expression(chain.fallback(), direct, d)
    }
    fn class(&mut self, class: &ClassDefinition, depth: usize) -> Result<(), JsError> {
        self.step(depth)?;
        if let Some(base) = &class.base {
            self.expression(base, false, depth + 1)?;
        }
        for member in &class.members {
            if let Some(value) = &member.value {
                self.expression(value, false, depth + 1)?;
            }
        }
        Ok(())
    }
}

#[test]
fn iterative_expression_validator_matches_recursive_oracle_all_families() {
    let mut expressions = Vec::new();
    for source in [
        "1",
        "/a/g",
        "name",
        "[1,...xs,,(yield 2)]",
        "({x:1,y:2})",
        "obj[yield 1]",
        "f(1,yield 2,3)",
        "f(...xs)",
        "new F(1,2)",
        "await 1",
        "yield 1",
        "yield",
        "yield* xs",
        "typeof name",
        "delete obj.x",
        "1+(yield 2)",
        "a&&(yield 2)",
        "(1,2,yield 3,4)",
        "obj[yield 1]=yield 2",
        "obj.x+=yield 1",
        "obj[yield 1]++",
        "true?(yield 1):(yield 2)",
        "false?(yield 1):false?(yield 2):(yield 3)",
        "`a${yield 1}b${2}`",
        "class C extends Base {x=1; y=2; m(){return 3;}}",
        "function nested(){return 1;}",
    ] {
        let (body, _) = bodies(&format!("async function* f(){{return ({source});}}"));
        let Stmt::Return(Some(expr)) = &body[0] else {
            panic!("{source}")
        };
        expressions.push(expr.clone());
    }
    expressions.extend([
        // Invalid suspension locations are constructed directly: parsing an
        // async/generator body validates and rejects these before the oracle.
        Expr::Object(vec![
            ("x".into(), Expr::Literal(Value::Number(1.))),
            (
                "y".into(),
                Expr::Yield(Some(Box::new(Expr::Literal(Value::Number(2.))))),
            ),
        ]),
        Expr::Construct(Box::new(Expr::Name("F".into())), vec![Expr::Yield(None)]),
        Expr::Call(
            Box::new(Expr::Name("f".into())),
            vec![
                Expr::Spread(Box::new(Expr::Name("xs".into()))),
                Expr::Yield(None),
            ],
        ),
        Expr::Assign(
            "??=".into(),
            Box::new(Expr::Name("x".into())),
            Box::new(Expr::Yield(None)),
        ),
        Expr::ImportMeta,
        Expr::ImportCall {
            specifier: Box::new(Expr::Literal(Value::string("x"))),
            options: None,
        },
        Expr::Super,
        Expr::Hole,
        Expr::Spread(Box::new(Expr::Yield(None))),
        Expr::PrivateMember(Box::new(Expr::Yield(None)), "secret".into()),
        Expr::Member(Box::new(Expr::Super), Box::new(Expr::Yield(None))),
        Expr::ImportCall {
            specifier: Box::new(Expr::Literal(Value::string("x"))),
            options: Some(Box::new(Expr::Yield(None))),
        },
        Expr::Class(ClassDefinition {
            name: None,
            base: Some(Box::new(Expr::Yield(None))),
            members: Vec::new(),
        }),
        Expr::Class(ClassDefinition {
            name: None,
            base: None,
            members: vec![crate::parser::ClassMember {
                name: "x".into(),
                value: Some(Expr::Await(Box::new(Expr::Literal(Value::Number(1.))))),
                static_member: false,
                private: false,
                method: false,
                accessor: None,
            }],
        }),
    ]);
    for (index, expr) in expressions.iter().enumerate() {
        for kind in [
            FunctionKind::Ordinary,
            FunctionKind::Async,
            FunctionKind::Generator,
            FunctionKind::AsyncGenerator,
        ] {
            for direct in [false, true] {
                for depth in [0, LIMIT - 2, LIMIT - 1, LIMIT, LIMIT + 1] {
                    for allowance in 0..24 {
                        let mut iterative = Validator {
                            kind,
                            work: 65_536 - allowance,
                        };
                        let mut legacy = LegacyValidator {
                            kind,
                            work: 65_536 - allowance,
                        };
                        let actual = iterative
                            .expression(expr, direct, depth)
                            .map_err(|e| (e.kind, e.message));
                        let expected = legacy
                            .expression(expr, direct, depth)
                            .map_err(|e| (e.kind, e.message));
                        assert_eq!(
                            (actual, iterative.work),
                            (expected, legacy.work),
                            "fixture={index}, kind={kind:?}, direct={direct}, depth={depth}, allowance={allowance}"
                        );
                    }
                }
            }
        }
        if let Expr::Class(class) = expr {
            for depth in [0, LIMIT - 1, LIMIT, LIMIT + 1] {
                for allowance in 0..24 {
                    let mut iterative = Validator {
                        kind: FunctionKind::AsyncGenerator,
                        work: 65_536 - allowance,
                    };
                    let mut legacy = LegacyValidator {
                        kind: FunctionKind::AsyncGenerator,
                        work: 65_536 - allowance,
                    };
                    let actual = iterative
                        .class(class, depth)
                        .map_err(|e| (e.kind, e.message));
                    let expected = legacy.class(class, depth).map_err(|e| (e.kind, e.message));
                    assert_eq!(
                        (actual, iterative.work),
                        (expected, legacy.work),
                        "class fixture={index}, depth={depth}, allowance={allowance}"
                    );
                }
            }
        }
    }
}

#[test]
fn iterative_expression_validation_scratch_is_bounded_and_width_independent() {
    assert_eq!(LIMIT, 256); // Continuation limit, distinct from parser residual 64.
    assert_eq!(VALIDATION_WORK_SLOTS, 773);
    let slot_bytes = std::mem::size_of::<ValidationWork<'_>>();
    assert!(slot_bytes <= 48);
    let mut stack = ValidationStack::new().unwrap();
    let capacity = stack.0.capacity();
    assert_eq!(capacity, VALIDATION_WORK_SLOTS);
    for _ in 0..VALIDATION_WORK_SLOTS {
        stack
            .push(ValidationWork::Expressions(&[], true, 0, false))
            .unwrap();
    }
    assert_eq!(stack.0.capacity(), capacity);
    assert_eq!(
        stack
            .push(ValidationWork::Expressions(&[], true, 0, false))
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
    eprintln!(
        "validation scratch: {VALIDATION_WORK_SLOTS} slots, {slot_bytes} bytes/slot, {} requested bytes, {capacity} capacity",
        VALIDATION_WORK_SLOTS * slot_bytes
    );
    let wide = Expr::Array(
        (0..4096)
            .map(|_| Expr::Literal(Value::Number(1.)))
            .collect(),
    );
    let mut validator = Validator {
        kind: FunctionKind::Generator,
        work: 0,
    };
    validator.expression(&wide, true, 0).unwrap();
    assert_eq!(validator.work, 4097);
}
