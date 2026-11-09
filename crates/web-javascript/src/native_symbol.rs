//! Bounded own properties of the Symbol intrinsic function.

use crate::{
    JsError, Runtime, Value,
    descriptors::{Descriptor, PropertyMetadata},
};
use std::collections::BTreeMap;

pub(super) const FUNCTION_PROTOTYPE_NAMES: &[&str] =
    &["length", "name", "constructor", "call", "apply", "bind"];
// Exactly the well-known symbols previously exposed by stdlib_get. This is not
// the full ECMAScript inventory: e.g. matchAll/dispose are not added here.
const WELL_KNOWN_NAMES: &[&str] = &[
    "asyncIterator",
    "hasInstance",
    "isConcatSpreadable",
    "iterator",
    "match",
    "replace",
    "search",
    "species",
    "split",
    "toPrimitive",
    "toStringTag",
    "unscopables",
];

impl Runtime {
    /// The callable identity remains Native("Symbol"). Only its property storage
    /// uses an ordinary heap object. Never return that object to page code.
    pub(super) fn symbol_constructor_owner(&mut self) -> Result<usize, JsError> {
        if let Some(id) = self.reflection.symbol_constructor {
            return Ok(id);
        }
        let prototype = self.symbol_prototype()?;
        let name = self.string("Symbol".into())?;
        let mut entries = vec![
            ("length", Value::Number(0.), false, true),
            ("name", name, false, true),
            ("prototype", prototype, false, false),
            ("for", Value::Native("Symbol.for"), true, true),
            ("keyFor", Value::Native("Symbol.keyFor"), true, true),
        ];
        for &name in WELL_KNOWN_NAMES {
            entries.push((name, self.well_known_symbol(name)?, false, false));
        }
        self.charge_string(entries.iter().map(|(key, _, _, _)| key.len() * 3).sum())?;
        let object = self.object(
            entries
                .iter()
                .map(|(key, value, _, _)| ((*key).into(), value.clone()))
                .collect(),
            Some(Value::Native("Function.prototype")),
        )?;
        let Value::Object(id) = object else {
            unreachable!()
        };
        self.remember_property_order(
            id,
            entries.iter().map(|(key, _, _, _)| (*key).into()).collect(),
        );
        let metadata = entries
            .into_iter()
            .map(|(key, _, writable, configurable)| {
                (
                    key.into(),
                    PropertyMetadata {
                        writable,
                        enumerable: false,
                        configurable,
                        accessor: None,
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        self.property_metadata.insert(id, metadata);
        self.reflection.symbol_constructor = Some(id);
        Ok(id)
    }

    /// Read-only inventory of Function.prototype fields already exposed by the
    /// VM. It enables inherited lookup without permitting native mutations.
    pub(super) fn function_prototype_descriptor(
        &mut self,
        key: &str,
    ) -> Result<Option<Descriptor>, JsError> {
        let value = match key {
            "length" => Value::Number(0.),
            "name" => self.string(String::new())?,
            _ => match crate::callables::callable_method(key) {
                Some(value) => value,
                None => return Ok(None),
            },
        };
        Ok(Some(Descriptor {
            value,
            metadata: PropertyMetadata {
                writable: !matches!(key, "length" | "name"),
                enumerable: false,
                configurable: true,
                accessor: None,
            },
        }))
    }
}

#[cfg(test)]
mod tests {
    use crate::{Host, JsError, Runtime, Value};

    struct Empty;
    impl Host for Empty {
        fn global(&mut self, name: &str) -> Option<Value> {
            (name == "opaque").then_some(Value::Host(7))
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

    #[test]
    fn native_symbol_polyfill_defines_real_property() {
        let html = include_str!("fixtures/symbol-constructor-polyfill.html");
        let source = html
            .split_once("<script>")
            .unwrap()
            .1
            .split_once("</script>")
            .unwrap()
            .0;
        assert_eq!(
            Runtime::default().execute(source, &mut Empty).unwrap(),
            Value::Bool(true)
        );
    }

    fn check(sources: &[&str]) {
        for source in sources {
            let result = Runtime::default().execute(source, &mut Empty);
            assert_eq!(
                result.unwrap_or_else(|error| panic!("{source}\n{error:?}")),
                Value::Bool(true),
                "{source}"
            );
        }
    }

    #[test]
    fn intrinsic_descriptors_and_callable_identity_are_preserved() {
        check(&[
            "Symbol.dispose===undefined && !('dispose' in Symbol) && !Object.hasOwn(Symbol,'dispose');",
            "const d=Object.getOwnPropertyDescriptor(Symbol,'name');d.value==='Symbol'&&!d.writable&&!d.enumerable&&d.configurable;",
            "const d=Object.getOwnPropertyDescriptor(Symbol,'length');d.value===0&&!d.writable&&!d.enumerable&&d.configurable;",
            "const d=Object.getOwnPropertyDescriptor(Symbol,'prototype');d.value===Symbol.prototype&&!d.writable&&!d.enumerable&&!d.configurable;",
            "const d=Object.getOwnPropertyDescriptor(Symbol,'iterator');d.value===Symbol.iterator&&!d.writable&&!d.enumerable&&!d.configurable;",
            "const d=Object.getOwnPropertyDescriptor(Symbol,'for');d.value===Symbol.for&&d.writable&&!d.enumerable&&d.configurable&&d.value.length===1;",
            "const d=Object.getOwnPropertyDescriptor(Symbol,'keyFor');d.value===Symbol.keyFor&&d.writable&&!d.enumerable&&d.configurable&&d.value.length===1;",
            "Object.keys(Symbol).length===0&&Object.getOwnPropertyNames(Symbol).length===17&&Reflect.ownKeys(Symbol).length===17;",
            "const before=Symbol;Symbol.extra=5;Object(Symbol)===before&&Symbol.prototype.constructor===before&&typeof Symbol()==='symbol'&&Symbol.call(null,'x').description==='x'&&Symbol.bind(null,'y')().description==='y';",
            "Object.getPrototypeOf(Symbol)===Function.prototype&&Reflect.getPrototypeOf(Symbol)===Function.prototype&&Object.getPrototypeOf(Function.prototype)===Object.prototype;",
            "Object.getOwnPropertyDescriptor(Function.prototype,'name').value===Function.prototype.name&&Object.getOwnPropertyDescriptor(Function.prototype,'length').value===Function.prototype.length;",
            "Symbol instanceof Function&&Symbol instanceof Object&&Object.getPrototypeOf(Object.create(Symbol))===Symbol;",
            "try{new Symbol();false;}catch(e){e.name==='TypeError';}",
        ]);
    }

    #[test]
    fn string_keys_assign_define_delete_and_redefine_consistently() {
        check(&[
            "Symbol.x=7;Object.hasOwn(Symbol,'x')&&Symbol.hasOwnProperty('x')&&Symbol.propertyIsEnumerable('x')&&Symbol.x===7&&Object.keys(Symbol).join(',')==='x';",
            "Object.defineProperty(Symbol,'x',{value:3});Symbol.x=4;const d=Object.getOwnPropertyDescriptor(Symbol,'x');Symbol.x===3&&!d.writable&&!d.configurable&&!d.enumerable&&!delete Symbol.x;",
            "Object.defineProperty(Symbol,'x',{value:3});try{(function(){'use strict';Symbol.x=4;})();false;}catch(e){e.name==='TypeError'&&Symbol.x===3;}",
            "Object.defineProperty(Symbol,'x',{value:3});try{(function(){'use strict';delete Symbol.x;})();false;}catch(e){e.name==='TypeError';}",
            "const fn=Symbol.for;Symbol.for=function(){return 4;};const changed=Symbol.for()===4;delete Symbol.for;changed&&!('for' in Symbol)&&!Object.hasOwn(Symbol,'for')&&Symbol.for===undefined&&fn('x')===fn('x');",
            "delete Symbol.name;!Object.hasOwn(Symbol,'name')&&Symbol.name===''&&('name' in Symbol)&&Object.keys(Symbol).length===0;",
            "Object.defineProperty(Symbol,'name',{value:'changed'});Symbol.name==='changed'&&!Object.getOwnPropertyDescriptor(Symbol,'name').writable;",
            "Symbol[2]=2;Symbol.b=3;Symbol.a=4;delete Symbol.b;Symbol.b=5;Object.keys(Symbol).join(',')==='2,a,b';",
            "Symbol.x=1;const d=Object.getOwnPropertyDescriptors(Symbol);Object.keys(d).length===18&&d.x.value===1&&d.iterator.value===Symbol.iterator;",
            "Object.defineProperties(Symbol,{x:{value:1},y:{value:2}})===Symbol&&Symbol.x===1&&Symbol.y===2;",
            "const copy=Object.assign({},Symbol);Object.keys(copy).length===0&&Object.assign(Symbol,{x:4})===Symbol&&Symbol.x===4;",
            "Symbol.x=2;const key=Symbol();Symbol[key]=3;const copy={...Symbol};Object.keys(copy).join(',')==='x'&&copy.x===2&&copy[key]===3;",
            "Object.defineProperty(Symbol,'x',{get:function(){return 2;},configurable:true,enumerable:true});Object.defineProperty(Symbol,'x',{value:3,writable:true});const d=Object.getOwnPropertyDescriptor(Symbol,'x');Symbol.x===3&&d.writable&&d.configurable&&d.enumerable&&d.get===undefined;",
        ]);
    }

    #[test]
    fn symbol_keys_keep_descriptors_accessors_order_and_deletion() {
        check(&[
            "const a=Symbol(),b=Symbol();Symbol[a]=3;Symbol[b]=4;const d=Object.getOwnPropertyDescriptor(Symbol,a);d.value===3&&d.writable&&d.enumerable&&d.configurable&&Object.getOwnPropertySymbols(Symbol)[0]===a&&Reflect.ownKeys(Symbol)[18]===b;",
            "const k=Symbol();Object.defineProperty(Symbol,k,{value:3});Symbol[k]=4;Symbol[k]===3&&!Reflect.set(Symbol,k,4)&&!Reflect.defineProperty(Symbol,k,{value:4})&&!Reflect.deleteProperty(Symbol,k);",
            "const k=Symbol();Symbol[k]=3;const d=Object.getOwnPropertyDescriptors(Symbol);d[k].value===3&&Object.getOwnPropertySymbols(d)[0]===k;",
            "const k=Symbol();let seen;Object.defineProperty(Symbol,k,{get:function(){return this.v;},set:function(x){seen=this;this.v=x;},configurable:true});Symbol[k]=5;const r={v:8};seen===Symbol&&Symbol[k]===5&&Reflect.get(Symbol,k,r)===8&&Reflect.set(Symbol,k,9,r)&&r.v===9&&seen===r;",
            "const k=Symbol();Symbol[k]=3;delete Symbol[k];!(k in Symbol)&&Symbol[k]===undefined&&Object.getOwnPropertySymbols(Symbol).length===0;",
            "const k=Symbol();const p={};Object.defineProperty(Symbol,k,{get:function(){return this.x;}});const o=Object.create(Symbol);o.x=4;o[k]===4&&Reflect.has(o,k)&&!Object.hasOwn(o,k);",
            "const k=Symbol();Object.defineProperty(Symbol,k,{value:3,configurable:true,enumerable:true});Object.defineProperty(Symbol,k,{get:function(){return 4;}});const d=Object.getOwnPropertyDescriptor(Symbol,k);Symbol[k]===4&&d.value===undefined&&d.writable===undefined&&d.configurable&&d.enumerable;",
        ]);
    }

    #[test]
    fn inherited_descriptors_and_receivers_are_observed_without_early_gets() {
        check(&[
            "let n=0;Object.defineProperty(Symbol,'x',{get:function(){n++;return this.v;},configurable:true});Symbol.v=4;const d=Object.getOwnPropertyDescriptor(Symbol,'x');const present='x' in Symbol;const own=Object.hasOwn(Symbol,'x');const reflected=Reflect.has(Symbol,'x');n===0&&present&&own&&reflected&&typeof d.get==='function'&&Symbol.x===4&&n===1&&Reflect.get(Symbol,'x',{v:8})===8&&n===2;",
            "let seen;Object.defineProperty(Symbol,'x',{set:function(v){seen=this;this.v=v;}});const o=Object.create(Symbol);o.x=5;seen===o&&o.v===5&&!Object.hasOwn(o,'x');",
            "let seen;Object.defineProperty(Object.prototype,'x',{set:function(v){seen=this;this.v=v;},configurable:true});Symbol.x=6;seen===Symbol&&Symbol.v===6&&!Object.hasOwn(Symbol,'x');",
            "let n=0;Object.defineProperty(Object.prototype,'x',{get:function(){n++;return this.v;},configurable:true});Symbol.v=6;('x' in Symbol)&&Reflect.has(Symbol,'x')&&n===0&&Symbol.x===6&&Reflect.get(Symbol,'x',{v:7})===7&&n===2;",
            "const key=Symbol();Object.defineProperty(Object.prototype,key,{get:function(){return this.v;},configurable:true});Symbol.v=6;key in Symbol&&Symbol[key]===6&&Reflect.get(Symbol,key,{v:7})===7;",
            "const key=Symbol();let seen;Object.defineProperty(Object.prototype,key,{set:function(v){seen=this;this.v=v;},configurable:true});Symbol[key]=8;seen===Symbol&&Symbol.v===8&&!Object.hasOwn(Symbol,key);",
            "Symbol.x=1;Object.prototype.y=2;let keys='';for(const key in Symbol){keys+=key+',';}keys==='x,y,';",
            "const d=Object.getOwnPropertyDescriptor(Function.prototype,'call');d.value===Symbol.call&&d.writable&&!d.enumerable&&d.configurable&&!Object.hasOwn(Symbol,'call');",
            "const o=Object.create(Symbol);Symbol.isPrototypeOf(o)&&Function.prototype.isPrototypeOf(Symbol)&&Object.prototype.isPrototypeOf(Symbol);",
            "Reflect.set(Symbol,'call',7)&&Symbol.call===7&&Function.prototype.call!==7;",
        ]);
    }

    #[test]
    fn reflect_rejects_invariants_with_false_but_propagates_real_exceptions() {
        check(&[
            "const x=Symbol.iterator;!Reflect.defineProperty(Symbol,'iterator',{value:Symbol()})&&!Reflect.set(Symbol,'iterator',Symbol())&&!Reflect.deleteProperty(Symbol,'iterator')&&Symbol.iterator===x;",
            "const x=Symbol.iterator;Reflect.defineProperty(Symbol,'iterator',{value:x})&&!Reflect.defineProperty(Symbol,'iterator',{writable:true})&&!Reflect.defineProperty(Symbol,'iterator',{enumerable:true})&&!Reflect.defineProperty(Symbol,'iterator',{configurable:true});",
            "try{Object.defineProperty(Symbol,'iterator',{value:0});false;}catch(e){e.name==='TypeError';}",
            "try{Reflect.defineProperty(Symbol,'x',{value:1,get:function(){}});false;}catch(e){e.name==='TypeError'&&!Object.hasOwn(Symbol,'x');}",
            "const d={};Object.defineProperty(d,'value',{get:function(){throw 17;}});try{Reflect.defineProperty(Symbol,'x',d);false;}catch(e){e===17&&!Object.hasOwn(Symbol,'x');}",
            "const k={};k[Symbol.toPrimitive]=function(){throw 19;};try{Reflect.defineProperty(Symbol,k,{});false;}catch(e){e===19;}",
            "Object.defineProperty(Symbol,'x',{get:function(){throw 23;}});try{Reflect.get(Symbol,'x');false;}catch(e){e===23;}",
            "Object.defineProperty(Symbol,'x',{set:function(){throw 29;}});try{Reflect.set(Symbol,'x',1);false;}catch(e){e===29;}",
            "Object.defineProperty(Symbol,'nan',{value:NaN});Reflect.defineProperty(Symbol,'nan',{value:NaN})&&!Reflect.defineProperty(Symbol,'nan',{value:1});",
            "Object.defineProperty(Symbol,'zero',{value:0});!Reflect.defineProperty(Symbol,'zero',{value:-0})&&Reflect.defineProperty(Symbol,'zero',{value:0});",
            "Symbol.x=2;const receiver={};Reflect.set(Symbol,'x',3,receiver)&&receiver.x===3&&Symbol.x===2&&!Reflect.set(Symbol,'x',4,1);",
            "Reflect.set(Symbol,'newKey',1)&&Symbol.newKey===1&&Reflect.getOwnPropertyDescriptor(Symbol,'newKey').enumerable;",
        ]);
    }

    #[test]
    fn integrity_levels_apply_to_strings_symbols_and_accessor_properties() {
        check(&[
            "Symbol.x=1;Object.preventExtensions(Symbol)===Symbol&&!Object.isExtensible(Symbol)&&!Reflect.isExtensible(Symbol)&&!Reflect.defineProperty(Symbol,'newKey',{value:2})&&!Reflect.set(Symbol,'newKey',2)&&Reflect.set(Symbol,'x',3)&&Symbol.x===3;",
            "Object.preventExtensions(Symbol);try{Object.defineProperty(Symbol,'newKey',{value:2});false;}catch(e){e.name==='TypeError';}",
            "Object.preventExtensions(Symbol);Symbol.x=4;!Object.hasOwn(Symbol,'x')&&Symbol.x===undefined;",
            "Symbol.x=1;const k=Symbol();Symbol[k]=2;Object.seal(Symbol)===Symbol&&Object.isSealed(Symbol)&&!Object.isFrozen(Symbol)&&!delete Symbol.x&&!delete Symbol[k]&&Reflect.set(Symbol,'x',3)&&Symbol.x===3;",
            "Symbol.x=1;const k=Symbol();Symbol[k]=2;Object.freeze(Symbol)===Symbol&&Object.isFrozen(Symbol)&&!Reflect.set(Symbol,'x',3)&&!Reflect.set(Symbol,k,3)&&Symbol.x===1&&Symbol[k]===2&&typeof Symbol()==='symbol';",
            "let n=0;Object.defineProperty(Symbol,'x',{get:function(){return n;},set:function(v){n=v;},configurable:true});Object.freeze(Symbol);Reflect.set(Symbol,'x',6)&&Symbol.x===6&&Object.isFrozen(Symbol);",
            "Object.preventExtensions(Symbol);Reflect.setPrototypeOf(Symbol,Function.prototype)&&!Reflect.setPrototypeOf(Symbol,{});",
            "Reflect.setPrototypeOf(Symbol,Function.prototype);try{Reflect.setPrototypeOf(Symbol,{});false;}catch(e){e.name==='TypeError'&&Object.getPrototypeOf(Symbol)===Function.prototype;}",
        ]);
    }

    #[test]
    fn unsupported_natives_remain_explicit_and_cannot_mutate_function_prototype() {
        check(&[
            "try{Object.defineProperty(Object,'x',{value:1});false;}catch(e){e.name==='TypeError';}",
            "try{Object.defineProperty(Function.prototype,'call',{value:1});false;}catch(e){e.name==='TypeError'&&typeof Function.prototype.call==='function';}",
            "try{delete Function.prototype.call;false;}catch(e){e.name==='TypeError'&&typeof Symbol.call==='function';}",
            "try{Reflect.defineProperty(Function.prototype,'call',{value:1});false;}catch(e){e.name==='TypeError';}",
            "try{Object.freeze(Function.prototype);false;}catch(e){e.name==='TypeError';}",
            "try{Object.defineProperty(opaque,'x',{value:1});false;}catch(e){e.name==='TypeError';}",
            "try{Reflect.defineProperty(opaque,'x',{value:1});false;}catch(e){e.name==='TypeError';}",
            "try{Object.getOwnPropertyDescriptor(opaque,'x');false;}catch(e){e.name==='TypeError';}",
        ]);
    }

    #[test]
    fn symbol_storage_and_accessors_are_gc_roots_but_do_not_leak_across_runtimes() {
        let mut runtime = Runtime::default();
        let mut host = Empty;
        runtime.execute("(function(){const value={n:7};const key=Symbol();Symbol.slot=key;Object.defineProperty(Symbol,'x',{get:function(){return value;},configurable:true});Symbol[key]={n:8};})();", &mut host).unwrap();
        runtime.collect_garbage(&[], &host).unwrap();
        assert_eq!(
            runtime
                .execute("Symbol.x.n===7&&Symbol[Symbol.slot].n===8;", &mut host)
                .unwrap(),
            Value::Bool(true)
        );
        runtime
            .execute(
                "delete Symbol.x;delete Symbol[Symbol.slot];delete Symbol.slot;",
                &mut host,
            )
            .unwrap();
        let stats = runtime.collect_garbage(&[], &host).unwrap();
        assert!(stats.reclaimed_objects >= 3, "{stats:?}");
        assert_eq!(
            Runtime::default()
                .execute(
                    "Symbol.x===undefined&&Symbol.slot===undefined&&Symbol.dispose===undefined;",
                    &mut Empty
                )
                .unwrap(),
            Value::Bool(true)
        );
    }

    #[test]
    fn symbol_storage_obeys_heap_string_property_symbol_and_work_quotas() {
        use crate::{ErrorKind, Limits};
        for source in [
            "for(let i=0;i<129;i++){Object.defineProperty(Symbol,'x'+i,{value:i});}",
            "for(let i=0;i<129;i++){Object.defineProperty(Symbol,Symbol(),{value:i});}",
            "Reflect.defineProperty(Symbol,'x'.repeat(1025),{value:1});",
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
        let limits = Limits {
            heap_objects: 1,
            ..Limits::default()
        };
        assert_eq!(
            Runtime::new(limits)
                .execute("Symbol.name;", &mut Empty)
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
        let limits = Limits {
            string_bytes: 20,
            ..Limits::default()
        };
        assert_eq!(
            Runtime::new(limits)
                .execute("Symbol.name;", &mut Empty)
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
        let mut runtime = Runtime::default();
        runtime.execute("Symbol.x=1;", &mut Empty).unwrap();
        runtime.remaining = 0;
        assert_eq!(
            runtime
                .own_names(&Value::Native("Symbol"))
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
    }

    #[test]
    fn exact_property_caps_and_failed_definitions_do_not_claim_success() {
        let mut runtime = Runtime::default();
        let mut host = Empty;
        runtime
            .execute(
                "for(let i=0;i<111;i++){Symbol['x'+i]=i;}const overflowKey=Symbol();",
                &mut host,
            )
            .unwrap();
        assert_eq!(
            runtime
                .execute(
                    "Object.getOwnPropertyNames(Symbol).length===128;",
                    &mut host
                )
                .unwrap(),
            Value::Bool(true)
        );
        let error = runtime
            .execute(
                "Reflect.defineProperty(Symbol,'overflow',{value:1});",
                &mut host,
            )
            .unwrap_err();
        assert_eq!(error.kind, crate::ErrorKind::Resource);
        assert!(
            !runtime
                .has_own_property(&Value::Native("Symbol"), "overflow")
                .unwrap()
        );
        let mut runtime = Runtime::default();
        runtime
            .execute(
                "for(let i=0;i<128;i++){Symbol[Symbol()]=i;}var extra=Symbol();",
                &mut host,
            )
            .unwrap();
        assert_eq!(
            runtime
                .execute(
                    "Object.getOwnPropertySymbols(Symbol).length===128;",
                    &mut host
                )
                .unwrap(),
            Value::Bool(true)
        );
        let error = runtime
            .execute("Reflect.defineProperty(Symbol,extra,{value:1});", &mut host)
            .unwrap_err();
        assert_eq!(error.kind, crate::ErrorKind::Resource);
        assert_eq!(
            runtime
                .execute("!Object.hasOwn(Symbol,extra);", &mut host)
                .unwrap(),
            Value::Bool(true)
        );
    }

    #[test]
    fn rollback_restores_native_property_state_and_cannot_refund_work() {
        let mut runtime = Runtime::default();
        let mut host = Empty;
        runtime.execute("Symbol.x=1;", &mut host).unwrap();
        let checkpoint = runtime.clone();
        runtime.execute("Symbol.x=2;delete Symbol.name;Symbol.extra={n:7};Object.preventExtensions(Symbol);", &mut host).unwrap();
        let remaining = runtime.remaining;
        runtime.rollback_to(checkpoint);
        assert_eq!(runtime.remaining, remaining);
        runtime.collect_garbage(&[], &host).unwrap();
        assert_eq!(runtime.execute("Symbol.x===1&&Symbol.name==='Symbol'&&Symbol.extra===undefined&&Object.isExtensible(Symbol);", &mut host).unwrap(), Value::Bool(true));
    }
}
