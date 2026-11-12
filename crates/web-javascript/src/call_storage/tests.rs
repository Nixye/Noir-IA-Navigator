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
fn list(count: usize, prefix: &str) -> String {
    (0..count)
        .map(|n| format!("{prefix}{n}"))
        .collect::<Vec<_>>()
        .join(",")
}
fn check(source: &str) {
    assert_eq!(
        Runtime::default()
            .execute(source, &mut Empty)
            .unwrap_or_else(|e| panic!("{source}: {e}")),
        Value::Bool(true),
        "{source}"
    );
}
fn resource(source: &str) {
    assert_eq!(
        Runtime::default()
            .execute(source, &mut Empty)
            .unwrap_err()
            .kind,
        ErrorKind::Resource,
        "{source}"
    );
}

#[test]
fn anonymous_functions_accept_thirty_four_and_sixty_four_parameters() {
    assert_eq!(MAX_FUNCTION_PARAMETERS, 64);
    assert_eq!(MAX_CALL_ARGUMENTS, 64);
    for n in [34, 64] {
        let names = list(n, "p");
        let args = list(64, "");
        check(&format!(
            "var f=function({names}){{return p{}=== {} && arguments.length===64 && arguments[63]===63;}};f.length==={n}&&f({args});",
            n - 1,
            n - 1
        ));
        check(&format!(
            "var f=({names})=>p{};f.length==={n}&&f({})==={};",
            n - 1,
            list(n, ""),
            n - 1
        ));
    }
}

#[test]
fn parameter_and_argument_sixty_five_are_fatal() {
    let names = list(65, "p");
    for source in [
        format!("(function({names}){{}});"),
        format!("({names})=>0;"),
        format!("function f({names}){{}}"),
        format!("class C{{m({names}){{}}}}"),
        format!("function* f({names}){{yield 1;}}"),
        format!("async function f({names}){{return 1;}}"),
        format!(
            "try{{(function(){{}})({});}}catch(e){{true;}}",
            list(65, "")
        ),
    ] {
        resource(&source);
    }
}

#[test]
fn defaults_rest_and_function_length_preserve_semantics() {
    let names = list(33, "p");
    check(&format!(
        "var f=function({names},last=99,...rest){{return last===99&&rest.length===0&&arguments.length===33;}};f.length===33&&f({});",
        list(33, "")
    ));
    check(&format!(
        "var f=function({names},...rest){{return rest.length===31&&rest[0]===33&&rest[30]===63&&arguments.length===64;}};f.length===33&&f({});",
        list(64, "")
    ));
    check(&format!(
        "var f=function({names},last=99){{return last===33&&arguments.length===64;}};f({});",
        list(64, "")
    ));
}

#[test]
fn adapters_reflect_and_constructors_accept_the_new_boundary() {
    let args = list(64, "");
    let names = list(64, "p");
    for expression in [
        format!("f({args})"),
        format!("f.apply(null,[{args}])"),
        format!("Reflect.apply(f,null,[{args}])"),
        format!(
            "f.bind(null,{})({})",
            list(32, ""),
            (32..64)
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join(",")
        ),
    ] {
        check(&format!(
            "function f({names}){{return p63===63&&arguments.length===64;}}{expression};"
        ));
    }
    check(&format!(
        "function f({}){{return p62===62&&arguments.length===63;}}f.call(null,{});",
        list(63, "p"),
        list(63, "")
    ));
    for expression in [
        format!("new C({args})"),
        format!("Reflect.construct(C,[{args}])"),
        format!(
            "new (C.bind(null,{}))({})",
            list(32, ""),
            (32..64)
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join(",")
        ),
    ] {
        check(&format!(
            "function C({names}){{this.last=p63;this.count=arguments.length;}}var c={expression};c.last===63&&c.count===64;"
        ));
    }
    check(&format!(
        "class B{{constructor({names}){{this.last=p63;this.count=arguments.length;}}}}class C extends B{{constructor({names}){{super({});}}}}var c=new C({args});c.last===63&&c.count===64;",
        list(64, "p")
    ));
    check(&format!(
        "class B{{constructor({names}){{this.last=p63;}}}}class C extends B{{}}new C({args}).last===63;"
    ));
}

#[test]
fn adapters_and_bound_argument_merging_reject_sixty_five() {
    for source in [
        "function f(){}f.apply(null,{length:65});".into(),
        "function f(){}Reflect.apply(f,null,{length:65});".into(),
        "function C(){}Reflect.construct(C,{length:65});".into(),
        format!(
            "function f(){{}}f.bind(null,{})({},64);",
            list(32, ""),
            (32..64)
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join(",")
        ),
        format!(
            "function C(){{}}new (C.bind(null,{}))({},64);",
            list(32, ""),
            (32..64)
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join(",")
        ),
    ] {
        resource(&source);
    }
}

#[test]
fn generator_and_async_calls_accept_sixty_four_and_preserve_rest() {
    let args = list(64, "");
    let names = list(33, "p");
    check(&format!(
        "function* g({names},...rest){{yield rest.length;return rest[30];}}var i=g({args});i.next().value===31&&i.next().value===63;"
    ));
    let mut vm = Runtime::default();
    vm.execute(&format!("var result=0;async function f({names},...rest){{return rest.length+rest[30];}}f({args}).then(v=>{{result=v;}});"),&mut Empty).unwrap();
    assert_eq!(vm.global_value("result"), Some(Value::Number(94.)));
}

#[test]
fn value_buffers_share_the_existing_aggregate_budget_and_do_not_refund() {
    let mut vm = Runtime::default();
    let checkpoint = vm.clone();
    let arguments = vec![Value::Undefined; 64];
    for _ in 0..1024 {
        let values = vm.copy_argument_values(&arguments).unwrap();
        assert_eq!(values.len(), 64);
    }
    assert_eq!(vm.array_slots, 65_536);
    assert_eq!(vm.value_buffer(1).unwrap_err().kind, ErrorKind::Resource);
    vm.rollback_to(checkpoint);
    vm.collect_garbage(&[], &Empty).unwrap();
    assert_eq!(vm.array_slots, 65_536);
    assert_eq!(vm.value_buffer(1).unwrap_err().kind, ErrorKind::Resource);
}

#[test]
fn reservation_failure_precedes_vector_growth_or_value_copy() {
    let mut vm = Runtime::default();
    let mut values = Vec::new();
    for _ in 0..64 {
        vm.reserve_argument_slot(&mut values).unwrap();
        values.push(Value::Undefined);
    }
    let capacity = values.capacity();
    assert_eq!(
        vm.reserve_argument_slot(&mut values).unwrap_err().kind,
        ErrorKind::Resource
    );
    assert_eq!(values.len(), 64);
    assert_eq!(values.capacity(), capacity);
    assert_eq!(vm.array_slots, 64);
    vm.array_slots = 65_536;
    let mut empty = Vec::new();
    assert_eq!(
        vm.reserve_argument_slot(&mut empty).unwrap_err().kind,
        ErrorKind::Resource
    );
    assert_eq!(empty.capacity(), 0);
    assert_eq!(
        vm.copy_argument_values(&[Value::string("retained")])
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
}

#[test]
fn apply_preflights_length_and_storage_before_indexed_getters() {
    for length in [64, 65] {
        let mut vm = Runtime::default();
        vm.execute(&format!("var calls=0;function f(){{calls+=100;}}var a={{length:{length}}};Object.defineProperty(a,'0',{{get:function(){{calls++;return 1;}}}});"),&mut Empty).unwrap();
        if length == 64 {
            vm.array_slots = 65_534;
        }
        assert_eq!(
            vm.execute("f.apply(null,a);", &mut Empty).unwrap_err().kind,
            ErrorKind::Resource
        );
        assert_eq!(vm.global_value("calls"), Some(Value::Number(0.)));
    }
}

#[test]
fn call_storage_exhaustion_precedes_parameter_default_effects() {
    let mut vm = Runtime::default();
    let callback = vm
        .execute(
            "var effects=0;(function(a=(effects++,1),...rest){effects+=100;});",
            &mut Empty,
        )
        .unwrap();
    vm.array_slots = 65_536;
    assert_eq!(
        vm.invoke(callback, vec![Value::Undefined], &mut Empty)
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
    assert_eq!(vm.global_value("effects"), Some(Value::Number(0.)));
}

#[test]
fn supplied_argument_effects_stop_before_the_sixty_fifth() {
    let mut vm = Runtime::default();
    vm.execute(
        "var effects=0;function bump(){effects++;return effects;}function f(){}",
        &mut Empty,
    )
    .unwrap();
    let source = format!("f({});", vec!["bump()"; 64].join(","));
    vm.execute(&source, &mut Empty).unwrap();
    assert_eq!(vm.global_value("effects"), Some(Value::Number(64.)));
    let source = format!("f({});", vec!["bump()"; 65].join(","));
    assert_eq!(
        vm.execute(&source, &mut Empty).unwrap_err().kind,
        ErrorKind::Resource
    );
    // The syntactic argument ceiling rejects this program before evaluation.
    assert_eq!(vm.global_value("effects"), Some(Value::Number(64.)));
}

#[test]
fn one_mib_string_budget_is_not_enlarged_by_parameter_support() {
    let mut vm = Runtime::new(Limits {
        string_bytes: 1024 * 1024,
        ..Limits::default()
    });
    let callback = vm
        .execute(
            &format!("(function({}){{return 1;}});", list(64, "parameter")),
            &mut Empty,
        )
        .unwrap();
    vm.string_bytes = 1024 * 1024;
    assert_eq!(
        vm.invoke(callback, vec![Value::Undefined; 64], &mut Empty)
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
    assert!(vm.string_bytes >= 1024 * 1024);
}

#[test]
fn manual_call_ast_stops_before_evaluating_argument_sixty_five() {
    use crate::Expr;
    let mut vm = Runtime::default();
    vm.execute("var effects=0;var targets=0;function bump(){effects++;return effects;}function f(){targets++;}", &mut Empty).unwrap();
    let argument = Expr::Call(Box::new(Expr::Name("bump".into())), vec![]);
    let expression = Expr::Call(Box::new(Expr::Name("f".into())), vec![argument; 65]);
    assert_eq!(
        vm.eval(&expression, 0, &mut Empty).unwrap_err().kind,
        ErrorKind::Resource
    );
    assert_eq!(vm.global_value("effects"), Some(Value::Number(64.)));
    assert_eq!(vm.global_value("targets"), Some(Value::Number(0.)));
}

#[test]
fn spread_overflow_stops_at_sixty_fifth_next_without_close_or_target() {
    let mut vm = Runtime::default();
    vm.execute("var nexts=0;var closes=0;var targets=0;function f(){targets++;}var iterator={next:function(){nexts++;return {value:nexts,done:false};},return:function(){closes++;return {done:true};}};var iterable={};iterable[Symbol.iterator]=function(){return iterator;};", &mut Empty).unwrap();
    assert_eq!(
        vm.execute("f(...iterable);", &mut Empty).unwrap_err().kind,
        ErrorKind::Resource
    );
    assert_eq!(vm.global_value("nexts"), Some(Value::Number(65.)));
    assert_eq!(vm.global_value("closes"), Some(Value::Number(0.)));
    assert_eq!(vm.global_value("targets"), Some(Value::Number(0.)));
}

#[test]
fn apply_and_reflect_preflight_exhausted_storage_before_indexed_getters() {
    for method in ["apply", "Reflect.apply", "Reflect.construct"] {
        let mut vm = Runtime::default();
        vm.execute("var effects=0;function f(){effects+=100;}var a={length:1};Object.defineProperty(a,'0',{get:function(){effects++;return 42;}});", &mut Empty).unwrap();
        let target = vm.global_value("f").unwrap();
        let array_like = vm.global_value("a").unwrap();
        vm.array_slots = 65_536;
        let result = match method {
            "apply" => {
                vm.callable_adapter("apply", target, vec![Value::Null, array_like], &mut Empty)
            }
            "Reflect.apply" => vm
                .reflect_call(
                    method,
                    &[target, Value::Null, array_like],
                    false,
                    &mut Empty,
                )
                .map(|value| value.unwrap()),
            _ => vm
                .reflect_call(method, &[target, array_like], false, &mut Empty)
                .map(|value| value.unwrap()),
        };
        assert_eq!(result.unwrap_err().kind, ErrorKind::Resource, "{method}");
        assert_eq!(
            vm.global_value("effects"),
            Some(Value::Number(0.)),
            "{method}"
        );
        assert_eq!(vm.array_slots, 65_536);
    }
}

#[test]
fn binding_adopts_reserved_argument_buffer_without_duplicate_charge() {
    let mut vm = Runtime::default();
    let target = vm
        .execute("(function(a,b){return a+b;});", &mut Empty)
        .unwrap();
    let arguments = vm
        .copy_argument_values(&[Value::Number(20.), Value::Number(22.)])
        .unwrap();
    let storage = arguments.as_ptr();
    let before = vm.array_slots;
    let bound = vm
        .bind_function(target, Value::Null, arguments, &mut Empty)
        .unwrap();
    assert_eq!(vm.array_slots, before);
    let Value::Function(id) = bound else {
        panic!("bound function");
    };
    assert_eq!(vm.bound_functions[&id].arguments.as_ptr(), storage);
    assert_eq!(vm.bound_functions[&id].arguments.len(), 2);
}

#[test]
fn arguments_and_rest_reserve_storage_before_materializing_arrays() {
    for source in ["(function(a,b){effects++;});", "((...rest)=>{effects++;});"] {
        let mut vm = Runtime::default();
        vm.execute("var effects=0;", &mut Empty).unwrap();
        let callback = vm.execute(source, &mut Empty).unwrap();
        let arrays_before = vm
            .heap
            .iter()
            .filter(|v| matches!(v, crate::Heap::Array(_)))
            .count();
        vm.array_slots = 65_535;
        assert_eq!(
            vm.call(
                callback,
                vec![Value::Number(1.), Value::Number(2.)],
                Value::Undefined,
                &mut Empty
            )
            .unwrap_err()
            .kind,
            ErrorKind::Resource,
            "{source}"
        );
        assert_eq!(
            vm.heap
                .iter()
                .filter(|v| matches!(v, crate::Heap::Array(_)))
                .count(),
            arrays_before,
            "{source}"
        );
        assert_eq!(vm.array_slots, 65_535);
        assert_eq!(vm.global_value("effects"), Some(Value::Number(0.)));
    }
}

#[test]
fn incremental_buffers_charge_geometric_capacity_before_growth() {
    let mut vm = Runtime::default();
    let mut values = Vec::new();
    for index in 0..64 {
        let expected = match index {
            0..=3 => 4,
            4..=7 => 8,
            8..=15 => 16,
            16..=31 => 32,
            _ => 64,
        };
        vm.reserve_argument_slot(&mut values).unwrap();
        assert_eq!(vm.array_slots, expected, "argument {}", index + 1);
        assert_eq!(values.capacity(), expected);
        values.push(Value::Undefined);
    }
    assert_eq!(vm.array_slots, 64);
    for length in [0, 4, 8, 16, 32] {
        let mut vm = Runtime::default();
        let mut values = vm.value_buffer(length).unwrap();
        values.resize(length, Value::Undefined);
        let pointer = values.as_ptr();
        let capacity = values.capacity();
        let growth = if length == 0 { 4 } else { length };
        vm.array_slots = 65_536 - growth + 1;
        let before = vm.array_slots;
        assert_eq!(
            vm.reserve_argument_slot(&mut values).unwrap_err().kind,
            ErrorKind::Resource
        );
        assert_eq!(vm.array_slots, before);
        assert_eq!(values.capacity(), capacity);
        assert_eq!(values.as_ptr(), pointer);
        assert_eq!(values.len(), length);
    }
}

#[test]
fn suspended_sixty_four_argument_call_keeps_objects_across_gc_without_replay() {
    let mut vm = Runtime::default();
    let arguments = (0..64)
        .map(|index| match index {
            16 => "(yield 7)".to_owned(),
            48 => "(yield 8)".to_owned(),
            _ => format!("make({index})"),
        })
        .collect::<Vec<_>>()
        .join(",");
    vm.execute(&format!("var effects=0;function make(n){{effects++;return {{n:n}};}}function target({}){{return arguments.length===64&&p0.n===0&&p16.n===16&&p48.n===48&&p63.n===63;}}function* run(){{return target({arguments});}}var generator=run();",list(64,"p")), &mut Empty).unwrap();
    assert_eq!(
        vm.execute("generator.next().value;", &mut Empty).unwrap(),
        Value::Number(7.)
    );
    assert_eq!(vm.global_value("effects"), Some(Value::Number(16.)));
    vm.collect_garbage(&[], &Empty).unwrap();
    assert_eq!(
        vm.execute("generator.next({n:16}).value;", &mut Empty)
            .unwrap(),
        Value::Number(8.)
    );
    assert_eq!(vm.global_value("effects"), Some(Value::Number(47.)));
    vm.collect_garbage(&[], &Empty).unwrap();
    assert_eq!(
        vm.execute(
            "var result=generator.next({n:48});result.done&&result.value;",
            &mut Empty
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(vm.global_value("effects"), Some(Value::Number(62.)));
}

#[test]
fn pattern_binding_preflight_rejects_before_scope_names_defaults_or_body() {
    use crate::parser::{Parameter, Pattern};
    use std::sync::Arc;
    for (pattern_count, names_per_pattern, exhausted_bytes, expected_message) in [
        (1, 129, false, "Pattern binding limit"),
        (8, 128, false, "Bindings per scope"),
        (1, 128, true, "string allocation budget"),
    ] {
        let mut vm = Runtime::default();
        let callback = vm.execute("var effects=0;function effect(){effects++;return [];} (function(){effects+=100;});", &mut Empty).unwrap();
        let Value::Function(id) = callback else {
            panic!("function");
        };
        let parameters = (0..pattern_count)
            .map(|pattern_index| Parameter {
                pattern: Pattern::Default(
                    Box::new(Pattern::Array(
                        (0..names_per_pattern)
                            .map(|index| {
                                Some(Pattern::Name(format!("binding_{pattern_index}_{index}")))
                            })
                            .collect(),
                        None,
                    )),
                    Box::new(crate::Expr::Call(
                        Box::new(crate::Expr::Name("effect".into())),
                        vec![],
                    )),
                ),
                rest: false,
            })
            .collect();
        let crate::Heap::Function(function) = &mut vm.heap[id] else {
            panic!("function");
        };
        function.parameters = Some(Arc::new(parameters));
        if exhausted_bytes {
            vm.string_bytes = vm.limits.string_bytes;
        }
        let before = (
            vm.scopes.len(),
            vm.heap.len(),
            vm.binding_count,
            vm.string_bytes,
            vm.array_slots,
        );
        let error = vm
            .call(Value::Function(id), vec![], Value::Undefined, &mut Empty)
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert!(error.message.contains(expected_message), "{error}");
        assert_eq!(
            (
                vm.scopes.len(),
                vm.heap.len(),
                vm.binding_count,
                vm.string_bytes,
                vm.array_slots
            ),
            before
        );
        assert_eq!(vm.global_value("effects"), Some(Value::Number(0.)));
    }
}

#[test]
fn repeated_arrow_calls_exhaust_argument_storage_without_any_js_arrays() {
    let mut vm = Runtime::new(Limits {
        instructions: 10_000_000,
        ..Limits::default()
    });
    let callback = vm.execute("var f=p0=>p0;f;", &mut Empty).unwrap();
    let checkpoint = vm.clone();
    let expressions = vec![crate::Expr::Literal(Value::Number(42.)); 64];
    for _ in 0..1024 {
        let arguments = vm.argument_values(&expressions, 0, &mut Empty).unwrap();
        assert_eq!(
            vm.call(callback.clone(), arguments, Value::Undefined, &mut Empty)
                .unwrap(),
            Value::Number(42.)
        );
    }
    assert_eq!(vm.array_slots, 65_536);
    assert!(
        !vm.heap
            .iter()
            .any(|value| matches!(value, crate::Heap::Array(_)))
    );
    vm.collect_garbage(&[], &Empty).unwrap();
    assert_eq!(vm.array_slots, 65_536);
    assert_eq!(
        vm.argument_values(&expressions, 0, &mut Empty)
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
    vm.rollback_to(checkpoint);
    assert_eq!(vm.array_slots, 65_536);
    assert_eq!(
        vm.argument_values(&expressions, 0, &mut Empty)
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
}

#[test]
fn thirty_four_and_sixty_four_parameters_execute_on_one_mib_stack() {
    std::thread::Builder::new()
        .name("bounded-call-stack".into())
        .stack_size(1024 * 1024)
        .spawn(|| {
            for count in [34, 64] {
                let source = format!(
                    "(function({}){{return p{};}})({});",
                    list(count, "p"),
                    count - 1,
                    list(count, "")
                );
                crate::parser::parse(&source).unwrap();
                assert_eq!(
                    Runtime::default().execute(&source, &mut Empty).unwrap(),
                    Value::Number((count - 1) as f64)
                );
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn external_invoke_adopts_capacity_and_separately_reserves_gc_roots() {
    for with_this in [false, true] {
        let mut vm = Runtime::default();
        let callback = vm.execute("value=>value;", &mut Empty).unwrap();
        let mut arguments = Vec::with_capacity(64);
        arguments.push(Value::Number(42.));
        let before = vm.array_slots;
        let result = if with_this {
            vm.invoke_with_this(callback, arguments, Value::Null, &mut Empty)
        } else {
            vm.invoke(callback, arguments, &mut Empty)
        };
        assert_eq!(result.unwrap(), Value::Number(42.));
        assert_eq!(vm.array_slots - before, if with_this { 67 } else { 66 });
        assert!(
            !vm.heap
                .iter()
                .any(|value| matches!(value, crate::Heap::Array(_)))
        );
    }
}

#[test]
fn oversized_external_argument_capacity_fails_before_callback_body() {
    for with_this in [false, true] {
        let mut vm = Runtime::default();
        let callback = vm
            .execute(
                "var effects=0;value=>{effects++;return value;};",
                &mut Empty,
            )
            .unwrap();
        let mut arguments = Vec::with_capacity(8193);
        arguments.push(Value::Number(42.));
        let before = vm.array_slots;
        let result = if with_this {
            vm.invoke_with_this(callback, arguments, Value::Null, &mut Empty)
        } else {
            vm.invoke(callback, arguments, &mut Empty)
        };
        assert_eq!(result.unwrap_err().kind, ErrorKind::Resource);
        assert_eq!(vm.array_slots, before);
        assert_eq!(vm.global_value("effects"), Some(Value::Number(0.)));
    }
}

#[test]
fn dynamic_function_builds_sixty_long_parameter_fragments_with_bounded_storage() {
    let names = (0..60)
        .map(|index| format!("parameter_{}_{index}", "x".repeat(64)))
        .collect::<Vec<_>>();
    let mut fragments = names.iter().map(Value::string).collect::<Vec<_>>();
    let body = format!("return {}+{};", names[0], names[59]);
    fragments.push(Value::string(&body));
    let mut vm = Runtime::default();
    let callback = vm.dynamic_function(&fragments, &mut Empty).unwrap();
    assert_eq!(
        vm.get(callback.clone(), "length", &mut Empty).unwrap(),
        Value::Number(60.)
    );
    assert_eq!(
        vm.dynamic_source_bytes,
        names.iter().map(String::len).sum::<usize>() + 59 + body.len()
    );
    assert!(vm.dynamic_source_bytes < 65_536);
    assert_eq!(
        vm.call(
            callback,
            (0..60).map(|value| Value::Number(value as f64)).collect(),
            Value::Undefined,
            &mut Empty
        )
        .unwrap(),
        Value::Number(59.)
    );
    let mut vm = Runtime::new(Limits {
        string_bytes: 0,
        ..Limits::default()
    });
    let before = (vm.heap.len(), vm.scopes.len(), vm.binding_count);
    assert_eq!(
        vm.dynamic_function(&fragments, &mut Empty)
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
    assert_eq!((vm.heap.len(), vm.scopes.len(), vm.binding_count), before);
    assert_eq!(vm.dynamic_source_bytes, 0);
}

#[test]
fn duplicate_var_pattern_names_charge_once_but_still_count_toward_width() {
    use crate::parser::Pattern;
    let source = format!(
        "{}for(var [x,x] of []){{}}true;",
        (0..1023)
            .map(|index| format!("var v{index};"))
            .collect::<String>()
    );
    let mut vm = Runtime::default();
    assert_eq!(vm.execute(&source, &mut Empty).unwrap(), Value::Bool(true));
    assert_eq!(vm.scopes[0].bindings.len(), 1024);
    assert_eq!(vm.binding_count, 1024);
    assert!(vm.scopes[0].bindings.contains_key("x"));

    let duplicate = Pattern::Array(vec![Some(Pattern::Name("x".into())); 2], None);
    let mut vm = Runtime::new(Limits {
        string_bytes: 1,
        ..Limits::default()
    });
    vm.binding_count = 16_383;
    vm.declare_pattern(&duplicate, crate::Declaration::Var, 0)
        .unwrap();
    assert_eq!(vm.binding_count, 16_384);
    assert_eq!(vm.string_bytes, 1);
    assert_eq!(vm.scopes[0].bindings.len(), 1);
    assert!(vm.scopes[0].bindings.contains_key("x"));
    // Redeclaration of the same var needs no new slot or string storage.
    vm.declare_pattern(&duplicate, crate::Declaration::Var, 0)
        .unwrap();
    assert_eq!(vm.binding_count, 16_384);
    assert_eq!(vm.string_bytes, 1);

    let oversized = Pattern::Array(vec![Some(Pattern::Name("x".into())); 129], None);
    let mut vm = Runtime::default();
    let error = vm
        .declare_pattern(&oversized, crate::Declaration::Var, 0)
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert!(error.message.contains("Pattern binding limit"));
    assert!(vm.scopes[0].bindings.is_empty());
    assert_eq!(vm.binding_count, 0);
    assert_eq!(vm.string_bytes, 0);
    assert!(vm.heap.is_empty());
}

#[test]
fn boxed_string_forwarding_reserves_before_argument_conversion() {
    let mut vm = Runtime::default();
    let argument = vm
        .execute(
            "var effects=0;({toString:function(){effects++;return 'suffix';}});",
            &mut Empty,
        )
        .unwrap();
    let boxed = vm.box_value(Value::string("prefix")).unwrap();
    let heap_before = vm.heap.len();
    let bytes_before = vm.string_bytes;
    vm.array_slots = 65_536;
    let error = vm
        .reflection_call("String.concat", &[argument], boxed, false, &mut Empty)
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert_eq!(vm.global_value("effects"), Some(Value::Number(0.)));
    assert_eq!(vm.array_slots, 65_536);
    assert_eq!(vm.heap.len(), heap_before);
    assert_eq!(vm.string_bytes, bytes_before);
}

#[test]
fn typed_array_of_reserves_forwarded_values_before_allocation_or_conversion() {
    let mut vm = Runtime::default();
    let argument = vm
        .execute(
            "var effects=0;({valueOf:function(){effects++;return 42;}});",
            &mut Empty,
        )
        .unwrap();
    let heap_before = vm.heap.len();
    let bytes_before = vm.string_bytes;
    vm.array_slots = 65_536;
    let error = vm
        .stdlib_call(
            "TypedArray.of",
            &[argument],
            Value::Native("Uint8Array"),
            false,
            &mut Empty,
        )
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert_eq!(vm.global_value("effects"), Some(Value::Number(0.)));
    assert_eq!(vm.array_slots, 65_536);
    assert_eq!(vm.heap.len(), heap_before);
    assert_eq!(vm.string_bytes, bytes_before);
}

#[test]
fn regexp_replace_reserves_callback_arguments_before_callback_effects() {
    let mut vm = Runtime::default();
    vm.execute(
        "var effects=0;var pattern=/a/;function replacement(){effects++;return 'b';}",
        &mut Empty,
    )
    .unwrap();
    vm.array_slots = 65_534;
    assert_eq!(
        vm.execute("'a'.replace(pattern,replacement);", &mut Empty)
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
    assert_eq!(vm.array_slots, 65_536);
    assert_eq!(vm.global_value("effects"), Some(Value::Number(0.)));
}
