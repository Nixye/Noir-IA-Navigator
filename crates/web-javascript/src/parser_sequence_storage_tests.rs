use super::*;

fn number() -> Expr {
    Expr::Literal(Value::Number(1.))
}
fn sequence(first: Expr, second: Expr, third: Expr) -> Expr {
    let mut bytes = 0;
    comma(comma(first, second, &mut bytes).unwrap(), third, &mut bytes).unwrap()
}
#[test]
fn sequence_storage_starts_at_three_and_charges_aligned_header_and_capacity() {
    let mut bytes = 0;
    let pair = comma(number(), number(), &mut bytes).unwrap();
    assert_eq!(bytes, 0);
    let triple = comma(pair, number(), &mut bytes).unwrap();
    let Expr::Sequence(value) = triple else {
        panic!("Expected sequence")
    };
    assert_eq!(value.items().len(), 3);
    assert_eq!(value.height(), 2);
    assert_eq!(
        bytes,
        header_bytes() + value.0.items.capacity() * size_of::<Expr>()
    );
    assert!(bytes <= 4 * size_of::<Expr>() + 2);
    assert!(header_bytes().is_multiple_of(align_of::<Data>()));
}
#[test]
fn sequence_storage_grows_geometrically_to_the_independent_width_limit() {
    let mut bytes = 0;
    let mut expr = number();
    for count in 2..=MAX_ITEMS {
        expr = comma(expr, number(), &mut bytes).unwrap();
        if let Expr::Sequence(sequence) = &expr {
            assert_eq!(sequence.items().len(), count);
            assert_eq!(sequence.height(), 2);
            assert!(sequence.0.items.capacity() >= count);
            assert!(sequence.0.items.capacity() <= MAX_ITEMS);
            assert_eq!(
                bytes,
                header_bytes() + sequence.0.items.capacity() * size_of::<Expr>()
            );
        }
    }
    let used = bytes;
    assert_eq!(
        comma(expr, number(), &mut bytes).unwrap_err().message,
        "Sequence length budget exceeded"
    );
    assert_eq!(bytes, used);
}
#[test]
fn sequence_storage_limit_is_cumulative_and_failed_reservations_are_not_refunded() {
    let mut bytes = MAX_BYTES - header_bytes() - 3 * size_of::<Expr>() + 1;
    let pair = comma(number(), number(), &mut bytes).unwrap();
    assert_eq!(
        comma(pair, number(), &mut bytes).unwrap_err().message,
        "Sequence storage budget exceeded"
    );
    assert!(bytes > MAX_BYTES);
    let previous = bytes;
    assert!(charge(&mut bytes, 1).is_err());
    assert_eq!(bytes, previous + 1);
}
#[test]
fn sequence_height_includes_empty_class_declarations_and_parameter_defaults() {
    let function = Expr::Function {
        kind: FunctionKind::Ordinary,
        method: false,
        name: None,
        params: Arc::new(vec![]),
        body: Arc::new(vec![Stmt::ClassDefinition(
            "C".into(),
            ClassDefinition {
                name: None,
                base: None,
                members: vec![],
            },
        )]),
        arrow: false,
        parameters: None,
    };
    let mut expr = sequence(function, number(), number());
    assert_eq!(depth::expression_height(&expr).unwrap(), 3);
    for _ in 0..61 {
        expr = Expr::Unary("!".into(), Box::new(expr));
    }
    assert_eq!(depth::expression_height(&expr).unwrap(), 64);
    expr = Expr::Unary("!".into(), Box::new(expr));
    assert!(depth::expression_height(&expr).is_err());
    let mut function = Expr::Function {
        kind: FunctionKind::Ordinary,
        method: false,
        name: None,
        params: Arc::new(vec![]),
        body: Arc::new(vec![]),
        arrow: true,
        parameters: Some(Arc::new(vec![Parameter {
            pattern: Pattern::Default(
                Box::new(Pattern::Name("x".into())),
                Box::new(sequence(number(), number(), number())),
            ),
            rest: false,
        }])),
    };
    assert_eq!(depth::expression_height(&function).unwrap(), 3);
    function = sequence(function, number(), number());
    assert_eq!(depth::expression_height(&function).unwrap(), 4);
}
#[test]
fn sequence_aliases_are_immutable_and_never_trigger_hidden_copy_on_write() {
    let expr = sequence(number(), number(), number());
    let copy = expr.clone();
    let mut bytes = 0;
    assert_eq!(
        comma(expr, number(), &mut bytes).unwrap_err().message,
        "Shared sequence cannot be extended"
    );
    assert_eq!(bytes, 0);
    assert_eq!(depth::expression_height(&copy).unwrap(), 2);
    let Expr::Sequence(sequence) = copy else {
        panic!()
    };
    assert_eq!(sequence.items().len(), 3);
}

#[test]
fn sequence_nested_cached_height_clone_and_drop_fit_one_mib_stack() {
    std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(|| {
            let mut expression = number();
            for _ in 0..63 {
                expression = sequence(number(), number(), expression);
            }
            assert_eq!(depth::expression_height(&expression).unwrap(), 64);
            let shared = expression.clone();
            let mut bytes = 0;
            let left = comma(number(), number(), &mut bytes).unwrap();
            assert_eq!(
                comma(left, shared, &mut bytes).unwrap_err().message,
                "Expression tree nesting budget exceeded"
            );
            assert_eq!(bytes, 0);
            drop(expression.clone());
            drop(expression);
        })
        .unwrap()
        .join()
        .unwrap();
}
