use super::*;
use crate::{ErrorKind, Host, Runtime};

struct Empty;
impl Host for Empty {
    fn global(&mut self, _: &str) -> Option<Value> {
        None
    }
    fn get(&mut self, _: u64, _: &str) -> Result<Value, JsError> {
        panic!("unexpected host read")
    }
    fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
        panic!("unexpected host write")
    }
    fn call(&mut self, _: u64, _: &str, _: &[Value]) -> Result<Value, JsError> {
        panic!("unexpected host call")
    }
}
fn small_stack(test: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .name("pratt-continuations-1mib".into())
        .stack_size(1024 * 1024)
        .spawn(test)
        .unwrap()
        .join()
        .unwrap();
}
fn make_parser(source: &str) -> Parser {
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
        labels: Vec::new(),
        visits: 0,
        group_visits: 0,
        flat_storage_bytes: 0,
    }
}
fn resource(source: &str, expected: &str) {
    let error = parse(source).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource, "{source}");
    assert_eq!(error.message, expected, "{source}");
}

#[test]
fn logical_children_no_longer_consume_native_expression_frames() {
    small_stack(|| {
        for (open, close) in [
            ("!", ""),
            ("f(", ")"),
            ("a[", "]"),
            ("a=", ""),
            ("2**", ""),
            ("false?0:", ""),
            ("true?", ":0"),
        ] {
            let source = format!("{}1{}", open.repeat(63), close.repeat(63));
            let tree = parse(&source).unwrap();
            drop(tree.clone());
            drop(tree);
            parse_module(&source).unwrap();
        }
        // There are 63 expression nodes, but v7 also counted native statement
        // frames and rejected the innermost argument at depth 65.
        let calls = format!("{}1{}", "f((".repeat(60), "))".repeat(60));
        let source = format!("function outer(){{return function inner(){{return {calls};}}}}");
        let tree = parse(&source).unwrap();
        drop(tree.clone());
        drop(tree);
        parse_module(&source).unwrap();
        parse_dynamic_function("", &source).unwrap();
    });
}

#[test]
fn continuation_limit_is_shared_across_complex_atom_reentry() {
    small_stack(|| {
        let outer = "!".repeat(40);
        let inner = "!".repeat(30);
        for source in [
            format!("{outer}function(){{return {inner}1;}}"),
            format!("{outer}[{inner}1]"),
            format!("{outer}{{x:{inner}1}}"),
            format!("{outer}class {{x={inner}1;}}"),
            format!("{outer}(x={inner}1)=>x"),
            format!("{outer}new F({inner}1)"),
            format!("{outer}`x${{{inner}1}}`"),
        ] {
            let mut parser = make_parser(&source);
            let error = parser.expression(0).unwrap_err();
            assert_eq!(error.kind, ErrorKind::Resource, "{source}");
            assert_eq!(
                error.message, "Expression nesting budget exceeded",
                "{source}"
            );
            assert_eq!(parser.expression_frames, 0, "{source}");
            assert_eq!(parser.depth, 0, "{source}");
        }
    });
}

#[test]
fn continuation_slots_return_after_success_and_all_error_paths() {
    small_stack(|| {
        for source in [
            "f(1+2)",
            "f(1+)",
            "f(1,)",
            "f(...)",
            "a[?]",
            "a[1",
            "!",
            "++1",
            "1=2",
            "true?1",
            "true?1:?",
            "f((1))",
            "function(){return f(?);}",
            "(x=f(?))=>x",
            "`x${f(?)}`",
        ] {
            let mut parser = make_parser(source);
            parser.expression_frames = 9;
            parser.depth = 3;
            let result = parser.expression(0);
            drop(result);
            assert_eq!(parser.expression_frames, 9, "{source}");
            assert_eq!(parser.depth, 3, "{source}");
            assert!(parser.visits > 0, "{source}");
        }
        let mut parser = make_parser("f(1+)");
        assert!(parser.expression(0).is_err());
        let consumed = parser.visits;
        parser.tokens = lex("g(2)").unwrap();
        parser.index = 0;
        parser.expression(0).unwrap();
        assert_eq!(parser.expression_frames, 0);
        assert_eq!(parser.visits, consumed + 2);
    });
}

#[test]
fn frame_and_visit_guards_precede_child_parsing_and_reservation() {
    small_stack(|| {
        for source in ["f(?)", "a[?]", "!?", "a+?", "true??", "true?1:?"] {
            let mut parser = make_parser(source);
            parser.expression_frames = 63;
            let error = parser.expression(0).unwrap_err();
            assert_eq!(error.kind, ErrorKind::Resource, "{source}");
            assert_eq!(
                error.message, "Expression nesting budget exceeded",
                "{source}"
            );
            assert_eq!(parser.expression_frames, 63);
            assert_eq!(parser.depth, 0);
            assert_eq!(parser.visits, 2);
        }
        let mut parser = make_parser("f(?)");
        parser.expression_frames = 63;
        parser.visits = MAX_PARSER_VISITS - 1;
        let error = parser.expression(0).unwrap_err();
        assert_eq!(error.message, "Parser visit budget exceeded");
        assert_eq!(parser.visits, MAX_PARSER_VISITS + 1);
        assert_eq!(parser.expression_frames, 63);
        parser.depth = 64;
        parser.visits = 0;
        parser.index = 0;
        let error = parser.expression(0).unwrap_err();
        assert_eq!(error.message, "Expression nesting budget exceeded");
        assert_eq!(parser.index, 0);
        assert_eq!(parser.depth, 64);
        assert_eq!(parser.expression_frames, 63);
    });
}

#[test]
fn group_and_child_visits_remain_cumulative() {
    let mut parser = make_parser("((f(1+2,...[3])))");
    parser.expression(0).unwrap();
    assert_eq!(parser.visits, 5);
    assert_eq!(parser.group_visits, 2);
    assert_eq!(parser.expression_frames, 0);
    let mut parser = make_parser("((f(1+2,...[?])))");
    assert!(parser.expression(0).is_err());
    assert_eq!(parser.visits, 5);
    assert_eq!(parser.group_visits, 2);
    assert_eq!(parser.expression_frames, 0);
}

#[test]
fn chain_budget_is_local_to_each_tail_and_survives_child_returns() {
    small_stack(|| {
        for tail in [".x", "()", "[0]", "+1"] {
            let source = format!("a{}", tail.repeat(63));
            parse(&source).unwrap();
            resource(
                &format!("a{}", tail.repeat(64)),
                "Expression chain budget exceeded",
            );
        }
        parse(&format!("(a{}){}", ".x".repeat(31), "[0]".repeat(32))).unwrap();
        resource(
            &format!("(a{}){}", ".x".repeat(32), "[0]".repeat(32)),
            "Expression tree nesting budget exceeded",
        );
        resource(
            &format!("{}1", "!".repeat(64)),
            "Expression nesting budget exceeded",
        );
    });
}

#[test]
fn partial_trees_drop_safely_on_syntax_and_resource_failures() {
    small_stack(|| {
        let checked = format!("1{}", "+1".repeat(63));
        for source in [
            format!("({checked}){}[?", ".x".repeat(61)),
            format!("true?{checked}:?"),
            format!("f({checked},?)"),
            format!("f(...({checked}),?)"),
        ] {
            assert_eq!(
                parse(&source).unwrap_err().kind,
                ErrorKind::Syntax,
                "{source}"
            );
        }
        for source in [
            format!("({checked}){}", ".x".repeat(61)),
            format!("true?{checked}:1"),
            format!("f({checked})"),
            format!("f(...({checked}))"),
        ] {
            resource(&source, "Expression tree nesting budget exceeded");
        }
    });
}

#[test]
fn pratt_continuations_preserve_evaluation_and_reference_order() {
    small_stack(|| {
        for (source, expected) in [
            ("1+2*3**2", Value::Number(19.)),
            ("2**3**2", Value::Number(512.)),
            ("var a,b,c;a=b=c=4;a+b+c", Value::Number(12.)),
            ("false?1:true?2:3", Value::Number(2.)),
            ("true?false?1:2:3", Value::Number(2.)),
            ("var a=0;false?(a=1):(a=2);a", Value::Number(2.)),
            ("var a=0;false&&(a=1);true||(a=2);a", Value::Number(0.)),
            (
                "var o={x:7,f(){return this.x;}};((o.f))()",
                Value::Number(7.),
            ),
            ("function f(a,b){return a+b;}f((1,2),3)", Value::Number(5.)),
            ("function f(a,b){return a+b;}f(...[1,2])", Value::Number(3.)),
            ("var a=1;a\n++a;a", Value::Number(2.)),
            ("((a=2)=>a)()", Value::Number(2.)),
            (
                "function* f(){return (yield)+1;}var g=f();g.next();g.next(2).value",
                Value::Number(3.),
            ),
            (
                "function* f(){return yield* [1,2];}f().next().value",
                Value::Number(1.),
            ),
            (
                "function* f(){for(yield;false;){};}f().next().done",
                Value::Bool(false),
            ),
            ("var a=3;`x${((a)+4)}`", Value::string("x7")),
            (
                "var s='';var o={};Object.defineProperty(o,'x',{get(){s+='x';return 1;},set(v){s+='s';}});function r(){s+='r';return 2;}o.x+=r();s",
                Value::string("xrs"),
            ),
        ] {
            assert_eq!(
                Runtime::default().execute(source, &mut Empty).unwrap(),
                expected,
                "{source}"
            );
        }
    });
}

#[test]
fn pratt_continuations_keep_syntax_error_offsets_and_argument_limits() {
    small_stack(|| {
        for source in [
            "()", "(1,)", "(1+)", "f(1,)", "f(,1)", "a[1", "1?2", "1?2:", "++1", "1++", "1=2",
            "((a))=>a", "(a)\n=>a", "await 1", "yield 1",
        ] {
            let error = parse(source).unwrap_err();
            assert_eq!(error.kind, ErrorKind::Syntax, "{source}");
            assert!(error.offset.is_some(), "{source}");
        }
        let source = format!("f({})", vec!["1"; 64].join(","));
        parse(&source).unwrap();
        resource(
            &format!("f({},?)", vec!["1"; 64].join(",")),
            "Call argument limit exceeded",
        );
        let error = parse("1=(?").unwrap_err();
        assert_eq!(error.message, "Expected expression");
        assert_eq!(error.offset, Some(3));
    });
}

#[test]
fn native_grammar_resource_guards_restore_the_entry_depth() {
    small_stack(|| {
        let mut parser = make_parser(&("new ".repeat(64) + "F"));
        let error = parser.expression(0).unwrap_err();
        assert_eq!(error.message, "Constructor nesting budget exceeded");
        assert_eq!(parser.depth, 0);
        assert_eq!(parser.expression_frames, 0);

        let mut parser = make_parser(&("{".repeat(64) + ";" + &"}".repeat(64)));
        let error = parser.statement().unwrap_err();
        assert_eq!(error.message, "Syntax nesting budget exceeded");
        assert_eq!(parser.depth, 0);
        assert_eq!(parser.expression_frames, 0);

        let mut parser = make_parser(&("[".repeat(64) + "x" + &"]".repeat(64)));
        let error = parser.pattern(false).unwrap_err();
        assert_eq!(error.message, "Binding pattern nesting exceeded");
        assert_eq!(parser.depth, 0);
        assert_eq!(parser.expression_frames, 0);
    });
}
