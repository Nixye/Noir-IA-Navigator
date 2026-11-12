//! Independent cumulative work ledgers, not an all-AST allocation census.
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
        labels: Vec::new(),
        visits: 0,
        group_visits: 0,
        flat_storage_bytes: 0,
    }
}
fn grouped(count: usize, expression: &str) -> String {
    format!("{}{expression}{}", "(".repeat(count), ")".repeat(count))
}
fn resource(error: JsError, groups: bool) {
    assert_eq!(error.kind, ErrorKind::Resource);
    assert_eq!(error.offset, None);
    let (message, diagnostic) = if groups {
        (
            "Parser group visit budget exceeded",
            "parser group-visits=16385 limit=16384",
        )
    } else {
        (
            "Parser visit budget exceeded",
            "parser visits=16385 limit=16384",
        )
    };
    assert_eq!(error.message, message);
    assert_eq!(error.diagnostic_contexts(), &[diagnostic]);
}

#[test]
fn each_ledger_has_its_own_exact_boundary_and_numeric_diagnostic() {
    let source = grouped(MAX_GROUP_VISITS, "1");
    let mut p = parser(&source);
    p.statement().unwrap();
    assert_eq!((p.visits, p.group_visits), (2, MAX_GROUP_VISITS));
    resource(parse(&format!("({source})")).unwrap_err(), true);
    assert_eq!(
        parse(&";".repeat(MAX_PARSER_VISITS)).unwrap().len(),
        MAX_PARSER_VISITS
    );
    resource(
        parse(&";".repeat(MAX_PARSER_VISITS + 1)).unwrap_err(),
        false,
    );
    // The same grouping ceiling applies to module and dynamic Function entry.
    parse_module(&source).unwrap();
    parse_dynamic_function("", &source).unwrap();
    resource(parse_module(&format!("({source})")).unwrap_err(), true);
}

#[test]
fn mixed_source_can_spend_both_full_ledgers_without_resetting_either() {
    // Interpolation lexers have local token limits. The ledgers are aggregate.
    let expression = grouped(MAX_GROUP_VISITS, "1");
    let source = format!("`${{{expression}}}`;{}", ";".repeat(MAX_PARSER_VISITS - 3));
    let mut p = parser(&source);
    while !matches!(p.token().kind, Kind::End) {
        p.statement().unwrap();
    }
    assert_eq!(
        (p.visits, p.group_visits),
        (MAX_PARSER_VISITS, MAX_GROUP_VISITS)
    );
    assert_eq!(p.visits + p.group_visits, 32_768);
    resource(parse(&(source + ";")).unwrap_err(), false);
    let source = format!("`${{{}}}`", grouped(MAX_GROUP_VISITS + 1, "1"));
    resource(parse(&source).unwrap_err(), true);
}

#[test]
fn the_failing_origin_is_not_substituted_by_the_other_ledger() {
    let mut p = parser("(a+1)");
    p.visits = MAX_PARSER_VISITS - 1;
    resource(p.expression(0).unwrap_err(), false);
    assert_eq!((p.visits, p.group_visits), (MAX_PARSER_VISITS + 1, 1));
    let mut p = parser("(1)");
    p.visits = MAX_PARSER_VISITS - 1;
    p.group_visits = MAX_GROUP_VISITS;
    resource(p.expression(0).unwrap_err(), true);
    assert_eq!(
        (p.visits, p.group_visits),
        (MAX_PARSER_VISITS, MAX_GROUP_VISITS + 1)
    );
    assert_eq!(p.index, 1);
    assert_eq!((p.depth, p.expression_frames), (0, 0));
    let mut p = parser("(1)");
    p.visits = MAX_PARSER_VISITS;
    p.group_visits = MAX_GROUP_VISITS;
    resource(p.expression(0).unwrap_err(), false);
    assert_eq!((p.index, p.group_visits), (0, MAX_GROUP_VISITS));
}

#[test]
fn templates_share_group_work_across_separate_and_nested_interpolations() {
    let half = grouped(MAX_GROUP_VISITS / 2, "1");
    let source = format!("`${{{half}}}`;`${{{half}}}`;");
    let mut p = parser(&source);
    p.statement().unwrap();
    p.statement().unwrap();
    assert_eq!((p.visits, p.group_visits), (6, MAX_GROUP_VISITS));
    let source = "`${(`inner${((1))}`)}`";
    let mut p = parser(source);
    p.group_visits = MAX_GROUP_VISITS - 3;
    p.expression(0).unwrap();
    assert_eq!((p.visits, p.group_visits), (3, MAX_GROUP_VISITS));
    let mut p = parser(source);
    p.group_visits = MAX_GROUP_VISITS - 2;
    resource(p.expression(0).unwrap_err(), true);
    assert_eq!((p.visits, p.group_visits), (3, MAX_GROUP_VISITS + 1));
    assert_eq!((p.depth, p.expression_frames), (0, 0));
}

#[test]
fn failed_templates_return_both_consumed_ledgers_before_propagating_error() {
    let mut p = parser("`${((1)+)}`");
    p.group_visits = 10;
    assert_eq!(p.expression(0).unwrap_err().kind, ErrorKind::Syntax);
    assert_eq!((p.visits, p.group_visits), (3, 12));
    p.tokens = lex("(2)").unwrap();
    p.index = 0;
    p.expression(0).unwrap();
    assert_eq!((p.visits, p.group_visits), (4, 13));
    let mut p = parser("`${((1)+)}`");
    p.visits = MAX_PARSER_VISITS - 2;
    resource(p.expression(0).unwrap_err(), false);
    assert_eq!((p.visits, p.group_visits), (MAX_PARSER_VISITS + 1, 2));
}

#[test]
fn retrying_failed_syntax_never_refunds_groups_or_main_visits() {
    let mut p = parser("(1+)");
    p.group_visits = MAX_GROUP_VISITS - 1;
    assert_eq!(p.expression(0).unwrap_err().kind, ErrorKind::Syntax);
    assert_eq!((p.visits, p.group_visits), (2, MAX_GROUP_VISITS));
    p.index = 0;
    resource(p.expression(0).unwrap_err(), true);
    assert_eq!((p.visits, p.group_visits), (3, MAX_GROUP_VISITS + 1));
    assert_eq!((p.depth, p.expression_frames), (0, 0));
}

#[test]
fn function_parameters_and_body_share_both_work_ledgers() {
    let half = grouped(MAX_GROUP_VISITS / 2, "1");
    let params = format!("a={half}");
    parse_dynamic_function(&params, &half).unwrap();
    resource(
        parse_dynamic_function(&params, &format!("({half})")).unwrap_err(),
        true,
    );
    let body = ";".repeat(MAX_PARSER_VISITS - 2);
    parse_dynamic_function("a=1", &body).unwrap();
    resource(
        parse_dynamic_function("a=1", &(body + ";")).unwrap_err(),
        false,
    );
}

#[test]
fn grouping_does_not_reclassify_patterns_children_or_validation_work() {
    let mut p = parser("((f(1+2,...[3])))");
    p.expression(0).unwrap();
    assert_eq!((p.visits, p.group_visits), (5, 2));
    let mut p = parser("var [x]=(1);");
    p.statement().unwrap();
    assert_eq!((p.visits, p.group_visits), (4, 1));
    let mut p = parser("for(let x of xs){;}");
    p.statement().unwrap();
    // Statement 3, pattern 1, expression 1, lexical-conflict walk 2.
    assert_eq!((p.visits, p.group_visits), (7, 0));
}

#[test]
fn changed_tails_keep_physical_depth_and_stack_guards_with_spare_visit_budgets() {
    std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(|| {
            let source = grouped(MAX_GROUP_VISITS, "[1,2,3]");
            for tree in [parse(&source).unwrap(), parse_module(&source).unwrap().body] {
                drop(tree.clone());
                drop(tree);
            }
            parse_dynamic_function("", &source).unwrap();
            let source = grouped(1024, &format!("{}a{}", "(".repeat(64), ").x".repeat(64)));
            let error = parse(&source).unwrap_err();
            assert_eq!(error.kind, ErrorKind::Resource);
            assert_eq!(error.message, "Expression tree nesting budget exceeded");
            let source = format!("{}1", "!".repeat(64));
            assert_eq!(
                parse(&source).unwrap_err().message,
                "Expression nesting budget exceeded"
            );
        })
        .unwrap()
        .join()
        .unwrap();
}
