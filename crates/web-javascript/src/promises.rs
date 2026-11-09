//! Original bounded Promise state machine and FIFO microtask queue.
use crate::{ErrorKind, Heap, Host, JsError, Runtime, Value};
use std::collections::{BTreeMap, VecDeque};

const MAX_PROMISES: usize = 512;
const MAX_JOBS: usize = 1024;
#[cfg(test)]
#[path = "promises/property_tests.rs"]
mod property_tests;

// A shared, metadata-only inventory keeps `in` and property reads consistent.
// Looking up existence must not allocate the lazy prototype or invoke callbacks.
pub(super) enum ConstructorProperty {
    Prototype,
    Method(&'static str),
}
pub(super) fn constructor_property(name: &str) -> Option<ConstructorProperty> {
    Some(match name {
        "prototype" => ConstructorProperty::Prototype,
        "resolve" => ConstructorProperty::Method("Promise.resolve"),
        "reject" => ConstructorProperty::Method("Promise.reject"),
        "withResolvers" => ConstructorProperty::Method("Promise.withResolvers"),
        "all" => ConstructorProperty::Method("Promise.all"),
        "race" => ConstructorProperty::Method("Promise.race"),
        "allSettled" => ConstructorProperty::Method("Promise.allSettled"),
        "any" => ConstructorProperty::Method("Promise.any"),
        _ => return None,
    })
}
#[derive(Clone, Debug)]
enum Status {
    Pending,
    Fulfilled(Value),
    Rejected(Value),
}
#[derive(Clone, Debug, PartialEq)]
pub enum PromiseInspection {
    Pending,
    Fulfilled(Value),
    Rejected(Value),
}
#[derive(Clone, Copy, Debug)]
enum GroupKind {
    All,
    Race,
    AllSettled,
    Any,
}
#[derive(Clone, Debug)]
struct Group {
    promise: usize,
    kind: GroupKind,
    values: Vec<Value>,
    seen: Vec<bool>,
    remaining: usize,
    completed: bool,
}
#[derive(Clone, Debug)]
struct GroupCallback {
    group: usize,
    index: usize,
}
#[derive(Clone, Debug)]
enum Reaction {
    AsyncGenerator {
        owner: usize,
        phase: crate::async_generators::AwaitPhase,
    },
    AsyncFromSync {
        owner: usize,
        child: usize,
        input: usize,
        iterator: crate::language::IteratorRecord,
        done: bool,
        close_on_rejection: bool,
    },
    Then {
        child: usize,
        fulfilled: Value,
        rejected: Value,
    },
    // Internal continuation identity, never exposed as a JS callable.
    Await {
        owner: usize,
    },
}
impl Reaction {
    fn trace(&self, roots: &mut Vec<Value>) {
        match self {
            Self::Then {
                child,
                fulfilled,
                rejected,
            } => roots.extend([Value::Object(*child), fulfilled.clone(), rejected.clone()]),
            Self::Await { owner } | Self::AsyncGenerator { owner, .. } => {
                roots.push(Value::Object(*owner))
            }
            Self::AsyncFromSync {
                owner,
                child,
                input,
                iterator,
                ..
            } => {
                roots.extend([
                    Value::Object(*owner),
                    Value::Object(*child),
                    Value::Object(*input),
                ]);
                iterator.trace_values(roots);
            }
        }
    }
}
#[derive(Clone, Debug)]
struct Record {
    status: Status,
    reactions: Vec<Reaction>,
}
#[derive(Clone, Debug)]
struct Resolver {
    promise: usize,
    done: bool,
}
#[derive(Clone, Debug)]
enum Job {
    Reaction(Reaction, bool, Value),
    Assimilate(usize, Value, Value),
    Microtask(Value),
    ImportModule {
        promise: usize,
        id: String,
    },
    CompleteImport {
        promise: usize,
        result: Result<Value, JsError>,
    },
}
#[derive(Clone, Debug, Default)]
pub(super) struct PromiseState {
    records: BTreeMap<usize, Record>,
    async_auxiliaries: BTreeMap<usize, usize>,
    resolvers: BTreeMap<usize, Resolver>,
    jobs: VecDeque<Job>,
    prototype: Option<usize>,
    draining: bool,
    groups: BTreeMap<usize, Group>,
    group_callbacks: BTreeMap<usize, GroupCallback>,
}
pub(super) fn global(name: &str) -> Option<Value> {
    match name {
        "Promise" => Some(Value::Native("Promise")),
        "AggregateError" => Some(Value::Native("AggregateError")),
        "queueMicrotask" => Some(Value::Native("queueMicrotask")),
        _ => None,
    }
}
impl Runtime {
    fn promise_prototype(&mut self) -> Result<Value, JsError> {
        if let Some(id) = self.promises.prototype {
            return Ok(Value::Object(id));
        }
        let prototype = self.object(
            BTreeMap::from([
                ("constructor".into(), Value::Native("Promise")),
                ("then".into(), Value::Native("Promise.then")),
                ("catch".into(), Value::Native("Promise.catch")),
                ("finally".into(), Value::Native("Promise.finally")),
            ]),
            None,
        )?;
        self.hide_properties(&prototype, &["constructor", "then", "catch", "finally"]);
        if let Value::Object(id) = prototype {
            self.promises.prototype = Some(id);
        }
        Ok(prototype)
    }
    fn new_promise(&mut self) -> Result<usize, JsError> {
        if self.promises.records.len() >= MAX_PROMISES {
            return Err(JsError::resource("Promise retention limit exceeded"));
        }
        let prototype = self.promise_prototype()?;
        let Value::Object(id) = self.object(BTreeMap::new(), Some(prototype))? else {
            unreachable!()
        };
        self.promises.records.insert(
            id,
            Record {
                status: Status::Pending,
                reactions: vec![],
            },
        );
        Ok(id)
    }
    pub(super) fn enqueue_module_import(
        &mut self,
        promise: usize,
        id: String,
    ) -> Result<(), JsError> {
        self.promise_job(Job::ImportModule { promise, id })
    }
    fn promise_job(&mut self, job: Job) -> Result<(), JsError> {
        if self.promises.jobs.len() >= MAX_JOBS {
            return Err(JsError::resource("Promise microtask queue limit exceeded"));
        }
        self.tick()?;
        self.promises.jobs.push_back(job);
        Ok(())
    }
    pub(super) fn settle_promise(
        &mut self,
        id: usize,
        rejected: bool,
        value: Value,
    ) -> Result<(), JsError> {
        let record = self
            .promises
            .records
            .get(&id)
            .ok_or_else(|| JsError::runtime("Invalid Promise receiver"))?;
        if !matches!(record.status, Status::Pending) {
            return Ok(());
        }
        if record
            .reactions
            .len()
            .saturating_add(self.promises.jobs.len())
            > MAX_JOBS
        {
            return Err(JsError::resource("Promise reaction budget exceeded"));
        }
        let count = record.reactions.len();
        // Reserve all work before changing state, so exhaustion cannot enqueue
        // only a prefix of the reactions and lose async continuations.
        for _ in 0..count {
            self.tick()?;
        }
        let record = self.promises.records.get_mut(&id).unwrap();
        record.status = if rejected {
            Status::Rejected(value.clone())
        } else {
            Status::Fulfilled(value.clone())
        };
        let reactions = std::mem::take(&mut record.reactions);
        self.finish_async_generator_settlement(id);
        for reaction in reactions {
            self.promises
                .jobs
                .push_back(Job::Reaction(reaction, rejected, value.clone()));
        }
        Ok(())
    }
    /// Creates the intrinsic result promise for an internal async activation.
    pub(super) fn new_async_promise(&mut self) -> Result<Value, JsError> {
        self.new_promise().map(Value::Object)
    }
    pub(super) fn settle_async_promise(
        &mut self,
        owner: usize,
        result: Result<Value, JsError>,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        self.cancel_async_await(owner);
        match result {
            Ok(value) => self.promise_resolve_value(owner, value, host),
            Err(error) if error.kind == ErrorKind::Resource => Err(error),
            Err(error) => {
                let reason = self.exception_value(&error)?;
                self.settle_promise(owner, true, reason)
            }
        }
    }
    /// PromiseResolve(%Promise%, value), without consulting Promise.resolve.
    /// Even a primitive or settled promise resumes through the FIFO job queue.
    pub(super) fn await_async(
        &mut self,
        owner: usize,
        value: Value,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        if !self.promises.records.contains_key(&owner) {
            return Err(JsError::runtime("Invalid async result promise"));
        }
        let id = self.intrinsic_iteration_promise(value, Some(owner), host)?;
        self.attach_promise_reaction(id, Reaction::Await { owner })
    }
    pub(super) fn release_async_generator_auxiliaries(&mut self, owner: usize) {
        let records = &self.promises.records;
        self.promises.async_auxiliaries.retain(|id, target| {
            *target != owner
                || records
                    .get(id)
                    .is_some_and(|record| matches!(record.status, Status::Pending))
        });
    }
    pub(super) fn cancel_async_generator_request(&mut self, promise: usize) {
        if let Some(record) = self.promises.records.get_mut(&promise)
            && matches!(record.status, Status::Pending)
        {
            record.reactions.clear();
        }
        for resolver in self.promises.resolvers.values_mut() {
            if resolver.promise == promise {
                resolver.done = true;
            }
        }
        self.promises
            .jobs
            .retain(|job| !matches!(job, Job::Assimilate(id, _, _) if *id == promise));
    }
    pub(super) fn await_async_generator(
        &mut self,
        owner: usize,
        phase: crate::async_generators::AwaitPhase,
        value: Value,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        let root = self.retain_value(value.clone())?;
        let result = (|| {
            let id = self.intrinsic_iteration_promise(value, Some(owner), host)?;
            self.set_async_generator_await(owner, id);
            self.attach_promise_reaction(id, Reaction::AsyncGenerator { owner, phase })
        })();
        self.release_value(root);
        result
    }
    fn intrinsic_iteration_promise(
        &mut self,
        value: Value,
        owner: Option<usize>,
        host: &mut impl Host,
    ) -> Result<usize, JsError> {
        let intrinsic = if let Value::Object(id) = &value {
            self.promises.records.contains_key(id)
                && self.get(value.clone(), "constructor", host)? == Value::Native("Promise")
        } else {
            false
        };
        let id = if intrinsic {
            let Value::Object(id) = value else {
                unreachable!()
            };
            id
        } else {
            let id = self.new_promise()?;
            if let Some(owner) = owner {
                self.promises.async_auxiliaries.insert(id, owner);
            }
            self.promise_resolve_value(id, value, host)?;
            id
        };
        Ok(id)
    }
    pub(super) fn adapt_sync_iteration(
        &mut self,
        child: usize,
        owner: usize,
        mut iterator: crate::language::IteratorRecord,
        result: Result<(Value, bool), JsError>,
        close_on_rejection: bool,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        let (value, done) = match result {
            Ok(result) => result,
            Err(error) if error.kind == ErrorKind::Resource => return Err(error),
            Err(error) => {
                let reason = self.exception_value(&error)?;
                return self.settle_promise(child, true, reason);
            }
        };
        self.promises.async_auxiliaries.insert(child, owner);
        let input = match self.intrinsic_iteration_promise(value, Some(owner), host) {
            Ok(input) => input,
            Err(error) if error.kind == ErrorKind::Resource => return Err(error),
            Err(error) => {
                if !done && close_on_rejection {
                    self.close_rejected_sync_iteration(&mut iterator, host)?;
                }
                let reason = self.exception_value(&error)?;
                self.promises.async_auxiliaries.remove(&child);
                return self.settle_promise(child, true, reason);
            }
        };
        self.attach_promise_reaction(
            input,
            Reaction::AsyncFromSync {
                owner,
                child,
                input,
                iterator,
                done,
                close_on_rejection,
            },
        )
    }
    pub(super) fn resolve_empty_sync_return(
        &mut self,
        child: usize,
        owner: usize,
        value: Value,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        self.promises.async_auxiliaries.insert(child, owner);
        // Missing sync return does not run AsyncFromSyncIteratorContinuation:
        // preserve the supplied value inside the result without awaiting it.
        let result = self.adapted_iteration_result(value, true)?;
        self.promise_resolve_value(child, result, host)
    }
    pub(super) fn adapted_iteration_result(
        &mut self,
        value: Value,
        done: bool,
    ) -> Result<Value, JsError> {
        let prototype = self.object_prototype_value()?;
        let result = self.object(
            BTreeMap::from([("value".into(), value), ("done".into(), Value::Bool(done))]),
            Some(prototype),
        )?;
        if let Value::Object(id) = result {
            self.remember_property_order(id, vec!["value".into(), "done".into()]);
        }
        Ok(result)
    }
    fn finish_sync_iteration(
        &mut self,
        reaction: Reaction,
        rejected: bool,
        value: Value,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        let mut values = vec![value.clone()];
        reaction.trace(&mut values);
        let mut roots = Vec::new();
        for value in values {
            match self.retain_value(value) {
                Ok(root) => roots.push(root),
                Err(error) => {
                    for root in roots {
                        self.release_value(root);
                    }
                    return Err(error);
                }
            }
        }
        let Reaction::AsyncFromSync {
            child,
            input,
            mut iterator,
            done,
            close_on_rejection,
            ..
        } = reaction
        else {
            unreachable!()
        };
        let result = (|| {
            if rejected {
                if !done && close_on_rejection {
                    self.close_rejected_sync_iteration(&mut iterator, host)?;
                }
                self.settle_promise(child, true, value)
            } else {
                let result = self.adapted_iteration_result(value, done)?;
                self.promise_resolve_value(child, result, host)
            }
        })();
        self.promises.async_auxiliaries.remove(&input);
        for root in roots {
            self.release_value(root);
        }
        result
    }
    /// Releases continuation edges when an activation aborts fatally.
    pub(super) fn cancel_async_await(&mut self, owner: usize) {
        let owned: std::collections::BTreeSet<usize> = self
            .promises
            .async_auxiliaries
            .iter()
            .filter_map(|(id, target)| (*target == owner).then_some(*id))
            .collect();
        self.promises
            .async_auxiliaries
            .retain(|_, target| *target != owner);
        for resolver in self.promises.resolvers.values_mut() {
            if owned.contains(&resolver.promise) {
                resolver.done = true;
            }
        }
        for record in self.promises.records.values_mut() {
            record.reactions.retain(|reaction| {
                !matches!(reaction, Reaction::Await { owner: target } | Reaction::AsyncGenerator { owner: target, .. } | Reaction::AsyncFromSync { owner: target, .. } if *target == owner)
            });
        }
        self.promises.jobs.retain(|job| {
            !matches!(job, Job::Reaction(Reaction::Await { owner: target } | Reaction::AsyncGenerator { owner: target, .. } | Reaction::AsyncFromSync { owner: target, .. }, _, _) if *target == owner)
                && !matches!(job, Job::Assimilate(id, _, _) if owned.contains(id))
        });
    }
    fn promise_resolvers(
        &mut self,
        id: usize,
        host: &mut impl Host,
    ) -> Result<(Value, Value), JsError> {
        let context = self.object(BTreeMap::new(), None)?;
        let Value::Object(owner) = context else {
            unreachable!()
        };
        self.promises.resolvers.insert(
            owner,
            Resolver {
                promise: id,
                done: false,
            },
        );
        let resolve = self.bind_function(
            Value::Native("Promise._resolve"),
            context.clone(),
            vec![],
            host,
        )?;
        let reject = self.bind_function(Value::Native("Promise._reject"), context, vec![], host)?;
        Ok((resolve, reject))
    }
    pub(super) fn promise_resolve_value(
        &mut self,
        id: usize,
        value: Value,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        if value == Value::Object(id) {
            let error = self.native(
                "TypeError",
                &[Value::string("Promise cannot resolve itself")],
            )?;
            return self.settle_promise(id, true, error);
        }
        if matches!(
            value,
            Value::Object(_)
                | Value::Function(_)
                | Value::Array(_)
                | Value::Host(_)
                | Value::Native(_)
        ) {
            let then = match self.get(value.clone(), "then", host) {
                Ok(value) => value,
                Err(error) if error.kind == ErrorKind::Resource => return Err(error),
                Err(error) => {
                    let value = self.exception_value(&error)?;
                    return self.settle_promise(id, true, value);
                }
            };
            if self.is_callable_value(&then, host) {
                return self.promise_job(Job::Assimilate(id, value, then));
            }
        }
        self.settle_promise(id, false, value)
    }
    fn attach_promise(
        &mut self,
        id: usize,
        fulfilled: Value,
        rejected: Value,
    ) -> Result<Value, JsError> {
        if !self.promises.records.contains_key(&id) {
            return Err(self.property_type_error("Promise.then requires a Promise receiver"));
        }
        let child = self.new_promise()?;
        let reaction = Reaction::Then {
            child,
            fulfilled,
            rejected,
        };
        self.attach_promise_reaction(id, reaction)?;
        Ok(Value::Object(child))
    }
    fn attach_promise_reaction(&mut self, id: usize, reaction: Reaction) -> Result<(), JsError> {
        let record = self
            .promises
            .records
            .get_mut(&id)
            .ok_or_else(|| JsError::runtime("Invalid Promise receiver"))?;
        match record.status.clone() {
            Status::Pending => {
                if record.reactions.len() >= MAX_JOBS {
                    return Err(JsError::resource("Promise listener limit exceeded"));
                }
                record.reactions.push(reaction);
            }
            Status::Fulfilled(value) => self.promise_job(Job::Reaction(reaction, false, value))?,
            Status::Rejected(value) => self.promise_job(Job::Reaction(reaction, true, value))?,
        }
        Ok(())
    }
    pub(super) fn promise_get(
        &mut self,
        object: &Value,
        property: &str,
    ) -> Result<Option<Value>, JsError> {
        if *object != Value::Native("Promise") {
            return Ok(None);
        }
        Ok(Some(match constructor_property(property) {
            Some(ConstructorProperty::Prototype) => self.promise_prototype()?,
            Some(ConstructorProperty::Method(name)) => Value::Native(name),
            None => return Ok(None),
        }))
    }
    pub(super) fn promise_call(
        &mut self,
        name: &str,
        args: &[Value],
        this: Value,
        construct: bool,
        host: &mut impl Host,
    ) -> Result<Option<Value>, JsError> {
        if name == "queueMicrotask" {
            if construct {
                return Err(self.property_type_error("queueMicrotask is not a constructor"));
            }
            let callback = args.first().cloned().unwrap_or(Value::Undefined);
            if !self.is_callable_value(&callback, host) {
                return Err(self.property_type_error("queueMicrotask requires a callable"));
            }
            self.promise_job(Job::Microtask(callback))?;
            return Ok(Some(Value::Undefined));
        }
        if !name.starts_with("Promise") && name != "AggregateError" {
            return Ok(None);
        }
        if construct && !matches!(name, "Promise" | "AggregateError") {
            return Err(self.property_type_error("Promise method is not a constructor"));
        }
        if matches!(
            name,
            "Promise.resolve" | "Promise.reject" | "Promise.withResolvers"
        ) && this != Value::Native("Promise")
        {
            return Err(self.property_type_error(
                "Promise static method requires the supported Promise constructor",
            ));
        }
        let first = args.first().cloned().unwrap_or(Value::Undefined);
        let result = match name {
            "AggregateError" => {
                let errors = self.iterable_values(first, host)?;
                let message = if args
                    .get(1)
                    .is_none_or(|value| matches!(value, Value::Undefined))
                {
                    String::new()
                } else {
                    self.text(&args[1], 0)?
                };
                self.aggregate_error(errors, &message)?
            }
            "Promise.all" | "Promise.race" | "Promise.allSettled" | "Promise.any" => {
                if construct {
                    return Err(
                        self.property_type_error("Promise static method is not a constructor")
                    );
                }
                if this != Value::Native("Promise") {
                    return Err(self.property_type_error(
                        "Promise combinator requires the supported Promise constructor",
                    ));
                }
                self.promise_group(name, first, host)?
            }
            "Promise._groupYes" | "Promise._groupNo" => {
                if construct {
                    return Err(
                        self.property_type_error("Promise group callback is not a constructor")
                    );
                }
                let Value::Object(owner) = this else {
                    return Err(self.property_type_error("Invalid Promise group callback"));
                };
                self.promise_group_result(owner, name == "Promise._groupNo", first, host)?;
                Value::Undefined
            }
            "Promise" => {
                if !construct {
                    return Err(self.property_type_error("Promise constructor requires new"));
                }
                if !self.is_callable_value(&first, host) {
                    return Err(self.property_type_error("Promise executor is not callable"));
                }
                let id = self.new_promise()?;
                let (resolve, reject) = self.promise_resolvers(id, host)?;
                if let Err(error) =
                    self.call(first, vec![resolve, reject.clone()], Value::Undefined, host)
                {
                    if error.kind == ErrorKind::Resource {
                        return Err(error);
                    }
                    let value = self.exception_value(&error)?;
                    self.call(reject, vec![value], Value::Undefined, host)?;
                }
                Value::Object(id)
            }
            "Promise._resolve" | "Promise._reject" => {
                let Value::Object(owner) = this else {
                    return Err(self.property_type_error("Invalid Promise resolver"));
                };
                let context = self
                    .promises
                    .resolvers
                    .get_mut(&owner)
                    .ok_or_else(|| JsError::runtime("Invalid Promise resolver"))?;
                if !context.done {
                    context.done = true;
                    let id = context.promise;
                    // A thenable can retain this resolver beyond its assimilation
                    // job. Capture ownership before resolution can remove it.
                    let activation = self
                        .promises
                        .async_auxiliaries
                        .get(&id)
                        .copied()
                        .or_else(|| self.settling_async_generator_owner(id));
                    let resolved = if name == "Promise._reject" {
                        self.settle_promise(id, true, first)
                    } else {
                        self.promise_resolve_value(id, first, host)
                    };
                    if let Err(error) = resolved {
                        if error.kind == ErrorKind::Resource
                            && let Some(activation) = activation
                        {
                            self.cancel_async_generator(activation);
                        }
                        return Err(error);
                    }
                }
                Value::Undefined
            }
            "Promise.resolve" => {
                if let Value::Object(id) = first
                    && self.promises.records.contains_key(&id)
                    && self.get(Value::Object(id), "constructor", host)? == this
                {
                    return Ok(Some(Value::Object(id)));
                }
                let id = self.new_promise()?;
                self.promise_resolve_value(id, first, host)?;
                Value::Object(id)
            }
            "Promise.reject" => {
                let id = self.new_promise()?;
                self.settle_promise(id, true, first)?;
                Value::Object(id)
            }
            "Promise.withResolvers" => {
                let id = self.new_promise()?;
                let (resolve, reject) = self.promise_resolvers(id, host)?;
                self.object(
                    BTreeMap::from([
                        ("promise".into(), Value::Object(id)),
                        ("resolve".into(), resolve),
                        ("reject".into(), reject),
                    ]),
                    None,
                )?
            }
            "Promise.then" | "Promise.catch" => {
                let Value::Object(id) = this else {
                    return Err(
                        self.property_type_error("Promise method requires a Promise receiver")
                    );
                };
                let (yes, no) = if name == "Promise.catch" {
                    (Value::Undefined, first)
                } else {
                    (first, args.get(1).cloned().unwrap_or(Value::Undefined))
                };
                self.attach_promise(id, yes, no)?
            }
            "Promise.finally" => {
                let Value::Object(id) = this else {
                    return Err(
                        self.property_type_error("Promise.finally requires a Promise receiver")
                    );
                };
                if !self.is_callable_value(&first, host) {
                    self.attach_promise(id, first.clone(), first)?
                } else {
                    let callback =
                        self.object(BTreeMap::from([("callback".into(), first)]), None)?;
                    let yes = self.bind_function(
                        Value::Native("Promise._finallyYes"),
                        callback.clone(),
                        vec![],
                        host,
                    )?;
                    let no = self.bind_function(
                        Value::Native("Promise._finallyNo"),
                        callback,
                        vec![],
                        host,
                    )?;
                    self.attach_promise(id, yes, no)?
                }
            }
            "Promise._finallyYes" | "Promise._finallyNo" => {
                let callback = self.get(this, "callback", host)?;
                let returned = self.call(callback, vec![], Value::Undefined, host)?;
                let resolved = self
                    .promise_call(
                        "Promise.resolve",
                        &[returned],
                        Value::Native("Promise"),
                        false,
                        host,
                    )?
                    .unwrap();
                let context = self.object(BTreeMap::from([("value".into(), first)]), None)?;
                let forward = self.bind_function(
                    Value::Native(if name == "Promise._finallyNo" {
                        "Promise._throw"
                    } else {
                        "Promise._return"
                    }),
                    context,
                    vec![],
                    host,
                )?;
                let Value::Object(id) = resolved else {
                    unreachable!()
                };
                self.attach_promise(id, forward, Value::Undefined)?
            }
            "Promise._return" | "Promise._throw" => {
                let value = self.get(this, "value", host)?;
                if name == "Promise._throw" {
                    let mut error = JsError::runtime("Promise rejection");
                    error.thrown = Some(value);
                    return Err(error);
                }
                value
            }
            _ => return Ok(None),
        };
        Ok(Some(result))
    }
    pub(super) fn promise_jobs_are_draining(&self) -> bool {
        self.promises.draining
    }
    #[cfg(test)]
    pub(super) fn drain_promise_jobs(&mut self, host: &mut impl Host) -> Result<(), JsError> {
        self.drain_promise_jobs_with_budget(host, &mut 0)
    }
    /// Share one work allowance across cleanup jobs in the same checkpoint.
    pub(super) fn drain_promise_jobs_with_budget(
        &mut self,
        host: &mut impl Host,
        count: &mut usize,
    ) -> Result<(), JsError> {
        if self.promises.draining || self.call_depth != 0 {
            return Ok(());
        }
        self.promises.draining = true;
        let result = (|| {
            while !self.promises.jobs.is_empty() {
                self.tick()?;
                *count = count.saturating_add(1);
                if *count > MAX_JOBS {
                    return Err(JsError::resource(
                        "Microtask checkpoint work limit exceeded",
                    ));
                }
                // Do not discard a continuation if the checkpoint budget is
                // exhausted before it can run.
                let job = self.promises.jobs.pop_front().unwrap();
                match job {
                    Job::Reaction(Reaction::AsyncGenerator { owner, phase }, rejected, value) => {
                        if let Err(error) =
                            self.resume_async_generator(owner, phase, rejected, value, host)
                        {
                            self.cancel_async_generator(owner);
                            return Err(error);
                        }
                    }
                    Job::Reaction(
                        reaction @ Reaction::AsyncFromSync { owner, .. },
                        rejected,
                        value,
                    ) => {
                        if let Err(error) =
                            self.finish_sync_iteration(reaction, rejected, value, host)
                        {
                            self.cancel_async_generator(owner);
                            return Err(error);
                        }
                    }
                    Job::Reaction(Reaction::Await { owner }, rejected, value) => {
                        if let Err(error) = self.resume_async(owner, !rejected, value, host) {
                            self.cancel_async_await(owner);
                            return Err(error);
                        }
                    }
                    Job::Reaction(
                        Reaction::Then {
                            child,
                            fulfilled,
                            rejected: on_rejected,
                        },
                        rejected,
                        value,
                    ) => {
                        let handler = if rejected { on_rejected } else { fulfilled };
                        if !self.is_callable_value(&handler, host) {
                            self.settle_promise(child, rejected, value)?;
                            continue;
                        }
                        match self.call(handler, vec![value], Value::Undefined, host) {
                            Ok(value) => self.promise_resolve_value(child, value, host)?,
                            Err(error) if error.kind == ErrorKind::Resource => return Err(error),
                            Err(error) => {
                                let value = self.exception_value(&error)?;
                                self.settle_promise(child, true, value)?;
                            }
                        }
                    }
                    Job::Assimilate(id, receiver, then) => {
                        let auxiliary_owner = self
                            .promises
                            .async_auxiliaries
                            .get(&id)
                            .copied()
                            .or_else(|| self.settling_async_generator_owner(id));
                        let mut roots = Vec::new();
                        let result = (|| {
                            let mut values =
                                vec![Value::Object(id), receiver.clone(), then.clone()];
                            values.extend(auxiliary_owner.map(Value::Object));
                            for value in values {
                                roots.push(self.retain_value(value)?);
                            }
                            let (resolve, reject) = self.promise_resolvers(id, host)?;
                            if let Err(error) =
                                self.call(then, vec![resolve, reject.clone()], receiver, host)
                            {
                                if error.kind == ErrorKind::Resource {
                                    return Err(error);
                                }
                                let value = self.exception_value(&error)?;
                                self.call(reject, vec![value], Value::Undefined, host)?;
                            }
                            Ok(())
                        })();
                        for root in roots {
                            self.release_value(root);
                        }
                        if let Err(error) = result {
                            if let Some(owner) = auxiliary_owner {
                                self.cancel_async_generator(owner);
                            }
                            return Err(error);
                        }
                    }
                    Job::ImportModule { promise, id } => {
                        // Root the popped job until the evaluation continuation
                        // is safely in the same bounded FIFO queue.
                        let root = self.retain_value(Value::Object(promise))?;
                        let outcome = self.execute_module_staged(&id, host);
                        let queued = match outcome {
                            crate::modules::ModuleExecution::LinkFailed(error) => {
                                self.settle_async_promise(promise, Err(error), host)
                            }
                            crate::modules::ModuleExecution::Evaluated(Err(error))
                                if error.kind == ErrorKind::Resource =>
                            {
                                Err(error)
                            }
                            crate::modules::ModuleExecution::Evaluated(result) => {
                                self.promise_job(Job::CompleteImport { promise, result })
                            }
                        };
                        self.release_value(root);
                        queued?;
                    }
                    Job::CompleteImport { promise, result } => {
                        // ContinueDynamicImport's evaluation continuation is a
                        // distinct turn, including for an already evaluated module.
                        let root = self.retain_value(Value::Object(promise))?;
                        let value = match &result {
                            Ok(namespace) => Some(namespace.clone()),
                            Err(error) => error.thrown.clone(),
                        };
                        let value_root =
                            match value.map(|value| self.retain_value(value)).transpose() {
                                Ok(token) => token,
                                Err(error) => {
                                    self.release_value(root);
                                    return Err(error);
                                }
                            };
                        let settled = self.settle_async_promise(promise, result, host);
                        if let Some(token) = value_root {
                            self.release_value(token);
                        }
                        self.release_value(root);
                        settled?;
                    }
                    Job::Microtask(callback) => {
                        if let Err(error) = self.call(callback, vec![], Value::Undefined, host) {
                            if error.kind == ErrorKind::Resource {
                                return Err(error);
                            }
                            host.report_microtask_error(&error);
                        }
                    }
                }
            }
            Ok(())
        })();
        self.promises.draining = false;
        result
    }
    pub(super) fn promise_gc_cached_roots(&self) -> Vec<Value> {
        let mut roots = self
            .promises
            .prototype
            .map(Value::Object)
            .into_iter()
            .collect::<Vec<_>>();
        for job in &self.promises.jobs {
            match job {
                Job::Reaction(reaction, _, value) => {
                    reaction.trace(&mut roots);
                    roots.push(value.clone());
                }
                Job::Assimilate(id, receiver, then) => {
                    roots.extend([Value::Object(*id), receiver.clone(), then.clone()])
                }
                Job::Microtask(value) => roots.push(value.clone()),
                Job::ImportModule { promise, .. } => roots.push(Value::Object(*promise)),
                Job::CompleteImport { promise, result } => {
                    roots.push(Value::Object(*promise));
                    match result {
                        Ok(namespace) => roots.push(namespace.clone()),
                        Err(error) => roots.extend(error.thrown.iter().cloned()),
                    }
                }
            }
        }
        roots
    }
    pub(super) fn promise_gc_trace(&self, id: usize) -> Vec<Value> {
        let mut roots = vec![];
        roots.extend(
            self.promises
                .async_auxiliaries
                .iter()
                .filter_map(|(promise, owner)| (*owner == id).then_some(Value::Object(*promise))),
        );
        if let Some(record) = self.promises.records.get(&id) {
            match &record.status {
                Status::Fulfilled(value) | Status::Rejected(value) => roots.push(value.clone()),
                _ => {}
            }
            for reaction in &record.reactions {
                reaction.trace(&mut roots);
            }
        }
        if let Some(resolver) = self.promises.resolvers.get(&id) {
            roots.push(Value::Object(resolver.promise));
        }
        if let Some(group) = self.promises.groups.get(&id) {
            roots.push(Value::Object(group.promise));
            roots.extend(group.values.iter().cloned());
        }
        if let Some(callback) = self.promises.group_callbacks.get(&id) {
            roots.push(Value::Object(callback.group));
        }
        roots
    }
    pub(super) fn promise_gc_sweep(&mut self, live: &[bool]) {
        self.promises
            .async_auxiliaries
            .retain(|id, owner| live.get(*id) == Some(&true) && live.get(*owner) == Some(&true));
        self.promises
            .records
            .retain(|id, _| live.get(*id) == Some(&true));
        self.promises
            .resolvers
            .retain(|id, _| live.get(*id) == Some(&true));
        self.promises
            .groups
            .retain(|id, _| live.get(*id) == Some(&true));
        self.promises
            .group_callbacks
            .retain(|id, _| live.get(*id) == Some(&true));
    }

    /// Observes a promise without running callbacks or granting host authority.
    pub fn promise_status(&self, value: &Value) -> Option<PromiseInspection> {
        let Value::Object(id) = value else {
            return None;
        };
        self.promises
            .records
            .get(id)
            .map(|record| match &record.status {
                Status::Pending => PromiseInspection::Pending,
                Status::Fulfilled(value) => PromiseInspection::Fulfilled(value.clone()),
                Status::Rejected(value) => PromiseInspection::Rejected(value.clone()),
            })
    }
    fn aggregate_error(&mut self, errors: Vec<Value>, message: &str) -> Result<Value, JsError> {
        let errors = Value::Array(self.allocate(Heap::Array(errors))?);
        let name = self.string("AggregateError".into())?;
        let message = self.string(message.to_owned())?;
        let error = self.object(
            BTreeMap::from([
                ("name".into(), name),
                ("message".into(), message),
                ("errors".into(), errors),
            ]),
            None,
        )?;
        self.hide_properties(&error, &["name", "message", "errors"]);
        Ok(error)
    }
    fn promise_group(
        &mut self,
        name: &str,
        input: Value,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        if self.promises.groups.len() >= MAX_PROMISES {
            return Err(JsError::resource("Promise group retention limit exceeded"));
        }
        let promise = self.new_promise()?;
        let Value::Object(owner) = self.object(BTreeMap::new(), None)? else {
            unreachable!()
        };
        self.promises.groups.insert(
            owner,
            Group {
                promise,
                kind: match name {
                    "Promise.all" => GroupKind::All,
                    "Promise.race" => GroupKind::Race,
                    "Promise.allSettled" => GroupKind::AllSettled,
                    _ => GroupKind::Any,
                },
                values: vec![],
                seen: vec![],
                remaining: 1,
                completed: false,
            },
        );
        let result = (|| {
            let resolve = self.get(Value::Native("Promise"), "resolve", host)?;
            if !self.is_callable_value(&resolve, host) {
                return Err(self.property_type_error("Promise.resolve must be callable"));
            }
            let mut iterator = self.iterator(input, host)?;
            let result = (|| {
                while let Some(value) = self.iterator_next(&mut iterator, host)? {
                    self.tick()?;
                    let index = self.promises.groups[&owner].values.len();
                    if index >= 4096 {
                        return Err(JsError::resource("Promise combinator item limit exceeded"));
                    }
                    self.charge_array_slots(1)?;
                    let group = self.promises.groups.get_mut(&owner).unwrap();
                    group.values.push(Value::Undefined);
                    group.seen.push(false);
                    group.remaining += 1;
                    let next =
                        self.call(resolve.clone(), vec![value], Value::Native("Promise"), host)?;
                    let Value::Object(context) = self.object(BTreeMap::new(), None)? else {
                        unreachable!()
                    };
                    self.promises.group_callbacks.insert(
                        context,
                        GroupCallback {
                            group: owner,
                            index,
                        },
                    );
                    let yes = self.bind_function(
                        Value::Native("Promise._groupYes"),
                        Value::Object(context),
                        vec![],
                        host,
                    )?;
                    let no = self.bind_function(
                        Value::Native("Promise._groupNo"),
                        Value::Object(context),
                        vec![],
                        host,
                    )?;
                    let then = self.get(next.clone(), "then", host)?;
                    self.call(then, vec![yes, no], next, host)?;
                }
                Ok(())
            })();
            if let Err(error) = result {
                if error.kind != ErrorKind::Resource {
                    let _ = self.iterator_close(&mut iterator, host);
                }
                return Err(error);
            }
            self.promises.groups.get_mut(&owner).unwrap().remaining -= 1;
            self.finish_promise_group(owner, host)
        })();
        if let Err(error) = result {
            if error.kind == ErrorKind::Resource {
                return Err(error);
            }
            if let Some(group) = self.promises.groups.get_mut(&owner) {
                group.completed = true;
            }
            let value = self.exception_value(&error)?;
            self.settle_promise(promise, true, value)?;
        }
        Ok(Value::Object(promise))
    }
    fn promise_group_result(
        &mut self,
        owner: usize,
        rejected: bool,
        value: Value,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        let callback = self
            .promises
            .group_callbacks
            .get(&owner)
            .cloned()
            .ok_or_else(|| JsError::runtime("Invalid Promise group context"))?;
        let group = self
            .promises
            .groups
            .get_mut(&callback.group)
            .ok_or_else(|| JsError::runtime("Invalid Promise group"))?;
        if group.completed || group.seen.get(callback.index).copied() != Some(false) {
            return Ok(());
        }
        group.seen[callback.index] = true;
        let kind = group.kind;
        let target = group.promise;
        if matches!(kind, GroupKind::Race)
            || matches!(kind, GroupKind::All) && rejected
            || matches!(kind, GroupKind::Any) && !rejected
        {
            group.completed = true;
            return if rejected {
                self.settle_promise(target, true, value)
            } else {
                self.promise_resolve_value(target, value, host)
            };
        }
        let result = if matches!(kind, GroupKind::AllSettled) {
            let status = self.string(if rejected { "rejected" } else { "fulfilled" }.into())?;
            self.object(
                BTreeMap::from([
                    ("status".into(), status),
                    (if rejected { "reason" } else { "value" }.into(), value),
                ]),
                None,
            )?
        } else {
            value
        };
        let group = self.promises.groups.get_mut(&callback.group).unwrap();
        group.values[callback.index] = result;
        group.remaining -= 1;
        self.finish_promise_group(callback.group, host)
    }
    fn finish_promise_group(&mut self, owner: usize, host: &mut impl Host) -> Result<(), JsError> {
        let group = self
            .promises
            .groups
            .get_mut(&owner)
            .ok_or_else(|| JsError::runtime("Invalid Promise group"))?;
        if group.completed || group.remaining != 0 || matches!(group.kind, GroupKind::Race) {
            return Ok(());
        }
        group.completed = true;
        let target = group.promise;
        let kind = group.kind;
        let values = std::mem::take(&mut group.values);
        if matches!(kind, GroupKind::Any) {
            let error = self.aggregate_error(values, "All promises were rejected")?;
            self.settle_promise(target, true, error)
        } else {
            let value = Value::Array(self.allocate(Heap::Array(values))?);
            self.promise_resolve_value(target, value, host)
        }
    }
}
