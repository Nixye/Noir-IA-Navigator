use crate::{ErrorKind, Host, JsError, Limits, Runtime, Value};

struct Empty;
impl Host for Empty {
    fn global(&mut self, name: &str) -> Option<Value> {
        (name == "opaque").then_some(Value::Host(7))
    }
    fn get(&mut self, _: u64, _: &str) -> Result<Value, JsError> {
        Err(JsError::runtime("No host reads"))
    }
    fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
        Err(JsError::runtime("No host writes"))
    }
    fn call(&mut self, _: u64, _: &str, _: &[Value]) -> Result<Value, JsError> {
        Err(JsError::runtime("No host calls"))
    }
}
fn check(sources: &[&str]) {
    for source in sources {
        assert_eq!(
            Runtime::default()
                .execute(source, &mut Empty)
                .unwrap_or_else(|e| panic!("{source}\n{e:?}")),
            Value::Bool(true),
            "{source}"
        );
    }
}

#[test]
fn constructor_static_polyfill_uses_real_property_storage() {
    check(&[
        "const original=Number;function replacement(x){return x===17;}Object.defineProperty(Number,'isInteger',{configurable:true,writable:true,value:replacement});const d=Object.getOwnPropertyDescriptor(Number,'isInteger');Number===original&&Number.isInteger===replacement&&Number.isInteger(17)&&!Number.isInteger(18)&&d.writable&&d.configurable&&!d.enumerable&&Number('12')===12;",
        "Object.defineProperty(Number,'syntheticPolyfill',{configurable:true,writable:true,value:function(x){return x+1;}});const d=Object.getOwnPropertyDescriptor(Number,'syntheticPolyfill');Number.syntheticPolyfill(4)===5&&d.writable&&d.configurable&&!d.enumerable;",
    ]);
}

#[test]
fn guarded_constant_polyfills_keep_immutable_native_metadata() {
    check(&[
        "function install(k,v){if(Number[k]!=v&&v!=null){Object.defineProperty(Number,k,{configurable:true,writable:true,value:v});}}install('MAX_SAFE_INTEGER',9007199254740991);install('MIN_SAFE_INTEGER',-9007199254740991);const d=Object.getOwnPropertyDescriptor(Number,'MAX_SAFE_INTEGER');d.value===9007199254740991&&!d.writable&&!d.configurable&&!d.enumerable&&Number.MIN_SAFE_INTEGER===-9007199254740991;",
        "try{Object.defineProperty(Number,'MAX_SAFE_INTEGER',{value:9007199254740991,writable:true,configurable:true});false;}catch(e){e.name==='TypeError'&&!Object.getOwnPropertyDescriptor(Number,'MAX_SAFE_INTEGER').writable;}",
    ]);
}

#[test]
fn rollback_also_discards_lazy_owner_and_preserves_consumed_work() {
    let mut vm = Runtime::default();
    let before = vm.clone();
    vm.execute("Number.extra={n:2};Object.freeze(Number);", &mut Empty)
        .unwrap();
    let remaining = vm.remaining;
    vm.rollback_to(before);
    assert!(vm.reflection.number_constructor.is_none());
    assert!(vm.remaining <= remaining);
    assert_eq!(vm.execute("Number.extra===undefined&&Object.isExtensible(Number)&&Number.MAX_SAFE_INTEGER===9007199254740991;", &mut Empty).unwrap(),Value::Bool(true));
}

#[test]
fn rejected_property_insertions_do_not_reserve_ordering_slots() {
    let mut vm = Runtime::default();
    vm.execute("for(let i=0;i<111;i++){Number['x'+i]=i;}", &mut Empty)
        .unwrap();
    assert_eq!(
        vm.execute("Number.failed=1;", &mut Empty).unwrap_err().kind,
        ErrorKind::Resource
    );
    assert_eq!(vm.execute("delete Number.x0;delete Number.x1;Number.later=2;Number.failed=3;const keys=Object.keys(Number);keys[keys.length-2]==='later'&&keys[keys.length-1]==='failed';", &mut Empty).unwrap(),Value::Bool(true));
}

#[test]
fn oversize_keys_and_native_coercion_remain_explicit_failures() {
    for source in [
        "Object.defineProperty(Number,'x'.repeat(1025),{value:1});",
        "Number['x'.repeat(1025)]=1;",
    ] {
        assert_eq!(
            Runtime::default()
                .execute(source, &mut Empty)
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
    }
    check(&[
        "Number[Symbol.toPrimitive]=function(){return 1;};try{+Number;false;}catch(e){e.name==='TypeError';}",
    ]);
}

#[test]
fn inventory_constants_metadata_aliases_and_callability_agree() {
    check(&[
        "const d=Object.getOwnPropertyDescriptor(Number,'name');d.value==='Number'&&!d.writable&&!d.enumerable&&d.configurable&&Number.length===1;",
        "const d=Object.getOwnPropertyDescriptor(Number,'prototype');d.value===Number.prototype&&!d.writable&&!d.enumerable&&!d.configurable&&Number.prototype.constructor===Number&&Number.prototype.valueOf()===0;",
        "Number.EPSILON===2.220446049250313e-16&&Number.MAX_VALUE===1.7976931348623157e308&&Number.MIN_VALUE===5e-324&&Number.MAX_SAFE_INTEGER===9007199254740991&&Number.MIN_SAFE_INTEGER===-9007199254740991&&Number.POSITIVE_INFINITY===Infinity&&Number.NEGATIVE_INFINITY===-Infinity&&Number.isNaN(Number.NaN);",
        "const names=['EPSILON','MAX_VALUE','MIN_VALUE','MAX_SAFE_INTEGER','MIN_SAFE_INTEGER','NaN','POSITIVE_INFINITY','NEGATIVE_INFINITY'];names.every(function(k){const d=Object.getOwnPropertyDescriptor(Number,k);return !d.writable&&!d.enumerable&&!d.configurable;});",
        "Number.parseInt===parseInt&&Number.parseFloat===parseFloat&&Number.parseInt('12x')===12&&Number.parseFloat('1.5x')===1.5&&Number.isFinite(1)&&!Number.isFinite('1')&&Number.isInteger(3)&&!Number.isInteger(3.5)&&Number.isSafeInteger(42)&&!Number.isSafeInteger(1e100);",
        "Object.keys(Number).length===0&&Object.getOwnPropertyNames(Number).length===17&&Reflect.ownKeys(Number).length===17;",
        "Number.call(null,'4')===4&&Number.bind(null,'5')()===5&&new Number(6).valueOf()===6&&new Number(6) instanceof Number&&Number instanceof Function&&Number instanceof Object&&Object(Number)===Number;",
        "Object.getPrototypeOf(Number)===Function.prototype&&Reflect.getPrototypeOf(Number)===Function.prototype&&Object.getPrototypeOf(Object.create(Number))===Number;",
    ]);
}

#[test]
fn constants_obey_same_value_and_strict_assignment() {
    check(&[
        "Number.EPSILON=9;!delete Number.EPSILON&&!Reflect.set(Number,'EPSILON',9)&&Number.EPSILON===2.220446049250313e-16;",
        "try{(function(){'use strict';Number.MAX_VALUE=2;})();false;}catch(e){e.name==='TypeError';}",
        "try{(function(){'use strict';delete Number.NaN;})();false;}catch(e){e.name==='TypeError';}",
        "Reflect.defineProperty(Number,'NaN',{value:NaN})&&!Reflect.defineProperty(Number,'NaN',{value:0})&&!Reflect.defineProperty(Number,'EPSILON',{writable:true});",
        "try{Object.defineProperty(Number,'prototype',{value:{}});false;}catch(e){e.name==='TypeError';}",
        "Object.defineProperty(Number,'zero',{value:0});Reflect.defineProperty(Number,'zero',{value:0})&&!Reflect.defineProperty(Number,'zero',{value:-0});",
    ]);
}

#[test]
fn assignment_deletion_and_descriptor_changes_never_resurrect_builtins() {
    check(&[
        "Number.isFinite=4;const d=Object.getOwnPropertyDescriptor(Number,'isFinite');d.value===4&&d.writable&&d.configurable&&!d.enumerable;",
        "const saved=Number.isFinite;delete Number.isFinite;Number.isFinite===undefined&&!('isFinite' in Number)&&!Object.hasOwn(Number,'isFinite')&&saved(3);",
        "delete Number.parseInt;Number.parseInt===undefined&&parseInt('12')===12;",
        "delete Number.name;!Object.hasOwn(Number,'name')&&Number.name===''&&('name' in Number);",
        "Number.x=1;Number[2]=2;Number.y=3;delete Number.x;Number.x=4;Object.keys(Number).join(',')==='2,y,x'&&Number.hasOwnProperty('x')&&Number.propertyIsEnumerable('x');",
        "Object.defineProperty(Number,'x',{get:function(){return 2;},configurable:true,enumerable:true});Object.defineProperty(Number,'x',{value:3,writable:true});const d=Object.getOwnPropertyDescriptor(Number,'x');Number.x===3&&d.get===undefined&&d.writable&&d.enumerable&&d.configurable;",
        "Object.defineProperties(Number,{x:{value:4},y:{value:5}})===Number&&Number.x===4&&Object.getOwnPropertyDescriptors(Number).y.value===5;",
    ]);
}

#[test]
fn accessors_inheritance_and_reflect_keep_public_receiver() {
    check(&[
        "let seen;let n=0;Object.defineProperty(Number,'x',{get:function(){n++;return this.v;},set:function(v){seen=this;this.v=v;},configurable:true});const d=Object.getOwnPropertyDescriptor(Number,'x');const own=Object.hasOwn(Number,'x');const has=Reflect.has(Number,'x');Number.x=4;own&&has&&n===0&&seen===Number&&typeof d.get==='function'&&Number.x===4&&n===1&&Reflect.get(Number,'x',{v:8})===8;",
        "let seen;Object.defineProperty(Number,'x',{set:function(v){seen=this;this.v=v;}});const o=Object.create(Number);o.x=6;seen===o&&o.v===6&&!Object.hasOwn(o,'x');",
        "let seen;Object.defineProperty(Object.prototype,'x',{set:function(v){seen=this;this.v=v;},configurable:true});Number.x=7;seen===Number&&Number.v===7&&!Object.hasOwn(Number,'x');",
        "Number.x=1;const r={};Reflect.set(Number,'x',3,r)&&r.x===3&&Number.x===1&&!Reflect.set(Number,'x',4,1);",
        "Object.defineProperty(Number,'x',{get:function(){return 3;}});Number.x=4;Number.x===3&&!Reflect.set(Number,'x',5);",
        "const d={};Object.defineProperty(d,'value',{get:function(){throw 17;}});try{Reflect.defineProperty(Number,'x',d);false;}catch(e){e===17&&!Object.hasOwn(Number,'x');}",
        "const p=Object.create(Number);Number.isPrototypeOf(p)&&Function.prototype.isPrototypeOf(Number)&&Object.prototype.isPrototypeOf(Number);",
        "Number.x=1;Object.prototype.y=2;let keys='';for(const k in Number){keys+=k+',';}keys==='x,y,';",
    ]);
}

#[test]
fn symbol_keys_order_receivers_and_spread_agree() {
    check(&[
        "const a=Symbol(),b=Symbol();Number[a]=3;Number[b]=4;Number.x=2;const copy={...Number};Object.keys(copy).join(',')==='x'&&copy[a]===3&&Object.getOwnPropertySymbols(Number)[0]===a&&Reflect.ownKeys(Number)[19]===b;",
        "const k=Symbol();let seen;Object.defineProperty(Number,k,{get:function(){return this.v;},set:function(v){seen=this;this.v=v;},configurable:true});Number[k]=7;seen===Number&&Number[k]===7&&Reflect.get(Number,k,{v:8})===8&&Reflect.set(Number,k,9,{})&&seen!==Number;",
        "const k=Symbol();Object.defineProperty(Number,k,{value:3});!Reflect.defineProperty(Number,k,{value:4})&&!Reflect.set(Number,k,4)&&!Reflect.deleteProperty(Number,k)&&Number[k]===3;",
        "const k=Symbol();Number[k]=3;delete Number[k];!(k in Number)&&Number[k]===undefined&&Object.getOwnPropertySymbols(Number).length===0;",
    ]);
}

#[test]
fn integrity_levels_do_not_change_callable_or_prototype_identity() {
    check(&[
        "Number.x=1;Object.preventExtensions(Number)===Number&&!Object.isExtensible(Number)&&!Reflect.isExtensible(Number)&&!Reflect.defineProperty(Number,'y',{value:2})&&!Reflect.set(Number,'y',3)&&Reflect.set(Number,'x',4)&&Number.x===4;",
        "Number.x=1;Object.seal(Number)===Number&&Object.isSealed(Number)&&!Object.isFrozen(Number)&&!delete Number.x&&Reflect.set(Number,'x',3);",
        "Number.x=1;const k=Symbol();Number[k]=2;Object.freeze(Number)===Number&&Object.isFrozen(Number)&&!Reflect.set(Number,'x',3)&&!Reflect.set(Number,k,4)&&Number('12')===12&&new Number(4).valueOf()===4;",
        "let n=0;Object.defineProperty(Number,'x',{get:function(){return n;},set:function(v){n=v;},configurable:true});Object.freeze(Number);Reflect.set(Number,'x',6)&&Number.x===6&&Object.isFrozen(Number);",
        "Reflect.setPrototypeOf(Number,Function.prototype);try{Reflect.setPrototypeOf(Number,{});false;}catch(e){e.name==='TypeError'&&Object.getPrototypeOf(Number)===Function.prototype;}",
        "Object.preventExtensions(Number);Reflect.setPrototypeOf(Number,Function.prototype)&&!Reflect.setPrototypeOf(Number,{});",
    ]);
}

#[test]
fn other_natives_and_hosts_are_not_granted_descriptor_authority() {
    check(&[
        "try{Object.defineProperty(Object,'x',{value:1});false;}catch(e){e.name==='TypeError';}",
        "try{Object.defineProperty(Number.isFinite,'x',{value:1});false;}catch(e){e.name==='TypeError';}",
        "try{Object.defineProperty(Function.prototype,'call',{value:1});false;}catch(e){e.name==='TypeError'&&typeof Number.call==='function';}",
        "let touched=0;const key={toString:function(){touched++;return 'x';}};try{Object.defineProperty(opaque,key,{value:1});false;}catch(e){e.name==='TypeError'&&touched===0;}",
        "try{Reflect.defineProperty(opaque,'x',{value:1});false;}catch(e){e.name==='TypeError';}",
    ]);
}

#[test]
fn storage_accessors_symbols_are_gc_roots_and_runtime_local() {
    let mut vm = Runtime::default();
    vm.execute("(function(){const v={n:7};const k=Symbol();Number.slot=k;Number[k]={n:8};Object.defineProperty(Number,'x',{get:function(){return v;},configurable:true});})();", &mut Empty).unwrap();
    vm.collect_garbage(&[], &Empty).unwrap();
    assert_eq!(
        vm.execute("Number.x.n===7&&Number[Number.slot].n===8;", &mut Empty)
            .unwrap(),
        Value::Bool(true)
    );
    vm.execute(
        "delete Number.x;delete Number[Number.slot];delete Number.slot;",
        &mut Empty,
    )
    .unwrap();
    assert!(vm.collect_garbage(&[], &Empty).unwrap().reclaimed_objects >= 3);
    check(&["Number.x===undefined&&Number.slot===undefined&&Number.isFinite(2);"]);
}

#[test]
fn rollback_restores_storage_descriptors_and_integrity_together() {
    let mut vm = Runtime::default();
    vm.execute("Number.x=1;", &mut Empty).unwrap();
    let before = vm.clone();
    vm.execute(
        "Number.x=2;delete Number.isFinite;Object.freeze(Number);",
        &mut Empty,
    )
    .unwrap();
    vm.rollback_to(before);
    assert_eq!(vm.execute("Number.x===1&&Number.isFinite(2)&&Object.isExtensible(Number)&&Object.getOwnPropertyDescriptor(Number,'x').writable;", &mut Empty).unwrap(),Value::Bool(true));
}

#[test]
fn storage_preserves_property_heap_string_and_instruction_limits() {
    let mut vm = Runtime::default();
    vm.execute("for(let i=0;i<111;i++){Number['x'+i]=i;}", &mut Empty)
        .unwrap();
    assert_eq!(vm.own_names(&Value::Native("Number")).unwrap().len(), 128);
    assert_eq!(
        vm.execute(
            "Reflect.defineProperty(Number,'overflow',{value:1});",
            &mut Empty
        )
        .unwrap_err()
        .kind,
        ErrorKind::Resource
    );
    assert!(
        !vm.has_own_property(&Value::Native("Number"), "overflow")
            .unwrap()
    );
    let mut vm = Runtime::default();
    vm.execute(
        "for(let i=0;i<128;i++){Number[Symbol()]=i;}var extra=Symbol();",
        &mut Empty,
    )
    .unwrap();
    assert_eq!(
        vm.execute("Object.defineProperty(Number,extra,{value:1});", &mut Empty)
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
    for limits in [
        Limits {
            heap_objects: 1,
            ..Limits::default()
        },
        Limits {
            string_bytes: 20,
            ..Limits::default()
        },
    ] {
        assert_eq!(
            Runtime::new(limits)
                .execute("Number.name;", &mut Empty)
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
    }
    vm.remaining = 0;
    assert_eq!(
        vm.own_names(&Value::Native("Number")).unwrap_err().kind,
        ErrorKind::Resource
    );
}
