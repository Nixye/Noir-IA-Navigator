use super::*;
use crate::{ErrorKind, Host, Limits, Runtime};

pub(super) fn parser(source: &str) -> Parser {
    Parser {
        tokens: lex(source).unwrap(),
        index: 0,
        depth: 0,
        expression_frames: 0,
        function_depth: 0,
        function_kind: FunctionKind::Ordinary,
        module_context: false,
        loop_depth: 0,
        switch_depth: 0,
        labels: vec![],
        visits: 0,
        group_visits: 0,
        flat_storage_bytes: 0,
    }
}
fn chain(count: usize, condition: &str, body: &str) -> String {
    std::iter::repeat_n(format!("if({condition}){body}"), count)
        .collect::<Vec<_>>()
        .join("else ")
}
#[derive(Default)]
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
fn legacy(statement: &Stmt) -> Stmt {
    match statement {
        Stmt::IfChain(chain) => {
            let mut no = chain.fallback().map(|body| Box::new(legacy(body)));
            for (condition, body) in chain.clauses().iter().rev() {
                no = Some(Box::new(Stmt::If(
                    condition.clone(),
                    Box::new(legacy(body)),
                    no,
                )));
            }
            *no.unwrap()
        }
        Stmt::Block(body) => Stmt::Block(body.iter().map(legacy).collect()),
        statement => statement.clone(),
    }
}
fn normalized(
    result: Result<crate::Flow, JsError>,
) -> Result<String, (ErrorKind, String, Option<Value>)> {
    result
        .map(|flow| format!("{flow:?}"))
        .map_err(|error| (error.kind, error.message, error.thrown))
}
#[test]
fn if_chain_only_flattens_immediate_edges_and_preserves_dangling_else_and_asi() {
    let statement = parse("if(a);else if(b);else if(c);else;")
        .unwrap()
        .remove(0);
    let Stmt::IfChain(flat) = &statement else {
        panic!()
    };
    assert_eq!(flat.clauses().len(), 3);
    assert!(matches!(flat.fallback(), Some(Stmt::Empty)));
    for source in [
        "if(a);else {if(b);else if(c);}",
        "if(a);else label:if(b);else if(c);",
    ] {
        assert!(
            matches!(&parse(source).unwrap()[0], Stmt::If(..)),
            "{source}"
        );
    }
    let Stmt::If(_, yes, no) = parse("if(a)if(b);else if(c);else;").unwrap().remove(0) else {
        panic!()
    };
    assert!(matches!(*yes, Stmt::IfChain(_)));
    assert!(no.is_none());
    let Stmt::IfChain(flat) = parse("if(a)1\nelse if(b)2\nelse 3").unwrap().remove(0) else {
        panic!()
    };
    assert_eq!(flat.clauses().len(), 2);
    assert!(flat.fallback().is_some());
    let mut p = parser("if(0);else if(0);else if(0);else;");
    p.statement().unwrap();
    assert_eq!((p.visits, p.depth, p.expression_frames), (10, 0, 0));
}
#[test]
fn if_chain_sync_matches_legacy_results_ticks_effects_and_failure_phases() {
    for chosen in [0, 1, 2, 7] {
        for fallback in ["", "else n+=100;"] {
            let source = format!(
                "if((n+=1,{chosen}===0))n+=10;else if((n+=2,{chosen}===1))n+=20;else if((n+=4,{chosen}===2))n+=40;{fallback}"
            );
            let flat = parse(&source).unwrap().remove(0);
            let old = legacy(&flat);
            for budget in 0..85 {
                let run = |statement: &Stmt| {
                    let mut runtime = Runtime::default();
                    runtime.execute("var n=0", &mut Empty).unwrap();
                    runtime.remaining = budget;
                    let result = normalized(runtime.statement(statement, 0, &mut Empty));
                    assert_eq!((runtime.statement_depth, runtime.eval_depth), (0, 0));
                    (result, runtime.remaining, runtime.global_value("n"))
                };
                assert_eq!(
                    run(&flat),
                    run(&old),
                    "choice={chosen} budget={budget} source={source}"
                );
            }
        }
    }
    for source in [
        "if(0);else if(missing);else if(1)throw 9;",
        "if(0);else if(0);else if(1)throw 9;",
        "if(0);else if(0);else throw 9;",
    ] {
        let flat = parse(source).unwrap().remove(0);
        let old = legacy(&flat);
        let mut a = Runtime::default();
        let mut b = Runtime::default();
        assert_eq!(
            normalized(a.statement(&flat, 0, &mut Empty)),
            normalized(b.statement(&old, 0, &mut Empty))
        );
        assert_eq!(a.remaining, b.remaining);
    }
}
#[test]
fn if_chain_var_detection_keeps_work_order_early_exit_and_function_boundaries() {
    let flat = parse("if(0){var first;}else if(0){function f(){var hidden;}var middle;}else if(0){var last;}else {var finalName;}").unwrap().remove(0);
    let old = legacy(&flat);
    for name in ["first", "middle", "last", "finalName", "missing", "hidden"] {
        for remaining in 0..20 {
            let run = |statement: &Stmt| {
                let mut p = parser("");
                p.visits = MAX_PARSER_VISITS - remaining;
                let result = p
                    .contains_var_declaration(statement, name)
                    .map_err(|e| (e.kind, e.message));
                (result, p.visits)
            };
            assert_eq!(run(&flat), run(&old), "{name}, remaining={remaining}");
        }
    }
    for source in [
        "for(let x of [])if(0);else if(0);else var x;",
        "for(let x in {})if(0);else if(0)var x;",
    ] {
        assert_eq!(parse(source).unwrap_err().kind, ErrorKind::Syntax);
    }
    parse("for(let x of [])if(0);else if(0){function f(){var x;}}").unwrap();
}
#[test]
fn if_chain_hoisting_preserves_interleaved_ticks_order_and_scope_boundaries() {
    #[derive(Default)]
    struct Names(Vec<String>);
    impl Host for Names {
        fn global(&mut self, name: &str) -> Option<Value> {
            self.0.push(name.into());
            None
        }
        fn global_object(&self) -> Option<u64> {
            Some(1)
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
    let flat = parse(
        "if(0){var a;}else if(0){function f(){var hidden;}var b;}else if(0)var c;else var d;",
    )
    .unwrap();
    let old = flat.iter().map(legacy).collect::<Vec<_>>();
    for budget in 0..40 {
        let run = |statements: &[Stmt]| {
            let mut runtime = Runtime::new(Limits {
                instructions: budget,
                ..Limits::default()
            });
            let mut host = Names::default();
            let result = runtime
                .collect_vars(statements, 0, &mut host)
                .map_err(|e| (e.kind, e.message));
            (
                result,
                runtime.remaining,
                host.0,
                ["a", "b", "c", "d", "hidden"].map(|name| runtime.global_value(name)),
            )
        };
        assert_eq!(run(&flat), run(&old), "budget={budget}");
    }
}
#[test]
fn if_chain_loops_abrupt_completions_and_scopes_remain_distinct() {
    for (source, expected) in [
        (
            "let n=0;outer:for(let i=0;i<4;i++){if(i===0)continue;else if(i===1)continue outer;else if(i===2)n+=3;else break outer;}n",
            3.,
        ),
        (
            "let x=7;let n=0;if(0){let x=1;}else if(1){let x=2;n=x;}else {let x=3;}n+x",
            9.,
        ),
        (
            "function f(){if(0)return 1;else if(0)return 2;else return 3;}f()",
            3.,
        ),
        (
            "let n=0;try{if(0);else if(1)throw 4;else throw 9;}catch(e){n=e}finally{n+=2}n",
            6.,
        ),
        ("let n=0;if(1)if(0)n=1;else if(1)n=2;else n=3;n", 2.),
    ] {
        assert_eq!(
            Runtime::default().execute(source, &mut Empty).unwrap(),
            Value::Number(expected),
            "{source}"
        );
    }
}
#[test]
fn if_chain_width_parse_clone_drop_execution_and_residual_guards_fit_one_mib_stack() {
    std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(|| {
            let source = chain(4096, "0", ";");
            let tree = parse(&source).unwrap();
            let Stmt::IfChain(chain) = &tree[0] else {
                panic!()
            };
            assert_eq!(chain.clauses().len(), 4096);
            let clone = tree.clone();
            drop(tree);
            drop(clone);
            parse_module(&source).unwrap();
            parse_dynamic_function("", &source).unwrap();
            assert_eq!(
                Runtime::default().execute(&source, &mut Empty).unwrap(),
                Value::Undefined
            );
            assert_eq!(
                parse(&super::if_chain_tests::chain(4097, "0", ";"))
                    .unwrap_err()
                    .message,
                "If-chain length budget exceeded"
            );
            // A wide chain does not exempt its final condition or body from the
            // existing native/physical expression and statement depth guards.
            for source in [
                format!(
                    "{}else if({}1);",
                    super::if_chain_tests::chain(100, "0", ";"),
                    "!".repeat(64)
                ),
                format!(
                    "{}else if(1){};{}",
                    super::if_chain_tests::chain(100, "0", ";"),
                    "{".repeat(64),
                    "}".repeat(64)
                ),
                format!("{};{}", "if(1)".repeat(64), ""),
            ] {
                assert_eq!(parse(&source).unwrap_err().kind, ErrorKind::Resource);
            }
            let wide = super::if_chain_tests::chain(100, "0", ";");
            // Genuine containing blocks still consume native parser depth.
            let mixed = format!("{}{}{}", "{".repeat(60), wide, "}".repeat(60));
            let tree = parse(&mixed).unwrap();
            drop(tree.clone());
            drop(tree);
            Runtime::default().execute(&mixed, &mut Empty).unwrap();
            let mut nested = ";".to_string();
            for _ in 0..25 {
                nested = format!("if(0);else if(1){{{nested}}}");
            }
            let tree = parse(&nested).unwrap();
            drop(tree.clone());
            drop(tree);
            Runtime::default().execute(&nested, &mut Empty).unwrap();
            let mut nested = ";".to_string();
            for _ in 0..62 {
                nested = format!("if(0);else if(1){nested}");
            }
            let tree = parse(&nested).unwrap();
            drop(tree.clone());
            drop(tree);
            Runtime::default().execute(&nested, &mut Empty).unwrap();
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn if_chain_depth_walk_includes_conditions_and_retained_function_bodies() {
    let body = parse("if(0);else if(0);else;").unwrap();
    let mut expression = Expr::Function {
        kind: FunctionKind::Ordinary,
        method: false,
        name: None,
        params: Arc::new(Vec::new()),
        body: Arc::new(body),
        arrow: false,
        parameters: None,
    };
    assert_eq!(depth::expression_height(&expression).unwrap(), 2);
    for _ in 0..62 {
        expression = Expr::Unary("!".into(), Box::new(expression));
    }
    assert_eq!(depth::expression_height(&expression).unwrap(), 64);
    expression = Expr::Unary("!".into(), Box::new(expression));
    assert_eq!(
        depth::expression_height(&expression).unwrap_err().kind,
        ErrorKind::Resource
    );
}

#[test]
fn if_chain_invalid_reserved_labels_keep_statement_dispatch_and_cursor() {
    for (source, visits, allocated) in [
        ("if(0);else if:;", 4, false),
        ("if(0);else if(0);else if:;", 7, true),
        ("if(0);else if /* comment */ :;", 4, false),
    ] {
        let mut p = parser(source);
        let error = p.statement().unwrap_err();
        assert_eq!(error.kind, ErrorKind::Syntax);
        assert_eq!(error.message, "Reserved or unsupported binding name");
        assert_eq!(error.offset, Some(source.rfind("if").unwrap()));
        assert!(p.is("if"));
        assert_eq!(p.visits, visits);
        assert_eq!(p.flat_storage_bytes != 0, allocated);
        assert_eq!((p.depth, p.expression_frames), (0, 0));
    }
}
