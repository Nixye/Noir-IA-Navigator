use crate::{ErrorKind, Host, JsError, Runtime, Value};

struct Empty;
impl Host for Empty {
    fn global(&mut self, name: &str) -> Option<Value> {
        (name == "opaque").then_some(Value::Host(91))
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
        Runtime::default()
            .execute(source, &mut Empty)
            .unwrap_or_else(|e| panic!("{source}\n{e:?}")),
        Value::Bool(true),
        "{source}"
    );
}

#[test]
fn object_set_prototype_returns_target_and_changes_real_inheritance() {
    check(
        "const target={own:1};const parent={inherited:2};Object.setPrototypeOf(target,parent)===target&&Object.getPrototypeOf(target)===parent&&Reflect.getPrototypeOf(target)===parent&&target.own===1&&target.inherited===2&&('inherited' in target)&&!Object.hasOwn(target,'inherited')&&parent.isPrototypeOf(target)&&target instanceof Object;",
    );
    check(
        "const target={own:1};Object.setPrototypeOf(target,null)===target&&Object.getPrototypeOf(target)===null&&!('toString' in target)&&target.own===1&&!(target instanceof Object);",
    );
}

#[test]
fn prototype_accessors_symbols_and_enumeration_keep_receiver() {
    check(
        "let seen;const p={};Object.defineProperty(p,'value',{get:function(){return this.own;},set:function(v){seen=this;this.own=v;},enumerable:true});const o={};Object.setPrototypeOf(o,p);o.value=7;const r={own:9};seen===o&&o.own===7&&o.value===7&&Reflect.get(o,'value',r)===9&&Object.keys(o).join(',')==='own';",
    );
    check(
        "let reads=0;const key=Symbol();const p={};Object.defineProperty(p,key,{get:function(){reads++;return this.own;}});const o={own:4};Object.setPrototypeOf(o,p);const present=key in o;present&&reads===0&&o[key]===4&&reads===1;",
    );
    check(
        "const a={a:1};const b={b:2};const o={own:0};Object.setPrototypeOf(o,a);Object.setPrototypeOf(o,b);let keys='';for(const k in o){keys+=k+',';}keys==='own,b,'&&!('a' in o)&&o.b===2;",
    );
}

#[test]
fn identity_is_allowed_before_non_extensible_and_immutable_checks() {
    check(
        "const o={};const p=Object.getPrototypeOf(o);Object.preventExtensions(o);Object.setPrototypeOf(o,p)===o&&!Reflect.setPrototypeOf(o,null);",
    );
    check(
        "const o={};Object.freeze(o);try{Object.setPrototypeOf(o,null);false;}catch(e){e.name==='TypeError'&&Object.getPrototypeOf(o)===Object.prototype;}",
    );
    check(
        "Object.setPrototypeOf(Object.prototype,null)===Object.prototype;try{Object.setPrototypeOf(Object.prototype,{});false;}catch(e){e.name==='TypeError'&&Object.getPrototypeOf(Object.prototype)===null;}",
    );
}

#[test]
fn object_throws_on_cycles_while_reflect_returns_false() {
    check(
        "const a={},b=Object.create(a);let count=0;try{Object.setPrototypeOf(a,a);}catch(e){if(e.name==='TypeError')count++;}try{Object.setPrototypeOf(a,b);}catch(e){if(e.name==='TypeError')count++;}count===2&&!Reflect.setPrototypeOf(a,b)&&Object.getPrototypeOf(a)===Object.prototype;",
    );
    check(
        "const a={},b={},c={};Object.setPrototypeOf(a,b);Object.setPrototypeOf(b,c);try{Object.setPrototypeOf(c,a);false;}catch(e){e.name==='TypeError'&&Object.getPrototypeOf(c)===Object.prototype&&Object.getPrototypeOf(a)===b;}",
    );
}

#[test]
fn primitives_are_not_boxed_and_invalid_arguments_still_throw() {
    check(
        "const key=Symbol();Object.setPrototypeOf(7,null)===7&&Object.setPrototypeOf('x',{})==='x'&&Object.setPrototypeOf(false,{})===false&&Object.setPrototypeOf(3n,null)===3n&&Object.setPrototypeOf(key,{})===key;",
    );
    for source in [
        "Object.setPrototypeOf(null,{});",
        "Object.setPrototypeOf(undefined,{});",
        "Object.setPrototypeOf({},3);",
        "Object.setPrototypeOf(7,3);",
        "Object.setPrototypeOf({});",
        "new Object.setPrototypeOf({},{});",
    ] {
        check(&format!(
            "try{{{source}false;}}catch(e){{e.name==='TypeError';}}"
        ));
    }
}

#[test]
fn unsupported_hosts_and_native_targets_do_not_acquire_authority() {
    for source in [
        "Object.setPrototypeOf(opaque,{});",
        "Object.setPrototypeOf({},opaque);",
        "Object.setPrototypeOf(Object,{});",
        "Object.setPrototypeOf({},new Map());",
    ] {
        check(&format!(
            "try{{{source}false;}}catch(e){{e.name==='TypeError';}}"
        ));
    }
}

#[test]
fn cycles_depth_and_instruction_failures_leave_target_unchanged() {
    let mut vm = Runtime::default();
    let target = vm.ordinary_object(Default::default()).unwrap();
    let old = vm.get(target.clone(), "toString", &mut Empty).unwrap();
    let mut parent = Value::Null;
    for _ in 0..65 {
        parent = vm.object(Default::default(), Some(parent)).unwrap();
    }
    let error = vm
        .reflect_set_prototype(&target, parent.clone())
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert_eq!(vm.get(target.clone(), "toString", &mut Empty).unwrap(), old);
    vm.remaining = 0;
    assert_eq!(
        vm.reflect_set_prototype(&target, parent).unwrap_err().kind,
        ErrorKind::Resource
    );
}

#[test]
fn mutated_prototype_is_traced_and_restored_with_runtime_rollback() {
    let mut vm = Runtime::default();
    vm.execute(
        "var held={};(function(){const parent={value:7};Object.setPrototypeOf(held,parent);})();",
        &mut Empty,
    )
    .unwrap();
    vm.collect_garbage(&[], &Empty).unwrap();
    assert_eq!(
        vm.execute("held.value===7;", &mut Empty).unwrap(),
        Value::Bool(true)
    );
    let before = vm.clone();
    vm.execute("Object.setPrototypeOf(held,{value:9});", &mut Empty)
        .unwrap();
    vm.rollback_to(before);
    assert_eq!(
        vm.execute("held.value===7;", &mut Empty).unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn function_internal_prototype_is_real_and_distinct_from_public_prototype() {
    check(
        "function f(){return 7;}const publicPrototype=f.prototype;const p={marker:3};Object.setPrototypeOf(f,p)===f&&Object.getPrototypeOf(f)===p&&Reflect.getPrototypeOf(f)===p&&f.marker===3&&('marker' in f)&&f.prototype===publicPrototype&&new f() instanceof f&&f()===7&&p.isPrototypeOf(f)&&!(f instanceof Function)&&f instanceof Object;",
    );
    check(
        "function f(){return 7;}function g(){return 8;}g.marker=4;Reflect.setPrototypeOf(f,g)&&f.marker===4&&Object.getPrototypeOf(f)===g&&f()===7&&f instanceof Function&&Function.prototype.isPrototypeOf(f)&&g.isPrototypeOf(f);",
    );
    check(
        "function f(){return 7;}Object.setPrototypeOf(f,null);f()===7&&f.call===undefined&&f.apply===undefined&&f.bind===undefined&&!('call' in f)&&Object.getPrototypeOf(f)===null&&!(f instanceof Function)&&!(f instanceof Object);",
    );
}

#[test]
fn inherited_function_accessors_use_original_or_explicit_receiver() {
    check(
        "function F(){}const p={};Object.defineProperty(p,'x',{get:function(){return this.n;},configurable:true});Object.setPrototypeOf(F,p);F.n=9;const o=Object.create(F);o.n=7;o.x===7&&F.x===9&&Reflect.get(F,'x',{n:8})===8;",
    );
    check(
        "let seen;function F(){}const p={};Object.defineProperty(p,'x',{set:function(v){seen=this;this.n=v;}});Object.setPrototypeOf(F,p);F.x=4;const a=seen===F&&F.n===4;const o=Object.create(F);o.x=5;a&&seen===o&&o.n===5&&!Object.hasOwn(o,'x')&&Reflect.set(F,'x',6,o)&&o.n===6;",
    );
    check(
        "function F(){}let reads=0;Object.defineProperty(F,'x',{get:function(){reads++;return this.n;}});const o=Object.create(F);o.n=7;('x' in o)&&reads===0&&o.x===7&&reads===1;",
    );
}

#[test]
fn function_chain_readonly_data_and_getter_only_properties_block_assignment() {
    check(
        "function F(){}const p={};Object.defineProperty(p,'x',{value:3});Object.setPrototypeOf(F,p);F.x=4;F.x===3&&!Object.hasOwn(F,'x')&&!Reflect.set(F,'x',4);",
    );
    check(
        "function F(){}const p={};Object.defineProperty(p,'x',{get:function(){return 3;}});Object.setPrototypeOf(F,p);try{(function(){'use strict';F.x=4;})();false;}catch(e){e.name==='TypeError'&&F.x===3;}",
    );
}

#[test]
fn function_symbol_chains_keep_receivers_existence_and_writability() {
    check(
        "const key=Symbol();let seen;const p={};Object.defineProperty(p,key,{get:function(){return this.n;},set:function(v){seen=this;this.n=v;}});function F(){}Object.setPrototypeOf(F,p);F[key]=7;const o=Object.create(F);o.n=8;seen===F&&F[key]===7&&key in F&&o[key]===8&&Reflect.get(F,key,{n:9})===9&&Reflect.set(F,key,10,o)&&o.n===10&&seen===o;",
    );
    check(
        "const key=Symbol();const p={};Object.defineProperty(p,key,{value:3});function F(){}Object.setPrototypeOf(F,p);F[key]=4;F[key]===3&&!Object.hasOwn(F,key)&&!Reflect.set(F,key,4);",
    );
}

#[test]
fn function_enumeration_and_native_method_shadowing_follow_actual_chain() {
    check(
        "function F(){}function G(){}F.a=1;G.b=2;Object.setPrototypeOf(F,G);let keys='';for(const k in F){keys+=k+',';}keys==='a,b,'&&Object.keys(F).join(',')==='a';",
    );
    check(
        "function F(){}const p={call:function(){return this===F?17:0;}};Object.setPrototypeOf(F,p);F.call()===17&&Reflect.get(F,'call')===p.call;",
    );
}

#[test]
fn function_cycles_non_extensibility_and_unsupported_targets_stay_atomic() {
    check(
        "function F(){}function G(){}const p={};Object.setPrototypeOf(F,G);Object.setPrototypeOf(G,p);!Reflect.setPrototypeOf(p,F)&&!Reflect.setPrototypeOf(G,F)&&Object.getPrototypeOf(G)===p;",
    );
    check(
        "function F(){}const p=Object.getPrototypeOf(F);Object.preventExtensions(F);Object.setPrototypeOf(F,p)===F&&!Reflect.setPrototypeOf(F,null);",
    );
    check(
        "function F(){}Object.freeze(F);try{Object.setPrototypeOf(F,{});false;}catch(e){e.name==='TypeError'&&Object.getPrototypeOf(F)===Function.prototype;}",
    );
    check(
        "class C{}const p=Object.getPrototypeOf(C);Object.setPrototypeOf(C,p);try{Object.setPrototypeOf(C,{});false;}catch(e){e.name==='TypeError'&&Object.getPrototypeOf(C)===p;}",
    );
    check(
        "const a=[];Object.setPrototypeOf(a,Array.prototype);try{Object.setPrototypeOf(a,null);false;}catch(e){e.name==='TypeError'&&Object.getPrototypeOf(a)===Array.prototype;}",
    );
}

#[test]
fn bound_function_captures_prototype_before_observable_metadata_reads() {
    check(
        "function F(x){return x+1;}const first={x:1},later={x:2};Object.setPrototypeOf(F,first);const b=Function.prototype.bind.call(F,null,3);Object.setPrototypeOf(F,later);b()===4&&Object.getPrototypeOf(b)===first&&b.x===1&&F.x===2;",
    );
    check(
        "function F(){return 7;}const first={},later={};Object.setPrototypeOf(F,first);Object.defineProperty(F,'length',{get:function(){Object.setPrototypeOf(F,later);return 0;}});const b=Function.prototype.bind.call(F,null);Object.getPrototypeOf(b)===first&&Object.getPrototypeOf(F)===later&&b()===7;",
    );
    check(
        "function F(){return 7;}Object.setPrototypeOf(F,null);const b=Function.prototype.bind.call(F,null);Object.getPrototypeOf(b)===null&&b.call===undefined&&b()===7;",
    );
}

#[test]
fn static_super_traverses_mutated_ordinary_function_base() {
    check(
        "function A(){}let seen;const p={};Object.defineProperty(p,'x',{get:function(){return this.n;},set:function(v){seen=this;this.n=v;}});Object.setPrototypeOf(A,p);class B extends A {static put(v){super.x=v;}static read(){return super.x;}}B.put(7);seen===B&&B.read()===7&&B.n===7&&Object.getPrototypeOf(B)===A;",
    );
}

#[test]
fn mixed_function_object_chains_have_one_limit_and_no_partial_symbol_write() {
    for operation in [
        "root.missing;",
        "'missing' in root;",
        "root[key];",
        "key in root;",
        "root[key]=1;",
        "root.missing=1;",
        "Reflect.get(root,'missing');",
        "Reflect.set(root,key,1);",
        "Reflect.has(root,'missing');",
        "root instanceof Object;",
        "Object.prototype.isPrototypeOf(root);",
        "for(const k in root){}",
    ] {
        let mut vm = Runtime::default();
        vm.execute("var root={};var key=Symbol();let tail=root;for(let i=0;i<80;i++){const next=i%2===0?function(){}:{};Object.setPrototypeOf(tail,next);tail=next;}",&mut Empty).unwrap();
        let error = vm.execute(operation, &mut Empty).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource, "{operation}: {error:?}");
        let root = vm.global_value("root").unwrap();
        assert!(!vm.has_own_property(&root, "missing").unwrap());
        let key = vm.global_value("key").unwrap();
        let Value::Symbol(key, _) = key else {
            unreachable!()
        };
        assert!(vm.symbol_descriptor(&root, key).unwrap().is_none());
    }
}

#[test]
fn function_prototype_edges_survive_gc_and_rollback_without_global_roots() {
    let mut vm = Runtime::default();
    vm.execute(
        "var held=function(){};(function(){const p={value:7};Object.setPrototypeOf(held,p);})();",
        &mut Empty,
    )
    .unwrap();
    vm.collect_garbage(&[], &Empty).unwrap();
    assert_eq!(
        vm.execute("held.value===7;", &mut Empty).unwrap(),
        Value::Bool(true)
    );
    let before = vm.clone();
    vm.execute("Object.setPrototypeOf(held,{value:9});", &mut Empty)
        .unwrap();
    vm.rollback_to(before);
    assert_eq!(
        vm.execute("held.value===7;", &mut Empty).unwrap(),
        Value::Bool(true)
    );
    vm.execute("held=null;", &mut Empty).unwrap();
    assert!(vm.collect_garbage(&[], &Empty).unwrap().reclaimed_objects >= 2);
}

#[test]
fn bound_prototype_remains_live_after_the_target_changes_parent() {
    let mut vm = Runtime::default();
    vm.execute("var held;(function(){function F(){return 7;}const p={marker:9};Object.setPrototypeOf(F,p);held=Function.prototype.bind.call(F,null);Object.setPrototypeOf(F,null);})();",&mut Empty).unwrap();
    vm.collect_garbage(&[], &Empty).unwrap();
    assert_eq!(
        vm.execute(
            "held()===7&&held.marker===9&&Object.getPrototypeOf(held).marker===9;",
            &mut Empty
        )
        .unwrap(),
        Value::Bool(true)
    );
}
