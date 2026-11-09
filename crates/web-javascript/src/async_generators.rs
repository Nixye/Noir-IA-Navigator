//! Intrinsic async generator requests. Identity records are weak GC side tables.
use crate::resumable::{Outcome, ResumeInput};
use crate::{ErrorKind, Host, JsError, Runtime, Value, parser::FunctionKind};
use std::collections::{BTreeMap, VecDeque};

const MAX_IDENTITIES: usize = 256;
const MAX_REQUESTS: usize = 256;
const MAX_TOTAL_REQUESTS: usize = 512;
// Synchronous PromiseResolve failures may reenter catch/await before a job.
const MAX_SYNC_RESUMES: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AwaitPhase {
    Body,
    ReturnAtYield,
    ReturnCompleted,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    SuspendedStart,
    Executing,
    SuspendedYield,
    AwaitingReturn,
    Completed,
}
#[derive(Clone, Debug)]
struct Request {
    input: ResumeInput,
    promise: usize,
}
#[derive(Clone, Debug)]
struct Generator {
    state: State,
    queue: VecDeque<Request>,
    pumping: bool,
    phase: Option<AwaitPhase>,
    awaited: Option<usize>,
}
#[derive(Clone, Debug, Default)]
pub(super) struct AsyncGenerators {
    records: BTreeMap<usize, Generator>,
    requests: BTreeMap<usize, usize>,
    // Result assimilation outlives its FIFO entry, but still belongs to the activation.
    settling_requests: BTreeMap<usize, usize>,
    prototype: Option<usize>,
    depth: usize,
}

impl Runtime {
    pub(super) fn async_generator_prototype(&mut self) -> Result<Value, JsError> {
        if let Some(id) = self.async_generators.prototype {
            return Ok(Value::Object(id));
        }
        let parent = self.object_prototype_value()?;
        let prototype = self.object(
            BTreeMap::from([
                ("next".into(), Value::Native("AsyncGenerator.next")),
                ("throw".into(), Value::Native("AsyncGenerator.throw")),
                ("return".into(), Value::Native("AsyncGenerator.return")),
            ]),
            Some(parent),
        )?;
        self.hide_properties(&prototype, &["next", "throw", "return"]);
        let Value::Object(id) = prototype else {
            unreachable!()
        };
        let Value::Symbol(symbol, _) = self.well_known_symbol("asyncIterator")? else {
            unreachable!()
        };
        self.symbol_properties
            .entry(id)
            .or_default()
            .insert(symbol, Value::Native("AsyncGenerator.identity"));
        self.symbol_order.entry(id).or_default().push(symbol);
        self.symbol_metadata.entry(id).or_default().insert(
            symbol,
            crate::descriptors::PropertyMetadata {
                writable: true,
                enumerable: false,
                configurable: true,
                accessor: None,
            },
        );
        self.async_generators.prototype = Some(id);
        Ok(Value::Object(id))
    }
    pub(super) fn create_async_generator(
        &mut self,
        function_id: usize,
        body: std::sync::Arc<Vec<crate::Stmt>>,
        scope: usize,
        strict: bool,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        if self.async_generators.records.len() >= MAX_IDENTITIES {
            return Err(JsError::resource("Async generator identity limit exceeded"));
        }
        let candidate = self.get(Value::Function(function_id), "prototype", host)?;
        let prototype = if matches!(
            candidate,
            Value::Object(_)
                | Value::Array(_)
                | Value::Function(_)
                | Value::Host(_)
                | Value::Native(_)
        ) {
            candidate
        } else {
            self.async_generator_prototype()?
        };
        let value = self.object(BTreeMap::new(), Some(prototype))?;
        let Value::Object(owner) = value else {
            unreachable!()
        };
        self.resumable_start(owner, body, scope, strict, FunctionKind::AsyncGenerator)?;
        self.async_generators.records.insert(
            owner,
            Generator {
                state: State::SuspendedStart,
                queue: VecDeque::new(),
                pumping: false,
                phase: None,
                awaited: None,
            },
        );
        Ok(Value::Object(owner))
    }
    pub(super) fn async_generator_native_call(
        &mut self,
        name: &str,
        args: &[Value],
        this: Value,
        host: &mut impl Host,
    ) -> Result<Option<Value>, JsError> {
        if name == "AsyncGenerator.identity" {
            return Ok(Some(this));
        }
        if !matches!(
            name,
            "AsyncGenerator.next" | "AsyncGenerator.throw" | "AsyncGenerator.return"
        ) {
            return Ok(None);
        }
        let target = match &this {
            Value::Object(owner) if self.async_generators.records.contains_key(owner) => {
                Some(*owner)
            }
            _ => None,
        };
        let promise = match self.new_async_promise() {
            Ok(promise) => promise,
            Err(error) => {
                if let Some(owner) = target {
                    self.cancel_async_generator(owner);
                }
                return Err(error);
            }
        };
        let Value::Object(child) = promise else {
            unreachable!()
        };
        let owner = match this {
            Value::Object(owner) if self.async_generators.records.contains_key(&owner) => owner,
            _ => {
                let error = self.property_type_error("Async generator receiver required");
                let reason = self.exception_value(&error)?;
                self.settle_promise(child, true, reason)?;
                return Ok(Some(promise));
            }
        };
        let result = (|| {
            self.tick()?;
            if self
                .async_generators
                .requests
                .len()
                .saturating_add(self.async_generators.settling_requests.len())
                >= MAX_TOTAL_REQUESTS
                || self.async_generators.records[&owner]
                    .queue
                    .len()
                    .saturating_add(
                        self.async_generators
                            .settling_requests
                            .values()
                            .filter(|target| **target == owner)
                            .count(),
                    )
                    >= MAX_REQUESTS
            {
                return Err(JsError::resource("Async generator request limit exceeded"));
            }
            self.charge_array_slots(1)?;
            let value = args.first().cloned().unwrap_or(Value::Undefined);
            let input = match name {
                "AsyncGenerator.return" => ResumeInput::Return(value),
                "AsyncGenerator.throw" => ResumeInput::Throw(value),
                _ => ResumeInput::Normal(value),
            };
            self.async_generators.requests.insert(child, owner);
            self.async_generators
                .records
                .get_mut(&owner)
                .unwrap()
                .queue
                .push_back(Request {
                    input,
                    promise: child,
                });
            self.pump_async_generator(owner, host)
        })();
        if let Err(error) = result {
            self.cancel_async_generator(owner);
            return Err(error);
        }
        Ok(Some(promise))
    }
    fn finish_async_generator_request(
        &mut self,
        owner: usize,
        rejected: bool,
        value: Value,
        done: bool,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        let child = self.async_generators.records[&owner]
            .queue
            .front()
            .unwrap()
            .promise;
        // Keep the front request and reverse edge until resolving finishes. A
        // getter can enqueue more requests or collect garbage reentrantly.
        if rejected {
            self.settle_promise(child, true, value)?;
        } else {
            let result = self.adapted_iteration_result(value, done)?;
            let root = self.retain_value(result.clone())?;
            self.async_generators.settling_requests.insert(child, owner);
            let settled = self.promise_resolve_value(child, result, host);
            self.release_value(root);
            settled?;
        }
        self.async_generators
            .records
            .get_mut(&owner)
            .unwrap()
            .queue
            .pop_front();
        self.async_generators.requests.remove(&child);
        Ok(())
    }
    fn suspend_async_generator(
        &mut self,
        owner: usize,
        phase: AwaitPhase,
        value: Value,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        let record = self.async_generators.records.get_mut(&owner).unwrap();
        record.phase = Some(phase);
        record.state = if phase == AwaitPhase::Body {
            State::Executing
        } else {
            State::AwaitingReturn
        };
        match self.await_async_generator(owner, phase, value, host) {
            Ok(()) => Ok(()),
            Err(error) if error.kind == ErrorKind::Resource => Err(error),
            Err(error) => {
                // PromiseResolve can throw synchronously through constructor.
                let value = self.exception_value(&error)?;
                self.continue_async_generator(owner, phase, true, value, host)
            }
        }
    }
    fn drive_async_generator(
        &mut self,
        owner: usize,
        input: ResumeInput,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        if self.async_generators.depth >= MAX_SYNC_RESUMES {
            return Err(JsError::resource(
                "Async generator continuation depth exceeded",
            ));
        }
        self.async_generators.depth += 1;
        let result = (|| {
            self.async_generators.records.get_mut(&owner).unwrap().state = State::Executing;
            match self.resumable_resume(owner, input, host) {
                Ok(Outcome::Await(value)) => {
                    self.suspend_async_generator(owner, AwaitPhase::Body, value, host)
                }
                Ok(Outcome::Yield(value)) => {
                    self.async_generators.records.get_mut(&owner).unwrap().state =
                        State::SuspendedYield;
                    self.finish_async_generator_request(owner, false, value, false, host)
                }
                Ok(Outcome::Completed(value)) => {
                    self.async_generators.records.get_mut(&owner).unwrap().state = State::Completed;
                    self.finish_async_generator_request(owner, false, value, true, host)
                }
                Ok(Outcome::YieldResult(_)) => {
                    Err(JsError::runtime("Async delegated yield is unsupported"))
                }
                Err(error) if error.kind == ErrorKind::Resource => Err(error),
                Err(error) => {
                    self.async_generators.records.get_mut(&owner).unwrap().state = State::Completed;
                    let value = self.exception_value(&error)?;
                    self.finish_async_generator_request(owner, true, value, true, host)
                }
            }
        })();
        self.async_generators.depth -= 1;
        result
    }
    fn pump_async_generator(&mut self, owner: usize, host: &mut impl Host) -> Result<(), JsError> {
        if self.async_generators.records[&owner].pumping {
            return Ok(());
        }
        let root = self.retain_value(Value::Object(owner))?;
        self.async_generators
            .records
            .get_mut(&owner)
            .unwrap()
            .pumping = true;
        let result = (|| {
            loop {
                let record = &self.async_generators.records[&owner];
                if record.phase.is_some() || record.state == State::Executing {
                    break;
                }
                let Some(request) = record.queue.front().cloned() else {
                    break;
                };
                let state = record.state;
                self.tick()?;
                match (state, request.input) {
                    (State::Completed | State::SuspendedStart, ResumeInput::Return(value)) => {
                        self.resumable_cancel(owner);
                        self.suspend_async_generator(
                            owner,
                            AwaitPhase::ReturnCompleted,
                            value,
                            host,
                        )?;
                    }
                    (State::Completed | State::SuspendedStart, ResumeInput::Throw(value)) => {
                        self.resumable_cancel(owner);
                        self.async_generators.records.get_mut(&owner).unwrap().state =
                            State::Completed;
                        self.finish_async_generator_request(owner, true, value, true, host)?;
                    }
                    (State::Completed, ResumeInput::Normal(_)) => self
                        .finish_async_generator_request(
                            owner,
                            false,
                            Value::Undefined,
                            true,
                            host,
                        )?,
                    (State::SuspendedYield, ResumeInput::Return(value)) => {
                        self.suspend_async_generator(owner, AwaitPhase::ReturnAtYield, value, host)?
                    }
                    (State::SuspendedStart, ResumeInput::Normal(_)) => self.drive_async_generator(
                        owner,
                        ResumeInput::Normal(Value::Undefined),
                        host,
                    )?,
                    (_, input) => self.drive_async_generator(owner, input, host)?,
                }
            }
            Ok(())
        })();
        self.async_generators
            .records
            .get_mut(&owner)
            .unwrap()
            .pumping = false;
        self.release_value(root);
        if result.is_err() {
            self.cancel_async_generator(owner);
        }
        result
    }
    fn continue_async_generator(
        &mut self,
        owner: usize,
        phase: AwaitPhase,
        rejected: bool,
        value: Value,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        self.release_async_generator_auxiliaries(owner);
        self.async_generators
            .records
            .get_mut(&owner)
            .unwrap()
            .awaited = None;
        self.async_generators.records.get_mut(&owner).unwrap().phase = None;
        match phase {
            AwaitPhase::ReturnCompleted => {
                self.async_generators.records.get_mut(&owner).unwrap().state = State::Completed;
                self.finish_async_generator_request(owner, rejected, value, true, host)
            }
            AwaitPhase::Body | AwaitPhase::ReturnAtYield => {
                let input = if rejected {
                    ResumeInput::Throw(value)
                } else if phase == AwaitPhase::ReturnAtYield {
                    ResumeInput::Return(value)
                } else {
                    ResumeInput::Normal(value)
                };
                self.drive_async_generator(owner, input, host)
            }
        }
    }
    pub(super) fn resume_async_generator(
        &mut self,
        owner: usize,
        phase: AwaitPhase,
        rejected: bool,
        value: Value,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        if self
            .async_generators
            .records
            .get(&owner)
            .and_then(|r| r.phase)
            != Some(phase)
        {
            return Ok(());
        }
        let root = self.retain_value(Value::Object(owner))?;
        let value_root = match self.retain_value(value.clone()) {
            Ok(root) => root,
            Err(error) => {
                self.release_value(root);
                self.cancel_async_generator(owner);
                return Err(error);
            }
        };
        self.async_generators
            .records
            .get_mut(&owner)
            .unwrap()
            .pumping = true;
        let result = self.continue_async_generator(owner, phase, rejected, value, host);
        self.async_generators
            .records
            .get_mut(&owner)
            .unwrap()
            .pumping = false;
        let result = result.and_then(|()| self.pump_async_generator(owner, host));
        self.release_value(value_root);
        self.release_value(root);
        if result.is_err() {
            self.cancel_async_generator(owner);
        }
        result
    }
    pub(super) fn set_async_generator_await(&mut self, owner: usize, promise: usize) {
        if let Some(record) = self.async_generators.records.get_mut(&owner) {
            record.awaited = Some(promise);
        }
    }
    pub(super) fn settling_async_generator_owner(&self, promise: usize) -> Option<usize> {
        self.async_generators
            .settling_requests
            .get(&promise)
            .copied()
    }
    pub(super) fn finish_async_generator_settlement(&mut self, promise: usize) {
        self.async_generators.settling_requests.remove(&promise);
    }
    pub(super) fn cancel_async_generator(&mut self, owner: usize) {
        self.resumable_cancel(owner);
        self.cancel_async_await(owner);
        if let Some(record) = self.async_generators.records.get_mut(&owner) {
            record.queue.clear();
            record.phase = None;
            record.awaited = None;
            record.state = State::Completed;
        }
        let requests: Vec<usize> = self
            .async_generators
            .requests
            .iter()
            .chain(self.async_generators.settling_requests.iter())
            .filter_map(|(promise, target)| (*target == owner).then_some(*promise))
            .collect();
        for promise in requests {
            self.cancel_async_generator_request(promise);
        }
        self.async_generators
            .requests
            .retain(|_, target| *target != owner);
        self.async_generators
            .settling_requests
            .retain(|_, target| *target != owner);
    }
    pub(super) fn async_generator_roots(&self) -> Vec<Value> {
        self.async_generators
            .prototype
            .into_iter()
            .map(Value::Object)
            .collect()
    }
    pub(super) fn async_generator_trace(&self, id: usize) -> Vec<Value> {
        let mut values = Vec::new();
        if let Some(owner) = self.async_generators.requests.get(&id) {
            values.push(Value::Object(*owner));
        }
        if let Some(owner) = self.async_generators.settling_requests.get(&id) {
            values.push(Value::Object(*owner));
        }
        values.extend(
            self.async_generators
                .settling_requests
                .iter()
                .filter_map(|(promise, owner)| (*owner == id).then_some(Value::Object(*promise))),
        );
        if let Some(record) = self.async_generators.records.get(&id) {
            values.extend(record.awaited.map(Value::Object));
            for request in &record.queue {
                values.push(Value::Object(request.promise));
                let (ResumeInput::Normal(value)
                | ResumeInput::Throw(value)
                | ResumeInput::Return(value)) = &request.input;
                values.push(value.clone());
            }
        }
        values
    }
    pub(super) fn async_generator_sweep(&mut self, live: &[bool]) {
        self.async_generators
            .records
            .retain(|id, _| live.get(*id) == Some(&true));
        self.async_generators
            .requests
            .retain(|id, owner| live.get(*id) == Some(&true) && live.get(*owner) == Some(&true));
        self.async_generators
            .settling_requests
            .retain(|id, owner| live.get(*id) == Some(&true) && live.get(*owner) == Some(&true));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_resolver_fatal_then_getter_cancels_await_or_result_settlement() {
        for source in [
            "let saved;let cleanup=0;async function* f(){try{yield {then:function(resolve){saved=resolve;}};}finally{cleanup++;}}let g=f();g.next();",
            "let saved;let cleanup=0;async function* f(){try{yield 1;}finally{cleanup++;}}let g=f();Object.defineProperty(Object.prototype,'then',{get:function(){if(this.done===false)return function(resolve){saved=resolve;};return undefined;}});g.next();",
        ] {
            let mut vm = Runtime::default();
            vm.execute(source, &mut Empty).unwrap();
            let error = vm.execute("let bad={};Object.defineProperty(bad,'then',{get:function(){while(true){}}});saved(bad);", &mut Empty).unwrap_err();
            assert_eq!(error.kind, ErrorKind::Resource);
            let Value::Object(owner) = vm.global_value("g").unwrap() else {
                unreachable!()
            };
            assert_eq!(vm.async_generators.records[&owner].state, State::Completed);
            assert!(vm.async_generators.records[&owner].queue.is_empty());
            assert!(vm.async_generators.requests.is_empty());
            assert!(vm.async_generators.settling_requests.is_empty());
            assert!(!vm.resumable_exists(owner));
            assert_eq!(vm.global_value("cleanup"), Some(Value::Number(0.)));
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
    fn fifo_requests_wait_for_active_await_and_start_lazily() {
        let mut vm = Runtime::default();
        vm.execute("let gate=Promise.withResolvers();let entered=0;let log='';async function* run(){entered++;yield await gate.promise;yield 2;return 3;}let g=run();", &mut Empty).unwrap();
        assert_eq!(
            vm.execute("entered", &mut Empty).unwrap(),
            Value::Number(0.)
        );
        vm.execute("g.next(99).then(r=>{log+=r.value;});g.next().then(r=>{log+=r.value;});g.return(8).then(r=>{log+=r.value;});", &mut Empty).unwrap();
        assert_eq!(vm.execute("log", &mut Empty).unwrap(), Value::string(""));
        vm.execute("gate.resolve(1)", &mut Empty).unwrap();
        assert_eq!(vm.execute("log", &mut Empty).unwrap(), Value::string("128"));
        assert_eq!(
            vm.execute("entered", &mut Empty).unwrap(),
            Value::Number(1.)
        );
    }
    #[test]
    fn return_before_start_awaits_without_body_and_completed_requests_are_promises() {
        let mut vm = Runtime::default();
        vm.execute("let n=0;let out='';let gate=Promise.withResolvers();async function* f(){n++;yield 1;}let g=f();g.return(gate.promise).then(r=>{out+=r.value;});g.next().then(r=>{out+=r.done;});g.throw(7).catch(e=>{out+=e;});", &mut Empty).unwrap();
        assert_eq!(vm.execute("out", &mut Empty).unwrap(), Value::string(""));
        vm.execute("gate.resolve(4)", &mut Empty).unwrap();
        assert_eq!(
            vm.execute("out", &mut Empty).unwrap(),
            Value::string("4true7")
        );
        assert_eq!(vm.execute("n", &mut Empty).unwrap(), Value::Number(0.));
        vm.execute(
            "let bad='';g.next.call({}).catch(e=>{bad=e.name;});",
            &mut Empty,
        )
        .unwrap();
        assert_eq!(
            vm.execute("bad", &mut Empty).unwrap(),
            Value::string("TypeError")
        );
    }
    #[test]
    fn rejected_external_return_is_thrown_at_yield_and_finally_can_yield() {
        let mut vm = Runtime::default();
        vm.execute("let out='';async function* f(){try{yield 1;}catch(e){yield e;}finally{yield 3;}}let g=f();g.next();", &mut Empty).unwrap();
        vm.execute("g.return(Promise.reject(8)).then(r=>{out+=r.value+':'+r.done;});g.next().then(r=>{out+=':'+r.value;});g.next().then(r=>{out+=':'+r.done;});", &mut Empty).unwrap();
        assert_eq!(
            vm.execute("out", &mut Empty).unwrap(),
            Value::string("8:false:3:true")
        );
        vm.execute(
            "async function* h(){try{yield 1;}finally{yield 2;}}let j=h();j.next();",
            &mut Empty,
        )
        .unwrap();
        vm.execute("let final='';j.return(9).then(r=>{final+=r.value+':'+r.done;});j.next().then(r=>{final+=':'+r.value+':'+r.done;});", &mut Empty).unwrap();
        assert_eq!(
            vm.execute("final", &mut Empty).unwrap(),
            Value::string("2:false:9:true")
        );
    }
    #[test]
    fn normal_and_throw_inputs_are_not_assimilated() {
        let mut vm = Runtime::default();
        vm.execute("let reads=0;let value={};Object.defineProperty(value,'then',{get:function(){reads++;throw 99;}});let seen=0;async function* f(){let sent=yield 1;if(sent===value)seen++;try{yield 2;}catch(e){if(e===value)seen++;}}let g=f();g.next();", &mut Empty).unwrap();
        vm.execute("g.next(value);g.throw(value);", &mut Empty)
            .unwrap();
        assert_eq!(vm.execute("reads", &mut Empty).unwrap(), Value::Number(0.));
        assert_eq!(vm.execute("seen", &mut Empty).unwrap(), Value::Number(2.));
    }
    #[test]
    fn pending_request_roots_generator_but_unreachable_cycles_are_swept() {
        let mut vm = Runtime::default();
        vm.execute("let gate=Promise.withResolvers();async function* f(){let held={n:42};await gate.promise;yield held.n;}let g=f();let pending=g.next();g=null;", &mut Empty).unwrap();
        vm.collect_garbage(&[], &Empty).unwrap();
        assert_eq!(vm.async_generators.records.len(), 1);
        vm.execute(
            "let answer=0;pending.then(r=>{answer=r.value;});gate.resolve();",
            &mut Empty,
        )
        .unwrap();
        assert_eq!(
            vm.execute("answer", &mut Empty).unwrap(),
            Value::Number(42.)
        );
        vm.execute("pending=null;gate=null;", &mut Empty).unwrap();
        vm.collect_garbage(&[], &Empty).unwrap();
        assert!(vm.async_generators.records.is_empty());
        assert!(vm.async_generators.requests.is_empty());
    }
    #[test]
    fn reentrant_result_then_getter_queues_without_reentering_machine() {
        let mut vm = Runtime::default();
        vm.execute("let count=0;let out='';async function* f(){yield 1;yield 2;}let g=f();Object.defineProperty(Object.prototype,'then',{configurable:true,get:function(){if(this.done===false&&count===0){count++;g.next().then(r=>{out+=r.value;});}return undefined;}});g.next().then(r=>{out+=r.value;});", &mut Empty).unwrap();
        assert_eq!(vm.execute("out", &mut Empty).unwrap(), Value::string("12"));
        assert_eq!(vm.execute("count", &mut Empty).unwrap(), Value::Number(1.));
    }
    #[test]
    fn resource_abort_cancels_only_affected_activation_without_finally() {
        let mut vm = Runtime::default();
        let error = vm.execute("let gate=Promise.withResolvers();let saved;let cleanup=0;async function* other(){await gate.promise;yield 5;}let keep=other();keep.next();async function* bad(){try{yield {then:function(resolve){saved=resolve;while(true){}}};}finally{cleanup++;}}let broken=bad();broken.next();", &mut Empty).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        let owner = match vm.global_value("broken").unwrap() {
            Value::Object(id) => id,
            _ => unreachable!(),
        };
        assert_eq!(vm.async_generators.records[&owner].state, State::Completed);
        assert!(vm.async_generators.records[&owner].queue.is_empty());
        assert_eq!(vm.async_generators.requests.len(), 1);
        assert_eq!(vm.global_value("cleanup"), Some(Value::Number(0.)));
    }
    #[test]
    fn fatal_in_inherited_result_then_cancels_owner_and_saved_resolver() {
        let mut vm = Runtime::default();
        let error = vm.execute("let saved;let cleanup=0;let observed=0;let gate=Promise.withResolvers();async function* other(){await gate.promise;yield 5;}let keep=other();keep.next();async function* f(){try{yield 1;yield 2;}finally{cleanup++;}}let g=f();Object.defineProperty(Object.prototype,'then',{configurable:true,get:function(){if(this.done===false&&this.value===1)return function(resolve){saved=resolve;while(true){}};return undefined;}});let first=g.next();first.then(r=>{observed++;});g.next();", &mut Empty).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        let Value::Object(owner) = vm.global_value("g").unwrap() else {
            unreachable!()
        };
        assert_eq!(vm.async_generators.records[&owner].state, State::Completed);
        assert!(!vm.resumable_exists(owner));
        assert!(vm.async_generators.settling_requests.is_empty());
        assert_eq!(vm.async_generators.requests.len(), 1);
        assert_eq!(vm.global_value("cleanup"), Some(Value::Number(0.)));
        // Only this regression harness replenishes work to probe the disabled resolver.
        vm.remaining = vm.limits.instructions;
        vm.execute("saved(99);", &mut Empty).unwrap();
        assert_eq!(vm.global_value("observed"), Some(Value::Number(0.)));
        assert_eq!(
            vm.promise_status(&vm.global_value("first").unwrap()),
            Some(crate::PromiseInspection::Pending)
        );
    }
    #[test]
    fn fatal_after_result_resolution_still_cancels_captured_owner() {
        let mut vm = Runtime::default();
        let error = vm.execute("let saved;let cleanup=0;async function* f(){try{yield 1;yield 2;}finally{cleanup++;}}let g=f();Object.defineProperty(Object.prototype,'then',{get:function(){if(this.done===false&&this.value===1)return function(resolve){saved=resolve;resolve(8);while(true){}};return undefined;}});let first=g.next();g.next();", &mut Empty).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        let Value::Object(owner) = vm.global_value("g").unwrap() else {
            unreachable!()
        };
        assert_eq!(vm.async_generators.records[&owner].state, State::Completed);
        assert!(vm.async_generators.records[&owner].queue.is_empty());
        assert!(vm.async_generators.settling_requests.is_empty());
        assert_eq!(vm.global_value("cleanup"), Some(Value::Number(0.)));
        assert_eq!(
            vm.promise_status(&vm.global_value("first").unwrap()),
            Some(crate::PromiseInspection::Fulfilled(Value::Number(8.)))
        );
    }
    #[test]
    fn result_assimilation_does_not_block_fifo_or_normal_completed_return() {
        let mut vm = Runtime::default();
        vm.execute("let saved;let observed=0;let finished=0;async function* f(){yield 1;}let g=f();Object.defineProperty(Object.prototype,'then',{get:function(){if(this.done===false&&this.value===1)return function(resolve){saved=resolve;};return undefined;}});let first=g.next();first.then(v=>{observed=v;});g.next().then(r=>{if(r.done)finished++;});g.return(4).then(r=>{if(r.done)finished++;});g.throw(7).catch(e=>{if(e===7)finished++;});", &mut Empty).unwrap();
        assert_eq!(vm.global_value("finished"), Some(Value::Number(3.)));
        assert_eq!(vm.async_generators.settling_requests.len(), 1);
        vm.execute("g=null;", &mut Empty).unwrap();
        vm.collect_garbage(&[], &Empty).unwrap();
        assert_eq!(vm.async_generators.records.len(), 1);
        vm.execute("saved(9);", &mut Empty).unwrap();
        assert_eq!(vm.global_value("observed"), Some(Value::Number(9.)));
        assert!(vm.async_generators.settling_requests.is_empty());
        vm.execute("saved=null;first=null;", &mut Empty).unwrap();
        vm.collect_garbage(&[], &Empty).unwrap();
        assert!(vm.async_generators.records.is_empty());
    }
}
