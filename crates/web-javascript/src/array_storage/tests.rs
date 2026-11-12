use super::*;
use crate::{ErrorKind, Heap, Host, Limits, Runtime, Value};
use std::collections::BTreeSet;
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
fn bitmap_matches_set_semantics_at_word_and_array_boundaries() {
    let mut holes = ArrayHoles::new();
    let mut reference = BTreeSet::new();
    for (start, end) in [(3, 66), (128, 129), (4490, 4500), (8190, 8192), (22, 7)] {
        holes.insert_range(start, end).unwrap();
        reference.extend(start..end);
        for index in 0..MAX_ARRAY_LENGTH {
            assert_eq!(holes.contains(&index), reference.contains(&index));
        }
    }
    for index in [0, 3, 63, 64, 128, 8191, usize::MAX] {
        holes.remove(&index);
        reference.remove(&index);
    }
    for length in [8192, 8191, 4500, 1025, 65, 64, 63, 1, 0] {
        holes.truncate(length);
        reference.retain(|index| *index < length);
        for index in 0..MAX_ARRAY_LENGTH {
            assert_eq!(holes.contains(&index), reference.contains(&index));
        }
        assert_eq!(holes.is_empty(), reference.is_empty());
    }
    let mut holes = ArrayHoles::full(64).unwrap();
    holes.truncate(1000);
    assert!(holes.contains(&63));
}
#[test]
fn absence_metadata_storage_is_bounded_without_tree_entry_per_hole() {
    for length in [0, 1, 64, 65, 4096, 4919, 7823, 8192] {
        let holes = ArrayHoles::full(length).unwrap();
        assert!(holes.words.capacity() * std::mem::size_of::<u64>() <= 1024);
        assert_eq!(holes.words.len(), length.div_ceil(64));
        assert_eq!(holes.is_empty(), length == 0);
        assert!(!holes.contains(&length));
        if length > 0 {
            assert!(holes.contains(&(length - 1)));
        }
    }
    let mut holes = ArrayHoles::full(4500).unwrap();
    holes.insert_range(4500, 8000).unwrap();
    assert!(holes.words.capacity() <= 128);
    let before = holes.words.clone();
    assert_eq!(holes.insert(8192).unwrap_err().kind, ErrorKind::Resource);
    assert_eq!(
        holes.insert_range(0, 8193).unwrap_err().kind,
        ErrorKind::Resource
    );
    assert_eq!(holes.words, before);
}
#[test]
fn larger_arrays_preserve_absence_undefined_prototypes_and_length() {
    check(
        "let a=Array(4919);a.length===4919&&!(0 in a)&&!(4918 in a)&&a[4918]===undefined&&a.join(',').length===4918;",
    );
    check(
        "let a=Array(8192);a[8191]=undefined;const own=Object.hasOwn(a,'8191');delete a[8191];own&&!Object.hasOwn(a,'8191')&&a.length===8192;",
    );
    check(
        "let a=[];a[4918]=7;const absent=!(4917 in a);a.length=5000;a[4999]=9;a.length=100;a.length=5000;absent&&a.length===5000&&!(4918 in a)&&!(4999 in a);",
    );
    check(
        "Array.prototype[4918]=7;let a=Array(4919);let inherited=a[4918]===7&&4918 in a&&!Object.hasOwn(a,'4918');delete Array.prototype[4918];inherited&&!(4918 in a);",
    );
}
#[test]
fn larger_array_methods_share_the_same_length_ceiling() {
    // Deliberately test several linear methods together under an explicit host
    // work budget; the product default is unchanged and tested separately.
    let check = |source: &str| {
        let mut vm = Runtime::new(Limits {
            instructions: 1_000_000,
            ..Limits::default()
        });
        assert_eq!(
            vm.execute(source, &mut Empty)
                .unwrap_or_else(|error| panic!("{source}: {error}")),
            Value::Bool(true),
            "{source}"
        );
    };
    check(
        "let a=Array(4919);a.fill(1);a[4918]=2;let b=a.slice();b.length===4919&&b[4918]===2&&b.includes(2)&&b.indexOf(2)===4918;",
    );
    check(
        "let a=Array(4919);a[4918]=4;let b=a.map(function(x){return x+1;});let c=a.concat([3]);b.length===4919&&!(4917 in b)&&b[4918]===5&&c.length===4920&&c[4919]===3;",
    );
    check("let a=Array(8191);a.push(8);a.length===8192&&a.pop()===8&&a.length===8191;");
    check("let a=Array(4919);a[4918]=4;a.reverse();a[0]===4&&!(4918 in a);");
    check(
        "let a=Array.from({length:4919});a.length===4919&&a[4918]===undefined&&Object.hasOwn(a,'4918');",
    );
    check(
        "let a=Array.from(Array(4919));a.length===4919&&Object.hasOwn(a,'4918')&&a[4918]===undefined;",
    );
}
#[test]
fn array_preflight_checks_work_and_aggregate_storage_before_allocation() {
    let mut vm = Runtime::new(Limits {
        instructions: 1,
        ..Limits::default()
    });
    let error = vm.native("Array", &[Value::Number(4919.)]).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert!(vm.heap.is_empty());
    assert_eq!(vm.array_slots, 0);
    let mut vm = Runtime::new(Limits {
        instructions: 1_000_000,
        ..Limits::default()
    });
    for _ in 0..8 {
        vm.native("Array", &[Value::Number(8192.)]).unwrap();
    }
    assert_eq!(vm.array_slots, MAX_ARRAY_SLOTS);
    let objects = vm.heap.len();
    assert_eq!(
        vm.native("Array", &[Value::Number(1.)]).unwrap_err().kind,
        ErrorKind::Resource
    );
    assert_eq!(vm.heap.len(), objects);
}
#[test]
fn rollback_and_collection_do_not_refund_array_reservations() {
    let mut vm = Runtime::default();
    let checkpoint = vm.clone();
    vm.native("Array", &[Value::Number(4919.)]).unwrap();
    vm.rollback_to(checkpoint);
    assert!(vm.heap.is_empty());
    assert_eq!(vm.array_slots, 4919);
    let value = vm.native("Array", &[Value::Number(4919.)]).unwrap();
    vm.collect_garbage(std::slice::from_ref(&value), &Empty)
        .unwrap();
    let Value::Array(id) = value else {
        panic!("array")
    };
    assert!(matches!(&vm.heap[id],Heap::Array(values) if values.len()==4919));
    assert!(vm.array_holes[&id].contains(&4918));
    vm.collect_garbage(&[], &Empty).unwrap();
    assert_eq!(vm.array_slots, 9838);
    assert!(vm.array_holes.is_empty());
}
#[test]
fn larger_array_limit_stays_fatal_without_widening_other_resource_budgets() {
    for source in [
        "try{Array(8193);}catch(e){42;}",
        "let a=[];a[8192]=1;",
        "let a=[];a.length=8193;",
        "Array(8192).push(1);",
        "Array(8192).concat([1]);",
        "Array.from({length:8193});",
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
    assert_eq!(
        crate::parser::parse(&";".repeat(32_769)).unwrap_err().kind,
        ErrorKind::Resource
    );
    assert_eq!(
        Runtime::default()
            .execute("'x'.repeat(65537);", &mut Empty)
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
}

#[test]
fn larger_array_methods_continue_to_debit_the_host_work_budget() {
    let mut vm = Runtime::new(Limits {
        instructions: 5000,
        ..Limits::default()
    });
    let error = vm
        .execute("Array(4919).join(',');", &mut Empty)
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert!(error.message.contains("instruction budget"));
}

#[test]
fn larger_arrays_do_not_grant_extra_callback_heap_or_scope_budget() {
    let mut vm = Runtime::new(Limits {
        instructions: 1_000_000,
        ..Limits::default()
    });
    let error = vm
        .execute(
            "Array.from({length:4919},function(v,i){return i;});",
            &mut Empty,
        )
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert!(error.message.contains("heap object budget") || error.message.contains("scope budget"));
}

#[test]
fn result_reservations_precede_getters_and_do_not_charge_twice() {
    for (expression, arguments) in [
        ("a.slice()", 0),
        ("a.concat()", 0),
        ("a.toReversed()", 0),
        ("a.toSorted()", 0),
        ("a.toSpliced()", 0),
        ("a.splice(0,1)", 2),
        ("a.unshift(7)", 1),
    ] {
        let mut vm = Runtime::default();
        vm.execute("var calls=0;var a=Array(2);Object.defineProperty(a,'0',{get:()=>{calls++;return 1;},configurable:true});",&mut Empty).unwrap();
        // Let the argument builder finish; result reservation must still fail
        // before any source getter runs.
        vm.array_slots = MAX_ARRAY_SLOTS - arguments;
        let error = vm.execute(expression, &mut Empty).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource, "{expression}");
        assert_eq!(
            vm.global_value("calls"),
            Some(Value::Number(0.)),
            "{expression}"
        );
    }
    for (expression, additional) in [
        ("a.slice()", 2),
        ("a.concat(3)", 1 + 3),
        ("a.toReversed()", 2),
        ("a.toSorted()", 2),
        ("a.toSpliced(1,0,3)", 3 + 3),
        ("a.map(x=>x)", 1 + 2),
        ("a.filter(x=>true)", 1 + 2),
        ("a.flat()", 2),
        ("Array.from(a)", 1 + 2),
    ] {
        let mut vm = Runtime::default();
        vm.execute("var a=[1,2];", &mut Empty).unwrap();
        let before = vm.array_slots;
        vm.execute(expression, &mut Empty)
            .unwrap_or_else(|error| panic!("{expression}: {error}"));
        assert_eq!(vm.array_slots, before + additional, "{expression}");
    }
}

#[test]
fn callbacks_cannot_steal_reserved_result_slots_and_errors_do_not_refund() {
    for (expression, arguments) in [
        ("a.map(()=>{calls++;Array(1);return 1;})", 1),
        ("Array.from(a,()=>{calls++;Array(1);return 1;})", 2),
    ] {
        let mut vm = Runtime::default();
        vm.execute("var calls=0;var a=[1];", &mut Empty).unwrap();
        vm.array_slots = MAX_ARRAY_SLOTS - arguments - 1;
        let error = vm.execute(expression, &mut Empty).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource, "{expression}");
        assert_eq!(vm.global_value("calls"), Some(Value::Number(1.)));
        assert_eq!(vm.array_slots, MAX_ARRAY_SLOTS);
    }
    let mut vm = Runtime::default();
    vm.execute("var a=[1,2];", &mut Empty).unwrap();
    let before = vm.array_slots;
    assert_eq!(
        vm.execute("a.map(()=>{throw 17;});", &mut Empty)
            .unwrap_err()
            .kind,
        ErrorKind::Runtime
    );
    // One callable argument and two reserved result slots, no refund on throw.
    assert_eq!(vm.array_slots, before + 1 + 2);
}

#[test]
fn foreach_and_filter_false_need_no_result_slots_but_filter_true_reserves_before_push() {
    for expression in [
        "a.forEach(()=>{calls++;})",
        "a.filter(()=>{calls++;return false;})",
    ] {
        let mut vm = Runtime::default();
        vm.execute("var calls=0;var a=[1,2];", &mut Empty).unwrap();
        // The outer callback argument is a real temporary Value slot.
        vm.array_slots = MAX_ARRAY_SLOTS - 1;
        vm.execute(expression, &mut Empty).unwrap();
        assert_eq!(vm.global_value("calls"), Some(Value::Number(2.)));
        assert_eq!(vm.array_slots, MAX_ARRAY_SLOTS);
    }
    let mut vm = Runtime::default();
    vm.execute("var calls=0;var a=[1,2];", &mut Empty).unwrap();
    vm.array_slots = MAX_ARRAY_SLOTS - 1;
    assert_eq!(
        vm.execute("a.filter(()=>{calls++;return true;});", &mut Empty)
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
    assert_eq!(vm.global_value("calls"), Some(Value::Number(1.)));
}

#[test]
fn partially_blocked_length_truncation_keeps_holes_across_word_boundary() {
    check(
        "let a=Array(130);Object.defineProperty(a,'64',{value:7,configurable:false});a[129]=9;let blocked=false;try{a.length=1;}catch(e){blocked=e.name==='TypeError';}let kept=blocked&&a.length===65&&a[64]===7&&!(63 in a)&&!(129 in a);a.length=130;kept&&a.length===130&&!(129 in a);",
    );
}
