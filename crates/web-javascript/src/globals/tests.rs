use crate::{ErrorKind, Host, JsError, Limits, Runtime, Value};
struct Window;
impl Host for Window {
    fn global_object(&self) -> Option<u64> {
        Some(2)
    }
    fn global(&mut self, name: &str) -> Option<Value> {
        match name {
            "window" | "globalThis" | "self" => Some(Value::Host(2)),
            "document" => Some(Value::Host(1)),
            _ => None,
        }
    }
    fn get(&mut self, object: u64, name: &str) -> Result<Value, JsError> {
        if object == 2 {
            Ok(self.global(name).unwrap_or(Value::Undefined))
        } else {
            Ok(Value::Undefined)
        }
    }
    fn has_property(&mut self, object: u64, name: &str) -> Result<bool, JsError> {
        Ok(object == 2
            && matches!(
                name,
                "window" | "globalThis" | "self" | "document" | "location"
            ))
    }
    fn global_accessor(&self, name: &str) -> bool {
        name == "location"
    }
    fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
        Err(JsError::runtime("Unexpected host mutation"))
    }
    fn call(&mut self, _: u64, _: &str, _: &[Value]) -> Result<Value, JsError> {
        Err(JsError::runtime("Unexpected host call"))
    }
}
fn check(source: &str) {
    assert_eq!(
        Runtime::default().execute(source, &mut Window).unwrap(),
        Value::Bool(true),
        "{source}"
    );
}

#[test]
fn map_polyfill_replaces_only_the_global_binding() {
    check(
        "const original=Map;function Polyfill(){this.ok=true;}const result=Object.defineProperty(window,'Map',{configurable:true,writable:true,value:Polyfill});const d=Object.getOwnPropertyDescriptor(window,'Map');result===window&&Map===Polyfill&&window.Map===Map&&globalThis.Map===Map&&new Map().ok&&original!==Map&&d.value===Polyfill&&d.writable&&d.configurable&&!d.enumerable;",
    );
}
#[test]
fn default_flags_strict_writes_and_same_value_invariants() {
    check(
        "Object.defineProperty(window,'x',{value:17});x=99;window.x=4;let strict=false;try{(function(){'use strict';x=2;})();}catch(e){strict=e.name==='TypeError';}let redefined=false;try{Object.defineProperty(window,'x',{value:18});}catch(e){redefined=e.name==='TypeError';}Object.defineProperty(window,'x',{value:17});const d=Object.getOwnPropertyDescriptor(window,'x');x===17&&strict&&redefined&&!d.writable&&!d.enumerable&&!d.configurable&&!(delete window.x);",
    );
    check(
        "Object.defineProperty(window,'n',{value:NaN});Object.defineProperty(window,'n',{value:NaN});Object.defineProperty(window,'z',{value:0});let failed=false;try{Object.defineProperty(window,'z',{value:-0});}catch(e){failed=true;}failed;",
    );
}
#[test]
fn getters_setters_use_window_and_metadata_reads_never_invoke_them() {
    check(
        "let gets=0;let stored=1;Object.defineProperty(window,'x',{get:function(){gets++;return this===window?stored:-1;},set:function(v){stored=this===window?v:-2;},configurable:true,enumerable:true});const d=Object.getOwnPropertyDescriptor(window,'x');const presence=Object.hasOwn(window,'x')&&('x' in window)&&Object.prototype.propertyIsEnumerable.call(window,'x');const noRead=gets===0;x=7;const a=x;window.x++;noRead&&presence&&a===7&&window.x===8&&gets===3&&typeof d.get==='function'&&d.value===undefined;",
    );
}
#[test]
fn accessor_exceptions_and_missing_setters_keep_ordinary_semantics() {
    check(
        "Object.defineProperty(window,'x',{get:function(){throw 73;},configurable:true});let caught=false;try{x;}catch(e){caught=e===73;}x=1;let strict=false;try{(function(){'use strict';window.x=2;})();}catch(e){strict=e.name==='TypeError';}caught&&strict&&delete window.x&&typeof x==='undefined';",
    );
}
#[test]
fn existing_var_data_bindings_remain_nonconfigurable_and_synchronized() {
    check(
        "var x=1;Object.defineProperty(window,'x',{value:2});const d=Object.getOwnPropertyDescriptor(window,'x');x++;window.x+=2;const updated=x===5;Object.defineProperty(window,'x',{writable:false});x=9;var x=10;updated&&x===5&&window.x===5&&d.enumerable&&d.writable&&!d.configurable&&!(delete window.x);",
    );
}
#[test]
fn var_redeclaration_preserves_preexisting_accessors_and_initializers_use_setters() {
    let mut runtime = Runtime::default();
    runtime.execute("var writes=0;Object.defineProperty(window,'x',{get:function(){return 4;},set:function(v){writes+=v;},configurable:true});", &mut Window).unwrap();
    assert_eq!(runtime.execute("var x;x+=3;var x=9;var {x}={x:2};for(var x of [1,2]){};x===4&&window.x===4&&writes===21;", &mut Window).unwrap(), Value::Bool(true));
}
#[test]
fn lexical_bindings_stay_separate_and_restricted_globals_block_declarations() {
    check(
        "let x=7;Object.defineProperty(window,'x',{value:9,writable:true,configurable:true});x++;window.x++;x===8&&window.x===10;",
    );
    let mut runtime = Runtime::default();
    runtime
        .execute(
            "Object.defineProperty(window,'blocked',{value:1});",
            &mut Window,
        )
        .unwrap();
    assert_eq!(
        runtime
            .execute("let blocked=2;", &mut Window)
            .unwrap_err()
            .kind,
        ErrorKind::Syntax
    );
}
#[test]
fn function_redeclarations_observe_existing_property_invariants() {
    let mut runtime = Runtime::default();
    runtime.execute("window.f=3;", &mut Window).unwrap();
    assert_eq!(
        runtime
            .execute("function f(){return 7;}f===window.f&&f()===7;", &mut Window)
            .unwrap(),
        Value::Bool(true)
    );
    let mut runtime = Runtime::default();
    runtime
        .execute("Object.defineProperty(window,'f',{value:3});", &mut Window)
        .unwrap();
    assert!(runtime.execute("function f(){}", &mut Window).is_err());
    assert_eq!(
        runtime
            .execute("f===3&&window.f===3;", &mut Window)
            .unwrap(),
        Value::Bool(true)
    );
}
#[test]
fn configurable_global_deletion_removes_metadata_binding_and_intrinsic_fallback() {
    let mut runtime = Runtime::default();
    runtime
        .execute(
            "window.x=2;Object.defineProperty(window,'x',{enumerable:false});",
            &mut Window,
        )
        .unwrap();
    assert_eq!(runtime.execute("var x;const deleted=delete window.x;window.x=8;const d=Object.getOwnPropertyDescriptor(window,'x');deleted&&x===8&&d.enumerable&&d.writable&&d.configurable;", &mut Window).unwrap(), Value::Bool(true));
    check(
        "Object.defineProperty(window,'Map',{value:3});const gone=delete window.Map;gone&&window.Map===undefined&&typeof Map==='undefined'&&!('Map' in window)&&!Object.hasOwn(window,'Map')&&Object.getOwnPropertyDescriptor(window,'Map')===undefined;",
    );
}
#[test]
fn descriptor_batches_coerce_in_order_and_do_not_commit_before_conversion_finishes() {
    check(
        "let order='';const key={toString:function(){order+='k';return 'x';}};const descriptor={};Object.defineProperty(descriptor,'value',{get:function(){order+='v';return 3;}});Object.defineProperty(descriptor,'writable',{get:function(){order+='w';return true;}});Object.defineProperty(window,key,descriptor);Object.defineProperties(window,{a:{value:1},b:{value:2}});order==='kvw'&&x===3&&a===1&&b===2;",
    );
    check(
        "const bad={};Object.defineProperty(bad,'value',{get:function(){throw 42;}});const descriptors={a:{value:1},b:bad};let caught=false;try{Object.defineProperties(window,descriptors);}catch(e){caught=e===42;}caught&&!Object.hasOwn(window,'a');",
    );
}
#[test]
fn globals_trace_accessors_symbols_and_data_through_gc() {
    let mut runtime = Runtime::default();
    runtime.execute("var key=Symbol('key');Object.defineProperty(window,'held',{get:(function(){const data={value:17};return function(){return data;};})()});Object.defineProperty(window,key,{value:{value:19}});", &mut Window).unwrap();
    runtime.collect_garbage(&[], &Window).unwrap();
    assert_eq!(runtime.execute("held.value===17&&window[key].value===19&&Object.getOwnPropertySymbols(window)[0]===key;", &mut Window).unwrap(),Value::Bool(true));
}
#[test]
fn host_backed_properties_and_other_hosts_remain_refused_without_running_getters() {
    check(
        "let count=0;let failures=0;const descriptor={};Object.defineProperty(descriptor,'value',{get:function(){count++;return 1;}});try{Object.defineProperty(document,'x',descriptor);}catch(e){failures++;}try{Object.defineProperty(window,'document',{value:1});}catch(e){failures++;}try{Object.defineProperty(window,'location',{value:'https://example.test/'});}catch(e){failures++;}count===0&&failures===3&&typeof document==='object';",
    );
    check(
        "let failures=0;try{Object.keys(window);}catch(e){failures++;}try{Object.freeze(window);}catch(e){failures++;}try{Object.getOwnPropertyDescriptor(window,'document');}catch(e){failures++;}failures===3;",
    );
}
#[test]
fn descriptor_limits_remain_fatal_and_do_not_expand_host_authority() {
    assert_eq!(
        Runtime::default()
            .execute(
                "Object.defineProperty(window,'x'.repeat(1025),{value:1});",
                &mut Window
            )
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
    let mut vm = Runtime::new(Limits {
        instructions: 10_000_000,
        ..Limits::default()
    });
    vm.capture_global_object(&Window).unwrap();
    for i in 0..4096 {
        vm.define_global_property(
            &format!("p{i}"),
            crate::descriptors::Definition {
                value: Some(Value::Number(i as f64)),
                ..crate::descriptors::Definition::default()
            },
            &mut Window,
        )
        .unwrap();
    }
    assert_eq!(vm.global_stored("p4095"), Some(Value::Number(4095.)));
    let error = vm
        .define_global_property(
            "overflow",
            crate::descriptors::Definition {
                value: Some(Value::Null),
                ..crate::descriptors::Definition::default()
            },
            &mut Window,
        )
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert_eq!(error.message, "Object property limit exceeded");
    assert_eq!(vm.global_stored("overflow"), None);
}

#[test]
fn builtin_var_declarations_preserve_original_descriptor_flags_and_namespace_identity() {
    check(
        "var Map;var Math;const m=Object.getOwnPropertyDescriptor(window,'Map');const math=Object.getOwnPropertyDescriptor(window,'Math');m.writable&&!m.enumerable&&m.configurable&&m.value===Map&&math.value===Math&&math.value===window.Math&&math.configurable;",
    );
    let mut runtime = Runtime::default();
    runtime.execute("var Map;", &mut Window).unwrap();
    assert_eq!(runtime.execute("function Map(){return 23;}const d=Object.getOwnPropertyDescriptor(window,'Map');!d.configurable&&d.enumerable&&d.writable&&Map()===23;", &mut Window).unwrap(),Value::Bool(true));
}
#[test]
fn eval_function_and_nested_scripts_share_the_descriptor_backed_global() {
    check(
        "Object.defineProperty(window,'x',{value:1,writable:true,configurable:true});eval('x+=2;');(0,eval)('var x=4;');const read=Function('return x;');read()===4&&window.x===4;",
    );
    let mut runtime = Runtime::default();
    runtime
        .execute(
            "Object.defineProperty(window,'x',{value:1,writable:true});",
            &mut Window,
        )
        .unwrap();
    runtime
        .execute_nested_script("var x=7;", &mut Window)
        .unwrap();
    assert_eq!(
        runtime
            .execute("window.x===7&&x===7;", &mut Window)
            .unwrap(),
        Value::Bool(true)
    );
}
#[test]
fn configurable_var_delete_identifier_and_strict_deleted_builtin_assignment() {
    let mut runtime = Runtime::default();
    runtime.execute("window.x=1;", &mut Window).unwrap();
    assert_eq!(
        runtime
            .execute(
                "var x;const gone=delete x;gone&&typeof x==='undefined';",
                &mut Window
            )
            .unwrap(),
        Value::Bool(true)
    );
    check(
        "delete window.Map;let failed=false;try{(function(){'use strict';Map=1;})();}catch(e){failed=true;}failed&&window.Map===undefined;",
    );
}
#[test]
fn global_constants_report_real_descriptors_and_lexical_tdz_takes_precedence() {
    check(
        "const d=Object.getOwnPropertyDescriptor(window,'undefined');const n=Object.getOwnPropertyDescriptor(window,'NaN');!d.writable&&!d.enumerable&&!d.configurable&&d.value===undefined&&Number.isNaN(n.value)&&!(delete window.NaN);",
    );
    let mut runtime = Runtime::default();
    runtime
        .execute(
            "Object.defineProperty(window,'x',{value:3,configurable:true});",
            &mut Window,
        )
        .unwrap();
    assert_eq!(runtime.execute("let failed=false;try{x;}catch(e){failed=true;}let x=7;failed&&x===7&&window.x===3;", &mut Window).unwrap(),Value::Bool(true));
}
#[test]
fn global_target_cannot_authorize_a_different_host_identity() {
    struct Other;
    impl Host for Other {
        fn global(&mut self, _: &str) -> Option<Value> {
            None
        }
        fn global_object(&self) -> Option<u64> {
            Some(3)
        }
        fn get(&mut self, _: u64, _: &str) -> Result<Value, JsError> {
            panic!("no host read allowed")
        }
        fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
            panic!("no host write allowed")
        }
        fn call(&mut self, _: u64, _: &str, _: &[Value]) -> Result<Value, JsError> {
            panic!("no host call allowed")
        }
    }
    let mut runtime = Runtime::default();
    runtime
        .execute("Object.defineProperty(window,'x',{value:1});", &mut Window)
        .unwrap();
    assert!(
        runtime
            .execute("1;", &mut Other)
            .unwrap_err()
            .message
            .contains("identity changed")
    );
}

#[test]
fn deleted_intrinsics_do_not_return_during_var_redeclaration() {
    for name in ["Map", "Math", "JSON", "Reflect", "WebAssembly"] {
        let mut runtime = Runtime::default();
        runtime
            .execute(&format!("delete window.{name};"), &mut Window)
            .unwrap();
        assert_eq!(runtime.execute(&format!("var {name};{name}===undefined&&window.{name}===undefined&&Object.hasOwn(window,'{name}')&&('{name}' in window);"), &mut Window).unwrap(),Value::Bool(true),"{name}");
    }
}
#[test]
fn ordinary_builtin_assignments_preserve_existing_non_enumerability() {
    for prefix in ["", "window."] {
        check(&format!(
            "{prefix}Map=1;const d=Object.getOwnPropertyDescriptor(window,'Map');d.value===1&&!d.enumerable&&d.writable&&d.configurable;"
        ));
    }
}
#[test]
fn script_functions_replace_configurable_globals_with_nonconfigurable_properties() {
    let mut runtime = Runtime::default();
    runtime.execute("window.f=3;", &mut Window).unwrap();
    assert_eq!(runtime.execute("function f(){}const d=Object.getOwnPropertyDescriptor(window,'f');!d.configurable&&d.enumerable&&d.writable&&!(delete window.f);", &mut Window).unwrap(),Value::Bool(true));
}
#[test]
fn eval_created_globals_are_deletable_but_existing_script_globals_are_not() {
    check(
        "(0,eval)('var fromEval=2;function evalFn(){return 3;}');var scriptVar=1;(0,eval)('scriptVar=4;');const a=Object.getOwnPropertyDescriptor(window,'fromEval');const b=Object.getOwnPropertyDescriptor(window,'evalFn');const c=Object.getOwnPropertyDescriptor(window,'scriptVar');a.configurable&&b.configurable&&!c.configurable&&delete window.fromEval&&delete evalFn&&scriptVar===4;",
    );
}
#[test]
fn module_first_execution_registers_global_identity_before_symbol_writes() {
    let mut runtime = Runtime::default();
    runtime.register_module("https://example.test/a.mjs", "window.key=Symbol('key');window[window.key]=5;Object.defineProperty(window,'x',{get:function(){return this===window?7:0;}});export const result=x;").unwrap();
    runtime
        .execute_module("https://example.test/a.mjs", &mut Window)
        .unwrap();
    assert_eq!(
        runtime
            .execute("window[key]===5&&x===7;", &mut Window)
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn eval_function_preserves_nonconfigurable_existing_script_var() {
    check(
        "var existing=1;(0,eval)('function existing(){return 8;}');const d=Object.getOwnPropertyDescriptor(window,'existing');existing()===8&&!d.configurable&&!(delete existing);",
    );
}
