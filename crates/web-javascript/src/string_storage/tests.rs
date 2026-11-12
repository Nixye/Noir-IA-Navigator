use super::*;
use crate::{ErrorKind, Host, Limits};

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
fn check(source: &str) {
    assert_eq!(
        Runtime::default().execute(source, &mut Empty).unwrap(),
        Value::Bool(true),
        "{source}"
    );
}

#[test]
fn concatenation_crosses_old_limit_without_temporary_utf16_storage() {
    check(
        "var a='x'.repeat(40000); var b=a+a; b.length===80000 && b[79999]==='x' && b.constructor===String;",
    );
    check("var a='x'.repeat(60000); var b=a.concat(a); b.length===120000 && b[119999]==='x';");
    let mut vm = Runtime::default();
    let value = vm
        .concat_strings(
            Value::string("a".repeat(40000)),
            Value::string("b".repeat(40000)),
        )
        .unwrap();
    assert_eq!(value.as_str().unwrap().len(), 80000);
    assert_eq!(vm.string_bytes, 160000); // String buffer + Arc payload, no UTF-16 copies.
}
#[test]
fn exact_storage_boundary_is_checked_in_bytes_not_units() {
    for text in ["x".repeat(65536), "é".repeat(32768), "😀".repeat(16384)] {
        let mut vm = Runtime::default();
        let value = vm
            .concat_strings(Value::string(&text), Value::string(&text))
            .unwrap();
        assert_eq!(value.as_str().unwrap().len(), MAX_CONCAT_BYTES);
        assert_eq!(
            vm.concat_strings(value, Value::string("x"))
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
    }
}
#[test]
fn utf16_boundary_pair_is_joined_and_lone_surrogates_are_preserved() {
    let mut vm = Runtime::default();
    let joined = vm
        .concat_strings(
            Value::Utf16(Arc::from([0xd83d])),
            Value::Utf16(Arc::from([0xde00])),
        )
        .unwrap();
    assert_eq!(joined, Value::string("😀"));
    let value = vm
        .concat_strings(Value::Utf16(Arc::from([0xd800])), Value::string("é😀"))
        .unwrap();
    assert_eq!(
        value,
        Value::Utf16(Arc::from([0xd800, 0xe9, 0xd83d, 0xde00]))
    );
    let left = Value::Utf16(Arc::from(vec![0xd800; 32768]));
    let value = vm.concat_strings(left.clone(), left).unwrap();
    assert!(matches!(&value,Value::Utf16(v) if v.len()==65536));
    assert_eq!(
        vm.concat_strings(value, Value::string("x"))
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
}
#[test]
fn output_reservation_is_exact_and_precedes_allocation() {
    for budget in [0, 11, 12] {
        let mut vm = Runtime::new(Limits {
            string_bytes: budget,
            ..Limits::default()
        });
        let result = vm.concat_strings(Value::string("abc"), Value::string("def"));
        assert_eq!(result.is_ok(), budget == 12);
        assert_eq!(vm.string_bytes, 12);
        assert!(vm.heap.is_empty());
    }
}
#[test]
fn work_preflight_rejects_before_storage_reservation() {
    let mut vm = Runtime::new(Limits {
        instructions: 1,
        ..Limits::default()
    });
    let error = vm
        .concat_strings(Value::string("x".repeat(65536)), Value::string("y"))
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert!(error.message.contains("work budget"));
    assert_eq!(vm.string_bytes, 0);
}
#[test]
fn string_reservations_survive_rollback_and_collection() {
    let mut vm = Runtime::new(Limits {
        string_bytes: 20,
        ..Limits::default()
    });
    let checkpoint = vm.clone();
    vm.concat_strings(Value::string("abc"), Value::string("def"))
        .unwrap();
    vm.rollback_to(checkpoint);
    assert_eq!(vm.string_bytes, 12);
    vm.collect_garbage(&[], &Empty).unwrap();
    assert_eq!(vm.string_bytes, 12);
    assert_eq!(
        vm.concat_strings(Value::string("abc"), Value::string("def"))
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
}
#[test]
fn language_plus_keeps_default_hint_and_left_to_right_conversion() {
    check(
        "var log=''; var a={valueOf:function(){log+='a';return 12;}}; var b={valueOf:function(){log+='b';return 'x';}}; var s=a+b; log==='ab' && s==='12x';",
    );
    check(
        "'x'+null==='xnull' && 'x'+undefined==='xundefined' && 'x'+true==='xtrue' && 42+'x'==='42x';",
    );
}
#[test]
fn method_concat_uses_observable_string_coercion_in_argument_order() {
    check(
        "var log=''; var a={toString:function(){log+='a';return 'A';}}; var b={toString:function(){log+='b';return 'B';}}; var s='x'.concat(a,b); log==='ab' && s==='xAB';",
    );
    check(
        "var log=''; try{'x'.concat({toString:function(){log+='a';throw 9;}},{toString:function(){log+='b';return 'B';}});}catch(e){log+=e;} log==='a9';",
    );
    check("try{'x'.concat(Symbol('s'));}catch(e){e instanceof TypeError;}");
}
#[test]
fn fatal_result_limit_does_not_run_catch_or_later_coercions() {
    let mut vm = Runtime::default();
    let error=vm.execute("var seen=0; var a='x'.repeat(65536); try{a.concat(a,'x',{toString:function(){seen=1;return '';}});}catch(e){seen=2;}",&mut Empty).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert_eq!(vm.global_value("seen"), Some(Value::Number(0.)));
}
#[test]
fn numeric_diagnostic_never_contains_string_data() {
    let mut vm = Runtime::default();
    let error = vm
        .concat_strings(
            Value::string("PRIVATE".repeat(18000)),
            Value::string("SECRET".repeat(5000)),
        )
        .unwrap_err();
    let message = error.to_string();
    assert!(
        message.contains("left-bytes=126000 right-bytes=30000 result-bytes=156000 limit=131072"),
        "{message}"
    );
    assert!(!message.contains("PRIVATE"));
    assert!(!message.contains("SECRET"));
}
#[test]
fn length_and_method_lookup_do_not_allocate_full_string_copies() {
    let mut vm = Runtime::default();
    let value = Value::string("😀".repeat(30000));
    assert_eq!(
        vm.string_get_property(&value, "length").unwrap(),
        Some(Value::Number(60000.))
    );
    assert_eq!(
        vm.string_get_property(&value, "concat").unwrap(),
        Some(Value::Native("String.concat"))
    );
    assert_eq!(vm.string_bytes, 0);
    assert_eq!(
        vm.string_get_property(&value, "59999").unwrap(),
        Some(Value::Utf16(Arc::from([0xde00])))
    );
    assert_eq!(vm.string_bytes, 2);
}
#[test]
fn other_producer_and_host_limits_are_not_expanded() {
    for source in [
        "'x'.repeat(65537)",
        "'é'.repeat(40000)",
        "['x'.repeat(40000),'x'.repeat(40000)].join('')",
    ] {
        assert_eq!(
            Runtime::default()
                .execute(source, &mut Empty)
                .unwrap_err()
                .kind,
            ErrorKind::Resource,
            "{source}"
        );
    }
    let mut vm = Runtime::default();
    let value = vm
        .concat_strings(
            Value::string("x".repeat(40000)),
            Value::string("x".repeat(40000)),
        )
        .unwrap();
    assert_eq!(
        vm.coerce_to_string(value, &mut Empty).unwrap_err().kind,
        ErrorKind::Resource
    );
}

fn padded_source(length: usize, suffix: &str) -> Value {
    assert!(length >= suffix.len());
    Value::string(format!("{}{suffix}", " ".repeat(length - suffix.len())))
}
#[test]
fn eval_accepts_bounded_larger_source_without_widening_parser() {
    let mut vm = Runtime::default();
    vm.execute("var marker=1;", &mut Empty).unwrap();
    let result = vm
        .dynamic_eval(
            &[padded_source(128 * 1024, "marker=7;")],
            0,
            true,
            &mut Empty,
        )
        .unwrap();
    assert_eq!(result, Value::Number(7.));
    assert_eq!(vm.global_value("marker"), Some(Value::Number(7.)));
    let scopes = vm.scopes.len();
    let error = vm
        .dynamic_eval(
            &[padded_source(128 * 1024 + 1, "marker=9;")],
            0,
            true,
            &mut Empty,
        )
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert!(
        error
            .to_string()
            .contains("input-bytes=131073 limit=131072")
    );
    assert_eq!(vm.scopes.len(), scopes);
    assert_eq!(vm.global_value("marker"), Some(Value::Number(7.)));
}
#[test]
fn eval_total_source_budget_remains_256_kib() {
    let mut vm = Runtime::default();
    for _ in 0..2 {
        assert_eq!(
            vm.dynamic_eval(&[padded_source(128 * 1024, "7;")], 0, true, &mut Empty)
                .unwrap(),
            Value::Number(7.)
        );
    }
    assert_eq!(vm.dynamic_source_bytes, 256 * 1024);
    let scopes = vm.scopes.len();
    let error = vm
        .dynamic_eval(&[Value::string("0")], 0, true, &mut Empty)
        .unwrap_err();
    assert!(error.message.contains("256 KiB"));
    assert_eq!(vm.scopes.len(), scopes);
}
#[test]
fn oversized_tokens_and_syntax_do_not_mutate_eval_environments() {
    for (source, expected_kind) in [
        (
            format!("{}{}", " ".repeat(40000), ";".repeat(40961)),
            ErrorKind::Resource,
        ),
        (format!("{}var =", " ".repeat(70000)), ErrorKind::Syntax),
    ] {
        let mut vm = Runtime::default();
        let checkpoint = vm.clone();
        let scopes = vm.scopes.len();
        let bindings = vm.binding_count;
        let bytes = source.len();
        let error = vm
            .dynamic_eval(&[Value::string(source)], 0, true, &mut Empty)
            .unwrap_err();
        assert_eq!(error.kind, expected_kind);
        if expected_kind == ErrorKind::Resource {
            assert_eq!(error.message, "Script token budget exceeded");
        }
        assert_eq!(vm.scopes.len(), scopes);
        assert_eq!(vm.binding_count, bindings);
        assert_eq!(vm.dynamic_depth, 0);
        assert!(!vm.strict);
        assert_eq!(vm.dynamic_source_bytes, bytes);
        vm.rollback_to(checkpoint);
        assert_eq!(vm.dynamic_source_bytes, bytes);
        assert!(vm.string_bytes >= bytes);
    }
}
#[test]
fn large_eval_preserves_direct_indirect_and_strict_scope() {
    check(
        "var padding=' '.repeat(35000); var source=padding+padding+'local+3'; function run(){var local=2;return eval(source);} run()===5;",
    );
    check(
        "var local=8;var padding=' '.repeat(35000);var source=padding+padding+'local+3';function run(){var local=2;return (0,eval)(source);}run()===11;",
    );
    let mut vm = Runtime::default();
    vm.dynamic_eval(
        &[padded_source(70000, "'use strict';var hidden=9;")],
        0,
        true,
        &mut Empty,
    )
    .unwrap();
    assert_eq!(vm.global_value("hidden"), None);
    assert!(!vm.strict);
    assert_eq!(vm.dynamic_depth, 0);
}
#[test]
fn late_lexical_collision_has_no_partial_hoisting() {
    let mut vm = Runtime::default();
    vm.execute("let protectedName=1;", &mut Empty).unwrap();
    let scopes = vm.scopes.len();
    let error = vm
        .dynamic_eval(
            &[padded_source(70000, "var freshName=2;var protectedName=3;")],
            0,
            true,
            &mut Empty,
        )
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Syntax);
    assert_eq!(vm.global_value("freshName"), None);
    assert_eq!(vm.global_value("protectedName"), Some(Value::Number(1.)));
    assert_eq!(vm.scopes.len(), scopes);
}
#[test]
fn utf16_eval_conversion_is_reserved_and_function_limit_is_independent() {
    let mut vm = Runtime::default();
    let mut units = vec![0x20; 70000];
    units.extend("8;".encode_utf16());
    assert_eq!(
        vm.dynamic_eval(&[Value::Utf16(Arc::from(units))], 0, true, &mut Empty)
            .unwrap(),
        Value::Number(8.)
    );
    assert_eq!(vm.string_bytes, 70002);
    let mut vm = Runtime::new(Limits {
        string_bytes: 4,
        ..Limits::default()
    });
    assert_eq!(
        vm.dynamic_eval(
            &[Value::Utf16(Arc::from([0x20, 0x20, 0x20, 0x38, 0x3b]))],
            0,
            true,
            &mut Empty
        )
        .unwrap_err()
        .kind,
        ErrorKind::Resource
    );
    assert_eq!(vm.scopes.len(), 1);
    assert_eq!(
        Runtime::default()
            .dynamic_function(&[padded_source(65537, "return 1;")], &mut Empty)
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
    assert_eq!(
        Runtime::default()
            .dynamic_eval(&[Value::Utf16(Arc::from([0xd800]))], 0, true, &mut Empty)
            .unwrap_err()
            .kind,
        ErrorKind::Syntax
    );
}

#[test]
fn plus_bigint_reserves_decimal_work_before_building_the_operand() {
    let number = crate::BigInt::parse(&format!("0x{}", "f".repeat(1024))).unwrap();
    let mut vm = Runtime::new(Limits {
        instructions: 10,
        ..Limits::default()
    });
    let error = vm
        .concat_strings(Value::string(""), Value::BigInt(Arc::new(number)))
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert!(error.message.contains("BigInt decimal conversion work"));
    assert_eq!(vm.string_bytes, 0);
    check("''+12345678901234567890n==='12345678901234567890' && 'x'.concat(123n)==='x123';");
}
#[test]
fn string_budget_overflow_is_fatal_without_wrapping() {
    let mut vm = Runtime {
        string_bytes: usize::MAX,
        ..Runtime::default()
    };
    let error = vm
        .concat_strings(Value::string("a"), Value::string("b"))
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert_eq!(error.message, "String budget overflow");
    assert_eq!(vm.string_bytes, usize::MAX);
}

#[test]
fn utf16_reservation_uses_final_representation_before_allocating() {
    for budget in [11, 12] {
        let mut vm = Runtime::new(Limits {
            string_bytes: budget,
            ..Limits::default()
        });
        let result = vm.concat_strings(
            Value::Utf16(Arc::from([0xd800, 0x4e00])),
            Value::string("中"),
        );
        assert_eq!(result.is_ok(), budget == 12);
        assert_eq!(vm.string_bytes, 12);
    }
    let mut vm = Runtime::default();
    let value = vm
        .concat_strings(
            Value::string("中".repeat(40000)),
            Value::Utf16(Arc::from([0xd800])),
        )
        .unwrap();
    assert!(matches!(value,Value::Utf16(units) if units.len()==40001));
    assert_eq!(vm.string_bytes, 160004);
}
#[test]
fn eval_checks_encoded_bytes_before_allocating_utf16_conversion() {
    let mut vm = Runtime::default();
    let error = vm
        .dynamic_eval(
            &[Value::Utf16(Arc::from(vec![0x4e00; 50000]))],
            0,
            true,
            &mut Empty,
        )
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert!(
        error
            .to_string()
            .contains("input-bytes=150000 limit=131072")
    );
    assert_eq!(vm.string_bytes, 0);
    assert_eq!(vm.dynamic_source_bytes, 0);
    assert_eq!(vm.scopes.len(), 1);
}
#[test]
fn compound_assignment_and_resumed_execution_share_bounded_concatenation() {
    check("var a='x'.repeat(40000);a+=a;a.length===80000;");
    check(
        "function* f(){var a='x'.repeat(40000);var b=yield a;return a+b;}var g=f();var first=g.next();var second=g.next(first.value);second.done && second.value.length===80000;",
    );
    let mut vm = Runtime::default();
    vm.execute("var answer=0;async function f(){var a='x'.repeat(40000);var b=await Promise.resolve(a);return (a+b).length;}f().then(function(value){answer=value;});",&mut Empty).unwrap();
    assert_eq!(vm.global_value("answer"), Some(Value::Number(80000.)));
}

#[test]
fn oversized_native_operand_is_rejected_without_claiming_a_measured_result() {
    let mut vm = Runtime::default();
    let error = vm
        .concat_strings(
            Value::string("x".repeat(MAX_CONCAT_BYTES + 1)),
            Value::string(""),
        )
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert!(
        error
            .to_string()
            .contains("left-bytes=131073 right-bytes=0 operand-limit=131072")
    );
    assert!(!error.to_string().contains("result-bytes="));
    assert_eq!(vm.string_bytes, 0);
}
