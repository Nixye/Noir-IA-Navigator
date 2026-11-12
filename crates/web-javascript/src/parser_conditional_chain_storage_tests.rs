use super::super::if_chain_tests::parser;
use super::*;
use crate::ErrorKind;

fn number() -> Expr {
    Expr::Literal(Value::Number(1.))
}
fn chain(condition: Expr, yes: Expr, fallback: Expr) -> Expr {
    let mut bytes = 0;
    let no = conditional(number(), number(), fallback, &mut bytes).unwrap();
    conditional(condition, yes, no, &mut bytes).unwrap()
}
#[test]
fn conditional_chain_layout_and_amortized_capacity_are_charged_exactly() {
    let header = flat_storage::header_bytes::<Data>();
    assert!(header.is_multiple_of(align_of::<Data>()));
    let mut bytes = 0;
    let mut value = conditional(number(), number(), number(), &mut bytes).unwrap();
    assert!(matches!(value, Expr::Conditional(..)));
    assert_eq!(bytes, 0);
    for count in 2..=MAX_CLAUSES {
        let previous = if let Expr::ConditionalChain(chain) = &value {
            chain.0.clauses_rev.capacity()
        } else {
            0
        };
        value = conditional(number(), number(), value, &mut bytes).unwrap();
        let Expr::ConditionalChain(chain) = &value else {
            panic!()
        };
        assert_eq!(chain.clauses_rev().len(), count);
        assert_eq!(chain.height(), 2);
        assert_eq!(
            bytes,
            header + chain.0.clauses_rev.capacity() * size_of::<(Expr, Expr)>()
        );
        assert!(chain.0.clauses_rev.capacity() <= (previous + previous / 2).max(count));
        assert!(chain.0.clauses_rev.capacity() <= MAX_CLAUSES);
    }
    let before = bytes;
    assert_eq!(
        conditional(number(), number(), value, &mut bytes)
            .unwrap_err()
            .message,
        "Conditional-chain length budget exceeded"
    );
    assert_eq!(bytes, before);
    eprintln!(
        "ConditionalChain layout: Expr={} clause={} Arc-data={} pair={} shared-limit={}",
        size_of::<Expr>(),
        size_of::<(Expr, Expr)>(),
        header,
        header + 2 * size_of::<(Expr, Expr)>(),
        flat_storage::MAX_BYTES
    );
    if cfg!(target_pointer_width = "64") {
        assert_eq!(size_of::<Expr>(), 64);
        assert_eq!(header, 112);
        assert_eq!(flat_storage::MAX_BYTES, 1_572_864);
    }
}
#[test]
fn conditional_chain_aliases_never_copy_or_recharge_storage() {
    let value = chain(number(), number(), number());
    let copy = value.clone();
    let mut bytes = 0;
    assert_eq!(
        conditional(number(), number(), value, &mut bytes)
            .unwrap_err()
            .message,
        "Shared conditional chain cannot be extended"
    );
    assert_eq!(bytes, 0);
    let Expr::ConditionalChain(chain) = &copy else {
        panic!()
    };
    let other = chain.clone();
    assert!(Arc::ptr_eq(&chain.0, &other.0));
    assert_eq!(chain.clauses_rev().len(), 2);
    assert_eq!(depth::expression_height(&copy).unwrap(), 2);
}
#[test]
fn conditional_chain_shares_sequence_if_storage_in_both_orders_without_refunds() {
    let units = ["0?1:0?2:3;", "1,2,3;", "if(0);else if(0);"];
    let costs: Vec<_> = units
        .iter()
        .map(|unit| {
            let mut p = parser(unit);
            p.statement().unwrap();
            p.flat_storage_bytes
        })
        .collect();
    for order in [[0, 1, 2], [2, 1, 0], [1, 0, 2], [2, 0, 1]] {
        let source: String = order.iter().map(|&index| units[index]).collect();
        let mut p = parser(&source);
        let mut expected = 0;
        for index in order {
            p.statement().unwrap();
            expected += costs[index];
            assert_eq!(p.flat_storage_bytes, expected);
        }
    }
    for (unit, cost) in units.iter().zip(costs) {
        let mut p = parser(unit);
        p.flat_storage_bytes = flat_storage::MAX_BYTES - cost + 1;
        assert_eq!(p.statement().unwrap_err().kind, ErrorKind::Resource);
        assert_eq!(p.flat_storage_bytes, flat_storage::MAX_BYTES + 1);
    }
    let mut bytes = flat_storage::MAX_BYTES - 1;
    let mut clauses = Vec::new();
    assert!(reserve(&mut clauses, 2, &mut bytes).is_err());
    assert_eq!(clauses.capacity(), 0);
    let consumed = bytes;
    assert!(reserve(&mut clauses, 2, &mut bytes).is_err());
    assert!(bytes > consumed);
}
#[test]
fn conditional_chain_failed_templates_and_dynamic_parts_keep_storage() {
    let mut once = parser("0?1:0?2:3");
    once.expression(0).unwrap();
    let cost = once.flat_storage_bytes;
    for (source, expected, success) in [
        ("`${0?1:0?2:3}${0?1:0?2:3}`", 2 * cost, true),
        ("`${(0?1:0?2:3)+}`", cost, false),
        ("(0?1:0?2:3)+", cost, false),
    ] {
        let mut p = parser(source);
        let result = p.expression(0);
        assert_eq!(p.flat_storage_bytes, expected, "{source}");
        assert_eq!(result.is_ok(), success, "{source}");
    }
    let mut p = parser("x=0?1:0?2:3");
    p.parameters(true).unwrap();
    assert_eq!(p.flat_storage_bytes, cost);
    p.tokens = lex("return 0?1:0?2:3;").unwrap();
    p.index = 0;
    p.function_depth = 1;
    p.statement().unwrap();
    assert_eq!(p.flat_storage_bytes, 2 * cost);
    parse_dynamic_function("x=0?1:0?2:3", "return 0?1:0?2:3;").unwrap();
}
#[test]
fn conditional_chain_height_includes_functions_classes_defaults_and_all_branches() {
    for source in [
        "function(){class C{}}",
        "function(){return !1;}",
        "(x=!1)=>x",
        "class {x=!1;}",
    ] {
        let Stmt::Expression(child) = parse(&format!("({source})")).unwrap().remove(0) else {
            panic!()
        };
        let height = depth::expression_height(&child).unwrap();
        for slot in 0..3 {
            let value = match slot {
                0 => chain(child.clone(), number(), number()),
                1 => chain(number(), child.clone(), number()),
                _ => chain(number(), number(), child.clone()),
            };
            assert_eq!(
                depth::expression_height(&value).unwrap(),
                height + 1,
                "{source}"
            );
            let mut value = value;
            for _ in 0..(63 - height) {
                value = Expr::Unary("!".into(), Box::new(value));
            }
            assert_eq!(depth::expression_height(&value).unwrap(), 64);
            assert!(depth::expression_height(&Expr::Unary("!".into(), Box::new(value))).is_err());
        }
    }
}
#[test]
fn conditional_chain_residual_clone_debug_drop_and_rejection_fit_one_mib() {
    std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(|| {
            let mut value = number();
            for _ in 0..63 {
                // The yes edge is intentionally not flattened into the outer chain.
                value = chain(number(), value, number());
            }
            assert_eq!(depth::expression_height(&value).unwrap(), 64);
            let shared = value.clone();
            assert!(!format!("{shared:?}").is_empty());
            let mut bytes = 0;
            let no = conditional(number(), number(), number(), &mut bytes).unwrap();
            assert_eq!(
                conditional(number(), shared, no, &mut bytes)
                    .unwrap_err()
                    .message,
                "Expression tree nesting budget exceeded"
            );
            assert_eq!(bytes, 0);
            drop(value.clone());
            drop(value);
        })
        .unwrap()
        .join()
        .unwrap();
}
