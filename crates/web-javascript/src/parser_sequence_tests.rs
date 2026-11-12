use super::*;
use crate::ErrorKind;

fn parser(source: &str) -> Parser {
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
fn grouped(count: usize) -> String {
    format!("{}1,1{}", "(".repeat(count - 2), "),1".repeat(count - 2))
}
#[test]
fn sequence_left_only_normalization_keeps_right_subtrees_and_local_chain_limit() {
    let tree = parse("1,(2,3,4),5").unwrap();
    let Stmt::Expression(Expr::Sequence(sequence)) = &tree[0] else {
        panic!()
    };
    assert_eq!(sequence.items().len(), 3);
    assert!(matches!(sequence.items()[1], Expr::Sequence(_)));
    assert_eq!(sequence.height(), 3);
    assert!(matches!(
        &parse("1,(2,3)").unwrap()[0],
        Stmt::Expression(Expr::Binary(..))
    ));
    let source = std::iter::repeat_n("1", 65).collect::<Vec<_>>().join(",");
    assert_eq!(
        parse(&source).unwrap_err().message,
        "Expression chain budget exceeded"
    );
    for source in [
        format!("{}1{}", "[".repeat(64), "]".repeat(64)),
        format!("{}1", "1,".repeat(64)),
    ] {
        assert_eq!(parse(&source).unwrap_err().kind, ErrorKind::Resource);
    }
}
#[test]
fn sequence_grouped_capacity_clone_drop_and_errors_fit_one_mib_stack() {
    std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(|| {
            let source = grouped(4096);
            let tree = parse(&source).unwrap();
            let Stmt::Expression(Expr::Sequence(sequence)) = &tree[0] else {
                panic!()
            };
            assert_eq!(sequence.items().len(), 4096);
            assert_eq!(sequence.height(), 2);
            drop(tree.clone());
            drop(tree);
            parse_module(&source).unwrap();
            parse_dynamic_function("", &source).unwrap();
            assert_eq!(
                parse(&grouped(4097)).unwrap_err().message,
                "Sequence length budget exceeded"
            );
            assert_eq!(
                parse(&format!("{source},")).unwrap_err().kind,
                ErrorKind::Syntax
            );
            let source = format!(
                "function f(){{return {};}}",
                std::iter::repeat_n("1", 64).collect::<Vec<_>>().join(",")
            );
            let tree = parse(&source).unwrap();
            drop(tree.clone());
            drop(tree);
        })
        .unwrap()
        .join()
        .unwrap();
}
#[test]
fn sequence_for_initializer_is_built_once_and_failure_does_not_refund_storage() {
    let mut once = parser("(1,2,3)");
    once.expression(0).unwrap();
    let mut loop_parser = parser("for ((1,2,3);false;) {}");
    loop_parser.statement().unwrap();
    assert_eq!(loop_parser.flat_storage_bytes, once.flat_storage_bytes);
    let mut failed = parser("for ((1,2,3);false;)");
    assert_eq!(failed.statement().unwrap_err().kind, ErrorKind::Syntax);
    assert_eq!(failed.flat_storage_bytes, loop_parser.flat_storage_bytes);
}
#[test]
fn sequence_template_subparser_propagates_storage_on_success_and_failure() {
    let mut once = parser("1,2,3");
    once.expression(0).unwrap();
    let mut success = parser("`${1,2,3}${4,5,6}`");
    success.expression(0).unwrap();
    assert_eq!(success.flat_storage_bytes, 2 * once.flat_storage_bytes);
    let mut failed = parser("`${1,2,3,}`");
    assert_eq!(failed.expression(0).unwrap_err().kind, ErrorKind::Syntax);
    assert_eq!(failed.flat_storage_bytes, once.flat_storage_bytes);
}
#[test]
fn sequence_children_keep_strict_dynamic_scope_and_suspension_validation() {
    for source in [
        "'use strict';(0,0,eval=1)",
        "'use strict';(0,0,delete missing)",
        "(0,0,function(){let x;var x;})",
        "(0,0,class C{m(){arguments=1;}})",
    ] {
        assert_eq!(
            crate::Runtime::default()
                .execute(source, &mut Empty)
                .unwrap_err()
                .kind,
            ErrorKind::Syntax,
            "{source}"
        );
    }
    for source in [
        "function* f(){return {x:(0,0,yield 1)};}",
        "function* f(){return new Array((0,0,yield 1));}",
    ] {
        assert_eq!(
            parse(source).unwrap_err().kind,
            ErrorKind::Syntax,
            "{source}"
        );
    }
}
struct Empty;
impl crate::Host for Empty {
    fn global(&mut self, _: &str) -> Option<Value> {
        None
    }
    fn get(&mut self, _: u64, _: &str) -> Result<Value, JsError> {
        panic!("unexpected host")
    }
    fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
        panic!("unexpected host")
    }
    fn call(&mut self, _: u64, _: &str, _: &[Value]) -> Result<Value, JsError> {
        panic!("unexpected host")
    }
}
