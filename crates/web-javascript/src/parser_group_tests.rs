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
        .stack_size(1024 * 1024)
        .spawn(test)
        .unwrap()
        .join()
        .unwrap();
}
fn run(source: &str) -> Value {
    Runtime::default().execute(source, &mut Empty).unwrap()
}

#[test]
fn grouped_call_arguments_no_longer_double_native_depth() {
    small_stack(|| {
        let body = format!("{}7{}", "identity((".repeat(40), "))".repeat(40));
        let source = format!(
            "function identity(x){{return x;}}function outer(){{return (function(){{return {body};}})();}}outer();"
        );
        assert_eq!(run(&source), Value::Number(7.));
        let tree = parse(&source).unwrap();
        drop(tree.clone());
        drop(tree);
        parse_module(&source).unwrap();
        parse_dynamic_function("", &source).unwrap();
    });
}

#[test]
fn group_resume_preserves_precedence_associativity_targets_and_receivers() {
    for (source, expected) in [
        ("((1)+2)*3", Value::Number(9.)),
        ("(1+(2))*3", Value::Number(9.)),
        ("(1+2*3)", Value::Number(7.)),
        ("((8)-3)-2", Value::Number(3.)),
        ("(8)-(3-2)", Value::Number(7.)),
        ("(2)**(3**2)", Value::Number(512.)),
        ("((false)?1:2)+3", Value::Number(5.)),
        ("var a=0,b=0;((a)=(b)=4);a+b", Value::Number(8.)),
        ("var a=1;((a))++;a", Value::Number(2.)),
        (
            "var o={x:7,f(){return this.x;}};((o.f))()",
            Value::Number(7.),
        ),
        ("function C(){this.x=7;}((new C()).x)", Value::Number(7.)),
        ("var o={x:3};delete ((o.x));!('x' in o)", Value::Bool(true)),
        ("((a=(2+3))=>a)()", Value::Number(5.)),
        ("((x=>x+1))(2)", Value::Number(3.)),
        ("var a=0;((false)&&((a=1)));a", Value::Number(0.)),
        ("var a=0;((true)||((a=1)));a", Value::Number(0.)),
        ("(0,(1,2))", Value::Number(2.)),
    ] {
        assert_eq!(run(source), expected, "{source}");
    }
}

#[test]
fn grouped_arrows_asi_yield_and_templates_keep_their_grammar() {
    for source in [
        "((a)\n=>a)",
        "((a)/*\n*/=>a)",
        "((a,,b)=>a)",
        "((a))=>a",
        "()",
        "(1,)",
        "((1)",
        "(1))",
    ] {
        assert_eq!(
            parse(source).unwrap_err().kind,
            ErrorKind::Syntax,
            "{source}"
        );
    }
    assert_eq!(run("var a=1;((a))\n++a;a"), Value::Number(2.));
    assert_eq!(
        run("function* f(){return ((yield 3)+1);}var g=f();g.next();g.next(6).value"),
        Value::Number(7.)
    );
    assert_eq!(run("var a=3;`x${((a)+4)}`"), Value::string("x7"));
    assert_eq!(run("((a=/[(]/)=>a.test('('))()"), Value::Bool(true));
}

#[test]
fn redundant_groups_are_iterative_but_still_consume_parser_work() {
    small_stack(|| {
        let count = MAX_GROUP_VISITS;
        let source = format!("{}7{}", "(".repeat(count), ")".repeat(count));
        assert_eq!(run(&source), Value::Number(7.));
        let tree = parse(&source).unwrap();
        drop(tree.clone());
        drop(tree);
        let source = format!("({source})");
        let error = parse(&source).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert_eq!(error.message, "Parser group visit budget exceeded");
    });
}

#[test]
fn resumed_groups_validate_each_changed_tree_before_more_composition() {
    small_stack(|| {
        let source = format!("{}a{}", "(".repeat(64), ").x".repeat(64));
        let error = parse(&source).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert_eq!(error.message, "Expression tree nesting budget exceeded");
        for source in [
            format!("{}1{}", "[".repeat(64), "]".repeat(64)),
            format!("{};{}", "{".repeat(64), "}".repeat(64)),
            format!("let {}x{}=[];", "[".repeat(64), "]".repeat(64)),
        ] {
            assert_eq!(parse(&source).unwrap_err().kind, ErrorKind::Resource);
        }
    });
}

#[test]
fn many_groups_around_wide_ast_do_not_build_native_or_auxiliary_stacks() {
    small_stack(|| {
        let array = format!("[{}1]", "1,".repeat(1023));
        let source = format!("{}{}{}.length", "(".repeat(8192), array, ")".repeat(8192));
        assert_eq!(run(&source), Value::Number(1024.));
        let tree = parse(&source).unwrap();
        drop(tree.clone());
        drop(tree);
        // Function bodies still retain their native parser frames. Group
        // elimination must never reset their shared safety guard.
        let source = format!("{}1{}", "function() {return ".repeat(32), ";}".repeat(32));
        let error = parse(&format!("var f={source};")).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
    });
}
