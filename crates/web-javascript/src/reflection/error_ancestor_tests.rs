use crate::{ErrorKind, Heap, Host, JsError, Runtime, Value};

struct Empty;
impl Host for Empty {
    fn global(&mut self, name: &str) -> Option<Value> {
        (name == "opaque").then_some(Value::Host(91))
    }
    fn get(&mut self, _: u64, _: &str) -> Result<Value, JsError> {
        panic!("Error ancestry must not read host properties")
    }
    fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
        panic!("Error ancestry must not write host properties")
    }
    fn call(&mut self, _: u64, _: &str, _: &[Value]) -> Result<Value, JsError> {
        panic!("Error ancestry must not call a host")
    }
}
fn check(source: &str) {
    assert_eq!(
        Runtime::default()
            .execute(source, &mut Empty)
            .unwrap_or_else(|error| panic!("{source}\n{error:?}")),
        Value::Bool(true),
        "{source}"
    );
}

#[test]
fn error_constructor_ancestors_preserve_function_identity_and_public_prototype() {
    for kind in [
        "Error",
        "TypeError",
        "RangeError",
        "ReferenceError",
        "SyntaxError",
    ] {
        check(&format!(
            "function F(x){{this.x=x;return x+1;}}const p=F.prototype;Object.setPrototypeOf(F,{kind})===F&&Reflect.getPrototypeOf(F)==={kind}&&F.prototype===p&&F.call({{}},3)===4&&new F(7).x===7&&new F() instanceof F&&F instanceof Function&&F instanceof Object&&!(F instanceof Error)&&Function.prototype.isPrototypeOf(F)&&{kind}.isPrototypeOf(F)&&Object.prototype.isPrototypeOf(F);"
        ));
        check(&format!(
            "const o={{}};Reflect.setPrototypeOf(o,{kind})&&o.prototype==={kind}.prototype&&o.name==='{kind}'&&o.length===1&&o.constructor===Function&&typeof o.call==='function'&&('prototype' in o)&&!('missing' in o)&&Object.getOwnPropertyDescriptor(o,'prototype')===undefined;"
        ));
    }
}

#[test]
fn five_constructor_chains_are_real_and_public_error_prototypes_are_separate() {
    check(
        "Object.getPrototypeOf(Error)===Function.prototype&&Reflect.getPrototypeOf(Error)===Function.prototype&&Object.getPrototypeOf(TypeError)===Error&&Reflect.getPrototypeOf(TypeError)===Error&&Object.getPrototypeOf(RangeError)===Error&&Object.getPrototypeOf(ReferenceError)===Error&&Object.getPrototypeOf(SyntaxError)===Error&&TypeError instanceof Function&&TypeError instanceof Object&&Error.isPrototypeOf(TypeError)&&!(TypeError instanceof Error)&&!Error.prototype.isPrototypeOf(Error)&&!Error.prototype.isPrototypeOf(TypeError);",
    );
    check(
        "function F(){}Object.setPrototypeOf(F,Error);Error.prototype.publicOnly=17;Object.setPrototypeOf(Error.prototype,F);F.publicOnly===undefined&&!('publicOnly' in F)&&!Error.prototype.isPrototypeOf(F)&&Object.getPrototypeOf(F)===Error&&Object.getPrototypeOf(Error)===Function.prototype&&Error.prototype.publicOnly===17;",
    );
}

#[test]
fn arrow_ancestry_never_grants_constructibility() {
    check(
        "const f=(x)=>x+1;Object.setPrototypeOf(f,Error);let failures=0;try{new f();}catch(e){if(e.name==='TypeError')failures++;}try{Reflect.construct(f,[]);}catch(e){if(e.name==='TypeError')failures++;}const b=f.bind(null,2);try{new b();}catch(e){if(e.name==='TypeError')failures++;}failures===3&&f(4)===5&&b()===3&&Object.getPrototypeOf(b)===Error&&!Object.hasOwn(f,'prototype')&&f.prototype===Error.prototype;",
    );
}

#[test]
fn string_accessors_preserve_receivers_and_existence_does_not_call_getters() {
    check(
        "let gets=0,sets=0,seen;Object.defineProperty(Object.prototype,'inherited',{get:function(){gets++;return this.n;},set:function(v){sets++;seen=this;this.n=v;},enumerable:true,configurable:true});function F(){}Object.setPrototypeOf(F,TypeError);const r={n:7};const before=Reflect.has(F,'inherited')&&('inherited' in F)&&Reflect.getOwnPropertyDescriptor(F,'inherited')===undefined&&gets===0;const read=Reflect.get(F,'inherited',r);Reflect.set(F,'inherited',9,r);F.inherited=3;before&&read===7&&seen===F&&F.n===3&&r.n===9&&gets===1&&sets===2&&F.inherited===3;",
    );
    check(
        "Object.defineProperty(Object.prototype,'bad',{get:function(){throw 41;},set:function(){throw 42;}});const o={};Object.setPrototypeOf(o,Error);let reads=false,writes=false;try{Reflect.get(o,'bad');}catch(e){reads=e===41;}try{Reflect.set(o,'bad',1);}catch(e){writes=e===42;}reads&&writes&&Reflect.has(o,'bad');",
    );
}

#[test]
fn symbols_preserve_receivers_without_native_owners_or_getters_in_has() {
    check(
        "const s=Symbol();let gets=0,sets=0,seen;Object.defineProperty(Object.prototype,s,{get:function(){gets++;return this.n;},set:function(v){sets++;seen=this;this.n=v;},configurable:true});function F(){}Object.setPrototypeOf(F,TypeError);const r={n:11};const present=(s in F)&&Reflect.has(F,s)&&gets===0&&Reflect.getOwnPropertyDescriptor(F,s)===undefined;const x=Reflect.get(F,s,r);F[s]=5;const first=seen===F;Reflect.set(F,s,8,r);present&&first&&seen===r&&x===11&&F.n===5&&r.n===8&&gets===1&&sets===2&&F[s]===5;",
    );
    check(
        "const s=Symbol(),o={};Object.setPrototypeOf(o,Error);!(s in o)&&o[s]===undefined&&!Reflect.has(o,s)&&Reflect.set(o,s,3)&&o[s]===3&&Object.getOwnPropertyDescriptor(o,s).value===3;",
    );
}

#[test]
fn inherited_intrinsic_flags_block_assignment_but_own_definition_stays_ordinary() {
    check(
        "const o={};Object.setPrototypeOf(o,TypeError);o.name='wrong';o.length=9;o.prototype={};const before=o.name==='TypeError'&&o.length===1&&o.prototype===TypeError.prototype;let strict=false;try{(function(){'use strict';o.name='bad';})();}catch(e){strict=e.name==='TypeError';}before&&strict&&!Reflect.set(o,'name','bad')&&!Reflect.set(o,'length',9)&&!Reflect.set(o,'prototype',{})&&Reflect.defineProperty(o,'name',{value:'own',writable:true})&&o.name==='own'&&Reflect.set(o,'name','updated')&&o.name==='updated';",
    );
    check(
        "const o={};Object.setPrototypeOf(o,Error);o.call=17;o.custom=3;const d=Reflect.getOwnPropertyDescriptor(o,'call');o.call===17&&o.custom===3&&d.writable&&d.enumerable&&d.configurable&&Error.call===Function.prototype.call;",
    );
}

#[test]
fn enumeration_respects_intrinsic_non_enumerable_shadowing_without_getters() {
    check(
        "let reads=0;Object.defineProperty(Object.prototype,'name',{value:'hidden',enumerable:true,configurable:true});Object.defineProperty(Object.prototype,'prototype',{get:function(){reads++;return 7;},enumerable:true,configurable:true});Object.defineProperty(Object.prototype,'visible',{get:function(){reads++;return 8;},enumerable:true,configurable:true});const o={own:1};Object.setPrototypeOf(o,TypeError);let keys='';for(const k in o){keys+=k+',';}keys==='own,visible,'&&reads===0&&Object.keys(o).join(',')==='own'&&Reflect.ownKeys(o).join(',')==='own';",
    );
}

#[test]
fn metadata_and_presence_do_not_materialize_public_error_prototypes() {
    let mut vm = Runtime::default();
    vm.execute("var o={};Object.setPrototypeOf(o,TypeError);", &mut Empty)
        .unwrap();
    let heap_before = vm.heap.len();
    let strings_before = vm.string_bytes;
    for name in [
        "Error",
        "TypeError",
        "RangeError",
        "ReferenceError",
        "SyntaxError",
    ] {
        for key in ["name", "length", "prototype"] {
            let descriptor = vm
                .readonly_ancestor_descriptor(&Value::Native(name), key, false)
                .unwrap()
                .unwrap();
            assert_eq!(descriptor.value, Value::Undefined);
            assert!(!descriptor.metadata.writable);
            assert!(!descriptor.metadata.enumerable);
            assert_eq!(descriptor.metadata.configurable, key != "prototype");
            assert!(
                vm.has_property(&Value::Native(name), key, &mut Empty)
                    .unwrap()
            );
        }
        assert!(!vm.reflection.prototypes.contains_key(name));
    }
    assert_eq!(vm.heap.len(), heap_before);
    assert_eq!(vm.string_bytes, strings_before);
    assert_eq!(vm.execute("Reflect.has(o,'prototype')&&('prototype' in o)&&Reflect.getOwnPropertyDescriptor(o,'prototype')===undefined;", &mut Empty).unwrap(), Value::Bool(true));
    vm.execute("for(const key in o){}", &mut Empty).unwrap();
    for name in ["Error", "TypeError"] {
        assert!(!vm.reflection.prototypes.contains_key(name));
    }
}

#[test]
fn bind_captures_constructor_parent_before_metadata_and_later_mutation() {
    check(
        "function F(x){return x+1;}Object.setPrototypeOf(F,Error);const b=F.bind(null,3);Object.setPrototypeOf(F,null);b()===4&&Object.getPrototypeOf(b)===Error&&b instanceof Function&&F.call===undefined&&Object.getPrototypeOf(TypeError.bind(null))===Error&&Object.getPrototypeOf(Error.bind(null))===Function.prototype;",
    );
    check(
        "function F(){return 7;}Object.setPrototypeOf(F,TypeError);Object.defineProperty(F,'length',{get:function(){Object.setPrototypeOf(F,null);return 0;}});const b=Function.prototype.bind.call(F,null);Object.getPrototypeOf(b)===TypeError&&Object.getPrototypeOf(F)===null&&b()===7;",
    );
}

#[test]
fn static_super_walks_through_error_ancestor_with_original_receiver() {
    check(
        "let seen;Object.defineProperty(Object.prototype,'value',{get:function(){return this.n;},set:function(v){seen=this;this.n=v;},configurable:true});function A(){}Object.setPrototypeOf(A,Error);class B extends A {static put(v){super.value=v;}static read(){return super.value;}}B.put(7);B.read()===7&&seen===B&&B.n===7&&Object.getPrototypeOf(B)===A;",
    );
}

#[test]
fn readonly_ancestry_does_not_grant_public_own_property_or_mutation_authority() {
    for target in [
        "Error",
        "TypeError",
        "RangeError",
        "ReferenceError",
        "SyntaxError",
    ] {
        for action in [
            "Object.defineProperty(T,'x',{value:1});",
            "Object.getOwnPropertyDescriptor(T,'name');",
            "Object.getOwnPropertyNames(T);",
            "Object.getOwnPropertySymbols(T);",
            "Object.freeze(T);",
            "Object.setPrototypeOf(T,Function.prototype);",
            "Reflect.setPrototypeOf(T,Function.prototype);",
            "Reflect.get(T,'name');",
            "Reflect.has(T,'name');",
            "Reflect.set(T,'x',1);",
            "Reflect.defineProperty(T,'x',{value:1});",
            "Reflect.deleteProperty(T,'name');",
            "Reflect.ownKeys(T);",
            "Reflect.getOwnPropertyDescriptor(T,'name');",
            "Reflect.preventExtensions(T);",
            "Reflect.isExtensible(T);",
            "Object.create(T);",
            "T[Symbol()]=1;",
        ] {
            check(&format!(
                "const T={target};try{{{action}false;}}catch(e){{e.name==='TypeError';}}"
            ));
        }
    }
    for target in ["opaque", "Object", "Map", "AggregateError", "Promise"] {
        check(&format!(
            "try{{Object.setPrototypeOf({{}},{target});false;}}catch(e){{e.name==='TypeError';}}"
        ));
        check(&format!(
            "try{{Reflect.getPrototypeOf({target});false;}}catch(e){{e.name==='TypeError';}}"
        ));
    }
    check(
        "let reads=0;const key={toString:function(){reads++;return 'x';}};const d={};Object.defineProperty(d,'value',{get:function(){reads++;return 1;}});try{Reflect.defineProperty(Error,key,d);}catch(e){}try{Reflect.get(Error,key);}catch(e){}reads===0;",
    );
}

#[test]
fn cycle_depth_counts_intrinsic_ancestors_and_never_partially_writes() {
    // Error, Function.prototype, Object.prototype and the terminating null all
    // consume a visit. 60 VM ancestors plus these four fit exactly in 64.
    let mut vm = Runtime::default();
    let target = vm.ordinary_object(Default::default()).unwrap();
    let original = vm.get(target.clone(), "toString", &mut Empty).unwrap();
    let mut chain = Value::Native("Error");
    for _ in 0..60 {
        chain = vm.object(Default::default(), Some(chain)).unwrap();
    }
    assert!(vm.reflect_set_prototype(&target, chain.clone()).unwrap());
    let too_deep = vm.object(Default::default(), Some(chain.clone())).unwrap();
    let error = vm.reflect_set_prototype(&target, too_deep).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    let Value::Object(id) = target else {
        unreachable!()
    };
    assert!(
        matches!(&vm.heap[id], Heap::Object(object) if object.prototype.as_ref() == Some(&chain))
    );
    vm.remaining = 1;
    let error = vm
        .reflect_set_prototype(&Value::Object(id), Value::Native("TypeError"))
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert!(
        matches!(&vm.heap[id], Heap::Object(object) if object.prototype.as_ref() == Some(&chain))
    );
    assert_eq!(original, Value::Native("Object.toString"));
}

#[test]
fn error_ancestry_gc_and_rollback_preserve_metadata_and_consumed_budgets() {
    let mut vm = Runtime::default();
    vm.execute("var F=function(){return 7;};Object.setPrototypeOf(F,TypeError);const s=Symbol();Object.defineProperty(Object.prototype,s,{get:(function(){const held={n:9};return function(){return held.n;};})(),configurable:true});var saved=s;", &mut Empty).unwrap();
    vm.collect_garbage(&[], &Empty).unwrap();
    assert_eq!(
        vm.execute(
            "F[saved]===9&&Object.getPrototypeOf(F)===TypeError&&F()===7;",
            &mut Empty
        )
        .unwrap(),
        Value::Bool(true)
    );
    let before = vm.clone();
    vm.execute("Object.setPrototypeOf(F,Error);F.extra=3;", &mut Empty)
        .unwrap();
    let consumed = format!("{:?}", vm.property_budget);
    vm.rollback_to(before);
    assert_eq!(format!("{:?}", vm.property_budget), consumed);
    assert_eq!(
        vm.execute(
            "Object.getPrototypeOf(F)===TypeError&&!Object.hasOwn(F,'extra')&&F[saved]===9;",
            &mut Empty
        )
        .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn unsupported_native_public_instance_prototypes_remain_outside_this_contract() {
    // This pre-existing construction limitation is not [[Prototype]] mutation.
    check(
        "function F(){}F.prototype=Error;const instance=new F();let unsupported=false;try{instance instanceof F;}catch(e){unsupported=e.name==='TypeError';}unsupported&&Object.getPrototypeOf(instance)===Object.prototype&&Object.getPrototypeOf(F)===Function.prototype;",
    );
}

#[test]
fn all_walks_charge_intrinsic_hops_and_fail_before_partial_property_writes() {
    for operation in [
        "root.missing;",
        "'missing' in root;",
        "root[key];",
        "key in root;",
        "root[key]=1;",
        "root.missing=1;",
        "Reflect.get(root,'missing');",
        "Reflect.get(root,key);",
        "Reflect.set(root,'missing',1);",
        "Reflect.set(root,key,1);",
        "Reflect.has(root,'missing');",
        "Reflect.has(root,key);",
        "root instanceof Object;",
        "Object.prototype.isPrototypeOf(root);",
        "for(const k in root){}",
    ] {
        let mut vm = Runtime::default();
        vm.execute("var root={};var key=Symbol();let tail=root;for(let i=0;i<61;i++){const next={};Object.setPrototypeOf(tail,next);tail=next;}Object.setPrototypeOf(tail,TypeError);", &mut Empty).unwrap();
        let error = vm.execute(operation, &mut Empty).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource, "{operation}: {error:?}");
        let root = vm.global_value("root").unwrap();
        assert!(!vm.has_own_property(&root, "missing").unwrap());
        let Value::Symbol(key, _) = vm.global_value("key").unwrap() else {
            unreachable!()
        };
        assert!(vm.symbol_descriptor(&root, key).unwrap().is_none());
    }
}

#[test]
fn bind_length_ignores_inherited_metadata_and_getters_after_own_deletion() {
    check(
        "function F(a,b){}Object.setPrototypeOf(F,TypeError);delete F.length;const bound=F.bind(null);F.length===1&&bound.length===0&&Object.getPrototypeOf(bound)===TypeError;",
    );
    check(
        "let reads=0;Object.defineProperty(Object.prototype,'length',{get:function(){reads++;throw 99;},configurable:true});function F(a,b){}delete F.length;Object.setPrototypeOf(F,Object.prototype);const bound=Function.prototype.bind.call(F,null);bound.length===0&&reads===0&&Object.getPrototypeOf(bound)===Object.prototype;",
    );
    check(
        "let reads=0;function F(a,b){}Object.setPrototypeOf(F,Error);Object.defineProperty(F,'length',{get:function(){reads++;return 3;},configurable:true});const bound=F.bind(null,1);reads===1&&bound.length===2&&Error.bind(null).length===1&&TypeError.bind(null).length===1;",
    );
    check(
        "delete Number.length;delete Symbol.length;Number.bind(null).length===0&&Symbol.bind(null).length===0;",
    );
}
