use super::*;
use crate::{ErrorKind, Host, Limits, descriptors::Definition};

struct Empty;
impl Host for Empty {
    fn global(&mut self, _: &str) -> Option<Value> {
        None
    }
    fn get(&mut self, _: u64, _: &str) -> Result<Value, JsError> {
        Err(JsError::runtime("No host"))
    }
    fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
        Err(JsError::runtime("No host"))
    }
    fn call(&mut self, _: u64, _: &str, _: &[Value]) -> Result<Value, JsError> {
        Err(JsError::runtime("No host"))
    }
}
fn vm() -> Runtime {
    Runtime::new(Limits {
        instructions: 10_000_000,
        ..Limits::default()
    })
}
fn object(vm: &mut Runtime) -> Value {
    vm.object(BTreeMap::new(), None).unwrap()
}
fn id(value: &Value) -> usize {
    match value {
        Value::Object(id) | Value::Array(id) | Value::Function(id) => *id,
        _ => panic!("heap owner"),
    }
}
fn count(vm: &Runtime, value: &Value) -> usize {
    match &vm.heap[id(value)] {
        Heap::Object(object) => object.properties.len(),
        Heap::Function(function) => function.properties.len(),
        Heap::Array(_) => vm.array_properties.get(&id(value)).map_or(0, BTreeMap::len),
        _ => panic!("heap owner"),
    }
}
fn check(source: &str) {
    assert_eq!(
        vm().execute(source, &mut Empty).unwrap(),
        Value::Bool(true),
        "{source}"
    );
}

#[test]
fn moderate_objects_functions_and_named_arrays_work_under_default_limits() {
    for source in [
        "let o={};for(let i=0;i<300;i++){o['k'+i]=i;}Object.keys(o).length===300&&o.k299===299;",
        "let f=function(){};for(let i=0;i<300;i++){f['k'+i]=i;}Object.keys(f).length===300&&f.k299===299;",
        "let a=[];for(let i=0;i<300;i++){a['k'+i]=i;}a.length===0&&Object.keys(a).length===300&&a.k299===299;",
    ] {
        assert_eq!(
            Runtime::default().execute(source, &mut Empty).unwrap(),
            Value::Bool(true),
            "{source}"
        );
    }
}

#[test]
fn literals_json_spread_descriptors_and_suspended_calls_share_reservations() {
    let pairs = (0..300)
        .map(|i| format!("k{i}:{i}"))
        .collect::<Vec<_>>()
        .join(",");
    check(&format!(
        "let o={{{pairs}}};let p={{...o}};let q=Object.assign({{}},o);Object.keys(p).length===300&&q.k299===299;"
    ));
    check(&format!(
        "function* f(){{yield {{{pairs}}};}}let o=f().next().value;Object.keys(o).length===300&&o.k299===299;"
    ));
    check(
        "let d={};for(let i=0;i<300;i++){d['k'+i]={value:i,writable:true,enumerable:true,configurable:true};}let o={};Object.defineProperties(o,d);let p=Object.getOwnPropertyDescriptors(o);Object.keys(o).length===300&&p.k299.value===299;",
    );
    let json = (0..300)
        .map(|i| format!("\"k{i}\":{i}"))
        .collect::<Vec<_>>()
        .join(",");
    check(&format!(
        "let o=JSON.parse('{{{json},\"k0\":9}}');Object.keys(o).length===300&&o.k0===9&&o.k299===299;"
    ));
    check(
        "let o={};for(let i=0;i<300;i++){o['k'+i]=i;}o.k0=9;delete o.k1;o.k1=7;o[9]=1;o[2]=1;let keys=Object.keys(o);keys[0]==='2'&&keys[1]==='9'&&keys[2]==='k0'&&keys[keys.length-1]==='k1';",
    );
    check("let o={};for(let i=0;i<300;i++){o['k'+i]=i;}let n=0;for(let k in o){n++;}n===300;");
}

#[test]
fn per_owner_boundary_rejects_before_storage_order_or_metadata_growth() {
    for kind in 0..3 {
        let mut vm = vm();
        let value = match kind {
            0 => object(&mut vm),
            1 => vm.execute("(function(){});", &mut Empty).unwrap(),
            _ => Value::Array(vm.allocate(Heap::Array(vec![])).unwrap()),
        };
        for i in 0..MAX_NAMED_PROPERTIES {
            vm.set_unchecked(
                value.clone(),
                &format!("k{i}"),
                Value::Number(i as f64),
                &mut Empty,
            )
            .unwrap();
        }
        let owner = id(&value);
        let order = vm.property_order[&owner].clone();
        let reserved = vm.property_budget.reservations;
        let error = vm
            .set_unchecked(value.clone(), "overflow", Value::Null, &mut Empty)
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert_eq!(error.message, "Object property limit exceeded");
        assert_eq!(count(&vm, &value), MAX_NAMED_PROPERTIES);
        assert_eq!(vm.property_order[&owner], order);
        assert_eq!(vm.property_budget.reservations, reserved);
        vm.set_unchecked(value.clone(), "k0", Value::Null, &mut Empty)
            .unwrap();
        assert_eq!(vm.property_budget.reservations, reserved);
    }
}

#[test]
fn aggregate_reservations_bound_many_owners() {
    let mut vm = vm();
    for _ in 0..MAX_PROPERTY_RESERVATIONS / MAX_NAMED_PROPERTIES {
        let value = object(&mut vm);
        for i in 0..MAX_NAMED_PROPERTIES {
            vm.set_unchecked(value.clone(), &format!("k{i}"), Value::Null, &mut Empty)
                .unwrap();
        }
    }
    assert_eq!(vm.property_budget.reservations, MAX_PROPERTY_RESERVATIONS);
    let value = object(&mut vm);
    let heap = vm.heap.len();
    let error = vm
        .set_unchecked(value.clone(), "x", Value::Null, &mut Empty)
        .unwrap_err();
    assert_eq!(
        error.message,
        "JavaScript aggregate property count budget exceeded"
    );
    assert_eq!(count(&vm, &value), 0);
    assert_eq!(vm.heap.len(), heap);
    assert!(vm.property_order.get(&id(&value)).is_none_or(Vec::is_empty));
}

#[test]
fn key_bytes_are_preflighted_with_all_persistent_copies() {
    let mut vm = vm();
    let value = object(&mut vm);
    let suffix = "x".repeat(1020);
    for i in 0..2048 {
        vm.set_unchecked(
            value.clone(),
            &format!("{i:04}{suffix}"),
            Value::Null,
            &mut Empty,
        )
        .unwrap();
    }
    assert_eq!(vm.property_budget.key_bytes, MAX_PROPERTY_KEY_BYTES);
    let reserved = vm.property_budget.reservations;
    let error = vm
        .set_unchecked(value.clone(), "extra", Value::Null, &mut Empty)
        .unwrap_err();
    assert_eq!(
        error.message,
        "JavaScript aggregate property key byte budget exceeded"
    );
    assert_eq!(count(&vm, &value), 2048);
    assert_eq!(vm.property_budget.reservations, reserved);
    assert_eq!(
        vm.set_unchecked(value, "x".repeat(1025).as_str(), Value::Null, &mut Empty)
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
}

#[test]
fn delete_gc_and_rollback_never_refund_property_reservations_or_work() {
    let mut vm = vm();
    let value = object(&mut vm);
    let before = vm.clone();
    vm.set_unchecked(value.clone(), "x", Value::Null, &mut Empty)
        .unwrap();
    assert!(vm.delete_property(value.clone(), "x", &mut Empty).unwrap());
    vm.set_unchecked(value.clone(), "x", Value::Null, &mut Empty)
        .unwrap();
    assert_eq!(vm.property_budget.reservations, 2);
    vm.collect_garbage(&[], &Empty).unwrap();
    assert_eq!(vm.property_budget.reservations, 2);
    let consumed = vm.property_budget.clone();
    let remaining = vm.remaining;
    vm.rollback_to(before);
    assert_eq!(vm.property_budget.reservations, consumed.reservations);
    assert_eq!(vm.property_budget.key_bytes, consumed.key_bytes);
    assert_eq!(vm.property_budget.work, consumed.work);
    assert_eq!(vm.remaining, remaining);
    assert_eq!(count(&vm, &value), 0);
    vm.set_unchecked(value, "x", Value::Null, &mut Empty)
        .unwrap();
    assert_eq!(vm.property_budget.reservations, 3);
}

#[test]
fn low_instruction_and_aggregate_work_limits_reject_before_mutation() {
    let mut vm = vm();
    let value = object(&mut vm);
    let owner = id(&value);
    vm.remaining = 0;
    assert_eq!(
        vm.set_unchecked(value.clone(), "x", Value::Null, &mut Empty)
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
    assert_eq!(count(&vm, &value), 0);
    assert_eq!(vm.property_budget.reservations, 0);
    assert!(vm.property_order.get(&owner).is_none_or(Vec::is_empty));
    vm.remaining = 100;
    vm.property_budget.work = MAX_PROPERTY_WORK;
    assert_eq!(
        vm.set_unchecked(value.clone(), "x", Value::Null, &mut Empty)
            .unwrap_err()
            .message,
        "JavaScript aggregate property work budget exceeded"
    );
    assert_eq!(count(&vm, &value), 0);
}

#[test]
fn descriptor_only_keys_reserve_before_array_length_changes() {
    let mut vm = vm();
    let value = Value::Array(vm.allocate(Heap::Array(vec![])).unwrap());
    let owner = id(&value);
    vm.property_budget.reservations = MAX_PROPERTY_RESERVATIONS;
    let error = vm
        .define_property(
            &value,
            "length",
            Definition {
                value: Some(Value::Number(10.)),
                writable: Some(false),
                ..Definition::default()
            },
            &mut Empty,
        )
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert!(matches!(&vm.heap[owner],Heap::Array(values) if values.is_empty()));
    assert!(!vm.property_metadata.contains_key(&owner));
    let error = vm
        .define_property(
            &value,
            "4",
            Definition {
                value: Some(Value::Number(10.)),
                ..Definition::default()
            },
            &mut Empty,
        )
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert!(matches!(&vm.heap[owner],Heap::Array(values) if values.is_empty()));
    assert!(!vm.property_metadata.contains_key(&owner));
}

#[test]
fn aggregate_property_failure_is_fatal_to_catch_finally_and_pending_jobs() {
    let mut vm = vm();
    vm.execute(
        "var caught=false;var cleaned=false;var ran=false;var o={};",
        &mut Empty,
    )
    .unwrap();
    vm.execute_nested_script(
        "Promise.resolve(1).then(function(){ran=true;});",
        &mut Empty,
    )
    .unwrap();
    vm.property_budget.reservations = MAX_PROPERTY_RESERVATIONS;
    let error = vm
        .execute(
            "try{o.x=1;}catch(e){caught=true;}finally{cleaned=true;}ran=true;",
            &mut Empty,
        )
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    for name in ["caught", "cleaned", "ran"] {
        assert_eq!(vm.global_value(name), Some(Value::Bool(false)));
    }
}

#[test]
fn large_enumerations_preflight_before_cloning_and_exotics_remain_restricted() {
    let mut vm = vm();
    let value = object(&mut vm);
    for i in 0..300 {
        vm.set_unchecked(value.clone(), &format!("k{i}"), Value::Null, &mut Empty)
            .unwrap();
    }
    let strings = vm.string_bytes;
    vm.remaining = 300;
    assert_eq!(vm.own_names(&value).unwrap_err().kind, ErrorKind::Resource);
    assert_eq!(vm.string_bytes, strings);
    vm.remaining = 300;
    assert_eq!(
        vm.enumerable_keys(&value).unwrap_err().kind,
        ErrorKind::Resource
    );
    assert_eq!(vm.string_bytes, strings);
    for source in [
        "let o={};for(let i=0;i<129;i++){o[Symbol()]=i;}",
        "for(let i=0;i<129;i++){Number['p'+i]=i;}",
        "for(let i=0;i<129;i++){Symbol['p'+i]=i;}",
        "let a=new Uint8Array(1);for(let i=0;i<129;i++){a['p'+i]=i;}",
    ] {
        assert_eq!(
            self::vm().execute(source, &mut Empty).unwrap_err().kind,
            ErrorKind::Resource,
            "{source}"
        );
    }
}

#[test]
fn checked_counter_overflow_rejects_without_storage_growth() {
    for bytes in [false, true] {
        let mut vm = vm();
        let value = object(&mut vm);
        if bytes {
            vm.property_budget.key_bytes = usize::MAX;
        } else {
            vm.property_budget.reservations = usize::MAX;
        }
        assert_eq!(
            vm.set_unchecked(value.clone(), "x", Value::Null, &mut Empty)
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
        assert_eq!(count(&vm, &value), 0);
        assert!(vm.property_order[&id(&value)].is_empty());
    }
}

#[test]
fn deleting_large_order_is_budgeted_before_mutating_the_object() {
    let mut vm = vm();
    let value = object(&mut vm);
    for i in 0..300 {
        vm.set_unchecked(value.clone(), &format!("k{i}"), Value::Null, &mut Empty)
            .unwrap();
    }
    let order = vm.property_order[&id(&value)].clone();
    vm.remaining = 2;
    assert_eq!(
        vm.delete_property(value.clone(), "k0", &mut Empty)
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
    assert_eq!(count(&vm, &value), 300);
    assert_eq!(vm.property_order[&id(&value)], order);
}

#[test]
fn partial_json_builders_spend_reservations_without_publishing_an_object() {
    let mut vm = vm();
    vm.property_budget.reservations = MAX_PROPERTY_RESERVATIONS - 1;
    let error = vm
        .json_parse(&[Value::string("{\"first\":0,\"second\":0}")])
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert_eq!(
        error.message,
        "JavaScript aggregate property count budget exceeded"
    );
    assert_eq!(vm.property_budget.reservations, MAX_PROPERTY_RESERVATIONS);
    assert_eq!(vm.property_budget.key_bytes, "first".len() * KEY_COPIES);
    assert!(vm.heap.is_empty());
    vm.collect_garbage(&[], &Empty).unwrap();
    assert_eq!(vm.property_budget.reservations, MAX_PROPERTY_RESERVATIONS);
}

#[test]
fn array_index_descriptors_use_array_ceiling_and_aggregate_not_named_limit() {
    let mut vm = vm();
    let value = Value::Array(vm.allocate(Heap::Array(vec![])).unwrap());
    for index in 0..crate::MAX_ARRAY_LENGTH {
        vm.define_property(
            &value,
            &index.to_string(),
            Definition {
                value: Some(Value::Number(index as f64)),
                writable: Some(true),
                enumerable: Some(true),
                configurable: Some(true),
                ..Definition::default()
            },
            &mut Empty,
        )
        .unwrap();
    }
    vm.define_property(
        &value,
        "length",
        Definition {
            writable: Some(false),
            ..Definition::default()
        },
        &mut Empty,
    )
    .unwrap();
    assert_eq!(count(&vm, &value), 0);
    assert_eq!(vm.property_budget.reservations, crate::MAX_ARRAY_LENGTH + 1);
    assert_eq!(
        vm.property_metadata[&id(&value)].len(),
        crate::MAX_ARRAY_LENGTH + 1
    );
}

#[test]
fn builtin_property_preflight_does_not_publish_partial_array_prototype() {
    let mut vm = vm();
    vm.property_budget.reservations = MAX_PROPERTY_RESERVATIONS;
    assert_eq!(
        vm.array_prototype_value().unwrap_err().kind,
        ErrorKind::Resource
    );
    assert!(vm.array_prototype.is_none());
    assert!(vm.heap.is_empty());
    assert!(vm.array_properties.is_empty());
    assert!(vm.array_non_enumerable.is_empty());
}

#[test]
fn fatal_length_allocation_does_not_install_readonly_metadata_after_failure() {
    let mut vm = vm();
    let value = Value::Array(vm.allocate(Heap::Array(vec![])).unwrap());
    vm.remaining = 8;
    let error = vm
        .define_property(
            &value,
            "length",
            Definition {
                value: Some(Value::Number(10.)),
                writable: Some(false),
                ..Definition::default()
            },
            &mut Empty,
        )
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert!(matches!(&vm.heap[id(&value)], Heap::Array(values) if values.is_empty()));
    assert!(!vm.property_metadata.contains_key(&id(&value)));
    assert_eq!(vm.property_budget.reservations, 1);
}
