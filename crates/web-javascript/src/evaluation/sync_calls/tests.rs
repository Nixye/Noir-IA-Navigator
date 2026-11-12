use crate::{ErrorKind, Expr, Host, JsError, Runtime, Value};

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

#[test]
fn captured_callee_and_receiver_survive_arguments_and_spread() {
    for source in [
        "let order='';const object={n:40};Object.defineProperty(object,'f',{get:function(){order+='g';return function(n){order+='f';return this.n+n;};}});function base(){order+='b';return object;}function key(){order+='k';return 'f';}function arg(){order+='a';return 2;}base()[key()](...[arg()])===42&&order==='bkgaf';",
        "let old={n:40,f:function(n){return this.n+n;}};let current=old;function arg(){current={n:1};old.f=function(){return 0;};return 2;}current.f(arg())===42;",
        "let effects=0;let caught;try{({f:1}).f(...[effects++],effects++);}catch(e){caught=e.name;}caught==='TypeError'&&effects===2;",
        "let effects=0;const object={};Object.defineProperty(object,'f',{get:function(){throw 7;}});let caught;try{object.f(effects++);}catch(e){caught=e;}caught===7&&effects===0;",
        "let effects=0;let caught;try{new (1)(effects++);}catch(e){caught=e.name;}caught==='TypeError'&&effects===1;",
        "function A(n){this.n=n;}let target=A;function arg(){target=function(){this.n=0;};return 42;}new target(arg()).n===42;",
        "const object={n:42,f:function(){'use strict';return this;}};(0,object.f)()===undefined&&object.f()===object;",
    ] {
        assert_eq!(
            Runtime::default().execute(source, &mut Empty).unwrap(),
            Value::Bool(true),
            "{source}"
        );
    }
}

#[test]
fn direct_eval_requires_both_name_shape_and_intrinsic_identity() {
    for source in [
        "var x=1;function f(){let x=40;return eval('x+2');}f()===42;",
        "var x=1;function f(){let x=40;return (0,eval)('x+2');}f()===3;",
        "function f(){let eval=function(n){return n+40;};return eval(2);}f()===42;",
        "function f(){let x=40;const object={eval:eval};return object.eval('typeof x');}f()==='undefined';",
        "var original=eval;function f(){let x=40;let eval=original;function arg(){eval=function(){return 0;};return 'x+2';}return eval(arg());}f()===42;",
    ] {
        assert_eq!(
            Runtime::default().execute(source, &mut Empty).unwrap(),
            Value::Bool(true),
            "{source}"
        );
    }
}

#[test]
fn private_super_and_constructor_calls_keep_their_receivers() {
    for source in [
        "class A{#f(n){return this.n+n;}constructor(){this.n=40;}run(){return this.#f(...[2]);}}new A().run()===42;",
        "let order='';class A{constructor(n){order+='a';this.n=n;}f(n){order+='f';return this.n+n;}}class B extends A{constructor(){super((order+='s',40));order+='b';}run(){return super.f((order+='r',2));}}new B().run()===42&&order==='sabrf';",
        "let order='';class A{get f(){order+='g';return function(n){return this.n+n;};}}class B extends A{constructor(){super();this.n=40;}run(){return super.f((order+='a',2));}}new B().run()===42&&order==='ga';",
        "let effects=0;class A{#f=1;run(){try{this.#f(effects++);}catch(e){return e.name==='TypeError';}}}new A().run()&&effects===1;",
    ] {
        assert_eq!(
            Runtime::default().execute(source, &mut Empty).unwrap(),
            Value::Bool(true),
            "{source}"
        );
    }
}

#[test]
fn legacy_host_named_dispatch_does_not_read_a_method_getter() {
    struct Named {
        gets: usize,
        calls: usize,
    }
    impl Host for Named {
        fn global(&mut self, name: &str) -> Option<Value> {
            (name == "bridge").then_some(Value::Host(7))
        }
        fn get(&mut self, _: u64, _: &str) -> Result<Value, JsError> {
            self.gets += 1;
            Err(JsError::runtime("No getter"))
        }
        fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
            panic!("Unexpected write")
        }
        fn call(&mut self, id: u64, method: &str, args: &[Value]) -> Result<Value, JsError> {
            assert_eq!(id, 7);
            assert_eq!(method, "send");
            self.calls += 1;
            Ok(args.first().cloned().unwrap_or(Value::Undefined))
        }
    }
    let mut host = Named { gets: 0, calls: 0 };
    let mut vm = Runtime::default();
    assert_eq!(
        vm.execute("bridge.send(...[42]);", &mut host).unwrap(),
        Value::Number(42.)
    );
    assert_eq!((host.gets, host.calls), (0, 1));
}

#[test]
fn call_phases_preserve_semantic_ticks() {
    let mut seed = Runtime::default();
    seed.execute("var order='';const object={n:40};Object.defineProperty(object,'f',{get:function(){order+='g';return function(n){return this.n+n;};}});function arg(){order+='a';return 2;}", &mut Empty).unwrap();
    let base = Expr::Name("object".into());
    let key = Expr::Literal(Value::string("f"));
    let arguments = vec![Expr::Call(Box::new(Expr::Name("arg".into())), Vec::new())];
    let expression = Expr::Call(
        Box::new(Expr::Member(Box::new(base.clone()), Box::new(key.clone()))),
        arguments.clone(),
    );
    let mut iterative = seed.clone();
    let actual = iterative.eval(&expression, 0, &mut Empty).unwrap();
    let mut previous = seed;
    previous.tick().unwrap();
    let object = previous.eval(&base, 0, &mut Empty).unwrap();
    let key = previous.eval(&key, 0, &mut Empty).unwrap();
    let target = previous
        .prepare_member_call_values(object, key, &mut Empty)
        .unwrap();
    let arguments = previous.argument_values(&arguments, 0, &mut Empty).unwrap();
    let expected = previous
        .invoke_prepared_call(target, arguments, &mut Empty)
        .unwrap();
    assert_eq!(actual, expected);
    assert_eq!(iterative.remaining, previous.remaining);
    assert_eq!(
        iterative.global_value("order"),
        previous.global_value("order")
    );
}

#[test]
fn dynamic_import_operands_fail_synchronously_before_conversion() {
    let mut vm = Runtime::default();
    vm.register_module("https://example.test/dep.js", "export const x=42;")
        .unwrap();
    vm.register_module("https://example.test/root.js", "export let order='';const spec={toString(){order+='s';return './dep.js';}};function bad(){order+='o';throw 9;}export let caught=0;try{import(spec,bad());}catch(e){caught=e;}export const pending=import(spec,(order+='p',{}));").unwrap();
    let namespace = vm
        .execute_module("https://example.test/root.js", &mut Empty)
        .unwrap();
    assert_eq!(
        vm.get(namespace.clone(), "caught", &mut Empty).unwrap(),
        Value::Number(9.)
    );
    assert_eq!(
        vm.get(namespace, "order", &mut Empty).unwrap(),
        Value::string("ops")
    );
}

#[test]
fn fatal_argument_stops_invocation_and_page_cleanup() {
    let mut vm = Runtime::default();
    vm.execute("var invoked=0;var cleaned=0;function target(){invoked++;}function exhaust(){return exhaust();}", &mut Empty).unwrap();
    let error = vm
        .execute("try{target(exhaust());}finally{cleaned++;}", &mut Empty)
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert!(error.thrown.is_none());
    assert_eq!(vm.global_value("invoked"), Some(Value::Number(0.)));
    assert_eq!(vm.global_value("cleaned"), Some(Value::Number(0.)));
    assert_eq!(
        (vm.eval_depth, vm.call_depth, vm.statement_depth),
        (0, 0, 0)
    );
    assert_eq!(vm.execute("2+3", &mut Empty).unwrap(), Value::Number(5.));
}
