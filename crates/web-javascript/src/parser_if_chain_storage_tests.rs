use super::*;
use crate::ErrorKind;

fn clause() -> (Expr, Stmt) {
    (Expr::Literal(Value::Bool(false)), Stmt::Empty)
}
#[test]
fn if_chain_storage_layout_is_charged_before_allocation_and_shared_with_sequences() {
    let header = flat_storage::header_bytes::<Data>();
    assert!(header.is_multiple_of(align_of::<Data>()));
    let mut p = super::super::if_chain_tests::parser("if(0);else if(0);");
    let Stmt::IfChain(chain) = p.statement().unwrap() else {
        panic!()
    };
    assert_eq!(chain.clauses().len(), 2);
    assert_eq!(
        p.flat_storage_bytes,
        header + chain.0.clauses.capacity() * size_of::<(Expr, Stmt)>()
    );
    // A single If keeps the established representation and allocation path.
    let mut single = super::super::if_chain_tests::parser("if(0);else;");
    assert!(matches!(single.statement().unwrap(), Stmt::If(..)));
    assert_eq!(single.flat_storage_bytes, 0);
    eprintln!(
        "IfChain layout: Expr={} Stmt={} clause={} Arc-data={} pair={} shared-limit={}",
        size_of::<Expr>(),
        size_of::<Stmt>(),
        size_of::<(Expr, Stmt)>(),
        header,
        p.flat_storage_bytes,
        flat_storage::MAX_BYTES
    );
    if cfg!(target_pointer_width = "64") {
        assert_eq!(size_of::<Expr>(), 64);
        assert_eq!(flat_storage::MAX_BYTES, 1_572_864);
    }

    let pair_cost = p.flat_storage_bytes;
    let mut mixed = super::super::if_chain_tests::parser("1,2,3;if(0);else if(0);");
    mixed.statement().unwrap();
    let sequence_cost = mixed.flat_storage_bytes;
    mixed.statement().unwrap();
    assert_eq!(mixed.flat_storage_bytes, sequence_cost + pair_cost);
    let mut exhausted = super::super::if_chain_tests::parser("if(0);else if(0);");
    exhausted.flat_storage_bytes = flat_storage::MAX_BYTES - pair_cost + 1;
    assert_eq!(
        exhausted.statement().unwrap_err().message,
        "If-chain storage budget exceeded"
    );
    assert_eq!(exhausted.flat_storage_bytes, flat_storage::MAX_BYTES + 1);
    // A Sequence allocation cannot reset the IfChain ledger, or vice versa.
    let mut exhausted = super::super::if_chain_tests::parser("1,2,3");
    exhausted.flat_storage_bytes = flat_storage::MAX_BYTES - sequence_cost + 1;
    assert_eq!(
        exhausted.expression(0).unwrap_err().message,
        "Sequence storage budget exceeded"
    );
    assert_eq!(exhausted.flat_storage_bytes, flat_storage::MAX_BYTES + 1);
}
#[test]
fn if_chain_capacity_growth_is_bounded_and_failure_never_refunds() {
    let mut clauses = Vec::new();
    let mut bytes = 0;
    reserve(&mut clauses, 2, &mut bytes).unwrap();
    for count in 0..MAX_CLAUSES {
        let before = clauses.capacity();
        reserve(&mut clauses, 1, &mut bytes).unwrap();
        assert!(clauses.capacity() <= before.max(before + before / 2).max(2));
        clauses.push(clause());
        assert_eq!(clauses.len(), count + 1);
        assert_eq!(bytes, clauses.capacity() * size_of::<(Expr, Stmt)>());
    }
    let used = bytes;
    assert_eq!(
        reserve(&mut clauses, 1, &mut bytes).unwrap_err().message,
        "If-chain length budget exceeded"
    );
    assert_eq!(bytes, used);
    let mut bytes = flat_storage::MAX_BYTES - 1;
    let mut empty = Vec::new();
    assert_eq!(
        reserve(&mut empty, 2, &mut bytes).unwrap_err().kind,
        ErrorKind::Resource
    );
    assert_eq!(empty.capacity(), 0);
    assert!(bytes > flat_storage::MAX_BYTES);
    let failed = bytes;
    assert!(reserve(&mut empty, 2, &mut bytes).is_err());
    assert!(bytes > failed);
}
#[test]
fn if_chain_error_templates_and_dynamic_parameters_preserve_the_same_ledger() {
    use super::super::if_chain_tests::parser;
    let mut successful = parser("if(0);else if(0);");
    successful.statement().unwrap();
    let cost = successful.flat_storage_bytes;
    let mut failed = parser("if(0);else if(0);");
    failed.tokens = lex("if(0);else if(").unwrap();
    assert_eq!(failed.statement().unwrap_err().kind, ErrorKind::Syntax);
    assert_eq!(failed.flat_storage_bytes, cost);
    for (source, expected) in [
        (
            "`${function(){if(0);else if(0);}}${function(){if(0);else if(0);}}`",
            2 * cost,
        ),
        ("`${function(){if(0);else if(}}`", cost),
    ] {
        let mut p = parser(source);
        let result = p.expression(0);
        assert_eq!(p.flat_storage_bytes, expected);
        if expected == cost {
            assert_eq!(result.unwrap_err().kind, ErrorKind::Syntax);
        } else {
            result.unwrap();
        }
    }
    let mut p = parser("x=function(){if(0);else if(0);}");
    p.parameters(true).unwrap();
    assert_eq!(p.flat_storage_bytes, cost);
    p.tokens = lex("if(0);else if(0);").unwrap();
    p.index = 0;
    p.function_depth = 1;
    p.statement().unwrap();
    assert_eq!(p.flat_storage_bytes, 2 * cost);
    parse_dynamic_function("x=function(){if(0);else if(0);}", "if(0);else if(0);").unwrap();
}

#[test]
fn if_chain_clone_shares_immutable_storage_without_copying_or_recharging() {
    let mut parser = super::super::if_chain_tests::parser("if(0);else if(0);else;");
    let Stmt::IfChain(chain) = parser.statement().unwrap() else {
        panic!()
    };
    let bytes = parser.flat_storage_bytes;
    let copy = chain.clone();
    assert!(Arc::ptr_eq(&chain.0, &copy.0));
    assert_eq!(parser.flat_storage_bytes, bytes);
    assert_eq!(Arc::strong_count(&chain.0), 2);
    drop(chain);
    assert_eq!(copy.clauses().len(), 2);
    assert!(matches!(copy.fallback(), Some(Stmt::Empty)));
}

#[test]
fn if_chain_real_sources_exhaust_shared_storage_before_work_or_token_limits() {
    for unit in ["if(0);else if(0);", "if(0,0,0);else if(0);"] {
        let mut once = super::super::if_chain_tests::parser(unit);
        once.statement().unwrap();
        let per_unit = once.flat_storage_bytes;
        let count = flat_storage::MAX_BYTES / per_unit + 1;
        let source = unit.repeat(count);
        assert!(source.len() < 128 * 1024);
        let mut parser = super::super::if_chain_tests::parser(&source);
        let mut accepted = Vec::new();
        let error = loop {
            match parser.statement() {
                Ok(statement) => accepted.push(statement),
                Err(error) => break error,
            }
        };
        assert_eq!(error.kind, ErrorKind::Resource);
        assert_eq!(error.message, "If-chain storage budget exceeded");
        assert_eq!(accepted.len(), count - 1);
        assert!(parser.flat_storage_bytes > flat_storage::MAX_BYTES);
        assert!(parser.visits < MAX_PARSER_VISITS);
        assert!(parser.group_visits < MAX_GROUP_VISITS);
        assert_eq!((parser.depth, parser.expression_frames), (0, 0));
    }
}
