//! Generic host construction contract; no DOM dependencies.
use crate::{ErrorKind, Host, JsError, Limits, Runtime, Value};

#[derive(Default)]
struct Constructors {
    constructions: Vec<Vec<Value>>,
    calls: usize,
    gets: Vec<String>,
    result: Option<Value>,
    recurse: bool,
}
impl Host for Constructors {
    fn global(&mut self, name: &str) -> Option<Value> {
        match name {
            "Build" => Some(Value::Host(1)),
            "Legacy" => Some(Value::Host(2)),
            "Other" => Some(Value::Host(3)),
            "OnlyNew" => Some(Value::Host(4)),
            "Bridge" => Some(Value::Host(5)),
            _ => None,
        }
    }
    fn get(&mut self, object: u64, property: &str) -> Result<Value, JsError> {
        self.gets.push(format!("{object}.{property}"));
        Ok(match property {
            "name" => Value::string("Build"),
            "length" => Value::Number(3.),
            _ => Value::Undefined,
        })
    }
    fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
        Err(JsError::runtime("Read-only fake host"))
    }
    fn is_callable(&self, object: u64) -> bool {
        matches!(object, 1..=3)
    }
    fn is_constructor(&self, object: u64) -> bool {
        matches!(object, 1 | 3 | 4)
    }
    fn call(&mut self, _: u64, _: &str, _: &[Value]) -> Result<Value, JsError> {
        panic!("Ordinary calls must use the runtime hook")
    }
    fn call_with_runtime(
        &mut self,
        _: u64,
        method: &str,
        args: &[Value],
        _: &Value,
        runtime: &mut Runtime,
    ) -> Result<Value, JsError> {
        if method == "dispatch" {
            for callback in args {
                runtime.invoke_with_this(callback.clone(), vec![], Value::Host(100), self)?;
            }
            return Ok(Value::Undefined);
        }
        if method == "reenter" {
            return runtime.method(Value::Host(1), "reenter", vec![], self);
        }
        assert_eq!(method, "call");
        self.calls += 1;
        Err(JsError::runtime("Use new"))
    }
    fn construct_with_runtime(
        &mut self,
        object: u64,
        args: &[Value],
        runtime: &mut Runtime,
    ) -> Result<Value, JsError> {
        assert!(self.is_constructor(object));
        self.constructions.push(args.to_vec());
        // The callback must not collect live constructor arguments.
        assert_eq!(runtime.collect_garbage(&[], self)?.reclaimed_objects, 0);
        if self.recurse {
            return runtime.construct(Value::Host(object), args.to_vec(), self);
        }
        Ok(self.result.clone().unwrap_or(Value::Host(100)))
    }
}

#[test]
fn host_new_is_opt_in_and_calls_are_separate() {
    let mut host = Constructors::default();
    let mut vm = Runtime::default();
    assert_eq!(
        vm.execute("new Build(7);", &mut host).unwrap(),
        Value::Host(100)
    );
    assert_eq!(host.constructions, vec![vec![Value::Number(7.)]]);
    assert!(vm.execute("new Legacy();", &mut host).is_err());
    assert!(vm.execute("Build();", &mut host).is_err());
    assert_eq!(host.calls, 1);
    assert_eq!(host.constructions.len(), 1);
    assert_eq!(
        vm.execute("typeof Build;", &mut host).unwrap(),
        Value::string("function")
    );
    assert_eq!(
        vm.execute("new OnlyNew();", &mut host).unwrap(),
        Value::Host(100)
    );
    assert!(vm.execute("OnlyNew();", &mut host).is_err());
    assert_eq!(host.calls, 1);
}

#[test]
fn reflect_checks_host_constructors_before_argument_getters() {
    let mut host = Constructors::default();
    let mut vm = Runtime::default();
    assert_eq!(
        vm.execute("Reflect.construct(Build,[1]);", &mut host)
            .unwrap(),
        Value::Host(100)
    );
    assert_eq!(
        vm.execute("Reflect.construct(Build,[2],Build);", &mut host)
            .unwrap(),
        Value::Host(100)
    );
    let result = vm
        .execute(
            "let reads=0;const args={};Object.defineProperty(args,'length',{get:function(){reads++;return 0;},enumerable:true,configurable:true});\
         try{Reflect.construct(Build,args,Other);}catch(e){}\
         try{Reflect.construct(Legacy,args);}catch(e){}\
         try{Reflect.construct(Build,args,Legacy);}catch(e){}reads;",
            &mut host,
        )
        .unwrap();
    assert_eq!(result, Value::Number(0.));
    assert_eq!(host.constructions.len(), 2);
}

#[test]
fn bound_host_construction_preserves_arguments_and_metadata() {
    let mut host = Constructors::default();
    let mut vm = Runtime::default();
    assert_eq!(
        vm.execute(
            "const B=Build.bind(null,10);const C=B.bind(null,20);\
         new C(30);Reflect.construct(C,[40],C);\
         B.name==='bound Build'&&B.length===2&&C.name==='bound bound Build'&&C.length===1;",
            &mut host,
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(host.gets, vec!["1.length", "1.name"]);
    assert_eq!(
        host.constructions,
        vec![
            vec![Value::Number(10.), Value::Number(20.), Value::Number(30.)],
            vec![Value::Number(10.), Value::Number(20.), Value::Number(40.)],
        ]
    );
    assert!(vm.execute("C();", &mut host).is_err());
    assert_eq!(host.calls, 1);
    host.gets.clear();
    assert_eq!(
        vm.execute(
            "const L=Legacy.bind(null);L.name==='bound '&&L.length===0;",
            &mut host
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert!(host.gets.is_empty());
    assert!(vm.execute("new L();", &mut host).is_err());
}

#[test]
fn host_construction_rejects_primitives_and_invalid_heap_references() {
    for result in [
        Value::Null,
        Value::Number(1.),
        Value::Object(usize::MAX),
        Value::Array(usize::MAX),
        Value::Function(usize::MAX),
    ] {
        let mut host = Constructors {
            result: Some(result),
            ..Constructors::default()
        };
        let mut vm = Runtime::default();
        assert_eq!(
            vm.construct(Value::Host(1), vec![], &mut host)
                .unwrap_err()
                .kind,
            ErrorKind::Runtime
        );
        assert_eq!(vm.call_depth, 0);
    }
    let mut vm = Runtime::default();
    let array = vm.execute("[];", &mut Constructors::default()).unwrap();
    let mut host = Constructors {
        result: Some(array.clone()),
        ..Constructors::default()
    };
    assert_eq!(
        vm.construct(Value::Host(1), vec![], &mut host).unwrap(),
        array
    );
    let mut host = Constructors {
        result: Some(Value::string("x".repeat(65_537))),
        ..Constructors::default()
    };
    assert_eq!(
        vm.construct(Value::Host(1), vec![], &mut host)
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
}

#[test]
fn host_construction_enforces_argument_depth_and_work_limits() {
    let mut host = Constructors::default();
    let mut vm = Runtime::default();
    assert!(
        vm.construct(Value::Host(1), vec![Value::Null; 64], &mut host)
            .is_ok()
    );
    assert_eq!(
        vm.construct(Value::Host(1), vec![Value::Null; 65], &mut host)
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
    assert_eq!(host.constructions.len(), 1);
    let bound = vm
        .bind_function(
            Value::Host(1),
            Value::Undefined,
            vec![Value::Null; 64],
            &mut host,
        )
        .unwrap();
    assert_eq!(
        vm.construct(bound, vec![Value::Null], &mut host)
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
    assert_eq!(host.constructions.len(), 1);
    let mut vm = Runtime::new(Limits {
        call_depth: 3,
        ..Limits::default()
    });
    let mut host = Constructors {
        recurse: true,
        ..Constructors::default()
    };
    assert_eq!(
        vm.construct(Value::Host(1), vec![], &mut host)
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
    assert_eq!(host.constructions.len(), 3);
    assert_eq!(vm.call_depth, 0);
    let mut vm = Runtime::new(Limits {
        instructions: 0,
        ..Limits::default()
    });
    let mut host = Constructors::default();
    assert_eq!(
        vm.construct(Value::Host(1), vec![], &mut host)
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
    assert!(host.constructions.is_empty());
}

#[test]
fn host_string_conversion_observes_user_hooks_and_rejects_symbols() {
    let mut vm = Runtime::default();
    let mut host = Constructors::default();
    let object = vm
        .execute("({toString(){return 'converted';}});", &mut host)
        .unwrap();
    assert_eq!(vm.coerce_to_string(object, &mut host).unwrap(), "converted");
    let symbol = vm.execute("Symbol('x');", &mut host).unwrap();
    assert!(vm.coerce_to_string(symbol, &mut host).is_err());
    assert!(
        vm.coerce_to_string(Value::Utf16(vec![0xd800].into()), &mut host)
            .is_err()
    );
}

#[test]
fn legacy_host_methods_keep_a_frame_until_all_callbacks_return() {
    let mut host = Constructors::default();
    let mut vm = Runtime::default();
    vm.execute(
        "const order=[];Bridge.dispatch(\
         function(){order.push(1);queueMicrotask(function(){order.push(3);});},\
         function(){order.push(2);});order.push(4);",
        &mut host,
    )
    .unwrap();
    assert_eq!(
        vm.execute("order.join(',');", &mut host).unwrap(),
        Value::string("1,2,4,3")
    );
    assert_eq!(vm.call_depth, 0);
    assert!(
        vm.execute("Bridge.dispatch(function(){throw 'failure';});", &mut host)
            .is_err()
    );
    assert_eq!(vm.call_depth, 0);
    let mut vm = Runtime::new(Limits {
        call_depth: 3,
        ..Limits::default()
    });
    assert_eq!(
        vm.method(Value::Host(1), "reenter", vec![], &mut host)
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
    assert_eq!(vm.call_depth, 0);
    assert_eq!(
        vm.method(Value::Host(1), "dispatch", vec![Value::Null; 65], &mut host)
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
}

#[test]
fn legacy_host_defaults_do_not_enable_construction() {
    struct Legacy;
    impl Host for Legacy {
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
        fn is_callable(&self, _: u64) -> bool {
            true
        }
    }
    let mut host = Legacy;
    let mut vm = Runtime::default();
    assert!(!host.is_constructor(1));
    assert!(host.construct_with_runtime(1, &[], &mut vm).is_err());
    assert!(vm.construct(Value::Host(1), vec![], &mut host).is_err());
}
