use super::*;
use crate::{ErrorKind, Host, Runtime};

struct Empty;
impl Host for Empty {
    fn global(&mut self, _: &str) -> Option<Value> {
        None
    }
    fn get(&mut self, _: u64, _: &str) -> Result<Value, JsError> {
        panic!("Unexpected host read")
    }
    fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
        panic!("Unexpected host write")
    }
    fn call(&mut self, _: u64, _: &str, _: &[Value]) -> Result<Value, JsError> {
        panic!("Unexpected host call")
    }
}

#[test]
fn parenthesis_links_use_padding_and_clean_unmatched_openers() {
    if usize::BITS == 64 {
        assert_eq!(std::mem::size_of::<Token>(), 48);
    }
    let tokens = lex("(a(b)c)").unwrap();
    assert_eq!(tokens[0].close_paren, 6);
    assert_eq!(tokens[2].close_paren, 4);
    for index in [1, 3, 4, 5, 6, 7] {
        assert_eq!(tokens[index].close_paren, NO_TOKEN);
    }
    let tokens = lex(")((x)").unwrap();
    assert_eq!(tokens[1].close_paren, NO_TOKEN);
    assert_eq!(tokens[2].close_paren, 4);
    let tokens = lex("'('; /[(]/; `)`; (x)").unwrap();
    let openers: Vec<_> = tokens
        .iter()
        .enumerate()
        .filter(|(_, token)| matches!(token.kind, Kind::Symbol("(")))
        .collect();
    assert_eq!(openers.len(), 1);
    let (index, token) = openers[0];
    assert_eq!(token.close_paren as usize, index + 2);
    assert!(matches!(
        tokens[token.close_paren as usize].kind,
        Kind::Symbol(")")
    ));
}

#[test]
fn grouped_comma_expressions_do_not_consume_function_parameter_budget() {
    let names: Vec<_> = (0..34).map(|i| format!("p{i}")).collect();
    let declarations = names
        .iter()
        .enumerate()
        .map(|(i, name)| format!("{name}={i}"))
        .collect::<Vec<_>>()
        .join(",");
    let source = format!("var {declarations};({});", names.join(","));
    assert_eq!(
        Runtime::default().execute(&source, &mut Empty).unwrap(),
        Value::Number(33.)
    );
    for source in [
        format!("function f({}){{}}", names.join(",")),
        format!("({})=>0", names.join(",")),
    ] {
        parse(&source).unwrap();
    }
    let too_many = (0..65)
        .map(|i| format!("p{i}"))
        .collect::<Vec<_>>()
        .join(",");
    for source in [
        format!("function f({too_many}){{}}"),
        format!("({too_many})=>0"),
    ] {
        let error = parse(&source).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert_eq!(error.message, "Function parameter limit exceeded");
    }
}

#[test]
fn groups_do_not_build_and_discard_parameter_patterns() {
    let mut parser = Parser {
        tokens: lex("((value));").unwrap(),
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
    };
    parser.statement().unwrap();
    // One statement/expression and two separately charged groups.
    assert_eq!(parser.visits, 2);
    assert_eq!(parser.group_visits, 2);
}

#[test]
fn arrow_selection_preserves_nested_defaults_patterns_and_line_terminators() {
    for source in [
        "const f=(a=(2+3),[b]=[4])=>a+b;f()===9;",
        "const f=(a=(x=>x+1)(2))=>a;f()===3;",
        "const f=({a}={a:7},...rest)=>a+rest.length;f(undefined,1,2)===9;",
        "const f=(a=/[(]/,b=`)`)=>a.test('(')&&b===')';f();",
        "const f=(a=1)/* no line break */=>a;f()===1;",
    ] {
        assert_eq!(
            Runtime::default().execute(source, &mut Empty).unwrap(),
            Value::Bool(true),
            "{source}"
        );
    }
    for source in ["(a)\n=>a", "(a)/*\n*/=>a", "(a,,b)=>a", "((a))=>a", "(a"] {
        assert_eq!(
            parse(source).unwrap_err().kind,
            ErrorKind::Syntax,
            "{source}"
        );
    }
}

#[test]
fn linked_groups_and_arrows_fit_one_mib_with_limits_intact() {
    std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(|| {
            let source = format!("{}7{}", "(".repeat(60), ")".repeat(60));
            assert_eq!(
                Runtime::default().execute(&source, &mut Empty).unwrap(),
                Value::Number(7.)
            );
            let source = format!("{}1{}", "(".repeat(64), ")".repeat(64));
            assert!(parse(&source).is_ok());
            assert_eq!(
                Runtime::default()
                    .execute("((a=7)=>a)()", &mut Empty)
                    .unwrap(),
                Value::Number(7.)
            );
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn failed_template_expression_does_not_refund_parser_visits() {
    for (initial, kind, expected) in [
        (10, ErrorKind::Syntax, 12),
        (
            MAX_PARSER_VISITS - 1,
            ErrorKind::Resource,
            MAX_PARSER_VISITS + 1,
        ),
    ] {
        let mut parser = Parser {
            tokens: lex("").unwrap(),
            index: 0,
            depth: 0,
            expression_frames: 0,
            function_depth: 0,
            function_kind: FunctionKind::Ordinary,
            module_context: false,
            loop_depth: 0,
            switch_depth: 0,
            labels: Vec::new(),
            visits: initial,
            group_visits: 0,
            flat_storage_bytes: 0,
        };
        let error = parser
            .template_literal(vec![(String::new(), Some("a +".into()))])
            .unwrap_err();
        assert_eq!(error.kind, kind);
        assert_eq!(parser.visits, expected);
        assert_eq!(error.input_source_offset(), None);
    }
}
