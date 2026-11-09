//! Shared reference and call target capture, before right-hand sides or arguments.
use crate::{BigInt, Expr, Host, JsError, Place, Runtime, Value, callables};
use std::sync::Arc;
#[derive(Clone, Debug)]
pub(super) enum PreparedCall {
    Callable {
        callable: Value,
        receiver: Value,
        context: Option<String>,
    },
    LegacyMethod {
        receiver: Value,
        property: String,
    },
}
impl PreparedCall {
    pub(super) fn trace_values(&self, values: &mut Vec<Value>) {
        match self {
            Self::Callable {
                callable, receiver, ..
            } => {
                values.push(callable.clone());
                values.push(receiver.clone());
            }
            Self::LegacyMethod { receiver, .. } => values.push(receiver.clone()),
        }
    }
}
impl Place {
    pub(super) fn trace_values(&self, values: &mut Vec<Value>, scopes: &mut Vec<usize>) {
        match self {
            Self::Binding(scope, _) => scopes.push(*scope),
            Self::Global(id, _) => values.push(Value::Host(*id)),
            Self::Member(object, _) | Self::SymbolMember(object, _) => values.push(object.clone()),
            Self::PrivateMember(object, class, _) => {
                values.push(object.clone());
                values.push(Value::Function(*class));
            }
            Self::SuperMember(base, receiver, _) => {
                values.push(base.clone());
                values.push(receiver.clone());
            }
        }
    }
}
impl Runtime {
    pub(super) fn ordinary_member_place_values(
        &mut self,
        base: Value,
        raw_key: Value,
        host: &mut impl Host,
    ) -> Result<Place, JsError> {
        let key = self.ordinary_member_key(&base, raw_key, host)?;
        if let Value::Symbol(key, _) = key {
            Ok(Place::SymbolMember(base, key))
        } else {
            Ok(Place::Member(base, self.text(&key, 0)?))
        }
    }
    pub(super) fn update_place(
        &mut self,
        place: Place,
        increment: bool,
        prefix: bool,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let previous = self.read_place(&place, host)?;
        let previous = self.operator_numeric(previous, host)?;
        if matches!(previous, Value::BigInt(_)) {
            let one = Value::BigInt(Arc::new(BigInt::parse("1")?));
            let next = self.bigint_binary(if increment { "+" } else { "-" }, &previous, &one)?;
            self.write_place(place, next.clone(), host)?;
            return Ok(if prefix { next } else { previous });
        }
        let previous = self.number(&previous);
        let next = Value::Number(previous + if increment { 1. } else { -1. });
        self.write_place(place, next.clone(), host)?;
        Ok(if prefix {
            next
        } else {
            Value::Number(previous)
        })
    }
    // Callers either hold an active eval frame or retain base/key and their
    // continuation while coercion and getters can re-enter the VM.
    pub(super) fn ordinary_member_key(
        &mut self,
        base: &Value,
        raw_key: Value,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        // The key expression has already run. Reject nullish before coercion.
        if matches!(base, Value::Null | Value::Undefined) {
            return Err(self.property_type_error("Cannot access a property of null or undefined"));
        }
        self.coerce_property_key(raw_key, host)
    }
    pub(super) fn read_member_values(
        &mut self,
        base: Value,
        raw_key: Value,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let key = self.ordinary_member_key(&base, raw_key, host)?;
        if let Value::Symbol(key, _) = key {
            self.get_symbol(base, key, host)
        } else {
            let property = self.text(&key, 0)?;
            self.get(base, &property, host)
        }
    }
    pub(super) fn prepare_member_call_values(
        &mut self,
        object: Value,
        raw_key: Value,
        host: &mut impl Host,
    ) -> Result<PreparedCall, JsError> {
        let key = self.ordinary_member_key(&object, raw_key, host)?;
        if let Value::Symbol(key, _) = key {
            let callable = self.get_symbol(object.clone(), key, host)?;
            return Ok(PreparedCall::Callable {
                callable,
                receiver: object,
                context: None,
            });
        }
        let property = self.text(&key, 0)?;
        if matches!(
            object,
            Value::Object(_)
                | Value::Array(_)
                | Value::String(_)
                | Value::Utf16(_)
                | Value::Symbol(..)
                | Value::Number(_)
                | Value::BigInt(_)
                | Value::Bool(_)
        ) || matches!(object, Value::Host(id) if Some(id) == host.global_object())
            || self.is_callable_value(&object, host)
        {
            let callable = self.get(object.clone(), &property, host)?;
            Ok(PreparedCall::Callable {
                callable,
                receiver: object,
                context: Some(property),
            })
        } else {
            // Non-callable Host objects expose named dispatch, not getters.
            Ok(PreparedCall::LegacyMethod {
                receiver: object,
                property,
            })
        }
    }
    pub(super) fn prepare_call(
        &mut self,
        callee: &Expr,
        scope: usize,
        host: &mut impl Host,
    ) -> Result<PreparedCall, JsError> {
        if let Expr::Member(object, property) = callee {
            let object = self.eval(object, scope, host)?;
            let key = self.eval(property, scope, host)?;
            self.prepare_member_call_values(object, key, host)
        } else {
            let callable = self.eval(callee, scope, host)?;
            let context = if let Expr::Name(name) = callee {
                Some(name.clone())
            } else {
                None
            };
            Ok(PreparedCall::Callable {
                callable,
                receiver: Value::Undefined,
                context,
            })
        }
    }
    pub(super) fn invoke_prepared_call(
        &mut self,
        target: PreparedCall,
        args: Vec<Value>,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        match target {
            PreparedCall::Callable {
                callable,
                receiver,
                context,
            } => self
                .call(callable, args, receiver, host)
                .map_err(|error| match context {
                    Some(name) => callables::error_context(error, &name),
                    None => error,
                }),
            PreparedCall::LegacyMethod { receiver, property } => {
                self.method(receiver, &property, args, host)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordinary_members_preserve_key_order_abrupt_completion_and_symbols() {
        for source in [
            "let order='';const key={toString(){order+='c';throw 9;}};function property(){order+='p';return key;}let error;try{null[property()];}catch(e){error=e.name;}error==='TypeError'&&order==='p';",
            "let order='';function property(){order+='p';throw 9;}let error;try{undefined[property()];}catch(e){error=e;}error===9&&order==='p';",
            "let order='';function property(){order+='p';throw 9;}let error;try{null[property()](order+='a');}catch(e){error=e;}error===9&&order==='p';",
            "let order='';const obj={};Object.defineProperty(obj,'x',{get:function(){order+='g';return 42;}});function base(){order+='b';return obj;}function key(){order+='k';return {toString(){order+='c';return 'x';}};}base()[key()]===42&&order==='bkcg';",
            "let order='';const obj={n:40};Object.defineProperty(obj,'x',{get:function(){order+='g';return function(a){return this.n+a;};}});function base(){order+='b';return obj;}function key(){order+='k';return {toString(){order+='c';return 'x';}};}function arg(){order+='a';return 2;}base()[key()](arg())===42&&order==='bkcga';",
            "let order='';const obj={};Object.defineProperty(obj,'x',{get:function(){order+='g';throw 7;}});let error;try{obj.x(order+='a');}catch(e){error=e;}error===7&&order==='g';",
            "let order='';const key={toString(){order+='c';throw 8;}};let error;try{({})[key](order+='a');}catch(e){error=e;}error===8&&order==='c';",
            "let effects=0;try{({method:3}).method(effects++);}catch(e){}effects===1;",
            "const key=Symbol('x');const obj={n:40};obj[key]=function(a){return this.n+a;};obj[key](2)===42&&obj[key]===obj[key];",
        ] {
            assert_eq!(
                Runtime::default().execute(source, &mut Empty).unwrap(),
                Value::Bool(true),
                "{source}"
            );
        }
    }
    #[test]
    fn nullish_receiver_rejects_before_key_coercion_and_arguments() {
        for source in [
            "let effects=0;try{null.method(effects++);}catch(e){}effects===0;",
            "let effects=0;function* g(){try{undefined.method(yield 1);}catch(e){return e.name==='TypeError';}}const result=g().next();result.done&&result.value&&effects===0;",
            "let order='';const key={toString(){order+='c';return 'method';}};function property(){order+='p';return key;}try{null[property()](order+='a');}catch(e){}order==='p';",
        ] {
            assert_eq!(
                Runtime::default().execute(source, &mut Empty).unwrap(),
                Value::Bool(true),
                "{source}"
            );
        }
    }
    #[test]
    fn ordinary_assignment_captures_reference_without_reading_simple_target() {
        for source in [
            "let order='';const key={toString(){order+='c';throw 9;}};function property(){order+='p';return key;}let error;try{null[property()]=(order+='r');}catch(e){error=e.name;}error==='TypeError'&&order==='p';",
            "let order='';function property(){order+='p';throw 9;}let error;try{undefined[property()]=(order+='r');}catch(e){error=e;}error===9&&order==='p';",
            "let order='';const key={toString(){order+='c';throw 8;}};let error;try{({})[key]=(order+='r');}catch(e){error=e;}error===8&&order==='c';",
            "let order='';let stored=0;const obj={};Object.defineProperty(obj,'x',{get:function(){order+='g';throw 9;},set:function(v){order+='s';stored=v;}});function base(){order+='b';return obj;}function key(){order+='k';return {toString(){order+='c';return 'x';}};}function rhs(){order+='r';return 42;}let result=base()[key()]=rhs();result===42&&stored===42&&order==='bkcrs';",
            "let object={x:1};let original=object;let key='x';function rhs(){object={x:99};key='y';return 42;}object[key]=rhs();original.x===42&&object.x===99&&original.y===undefined;",
            "const key=Symbol('x');const obj={};let conversions=0;const raw={toString(){conversions++;return key;}};obj[raw]=42;obj[key]===42&&conversions===1;",
        ] {
            assert_eq!(
                Runtime::default().execute(source, &mut Empty).unwrap(),
                Value::Bool(true),
                "{source}"
            );
        }
    }
    #[test]
    fn ordinary_compound_assignment_reads_before_rhs_and_converts_after_rhs() {
        for source in [
            "let order='';let stored=0;let old={valueOf(){order+='n';return 40;}};const obj={};Object.defineProperty(obj,'x',{get:function(){order+='g';return old;},set:function(v){order+='s';stored=v;}});const key={toString(){order+='k';return 'x';}};function rhs(){order+='r';return 2;}let result=obj[key]+=rhs();result===42&&stored===42&&order==='kgrns';",
            "let order='';const obj={};Object.defineProperty(obj,'x',{get:function(){order+='g';throw 7;}});let error;try{obj.x+=(order+='r');}catch(e){error=e;}error===7&&order==='g';",
            "let order='';const obj={x:{valueOf(){order+='n';throw 8;}}};function rhs(){order+='r';return 2;}let error;try{obj.x+=rhs();}catch(e){error=e;}error===8&&order==='rn';",
            "let object={x:40};let original=object;let key='x';function rhs(){object={x:99};key='y';original.x=100;return 2;}let result=object[key]+=rhs();result===42&&original.x===42&&object.x===99&&original.y===undefined;",
            "const key=Symbol('x');const obj={};obj[key]=40;let result=obj[key]+=2;result===42&&obj[key]===42;",
        ] {
            assert_eq!(
                Runtime::default().execute(source, &mut Empty).unwrap(),
                Value::Bool(true),
                "{source}"
            );
        }
    }
    #[test]
    fn ordinary_updates_preserve_numeric_conversion_return_values_and_errors() {
        for source in [
            "let x='40';let old=x++;old===40&&x===41&&++x===42&&x===42&&x--===42&&--x===40;",
            "let x=40n;let old=x++;old===40n&&x===41n&&++x===42n&&x--===42n&&--x===40n;",
            "let x=-0;let old=x++;Object.is(old,-0)&&x===1;",
            "let x=NaN;let old=x++;Number.isNaN(old)&&Number.isNaN(x)&&Number.isNaN(--x);",
            "let count=0;let x={valueOf(){count++;return '40';}};let old=x++;old===40&&x===41&&count===1;",
            "let count=0;let x={valueOf(){count++;return 40n;}};let old=x++;old===40n&&x===41n&&count===1;",
            "let order='';let stored=0;const obj={};Object.defineProperty(obj,'x',{get:function(){order+='g';return {valueOf(){order+='n';return 40;}};},set:function(v){order+='s';stored=v;}});const key={toString(){order+='k';return 'x';}};let old=obj[key]++;old===40&&stored===41&&order==='kgns';",
            "let order='';const obj={};Object.defineProperty(obj,'x',{get:function(){order+='g';return {valueOf(){order+='n';return 40;}};},set:function(v){order+='s';throw 8;}});let error;try{obj.x++;}catch(e){error=e;}error===8&&order==='gns';",
            "let order='';const obj={};Object.defineProperty(obj,'x',{get:function(){order+='g';return {valueOf(){order+='n';throw 7;}};},set:function(v){order+='s';}});let error;try{++obj.x;}catch(e){error=e;}error===7&&order==='gn';",
            "const key=Symbol('x');const obj={};obj[key]=40n;let old=obj[key]++;old===40n&&obj[key]===41n;",
        ] {
            assert_eq!(
                Runtime::default().execute(source, &mut Empty).unwrap(),
                Value::Bool(true),
                "{source}"
            );
        }
    }
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
    #[test]
    fn suspended_call_captures_getter_before_arguments_and_method_replacement() {
        let mut vm = Runtime::default();
        vm.execute("let order='';let obj={n:40};let method=function(x){return this.n+x;};Object.defineProperty(obj,'method',{get:function(){order+='g';return method;}});function arg(){order+='a';return 2;}let result=0;async function run(){return obj.method(await arg());}run().then(v=>{result=v;});method=function(){return 99;};", &mut Empty).unwrap();
        assert_eq!(
            vm.execute("result;", &mut Empty).unwrap(),
            Value::Number(42.)
        );
        assert_eq!(
            vm.execute("order;", &mut Empty).unwrap(),
            Value::String("ga".into())
        );
    }
    #[test]
    fn symbol_receiver_and_callable_adapters_share_synchronous_dispatch() {
        let mut vm = Runtime::default();
        vm.execute("let key=Symbol('m');let obj={n:40};obj[key]=function(x){return this.n+x;};function* run(){return obj[key](yield 1);}let gen=run();gen.next();", &mut Empty).unwrap();
        assert_eq!(
            vm.execute("gen.next(2).value;", &mut Empty).unwrap(),
            Value::Number(42.)
        );
        vm.execute("function add(x){return this.n+x;}function* callRun(){return add.call({n:40},yield 1);}let a=callRun();a.next();function* applyRun(){return add.apply({n:40},yield 1);}let b=applyRun();b.next();function* bindRun(){return add.bind({n:40},yield 1);}let c=bindRun();c.next();", &mut Empty).unwrap();
        assert_eq!(
            vm.execute("a.next(2).value;", &mut Empty).unwrap(),
            Value::Number(42.)
        );
        assert_eq!(
            vm.execute("b.next([2]).value;", &mut Empty).unwrap(),
            Value::Number(42.)
        );
        assert_eq!(
            vm.execute("c.next(2).value();", &mut Empty).unwrap(),
            Value::Number(42.)
        );
    }
    #[test]
    fn abrupt_argument_discards_call_and_unwinds_finally() {
        let mut vm = Runtime::default();
        vm.execute("let called=0;let after=0;function target(){called++;}function* run(){try{target(yield 1,after++);}finally{yield 2;}}let gen=run();gen.next();", &mut Empty).unwrap();
        assert_eq!(
            vm.execute("gen.return(7).value;", &mut Empty).unwrap(),
            Value::Number(2.)
        );
        assert_eq!(
            vm.execute("gen.next().value;", &mut Empty).unwrap(),
            Value::Number(7.)
        );
        assert_eq!(
            vm.execute("called+after;", &mut Empty).unwrap(),
            Value::Number(0.)
        );
        vm.execute("let other=run();other.next();other.throw(9);", &mut Empty)
            .unwrap();
        assert!(vm.execute("other.next();", &mut Empty).is_err());
        assert_eq!(
            vm.execute("called+after;", &mut Empty).unwrap(),
            Value::Number(0.)
        );
    }
    #[test]
    fn prepared_target_receiver_and_prior_arguments_survive_gc() {
        let mut vm = Runtime::default();
        vm.execute("function make(){return {n:20,method:function(a,b){return this.n+a.n+b;}};}function* run(){return make().method({n:20},yield 1);}let gen=run();gen.next();", &mut Empty).unwrap();
        vm.collect_garbage(&[], &Empty).unwrap();
        assert_eq!(
            vm.execute("gen.next(2).value;", &mut Empty).unwrap(),
            Value::Number(42.)
        );
    }
    #[test]
    fn non_callable_is_rejected_only_after_argument_effects() {
        let mut vm = Runtime::default();
        vm.execute("let effects=0;let obj={method:3};function* run(){try{obj.method(yield 1,effects++);}catch(e){return effects;}}let gen=run();gen.next();", &mut Empty).unwrap();
        assert_eq!(
            vm.execute("gen.next(2).value;", &mut Empty).unwrap(),
            Value::Number(1.)
        );
    }
    #[test]
    fn non_callable_values_throw_type_error_after_argument_effects() {
        for source in [
            "let effects=0;let error;try{(3)(effects++);}catch(e){error=e.name;}error==='TypeError'&&effects===1;",
            "let effects=0;let error;const target=null;try{target(effects++);}catch(e){error=e.name;}error==='TypeError'&&effects===1;",
            "let effects=0;let error;try{({method:3}).method(effects++);}catch(e){error=e.name;}error==='TypeError'&&effects===1;",
            "let effects=0;function* run(){try{return (yield 1)(effects++);}catch(e){return e.name==='TypeError'&&effects===1;}}let gen=run();gen.next();let result=gen.next(3);result.done&&result.value;",
        ] {
            assert_eq!(
                Runtime::default().execute(source, &mut Empty).unwrap(),
                Value::Bool(true),
                "{source}"
            );
        }
    }
    #[test]
    fn argument_33_is_never_evaluated_and_limit_errors_are_fatal() {
        let mut vm = Runtime::default();
        let args = std::iter::repeat_n("effects++", 32)
            .collect::<Vec<_>>()
            .join(",");
        let source = format!(
            "let effects=0;let cleaned=0;function target(){{}}function* run(){{try{{target({args},yield 1);}}finally{{cleaned++;}}}}let gen=run();"
        );
        assert_eq!(
            vm.execute(&source, &mut Empty).unwrap_err().kind,
            crate::ErrorKind::Resource
        );
        let args = std::iter::repeat_n("effects++", 31)
            .collect::<Vec<_>>()
            .join(",");
        vm.execute(&format!(
            "let effects=0;let cleaned=0;function target(){{}}function* run(){{try{{target({args},yield 1);}}finally{{cleaned++;}}}}"
        ), &mut Empty).unwrap();
        // A parser-bypassing AST proves the independent runtime argument guard.
        let Value::Function(id) = vm.global_value("run").unwrap() else {
            panic!("Expected generator function");
        };
        let crate::Heap::Function(function) = &mut vm.heap[id] else {
            panic!("Expected generator heap entry");
        };
        let crate::Stmt::Try { body, .. } = &mut Arc::make_mut(&mut function.body)[0] else {
            panic!("Expected try/finally");
        };
        let crate::Stmt::Expression(Expr::Call(_, arguments)) = &mut body[0] else {
            panic!("Expected call");
        };
        arguments.insert(0, arguments[0].clone());
        vm.execute("let gen=run();", &mut Empty).unwrap();
        assert_eq!(
            vm.execute("gen.next();", &mut Empty).unwrap_err().kind,
            crate::ErrorKind::Resource
        );
        assert_eq!(
            vm.execute("effects;", &mut Empty).unwrap(),
            Value::Number(32.)
        );
        assert_eq!(
            vm.execute("cleaned;", &mut Empty).unwrap(),
            Value::Number(0.)
        );
    }
    #[test]
    fn legacy_host_named_dispatch_does_not_turn_into_a_getter() {
        struct NamedHost {
            gets: usize,
            calls: usize,
        }
        impl Host for NamedHost {
            fn global(&mut self, name: &str) -> Option<Value> {
                if name == "bridge" {
                    Some(Value::Host(77))
                } else {
                    None
                }
            }
            fn get(&mut self, _: u64, _: &str) -> Result<Value, JsError> {
                self.gets += 1;
                Err(JsError::runtime("Unexpected host getter"))
            }
            fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
                Ok(())
            }
            fn call(
                &mut self,
                object: u64,
                method: &str,
                args: &[Value],
            ) -> Result<Value, JsError> {
                assert_eq!(object, 77);
                assert_eq!(method, "send");
                self.calls += 1;
                Ok(args[0].clone())
            }
            fn call_with_this(
                &mut self,
                object: u64,
                method: &str,
                args: &[Value],
                this: &Value,
            ) -> Result<Value, JsError> {
                assert_eq!(this, &Value::Host(77));
                self.call(object, method, args)
            }
        }
        let mut host = NamedHost { gets: 0, calls: 0 };
        let mut vm = Runtime::default();
        vm.execute(
            "function* run(){return bridge.send(yield 1);}let gen=run();gen.next();",
            &mut host,
        )
        .unwrap();
        assert_eq!(host.calls, 0);
        assert_eq!(
            vm.execute("gen.next(42).value;", &mut host).unwrap(),
            Value::Number(42.)
        );
        assert_eq!(host.gets, 0);
        assert_eq!(host.calls, 1);
        assert_eq!(
            vm.execute("bridge['send'](43);", &mut host).unwrap(),
            Value::Number(43.)
        );
        assert_eq!(host.gets, 0);
        assert_eq!(host.calls, 2);
        assert_eq!(
            vm.execute("let effects=0;let error;try{bridge(effects++);}catch(e){error=e.name;}error==='TypeError'&&effects===1;", &mut host).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            vm.execute("function* directHost(){try{return (yield 1)(effects++);}catch(e){return e.name==='TypeError'&&effects===2;}}let direct=directHost();direct.next();let result=direct.next(bridge);result.done&&result.value;", &mut host).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(host.gets, 0);
        assert_eq!(host.calls, 2);
        assert!(vm.execute("bridge.send;", &mut host).is_err());
        assert_eq!(host.gets, 1);
        assert_eq!(host.calls, 2);
    }
    #[test]
    fn suspended_value_callee_is_callable_after_resume() {
        let mut vm = Runtime::default();
        vm.execute(
            "function* run(){return (yield 1)(2);}let gen=run();",
            &mut Empty,
        )
        .unwrap();
        assert_eq!(
            vm.execute("gen.next().value;", &mut Empty).unwrap(),
            Value::Number(1.)
        );
        assert_eq!(
            vm.execute(
                "let result=gen.next(function(n){return n+40;});result.done&&result.value===42;",
                &mut Empty
            )
            .unwrap(),
            Value::Bool(true)
        );
    }
    #[test]
    fn unsupported_call_shapes_fail_before_function_effects() {
        for source in [
            "function* run(){eval(yield 1);}",
            "function* run(){target(...[1],yield 1);}",
            "async function run(){new Target(await 1);}",
        ] {
            let mut vm = Runtime::default();
            assert!(vm.execute(source, &mut Empty).is_err(), "{source}");
        }
    }
}
