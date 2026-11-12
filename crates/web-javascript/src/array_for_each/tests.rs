use crate::{ErrorKind, Host, JsError, Runtime, Value};
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
    let mut vm = Runtime::default();
    assert_eq!(
        vm.execute(source, &mut Empty).unwrap(),
        Value::Bool(true),
        "{source}"
    );
    assert!(vm.gc_roots.is_empty());
}
#[test]
fn generic_object_arguments_return_and_intrinsic_identity() {
    check(
        "const o={0:'a',1:'b',length:2};let text='';const context={x:4};const result=Array.prototype.forEach.call(o,function(v,k,source){text+=v+k+this.x+(source===o);},context);text==='a04trueb14true'&&result===undefined&&Array.prototype.forEach.length===1;",
    );
}
#[test]
fn length_get_and_conversion_precede_callback_validation() {
    check(
        "let log='';const o={};Object.defineProperty(o,'length',{get:function(){log+='g';return {valueOf:function(){log+='v';return 0;}};}});let errors=0;try{Array.prototype.forEach.call(o,null);}catch(e){errors++;}log==='gv'&&errors===1;",
    );
}
#[test]
fn null_receiver_fails_before_callback_and_length_is_read_once() {
    check(
        "let errors=0;try{Array.prototype.forEach.call(null,null);}catch(e){errors++;}try{Array.prototype.forEach.call(undefined,null);}catch(e){errors++;}let reads=0;const o={0:'x',1:'y'};Object.defineProperty(o,'length',{get:function(){reads++;return 2;}});let seen='';Array.prototype.forEach.call(o,function(v){seen+=v;});errors===2&&reads===1&&seen==='xy';",
    );
}
#[test]
fn current_holes_inheritance_and_getters_are_observed_in_order() {
    check(
        "const proto={1:'inherited'};const o=Object.create(proto);o[0]='first';o.length=4;let log='';Object.defineProperty(o,'2',{get:function(){log+='g';return 'getter';}});Array.prototype.forEach.call(o,function(v,k){log+=k+v;});log==='0first1inheritedg2getter';",
    );
}
#[test]
fn initial_length_is_fixed_but_properties_are_current() {
    check(
        "const o={0:'a',1:'b',2:'c',length:3};let seen='';Array.prototype.forEach.call(o,function(v,k){seen+=v;if(k===0){delete o[1];o[2]='new';o[3]='outside';o.length=4;}});seen==='anew';",
    );
}
#[test]
fn arrays_keep_holes_and_strict_this_arg_behavior() {
    check(
        "const a=[1,,3];let count=0;let correct=true;a.forEach(function(v,k,o){'use strict';count++;correct=correct&&this===null&&o===a&&v===k+1;},null);count===2&&correct;",
    );
}
#[test]
fn primitive_receivers_use_existing_boxing_contract() {
    check(
        "let seen='';let boxed=true;Array.prototype.forEach.call('ab',function(v,k,o){seen+=v+k;boxed=boxed&&typeof o==='object';});let calls=0;Array.prototype.forEach.call(7,function(){calls++;});Array.prototype.forEach.call(false,function(){calls++;});seen==='a0b1'&&boxed&&calls===0;",
    );
}
#[test]
fn throw_stops_callbacks_and_preserves_thrown_value() {
    check(
        "let seen='';const marker={id:7};let caught=false;try{[1,2,3].forEach(function(v){seen+=v;if(v===2){throw marker;}});}catch(e){caught=e===marker;}seen==='12'&&caught;",
    );
}
#[test]
fn reentrant_iteration_keeps_callback_context_and_job_boundary() {
    check(
        "let log='';[1,2].forEach(function(v){log+='a'+v;[3].forEach(function(w){log+='b'+w;});Promise.resolve().then(function(){log+='job';});});log==='a1b3a2b3';",
    );
}
#[test]
fn length_conversion_truncates_negative_nan_and_fractional_values() {
    check(
        "let count=0;const cb=function(){count++;};Array.prototype.forEach.call({0:1,length:-1},cb);Array.prototype.forEach.call({0:1,length:NaN},cb);Array.prototype.forEach.call({0:1,1:2,length:1.9},cb);count===1;",
    );
}
#[test]
fn sparse_infinite_length_spends_existing_instruction_budget_and_cleans_roots() {
    let mut vm = Runtime::default();
    assert_eq!(
        vm.execute(
            "Array.prototype.forEach.call({length:Infinity},function(){});",
            &mut Empty
        )
        .unwrap_err()
        .kind,
        ErrorKind::Resource
    );
    assert!(vm.gc_roots.is_empty());
}
#[test]
fn partial_root_acquisition_failure_preserves_only_callers_roots() {
    let mut vm = Runtime::default();
    let mut roots = Vec::new();
    for _ in 0..4094 {
        roots.push(vm.retain_value(Value::Null).unwrap());
    }
    let before = vm.gc_roots.len();
    assert_eq!(
        vm.array_for_each(Value::Number(1.), &[], &mut Empty)
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
    assert_eq!(vm.gc_roots.len(), before);
    for root in roots {
        assert!(vm.release_value(root));
    }
    assert!(vm.gc_roots.is_empty());
}
#[test]
fn length_getter_throw_and_string_wrapper_limit_release_roots() {
    let mut vm = Runtime::default();
    let error = vm.execute(
        "const o={};Object.defineProperty(o,'length',{get:function(){throw 9;}});Array.prototype.forEach.call(o,function(){});",
        &mut Empty
    ).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Runtime);
    assert_eq!(error.thrown, Some(Value::Number(9.)));
    assert!(vm.gc_roots.is_empty());
    let mut vm = Runtime::default();
    assert_eq!(
        vm.execute(
            "Array.prototype.forEach.call('x'.repeat(128),function(){});",
            &mut Empty
        )
        .unwrap_err()
        .kind,
        ErrorKind::Resource
    );
    assert!(vm.gc_roots.is_empty());
}
#[test]
fn callback_host_observes_retained_values_and_cannot_collect_active_frame() {
    struct Inspect {
        calls: usize,
    }
    impl Host for Inspect {
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
        fn is_callable(&self, id: u64) -> bool {
            id == 7
        }
        fn call_with_runtime(
            &mut self,
            _: u64,
            _: &str,
            args: &[Value],
            this: &Value,
            vm: &mut Runtime,
        ) -> Result<Value, JsError> {
            assert_eq!(args.len(), 3);
            assert_eq!(*this, Value::Null);
            assert!(vm.gc_roots.len() >= 4);
            assert_eq!(vm.collect_garbage(&[], self)?.reclaimed_objects, 0);
            self.calls += 1;
            Ok(Value::Undefined)
        }
    }
    let mut vm = Runtime::default();
    let mut host = Inspect { calls: 0 };
    let array = vm.execute("[1,2];", &mut host).unwrap();
    assert_eq!(
        vm.array_for_each(array, &[Value::Host(7), Value::Null], &mut host)
            .unwrap(),
        Value::Undefined
    );
    assert_eq!(host.calls, 2);
    assert!(vm.gc_roots.is_empty());
}

#[test]
fn unsupported_symbol_boxing_is_explicit_and_releases_roots() {
    check(
        "let failed=false;try{Array.prototype.forEach.call(Symbol('x'),function(){});}catch(e){failed=true;}failed;",
    );
}
