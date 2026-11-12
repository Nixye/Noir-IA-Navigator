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

fn member(name: &str, key: Expr) -> Expr {
    Expr::Member(Box::new(Expr::Name(name.into())), Box::new(key))
}
fn call(name: &str) -> Expr {
    Expr::Call(Box::new(Expr::Name(name.into())), Vec::new())
}

#[test]
fn references_keep_getvalue_coercion_and_write_in_order() {
    for source in [
        "let order='';let stored=0;const object={};Object.defineProperty(object,'x',{get:function(){order+='g';return {valueOf(){order+='n';return 40;}};},set:function(v){order+='s';stored=v;}});const key={toString(){order+='k';return 'x';}};function rhs(){order+='r';return 2;}object[key]+=rhs();stored===42&&order==='kgrns';",
        "let order='';const key={toString(){order+='c';return 'x';}};function k(){order+='k';return key;}let caught;try{null[k()]=(order+='r');}catch(e){caught=e.name;}caught==='TypeError'&&order==='k';",
        "let order='';let old={x:40};let current=old;let key='x';function rhs(){order+='r';current={x:100};key='y';return 2;}current[key]+=rhs();old.x===42&&current.x===100&&old.y===undefined&&order==='r';",
        "let order='';const object={};Object.defineProperty(object,'x',{get:function(){order+='g';return 40;},set:function(v){order+='s';throw 9;}});let caught;try{object.x++;}catch(e){caught=e;}caught===9&&order==='gs';",
        "const key=Symbol('x');const object={};object[key]=40n;let previous=object[key]++;previous===40n&&object[key]===41n&&++object[key]===42n;",
        "let reads=0;const object={};Object.defineProperty(object,'x',{get:function(){reads++;return 1;},configurable:true});delete object.x&&reads===0&&object.x===undefined;",
        "let object={x:1};delete(0,object.x)&&object.x===1&&typeof missing==='undefined';",
        "let caught=false;try{typeof later;let later=1;}catch(e){caught=true;}caught;",
        "let caught=false;try{typeof(0,missing);}catch(e){caught=true;}caught;",
    ] {
        assert_eq!(
            Runtime::default().execute(source, &mut Empty).unwrap(),
            Value::Bool(true),
            "{source}"
        );
    }
}

#[test]
fn private_and_super_references_preserve_the_existing_subset() {
    for source in [
        "class A{#x=40;inc(){let old=this.#x++;return old+ ++this.#x;}}new A().inc()===82;",
        "let order='';class A{get x(){order+='g';return 40;}set x(v){order+='s';this.stored=v;}}class B extends A{run(){return super.x+=(order+='r',2);}}const b=new B();b.run()===42&&b.stored===42&&order==='grs';",
        "let order='';class A{get x(){order+='g';return 1;}set x(v){order+='s';this.stored=v;}}class B extends A{run(){return super.x=(order+='r',42);}}const b=new B();b.run()===42&&b.stored===42&&order==='rs';",
        "const key=Symbol('x');class A{}class B extends A{read(){return super[key];}}let caught;try{new B().read();}catch(e){caught=e.name;}caught==='TypeError';",
    ] {
        assert_eq!(
            Runtime::default().execute(source, &mut Empty).unwrap(),
            Value::Bool(true),
            "{source}"
        );
    }
}

#[test]
fn internal_logical_assignment_short_circuits_without_repeating_reference() {
    // Source syntax remains intentionally unsupported. Parser-bypassing ASTs
    // exercise the reference goal without advertising a new grammar feature.
    for (op, previous, expected, writes) in [
        ("&&=", "0", 0., 0.),
        ("&&=", "1", 42., 1.),
        ("||=", "1", 1., 0.),
        ("||=", "0", 42., 1.),
        ("??=", "0", 0., 0.),
        ("??=", "null", 42., 1.),
        ("??=", "undefined", 42., 1.),
    ] {
        let mut vm = Runtime::default();
        vm.execute(&format!("var reads=0;var writes=0;var rights=0;var keys=0;const object={{}};Object.defineProperty(object,'x',{{get:function(){{reads++;return {previous};}},set:function(v){{writes++;}}}});function key(){{keys++;return 'x';}}function rhs(){{rights++;return 42;}}"), &mut Empty).unwrap();
        let expression = Expr::Assign(
            op.into(),
            Box::new(member("object", call("key"))),
            Box::new(call("rhs")),
        );
        assert_eq!(
            vm.eval(&expression, 0, &mut Empty).unwrap(),
            Value::Number(expected)
        );
        assert_eq!(vm.global_value("reads"), Some(Value::Number(1.)));
        assert_eq!(vm.global_value("keys"), Some(Value::Number(1.)));
        assert_eq!(vm.global_value("writes"), Some(Value::Number(writes)));
        assert_eq!(vm.global_value("rights"), Some(Value::Number(writes)));
    }
}

#[test]
fn member_and_compound_phases_preserve_semantic_ticks() {
    let setup = "var order='';var stored=0;const object={};Object.defineProperty(object,'x',{get:function(){order+='g';return {valueOf(){order+='n';return 40;}};},set:function(v){order+='s';stored=v;}});function key(){order+='k';return 'x';}function rhs(){order+='r';return 2;}";
    let mut seed = Runtime::default();
    seed.execute(setup, &mut Empty).unwrap();
    let target = member("object", call("key"));
    let expression = Expr::Assign("+=".into(), Box::new(target.clone()), Box::new(call("rhs")));
    let mut iterative = seed.clone();
    let actual = iterative.eval(&expression, 0, &mut Empty).unwrap();
    let mut previous = seed;
    // The former assignment entry: tick once, capture reference, GetValue,
    // evaluate RHS, coerce, write. Child eval uses its ordinary shared guards.
    previous.tick().unwrap();
    let place = previous.place(&target, 0, &mut Empty).unwrap();
    let left = previous.read_place(&place, &mut Empty).unwrap();
    let right = previous.eval(&call("rhs"), 0, &mut Empty).unwrap();
    let expected = previous.binary("+", left, right, &mut Empty).unwrap();
    previous
        .write_place(place, expected.clone(), &mut Empty)
        .unwrap();
    assert_eq!(actual, expected);
    assert_eq!(iterative.remaining, previous.remaining);
    assert_eq!(
        iterative.global_value("order"),
        previous.global_value("order")
    );
    assert_eq!(
        iterative.global_value("stored"),
        previous.global_value("stored")
    );
}

#[test]
fn fatal_rhs_discards_the_prepared_write_and_page_cleanup() {
    let mut vm = Runtime::default();
    vm.execute("var writes=0;var caught=0;var cleaned=0;const object={};Object.defineProperty(object,'x',{get:function(){return 1;},set:function(v){writes++;}});function exhaust(){return exhaust();}", &mut Empty).unwrap();
    let error = vm
        .execute(
            "try{object.x+=exhaust();}catch(e){caught++;}finally{cleaned++;}",
            &mut Empty,
        )
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert!(error.thrown.is_none());
    for name in ["writes", "caught", "cleaned"] {
        assert_eq!(vm.global_value(name), Some(Value::Number(0.)), "{name}");
    }
    assert_eq!(
        (vm.eval_depth, vm.call_depth, vm.statement_depth),
        (0, 0, 0)
    );
    assert_eq!(vm.execute("2+3", &mut Empty).unwrap(), Value::Number(5.));
}
