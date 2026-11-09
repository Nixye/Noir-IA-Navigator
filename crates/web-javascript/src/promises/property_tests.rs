use crate::{ErrorKind, Host, JsError, Limits, Runtime, Value};

struct Empty;
impl Host for Empty {
    fn global(&mut self, _: &str) -> Option<Value> {
        None
    }
    fn get(&mut self, _: u64, _: &str) -> Result<Value, JsError> {
        panic!("Unexpected host read")
    }
    fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
        panic!("Unexpected host write")
    }
    fn call(&mut self, _: u64, _: &str, _: &[Value]) -> Result<Value, JsError> {
        panic!("Unexpected host call")
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
fn prototype_existence_agrees_with_read_and_instance_identity() {
    check(
        "('prototype' in Promise)&&typeof Promise.prototype==='object'&&Promise.prototype.constructor===Promise&&Object.getPrototypeOf(Promise.resolve(3))===Promise.prototype;",
    );
}

#[test]
fn static_method_inventory_matches_reads_without_adding_unsupported_names() {
    check(
        "const keys=['resolve','reject','withResolvers','all','race','allSettled','any'];keys.every(function(k){return k in Promise&&typeof Promise[k]==='function';})&&('name' in Promise)&&('length' in Promise)&&('call' in Promise)&&!('try' in Promise)&&Promise.try===undefined&&!('missing' in Promise)&&Promise.missing===undefined;",
    );
}

#[test]
fn metadata_existence_never_allocates_the_lazy_prototype_or_runs_jobs() {
    let mut vm = Runtime::new(Limits {
        heap_objects: 0,
        string_bytes: 0,
        ..Limits::default()
    });
    let heap = vm.heap.len();
    let bytes = vm.string_bytes;
    let remaining = vm.remaining;
    for key in [
        "prototype",
        "resolve",
        "reject",
        "withResolvers",
        "all",
        "race",
        "allSettled",
        "any",
    ] {
        assert!(
            vm.has_property(&Value::Native("Promise"), key, &mut Empty)
                .unwrap()
        );
    }
    assert_eq!(vm.heap.len(), heap);
    assert_eq!(vm.string_bytes, bytes);
    assert!(vm.promises.prototype.is_none());
    assert!(vm.promises.jobs.is_empty());
    assert_eq!(vm.remaining, remaining - 8);
}

#[test]
fn existence_preserves_key_coercion_and_does_not_call_prototype_getters() {
    check(
        "let converted=0;const key={toString:function(){converted++;return 'prototype';}};const present=key in Promise;let read=0;Object.defineProperty(Promise.prototype,'probe',{get:function(){read++;return 7;},configurable:true});present&&converted===1&&('probe' in Promise.prototype)&&read===0;",
    );
    check("const key={toString:function(){throw 17;}};try{key in Promise;false;}catch(e){e===17;}");
}

#[test]
fn polyfill_path_resolution_reaches_existing_prototype_methods() {
    check(
        "const root={Promise:Promise};const parts='Promise.prototype.finally'.split('.');let target=root;let valid=true;for(let i=0;i<parts.length-1;i++){if(!(parts[i] in target)){valid=false;break;}target=target[parts[i]];}const before=target.finally;valid&&target===Promise.prototype&&typeof before==='function'&&('finally' in target)&&Promise.prototype.finally===before;",
    );
    let mut vm = Runtime::default();
    vm.execute("var result=0;const root={Promise:Promise};const pieces=['Promise','prototype'];let target=root;for(let i=0;i<pieces.length;i++){if(!(pieces[i] in target)){throw 'missing';}target=target[pieces[i]];}delete target.finally;Object.defineProperty(target,'finally',{configurable:true,writable:true,value:function(callback){return this.then(function(v){callback();return v;});}});Promise.resolve(7).finally(function(){result=1;}).then(function(v){result+=v;});", &mut Empty).unwrap();
    assert_eq!(
        vm.execute("result===8;", &mut Empty).unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn inspection_does_not_expand_constructor_descriptor_authority() {
    for source in [
        "try{Object.defineProperty(Promise,'extra',{value:1});false;}catch(e){e.name==='TypeError';}",
        "try{Reflect.has(Promise,'prototype');false;}catch(e){e.name==='TypeError';}",
        "try{Object.getOwnPropertyDescriptor(Promise,'prototype');false;}catch(e){e.name==='TypeError';}",
    ] {
        check(source);
    }
    for name in ["unrecognized-native", "Promise.resolve", "queueMicrotask"] {
        let error = Runtime::default()
            .has_property(&Value::Native(name), "prototype", &mut Empty)
            .unwrap_err();
        assert!(
            error
                .message
                .contains("Native constructor prototype inspection")
        );
    }
}

#[test]
fn existence_retains_instruction_and_key_length_limits() {
    let mut vm = Runtime {
        remaining: 0,
        ..Runtime::default()
    };
    assert_eq!(
        vm.has_property(&Value::Native("Promise"), "prototype", &mut Empty)
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
    let mut vm = Runtime::default();
    assert_eq!(
        vm.has_property(&Value::Native("Promise"), &"x".repeat(1025), &mut Empty)
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
    assert!(vm.promises.prototype.is_none());
}

#[test]
fn inherited_native_inventory_uses_the_same_bounded_walk() {
    let mut vm = Runtime::default();
    // Exercise an internal prototype chain without enabling Object.create for
    // native constructors outside the existing Number/Symbol whitelist.
    let value = vm
        .object(Default::default(), Some(Value::Native("Promise")))
        .unwrap();
    let remaining = vm.remaining;
    assert!(vm.has_property(&value, "prototype", &mut Empty).unwrap());
    assert_eq!(vm.remaining, remaining - 2);
    assert!(vm.promises.prototype.is_none());
    assert!(
        vm.has_own_property(&Value::Native("Promise"), "prototype")
            .is_err()
    );
    check("try{Object.create(Promise);false;}catch(e){e.name==='TypeError';}");
}
