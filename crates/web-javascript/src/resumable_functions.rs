//! Adapters between bounded resumable activations, Promise jobs and iterators.
use crate::resumable::{Outcome, ResumeInput};
use crate::{Heap, Host, JsError, Runtime, Value, parser::FunctionKind};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default)]
pub(super) struct ResumableFunctions {
    generators: BTreeSet<usize>,
    prototype: Option<usize>,
}

impl Runtime {
    pub(super) fn configure_resumable_function(
        &mut self,
        value: &Value,
        kind: FunctionKind,
        method: bool,
    ) -> Result<(), JsError> {
        let Value::Function(id) = value else {
            return Err(JsError::runtime("Invalid resumable function"));
        };
        let body = match self.heap.get(*id) {
            Some(Heap::Function(function)) => function.body.clone(),
            _ => return Err(JsError::runtime("Invalid resumable function reference")),
        };
        crate::resumable::validate_body(&body, kind)?;
        let prototype = if kind == FunctionKind::Generator {
            let parent = self.generator_prototype()?;
            Some(self.object(BTreeMap::new(), Some(parent))?)
        } else if kind == FunctionKind::AsyncGenerator {
            let parent = self.async_generator_prototype()?;
            Some(self.object(BTreeMap::new(), Some(parent))?)
        } else {
            None
        };
        if let Heap::Function(function) = &mut self.heap[*id] {
            function.kind = kind;
            function.method = method;
            if method || kind != FunctionKind::Ordinary {
                function.constructible = false;
            }
            if let Some(prototype) = prototype {
                function.properties.insert("prototype".into(), prototype);
                self.property_metadata.entry(*id).or_default().insert(
                    "prototype".into(),
                    crate::descriptors::PropertyMetadata {
                        writable: true,
                        enumerable: false,
                        configurable: false,
                        accessor: None,
                    },
                );
            }
        }
        Ok(())
    }

    fn generator_prototype(&mut self) -> Result<Value, JsError> {
        if let Some(id) = self.resumable_functions.prototype {
            return Ok(Value::Object(id));
        }
        let parent = self.object_prototype_value()?;
        let prototype = self.object(
            BTreeMap::from([
                ("next".into(), Value::Native("Generator.next")),
                ("throw".into(), Value::Native("Generator.throw")),
                ("return".into(), Value::Native("Generator.return")),
            ]),
            Some(parent),
        )?;
        let Value::Object(id) = prototype else {
            unreachable!()
        };
        if let Heap::Object(object) = &mut self.heap[id] {
            object
                .non_enumerable
                .extend(["next".into(), "throw".into(), "return".into()]);
        }
        let Value::Symbol(iterator, _) = self.well_known_symbol("iterator")? else {
            unreachable!()
        };
        self.symbol_properties
            .entry(id)
            .or_default()
            .insert(iterator, Value::Native("Generator.identity"));
        self.symbol_order.entry(id).or_default().push(iterator);
        self.symbol_metadata.entry(id).or_default().insert(
            iterator,
            crate::descriptors::PropertyMetadata {
                writable: true,
                enumerable: false,
                configurable: true,
                accessor: None,
            },
        );
        self.resumable_functions.prototype = Some(id);
        Ok(Value::Object(id))
    }

    pub(super) fn create_generator(
        &mut self,
        function_id: usize,
        body: std::sync::Arc<Vec<crate::Stmt>>,
        scope: usize,
        strict: bool,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        if self.resumable_functions.generators.len() >= 256 {
            return Err(JsError::resource("Generator identity limit exceeded"));
        }
        let candidate = self.get(Value::Function(function_id), "prototype", host)?;
        let prototype = if matches!(
            candidate,
            Value::Object(_) | Value::Array(_) | Value::Function(_) | Value::Host(_)
        ) {
            candidate
        } else {
            self.generator_prototype()?
        };
        let value = self.object(BTreeMap::new(), Some(prototype))?;
        let Value::Object(owner) = value else {
            unreachable!()
        };
        self.resumable_start(owner, body, scope, strict, FunctionKind::Generator)?;
        self.resumable_functions.generators.insert(owner);
        Ok(Value::Object(owner))
    }

    /// Called only by the typed internal Promise reaction or initial invocation.
    pub(super) fn resume_async(
        &mut self,
        owner: usize,
        fulfilled: bool,
        value: Value,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        if !self.resumable_exists(owner) {
            return Ok(());
        }
        let mut input = if fulfilled {
            ResumeInput::Normal(value)
        } else {
            ResumeInput::Throw(value)
        };
        loop {
            match self.resumable_resume(owner, input, host) {
                Ok(Outcome::Await(value)) => match self.await_async(owner, value, host) {
                    Ok(()) => return Ok(()),
                    Err(error) if error.kind == crate::ErrorKind::Resource => {
                        self.resumable_cancel(owner);
                        return Err(error);
                    }
                    Err(error) => {
                        let thrown = match self.exception_value(&error) {
                            Ok(value) => value,
                            Err(error) => {
                                self.resumable_cancel(owner);
                                return Err(error);
                            }
                        };
                        // Constructor lookup in PromiseResolve can throw without
                        // creating a job. Feed it back without Rust recursion.
                        input = ResumeInput::Throw(thrown);
                    }
                },
                Ok(Outcome::Completed(value)) => {
                    return self.settle_async_promise(owner, Ok(value), host);
                }
                Ok(Outcome::Yield(_) | Outcome::YieldResult(_)) => {
                    return Err(JsError::runtime(
                        "Unexpected generator suspension in async function",
                    ));
                }
                Err(error) => {
                    self.cancel_async_await(owner);
                    return self.settle_async_promise(owner, Err(error), host);
                }
            }
        }
    }
    pub(super) fn resumable_native_call(
        &mut self,
        name: &str,
        args: &[Value],
        this: Value,
        host: &mut impl Host,
    ) -> Result<Option<Value>, JsError> {
        if let Some(value) = self.async_generator_native_call(name, args, this.clone(), host)? {
            return Ok(Some(value));
        }
        if !matches!(
            name,
            "Generator.next" | "Generator.throw" | "Generator.return" | "Generator.identity"
        ) {
            return Ok(None);
        }
        if name == "Generator.identity" {
            return Ok(Some(this));
        }
        let Value::Object(owner) = this else {
            return Err(self.property_type_error("Generator receiver required"));
        };
        if !self.resumable_functions.generators.contains(&owner) {
            return Err(self.property_type_error("Generator receiver required"));
        }
        let value = args.first().cloned().unwrap_or(Value::Undefined);
        let input = match name {
            "Generator.throw" => ResumeInput::Throw(value.clone()),
            "Generator.return" => ResumeInput::Return(value.clone()),
            _ => ResumeInput::Normal(value.clone()),
        };
        let (value, done) = if self.resumable_exists(owner) {
            match self.resumable_resume(owner, input, host)? {
                Outcome::Yield(value) => (value, false),
                Outcome::YieldResult(result) => return Ok(Some(result)),
                Outcome::Completed(value) => (value, true),
                Outcome::Await(_) => {
                    return Err(JsError::runtime("Unexpected async suspension in generator"));
                }
            }
        } else {
            match name {
                "Generator.throw" => {
                    let mut error = JsError::runtime("Uncaught exception in completed generator");
                    error.thrown = Some(value);
                    return Err(error);
                }
                "Generator.return" => (value, true),
                _ => (Value::Undefined, true),
            }
        };
        self.ordinary_object(BTreeMap::from([
            ("value".into(), value),
            ("done".into(), Value::Bool(done)),
        ]))
        .map(Some)
    }

    pub(super) fn resumable_function_roots(&self) -> Vec<Value> {
        self.resumable_functions
            .prototype
            .into_iter()
            .map(Value::Object)
            .collect()
    }
    pub(super) fn resumable_function_sweep(&mut self, live: &[bool]) {
        self.resumable_functions
            .generators
            .retain(|id| live.get(*id) == Some(&true));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn async_await_always_defers_and_resumes_exactly_once() {
        let mut vm = Runtime::default();
        vm.execute("var order='';var calls=0;function operand(){calls++;return 1;}async function run(){order+='a';const value=await operand();order+='b';return value+1;}let promise=run();order+='c';promise.then(value=>{order+=value;});",&mut Empty).unwrap();
        assert_eq!(
            vm.execute("order;", &mut Empty).unwrap(),
            Value::string("acb2")
        );
        assert_eq!(vm.execute("calls;", &mut Empty).unwrap(), Value::Number(1.));
        assert_eq!(
            vm.execute("Object.hasOwn(run,'prototype');", &mut Empty)
                .unwrap(),
            Value::Bool(false)
        );
    }
    #[test]
    fn pending_async_scope_survives_without_returned_promise_root() {
        let mut vm = Runtime::default();
        vm.execute("let pending=Promise.withResolvers();var answer=0;async function run(){const held={answer:42};await pending.promise;answer=held.answer;}run();",&mut Empty).unwrap();
        vm.collect_garbage(&[], &Empty).unwrap();
        vm.execute("pending.resolve(1);", &mut Empty).unwrap();
        assert_eq!(
            vm.execute("answer;", &mut Empty).unwrap(),
            Value::Number(42.)
        );
    }
    #[test]
    fn generator_injects_values_and_preserves_completed_behavior() {
        let mut vm = Runtime::default();
        vm.execute("var effects=0;function* sequence(){effects++;let sent=yield 1;yield sent;return 3;}let iter=sequence();",&mut Empty).unwrap();
        assert_eq!(
            vm.execute("effects;", &mut Empty).unwrap(),
            Value::Number(0.)
        );
        assert_eq!(
            vm.execute("iter.next(99).value;", &mut Empty).unwrap(),
            Value::Number(1.)
        );
        vm.collect_garbage(&[], &Empty).unwrap();
        assert_eq!(
            vm.execute("iter.next(7).value;", &mut Empty).unwrap(),
            Value::Number(7.)
        );
        assert_eq!(
            vm.execute("iter.next().value;", &mut Empty).unwrap(),
            Value::Number(3.)
        );
        assert_eq!(
            vm.execute("iter.next().done;", &mut Empty).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            vm.execute("iter.return(8).value;", &mut Empty).unwrap(),
            Value::Number(8.)
        );
        assert_eq!(
            vm.execute("effects;", &mut Empty).unwrap(),
            Value::Number(1.)
        );
        assert!(vm.execute("new sequence();", &mut Empty).is_err());
        let error = vm.execute("iter.throw(12);", &mut Empty).unwrap_err();
        assert_eq!(error.thrown, Some(Value::Number(12.)));
    }
    #[test]
    fn generator_is_an_iterator_and_return_before_start_has_no_effects() {
        let mut vm = Runtime::default();
        vm.execute("var effects=0;function* sequence(){effects++;yield 1;yield 2;}let first=sequence();first.return(7);var total=0;for(const value of sequence()){total+=value;}",&mut Empty).unwrap();
        assert_eq!(
            vm.execute("effects;", &mut Empty).unwrap(),
            Value::Number(1.)
        );
        assert_eq!(vm.execute("total;", &mut Empty).unwrap(), Value::Number(3.));
    }
    #[test]
    fn async_parameter_errors_reject_and_unsupported_shapes_precede_effects() {
        let mut vm = Runtime::default();
        vm.execute("function fail(){throw 9;}async function run(value=fail()){return value;}var rejected=0;run().catch(error=>{rejected=error;});",&mut Empty).unwrap();
        assert_eq!(
            vm.execute("rejected;", &mut Empty).unwrap(),
            Value::Number(9.)
        );
        let error = vm
            .execute(
                "var bad=1;async function unsupported(){return new call(await 2);}",
                &mut Empty,
            )
            .unwrap_err();
        assert_eq!(error.kind, crate::ErrorKind::Syntax);
        assert!(vm.global_value("bad").is_none());
    }
    #[test]
    fn all_function_declaration_kinds_preserve_existing_global_lexicals() {
        for declaration in [
            "function blocked(){}",
            "async function blocked(){}",
            "function* blocked(){}",
        ] {
            let mut vm = Runtime::default();
            vm.execute("let blocked=1;", &mut Empty).unwrap();
            assert!(vm.execute(declaration, &mut Empty).is_err());
            assert_eq!(
                vm.execute("blocked;", &mut Empty).unwrap(),
                Value::Number(1.)
            );
        }
    }
    #[test]
    fn generator_finally_retains_returned_object_through_collection() {
        let mut vm = Runtime::default();
        vm.execute("function* run(){try{yield 1;}finally{yield 2;}}let gen=run();gen.next();let held={answer:42};gen.return(held);held=null;",&mut Empty).unwrap();
        vm.collect_garbage(&[], &Empty).unwrap();
        assert_eq!(
            vm.execute("gen.next().value.answer;", &mut Empty).unwrap(),
            Value::Number(42.)
        );
    }
    #[test]
    fn async_arrow_preserves_lexical_this_and_arguments() {
        let mut vm = Runtime::default();
        vm.execute("function outer(value){const arrow=async()=>{const sent=await arguments[0];return this.n+sent;};return arrow();}var answer=0;outer.call({n:5},7).then(value=>{answer=value;});",&mut Empty).unwrap();
        assert_eq!(
            vm.execute("answer;", &mut Empty).unwrap(),
            Value::Number(12.)
        );
    }
    #[test]
    fn rejected_loop_condition_unwinds_through_async_finally() {
        let mut vm = Runtime::default();
        vm.execute("var cleaned=0;var caught=0;async function run(){try{while(await Promise.reject(8)){throw 'unreachable';}}catch(error){return error;}finally{await 1;cleaned++;}}run().then(value=>{caught=value;});",&mut Empty).unwrap();
        assert_eq!(
            vm.execute("cleaned;", &mut Empty).unwrap(),
            Value::Number(1.)
        );
        assert_eq!(
            vm.execute("caught;", &mut Empty).unwrap(),
            Value::Number(8.)
        );
    }
    #[test]
    fn object_async_and_generator_methods_use_receivers_and_are_not_constructors() {
        let mut vm = Runtime::default();
        vm.execute("const object={n:3,async add(x){const value=await x;return this.n+value;},*items(){yield this.n;},async(){return 5;}};var answer=0;object.add(4).then(value=>{answer=value;});",&mut Empty).unwrap();
        assert_eq!(
            vm.execute("answer;", &mut Empty).unwrap(),
            Value::Number(7.)
        );
        assert_eq!(
            vm.execute("object.items().next().value;", &mut Empty)
                .unwrap(),
            Value::Number(3.)
        );
        assert_eq!(
            vm.execute("object.async();", &mut Empty).unwrap(),
            Value::Number(5.)
        );
        assert_eq!(
            vm.execute("Object.hasOwn(object.async,'prototype');", &mut Empty)
                .unwrap(),
            Value::Bool(false)
        );
        assert!(vm.execute("new object.async();", &mut Empty).is_err());
        assert!(vm.execute("new object.items();", &mut Empty).is_err());
    }
    #[test]
    fn class_method_scopes_retain_private_fields_and_super_across_suspension() {
        let mut vm = Runtime::default();
        vm.execute("class Base{value(){return 2;}}class Child extends Base{#n=4;async value(){await 1;return super.value()+this.#n;}*items(){yield this.#n;}static async ping(){return await 9;}}var answer=0;var ping=0;const child=new Child();child.value().then(value=>{answer=value;});Child.ping().then(value=>{ping=value;});",&mut Empty).unwrap();
        assert_eq!(
            vm.execute("answer;", &mut Empty).unwrap(),
            Value::Number(6.)
        );
        assert_eq!(vm.execute("ping;", &mut Empty).unwrap(), Value::Number(9.));
        assert_eq!(
            vm.execute("child.items().next().value;", &mut Empty)
                .unwrap(),
            Value::Number(4.)
        );
    }
    #[test]
    fn ordinary_for_uses_the_same_per_iteration_binding_rules() {
        let mut vm = Runtime::default();
        assert_eq!(vm.execute("let callbacks=[];for(let i=0;i<3;i++){callbacks.push(()=>i);}callbacks[0]()*100+callbacks[1]()*10+callbacks[2]();",&mut Empty).unwrap(),Value::Number(12.));
        assert_eq!(vm.execute("let other=[];for(let [i]=[0];i<3;i++){other.push(()=>i);}other[0]()*100+other[1]()*10+other[2]();",&mut Empty).unwrap(),Value::Number(12.));
        vm.execute("let outer=9;", &mut Empty).unwrap();
        assert!(
            vm.execute("for(let i=outer,outer=1;i<2;i++){}", &mut Empty)
                .is_err()
        );
    }

    #[test]
    fn delegated_yield_preserves_result_identity_and_captured_next() {
        let mut vm = Runtime::default();
        vm.execute("let reads=0;let gets=0;let calls=0;let raw={done:false,extra:42};Object.defineProperty(raw,'value',{get:function(){reads++;throw 99;}});let delegate={};Object.defineProperty(delegate,'next',{get:function(){gets++;return function(v){calls++;if(calls===1){if(arguments.length!==1||v!==undefined)throw 1;return raw;}return {done:true,value:v};};}});delegate[Symbol.iterator]=function(){return this;};function* run(){return yield* delegate;}let gen=run();let same=gen.next()===raw;", &mut Empty).unwrap();
        assert_eq!(
            vm.execute("same&&reads===0&&gets===1;", &mut Empty)
                .unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            vm.execute("gen.next(17).value;", &mut Empty).unwrap(),
            Value::Number(17.)
        );
        assert_eq!(vm.execute("gets;", &mut Empty).unwrap(), Value::Number(1.));
    }
    #[test]
    fn delegated_throw_completion_is_normal_and_return_can_be_replaced() {
        let mut vm = Runtime::default();
        vm.execute("let raw={done:false,value:2};let delegate={next:function(v){return {done:false,value:v};},throw:function(v){return {done:true,value:v+1};},return:function(v){return raw;}};delegate[Symbol.iterator]=function(){return this;};function* run(){let v=yield* delegate;return v+10;}let a=run();a.next();", &mut Empty).unwrap();
        assert_eq!(
            vm.execute("a.throw(4).value;", &mut Empty).unwrap(),
            Value::Number(15.)
        );
        vm.execute(
            "let b=run();b.next();let same=b.return(40)===raw;",
            &mut Empty,
        )
        .unwrap();
        assert_eq!(
            vm.execute("same&&b.next(7).value===7;", &mut Empty)
                .unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            vm.execute("b.throw(1).value;", &mut Empty).unwrap(),
            Value::Number(12.)
        );
    }
    #[test]
    fn delegated_return_unwinds_finally_without_completing_assignment() {
        let mut vm = Runtime::default();
        vm.execute("let written=0;let delegate={next:function(){return {done:false,value:1};},return:function(v){return {done:true,value:v+1};}};delegate[Symbol.iterator]=function(){return this;};function* run(){try{written=yield* delegate;}finally{yield 9;}}let gen=run();gen.next();", &mut Empty).unwrap();
        assert_eq!(
            vm.execute("gen.return(4).value;", &mut Empty).unwrap(),
            Value::Number(9.)
        );
        assert_eq!(
            vm.execute("gen.next().value;", &mut Empty).unwrap(),
            Value::Number(5.)
        );
        assert_eq!(
            vm.execute("written;", &mut Empty).unwrap(),
            Value::Number(0.)
        );
    }
    #[test]
    fn delegated_missing_throw_closes_and_close_error_wins() {
        let mut vm = Runtime::default();
        vm.execute("let closed=0;let delegate={next:function(){return {done:false};},throw:null,return:function(){closed++;if(arguments.length!==0)throw 88;throw 42;}};delegate[Symbol.iterator]=function(){return this;};function* run(){try{yield* delegate;}catch(e){return e;}}let gen=run();gen.next();", &mut Empty).unwrap();
        assert_eq!(
            vm.execute("gen.throw(9).value;", &mut Empty).unwrap(),
            Value::Number(42.)
        );
        assert_eq!(
            vm.execute("closed;", &mut Empty).unwrap(),
            Value::Number(1.)
        );
        vm.execute(
            "delegate.return=function(){closed++;return {};};let other=run();other.next();",
            &mut Empty,
        )
        .unwrap();
        assert_eq!(
            vm.execute("other.throw(9).value instanceof TypeError;", &mut Empty)
                .unwrap(),
            Value::Bool(true)
        );
        vm.execute("delegate.throw=3;let third=run();third.next();", &mut Empty)
            .unwrap();
        assert_eq!(
            vm.execute("third.throw(9).value instanceof TypeError;", &mut Empty)
                .unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            vm.execute("closed;", &mut Empty).unwrap(),
            Value::Number(2.)
        );
    }
    #[test]
    fn delegated_done_is_read_before_value_and_bad_results_throw() {
        let mut vm = Runtime::default();
        vm.execute("let order='';let result={};Object.defineProperty(result,'done',{get:function(){order+='d';return true;}});Object.defineProperty(result,'value',{get:function(){order+='v';return 8;}});let delegate={next:function(){return result;}};delegate[Symbol.iterator]=function(){return this;};function* run(){return yield* delegate;}", &mut Empty).unwrap();
        assert_eq!(
            vm.execute("run().next().value;", &mut Empty).unwrap(),
            Value::Number(8.)
        );
        assert_eq!(
            vm.execute("order;", &mut Empty).unwrap(),
            Value::String("dv".into())
        );
        vm.execute("delegate.next=function(){return 3;};", &mut Empty)
            .unwrap();
        assert!(vm.execute("run().next();", &mut Empty).is_err());
    }
    #[test]
    fn delegated_state_and_outer_expression_survive_collection() {
        let mut vm = Runtime::default();
        vm.execute("function make(){let n=0;let delegate={next:function(v){n++;return {done:n>1,value:n>1?v:2};}};delegate[Symbol.iterator]=function(){return this;};return delegate;}function* run(){return ({n:40}).n+(yield* make());}let gen=run();gen.next();", &mut Empty).unwrap();
        vm.collect_garbage(&[], &Empty).unwrap();
        assert_eq!(
            vm.execute("gen.next(2).value;", &mut Empty).unwrap(),
            Value::Number(42.)
        );
    }
    #[test]
    fn delegated_arrays_are_incremental_and_nested_operands_resume() {
        let mut vm = Runtime::default();
        vm.execute("let items=[1,2];function* run(){return yield* items;}let gen=run();gen.next();items[1]=9;", &mut Empty).unwrap();
        assert_eq!(
            vm.execute("gen.next().value;", &mut Empty).unwrap(),
            Value::Number(9.)
        );
        assert_eq!(
            vm.execute("gen.next().done;", &mut Empty).unwrap(),
            Value::Bool(true)
        );
        vm.execute(
            "function* nested(){return yield* (yield 3);}let other=nested();other.next();",
            &mut Empty,
        )
        .unwrap();
        assert_eq!(
            vm.execute("other.next([7]).value;", &mut Empty).unwrap(),
            Value::Number(7.)
        );
        assert_eq!(
            vm.execute("other.next().done;", &mut Empty).unwrap(),
            Value::Bool(true)
        );
    }

    #[test]
    fn delegated_absent_return_preserves_input_and_getters_are_lazy() {
        let mut vm = Runtime::default();
        vm.execute("let gets=0;let delegate={next:function(){return {done:false};}};Object.defineProperty(delegate,'return',{get:function(){gets++;return null;}});delegate[Symbol.iterator]=function(){return this;};function* run(){try{yield* delegate;}finally{yield 2;}}let gen=run();gen.next();", &mut Empty).unwrap();
        assert_eq!(vm.execute("gets;", &mut Empty).unwrap(), Value::Number(0.));
        assert_eq!(
            vm.execute("gen.return(7).value;", &mut Empty).unwrap(),
            Value::Number(2.)
        );
        assert_eq!(
            vm.execute("gen.next().value;", &mut Empty).unwrap(),
            Value::Number(7.)
        );
        assert_eq!(vm.execute("gets;", &mut Empty).unwrap(), Value::Number(1.));
    }
    #[test]
    fn delegated_step_budget_is_fatal_and_does_not_run_finally() {
        let mut vm = Runtime::default();
        vm.execute("let cleaned=0;let delegate={next:function(){return {done:false};}};delegate[Symbol.iterator]=function(){return this;};function* run(){try{yield* delegate;}finally{cleaned++;}}let gen=run();", &mut Empty).unwrap();
        for _ in 0..4096 {
            vm.execute("gen.next();", &mut Empty).unwrap();
        }
        assert!(vm.execute("gen.next();", &mut Empty).is_err());
        assert_eq!(
            vm.execute("cleaned;", &mut Empty).unwrap(),
            Value::Number(0.)
        );
    }
}
