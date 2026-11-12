use super::if_chain_tests::parser;
use super::*;
use crate::ErrorKind;

#[test]
fn conditional_chain_flattens_only_no_edges_and_preserves_source_order() {
    let Stmt::Expression(Expr::ConditionalChain(chain)) = parse("a?b:c?d:e?f:g").unwrap().remove(0)
    else {
        panic!()
    };
    let names: Vec<_> = chain
        .clauses_rev()
        .iter()
        .map(|(condition, yes)| {
            let (Expr::Name(condition), Expr::Name(yes)) = (condition, yes) else {
                panic!()
            };
            (condition.as_str(), yes.as_str())
        })
        .collect();
    assert_eq!(names, [("e", "f"), ("c", "d"), ("a", "b")]);
    assert!(matches!(chain.fallback(), Expr::Name(name) if name == "g"));
    for source in [
        "a?(b?c:d):e",
        "(a?b:c)?d:e",
        "a?b:(0,c?d:e)",
        "a?b:(0,c?d:e,f)",
        "a?b:[c?d:e]",
        "a?b:function(){return c?d:e;}",
        "a?b:f(c?d:e)",
    ] {
        assert!(
            matches!(
                &parse(source).unwrap()[0],
                Stmt::Expression(Expr::Conditional(..))
            ),
            "{source}"
        );
    }
    assert!(matches!(
        &parse("a?b:((c?d:e))").unwrap()[0],
        Stmt::Expression(Expr::ConditionalChain(_))
    ));
}
#[test]
fn conditional_chain_keeps_exact_pratt_visits_and_logical_native_chain_limits() {
    for count in [1, 2, 20, 63] {
        let source = format!("{}7", "0?1:".repeat(count));
        let mut p = parser(&source);
        p.expression(0).unwrap();
        assert_eq!(p.visits, 1 + 2 * count);
        assert_eq!((p.depth, p.expression_frames), (0, 0));
        let mut p = parser(&source);
        p.visits = MAX_PARSER_VISITS - (1 + 2 * count);
        p.expression(0).unwrap();
        assert_eq!(p.visits, MAX_PARSER_VISITS);
    }
    for (source, message) in [
        (
            format!("{}7", "0?1:".repeat(64)),
            "Expression nesting budget exceeded",
        ),
        (
            format!("{}7", "1+".repeat(64)),
            "Expression chain budget exceeded",
        ),
        (
            format!("{}7{}", "[".repeat(64), "]".repeat(64)),
            "Expression nesting budget exceeded",
        ),
    ] {
        let error = parse(&source).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert_eq!(error.message, message, "{source}");
    }
}
#[test]
fn conditional_chain_compound_admission_parse_clone_drop_module_and_dynamic_fit_one_mib() {
    std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(|| {
            let source = format!("({}7){}", "0?1:".repeat(20), "+1".repeat(45));
            let tree = parse(&source).unwrap();
            let Stmt::Expression(expression) = &tree[0] else {
                panic!()
            };
            assert_eq!(depth::expression_height(expression).unwrap(), 47);
            drop(tree.clone());
            drop(tree);
            parse_module(&source).unwrap();
            parse_dynamic_function("", &format!("return {source};")).unwrap();
            // yes and condition chains still have their residual physical cost.
            let source = format!(
                "({}7{}){}",
                "0?".repeat(20),
                ":1".repeat(20),
                "+1".repeat(45)
            );
            assert_eq!(
                parse(&source).unwrap_err().message,
                "Expression tree nesting budget exceeded"
            );
        })
        .unwrap()
        .join()
        .unwrap();
}
#[test]
fn conditional_chain_grammar_offsets_assignment_shapes_and_asi_stay_precise() {
    for source in ["a?b:c?d:", "a?b:c?d", "a?b:c?d:)"] {
        let error = parse(source).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Syntax);
        let expected = if source.ends_with(')') {
            source.len() - 1
        } else {
            source.len()
        };
        assert_eq!(error.offset, Some(expected), "{source}");
    }
    for source in [
        "(a?b:c?d:e)=1",
        "(a?b:c?d:e)++",
        "++(a?b:c?d:e)",
        "for((a?b:c?d:e) of []){}",
        "for((a?b:c?d:e) in {}){}",
    ] {
        assert_eq!(
            parse(source).unwrap_err().kind,
            ErrorKind::Syntax,
            "{source}"
        );
    }
    let tree = parse("a?b:c?d:e\nx").unwrap();
    assert_eq!(tree.len(), 2);
    let Stmt::FunctionDefinition(_, Expr::Function { body, .. }) =
        parse("function f(){return\na?b:c?d:e}").unwrap().remove(0)
    else {
        panic!()
    };
    assert!(matches!(body[0], Stmt::Return(None)));
    assert!(matches!(
        body[1],
        Stmt::Expression(Expr::ConditionalChain(_))
    ));
}

#[test]
fn conditional_chain_residual_branches_under_declarations_keep_one_mib_validation() {
    std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(|| {
            let mut expression = "1".to_owned();
            for _ in 0..60 {
                expression = format!("false?({expression}):false?0:0");
            }
            let source = format!(
                "{}{};{}",
                "function f(){".repeat(30),
                expression,
                "}".repeat(30)
            );
            let error = parse(&source).unwrap_err();
            assert_eq!(error.kind, ErrorKind::Resource);
            assert_eq!(error.message, "Expression tree nesting budget exceeded");
            assert_eq!(parse_module(&source).unwrap_err().kind, ErrorKind::Resource);
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn conditional_chain_mixed_yes_binary_call_and_function_owners_fit_one_mib() {
    std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(|| {
            let mut expression = "1".to_owned();
            for _ in 0..15 {
                expression =
                    format!("false?(1+(function(){{return ({expression});}})()):false?0:0");
            }
            let tree = parse(&expression).unwrap();
            let Stmt::Expression(value) = &tree[0] else {
                panic!()
            };
            assert_eq!(depth::expression_height(value).unwrap(), 61);
            drop(tree.clone());
            drop(tree);
            parse_module(&expression).unwrap();
            parse_dynamic_function("", &format!("return {expression};")).unwrap();
            let deeper = format!("false?(1+(function(){{return ({expression});}})()):false?0:0");
            let error = parse(&deeper).unwrap_err();
            assert_eq!(error.kind, ErrorKind::Resource);
            assert_eq!(error.message, "Expression tree nesting budget exceeded");
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn conditional_chain_legacy_leaves_under_sixty_declarations_keep_one_mib_validation() {
    for declarations in [60, 62] {
        for calls in [false, true] {
            std::thread::Builder::new()
                .name(format!("conditional-legacy-{declarations}-calls-{calls}"))
                .stack_size(1024 * 1024)
                .spawn(move || {
                    let leaf = if calls {
                        format!("{}1{}", "f(".repeat(60), ")".repeat(60))
                    } else {
                        format!("1{}", "+1".repeat(60))
                    };
                    let source = format!(
                        "{}{leaf};{}",
                        "function f(){".repeat(declarations),
                        "}".repeat(declarations)
                    );
                    for error in [
                        parse(&source).unwrap_err(),
                        parse_module(&source).unwrap_err(),
                    ] {
                        assert_eq!(error.kind, ErrorKind::Resource);
                        assert!(matches!(
                            error.message.as_str(),
                            "Expression tree nesting budget exceeded"
                                | "Expression nesting budget exceeded"
                                | "Syntax nesting budget exceeded"
                        ));
                    }
                })
                .unwrap()
                .join()
                .unwrap();
        }
    }
}
