//! Bounded continuations for the deliberately restricted async/generator subset.
//! No suspended expression is re-evaluated when its continuation resumes.
use crate::async_iteration::{AsyncIteratorOperation, AsyncIteratorRecord};
use crate::exception_diagnostics::CatchDiagnostic;
use crate::language::IteratorRecord;
use crate::parser::{ClassDefinition, ForBinding, FunctionKind, Pattern};
use crate::prepared_call::PreparedCall;
use crate::{Declaration, ErrorKind, Expr, Flow, Host, JsError, Place, Runtime, Stmt, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

const LIMIT: usize = 256;

struct ForEachSource {
    binding: ForBinding,
    source: Expr,
    body: Box<Stmt>,
    of: bool,
}

#[derive(Clone, Debug, Default)]
pub(super) struct ResumableState {
    machines: BTreeMap<usize, Machine>,
    running: BTreeSet<usize>,
}
#[derive(Clone, Debug)]
pub(super) enum ResumeInput {
    Normal(Value),
    Throw(Value),
    Return(Value),
}
#[derive(Clone, Debug)]
pub(super) enum Outcome {
    Completed(Value),
    Await(Value),
    Yield(Value),
    /// A delegated done=false IteratorResult must not be unwrapped or copied.
    YieldResult(Value),
}
#[derive(Clone, Debug)]
struct Machine {
    owner: usize,
    scope: usize,
    strict: bool,
    kind: FunctionKind,
    body: Arc<Vec<Stmt>>,
    started: bool,
    frames: Vec<Frame>,
    pending: Option<(usize, Action)>,
    abrupt: Option<Completion>,
}
#[derive(Clone, Debug)]
enum Frame {
    Sequence(Arc<Vec<Stmt>>, usize, usize),
    Statement(Stmt, usize),
    Evaluate(Expr, usize, Action),
    Apply(Value, usize, Action),
    Variables(Declaration, Arc<Vec<(String, Option<Expr>)>>, usize, usize),
    Loop(Expr, Box<Stmt>, usize),
    ForStart(ForState, usize),
    ForTest(ForState, usize),
    ForNext(ForState, usize),
    ForEachReady(ForBinding, Box<Stmt>, bool, Value, usize),
    OfNext(OfState, usize),
    OfBody(OfState, usize),
    AsyncOfNext(AsyncOfState, usize),
    AsyncOfBody(AsyncOfState, usize),
    InNext(InState, usize),
    SwitchReady(Vec<(Option<Expr>, Vec<Stmt>)>, Value, usize),
    SwitchBoundary(usize),
    Handler {
        catch: Option<(Option<String>, Vec<Stmt>)>,
        diagnostic: Option<Box<CatchDiagnostic>>,
        finally: Option<Vec<Stmt>>,
        scope: usize,
    },
    Finally(Option<Completion>, usize),
}
#[derive(Clone, Debug)]
enum Action {
    Discard,
    Bind(Declaration, String),
    Return,
    Throw,
    Branch(Box<Stmt>, Option<Box<Stmt>>),
    LoopCondition(Expr, Box<Stmt>),
    ForCondition(ForState),
    ForEachSource(ForBinding, Box<Stmt>, bool, usize),
    AsyncForSource(ForBinding, Box<Stmt>, usize),
    AsyncOfResult(AsyncOfState),
    AsyncOfClose(AsyncOfState, Completion),
    Switch(Vec<(Option<Expr>, Vec<Stmt>)>),
    Await(Box<Action>),
    Yield(Box<Action>),
    // Fulfillment of the intrinsic await performed by async-generator yield.
    AsyncYieldReady(Box<Action>),
    DelegateStart(Box<Action>),
    DelegateResume {
        state: Box<DelegateState>,
        next: Box<Action>,
    },
    AsyncDelegateResume {
        state: Box<AsyncDelegateState>,
        next: Box<Action>,
    },
    AsyncDelegateResult {
        state: Box<AsyncDelegateState>,
        next: Box<Action>,
        returning: bool,
    },
    AsyncDelegateMissingThrowClose {
        state: Box<AsyncDelegateState>,
        next: Box<Action>,
    },
    AsyncDelegateReturnAbsent {
        state: Box<AsyncDelegateState>,
        next: Box<Action>,
    },
    ReferenceBase {
        key: Box<Expr>,
        operation: ReferenceOperation,
        next: Box<Action>,
    },
    ReferenceKey {
        base: Value,
        operation: ReferenceOperation,
        next: Box<Action>,
    },
    MemberBase {
        key: Box<Expr>,
        next: Box<Action>,
    },
    MemberKey {
        base: Value,
        next: Box<Action>,
    },
    CallMemberBase {
        key: Box<Expr>,
        arguments: Arc<Vec<Expr>>,
        next: Box<Action>,
    },
    CallMemberKey {
        base: Value,
        arguments: Arc<Vec<Expr>>,
        next: Box<Action>,
    },
    CallCallee {
        context: Option<String>,
        arguments: Arc<Vec<Expr>>,
        next: Box<Action>,
    },
    CallArgument {
        target: PreparedCall,
        arguments: Arc<Vec<Expr>>,
        index: usize,
        values: Vec<Value>,
        next: Box<Action>,
    },
    ArrayElement {
        items: Arc<Vec<Expr>>,
        index: usize,
        values: Vec<Value>,
        holes: BTreeSet<usize>,
        next: Box<Action>,
    },
    TemplatePart {
        parts: Arc<Vec<Expr>>,
        index: usize,
        units: Vec<u16>,
        next: Box<Action>,
    },
    BinaryLeft {
        operator: String,
        right: Box<Expr>,
        next: Box<Action>,
    },
    BinaryRight {
        operator: String,
        left: Value,
        next: Box<Action>,
    },
    Conditional {
        yes: Box<Expr>,
        no: Box<Expr>,
        next: Box<Action>,
    },
    Unary {
        operator: String,
        next: Box<Action>,
    },
    Assign {
        place: Place,
        previous: Option<Value>,
        operator: String,
        next: Box<Action>,
    },
}
#[derive(Clone, Debug)]
enum ReferenceOperation {
    Delete,
    Assign { operator: String, right: Box<Expr> },
    Update { increment: bool, prefix: bool },
}
impl Action {
    fn depth(&self) -> usize {
        let mut depth = 1;
        let mut action = self;
        loop {
            if depth > LIMIT {
                return depth;
            }
            action = match action {
                Self::ReferenceBase { next, .. }
                | Self::ReferenceKey { next, .. }
                | Self::MemberBase { next, .. }
                | Self::MemberKey { next, .. }
                | Self::CallMemberBase { next, .. }
                | Self::CallMemberKey { next, .. }
                | Self::CallCallee { next, .. }
                | Self::CallArgument { next, .. }
                | Self::ArrayElement { next, .. }
                | Self::TemplatePart { next, .. }
                | Self::Assign { next, .. }
                | Self::BinaryLeft { next, .. }
                | Self::BinaryRight { next, .. }
                | Self::Conditional { next, .. }
                | Self::Unary { next, .. }
                | Self::Await(next)
                | Self::Yield(next)
                | Self::AsyncYieldReady(next)
                | Self::DelegateStart(next)
                | Self::DelegateResume { next, .. }
                | Self::AsyncDelegateResume { next, .. }
                | Self::AsyncDelegateResult { next, .. }
                | Self::AsyncDelegateMissingThrowClose { next, .. }
                | Self::AsyncDelegateReturnAbsent { next, .. } => next,
                _ => return depth,
            };
            depth += 1;
        }
    }
    fn trace(&self, values: &mut Vec<Value>, scopes: &mut Vec<usize>) {
        let mut action = self;
        loop {
            match action {
                Self::Assign {
                    place,
                    previous,
                    next,
                    ..
                } => {
                    place.trace_values(values, scopes);
                    values.extend(previous.iter().cloned());
                    action = next;
                }
                Self::ReferenceKey { base, next, .. }
                | Self::MemberKey { base, next }
                | Self::CallMemberKey { base, next, .. } => {
                    values.push(base.clone());
                    action = next;
                }
                Self::ReferenceBase { next, .. }
                | Self::MemberBase { next, .. }
                | Self::CallMemberBase { next, .. }
                | Self::CallCallee { next, .. } => action = next,
                Self::CallArgument {
                    target,
                    values: args,
                    next,
                    ..
                } => {
                    target.trace_values(values);
                    values.extend(args.iter().cloned());
                    action = next;
                }
                Self::ArrayElement {
                    values: items,
                    next,
                    ..
                } => {
                    values.extend(items.iter().cloned());
                    action = next;
                }
                Self::TemplatePart { next, .. } => action = next,
                Self::BinaryRight { left, next, .. } => {
                    values.push(left.clone());
                    action = next;
                }
                Self::BinaryLeft { next, .. }
                | Self::Conditional { next, .. }
                | Self::Unary { next, .. }
                | Self::Await(next)
                | Self::Yield(next)
                | Self::AsyncYieldReady(next)
                | Self::DelegateStart(next) => action = next,
                Self::DelegateResume { state, next } => {
                    values.push(state.source.clone());
                    state.iterator.trace_values(values);
                    values.extend(state.last_result.iter().cloned());
                    action = next;
                }
                Self::AsyncDelegateResume { state, next }
                | Self::AsyncDelegateResult { state, next, .. }
                | Self::AsyncDelegateMissingThrowClose { state, next }
                | Self::AsyncDelegateReturnAbsent { state, next } => {
                    values.push(state.source.clone());
                    state.iterator.trace_values(values);
                    action = next;
                }
                Self::AsyncOfResult(state) => {
                    state.trace(values);
                    break;
                }
                Self::AsyncOfClose(state, completion) => {
                    state.trace(values);
                    completion.trace(values);
                    break;
                }
                Self::AsyncForSource(_, _, scope) | Self::ForEachSource(_, _, _, scope) => {
                    scopes.push(*scope);
                    break;
                }
                _ => break,
            }
        }
    }
}
#[derive(Clone, Debug)]
struct DelegateState {
    source: Value,
    iterator: IteratorRecord,
    last_result: Option<Value>,
    steps: usize,
}
#[derive(Clone, Debug)]
struct AsyncDelegateState {
    source: Value,
    iterator: AsyncIteratorRecord,
    steps: usize,
}
/// The loop environment is carried by the frame or pending-action tuple.
/// Only let bindings are copied; var uses the containing function environment.
#[derive(Clone, Debug)]
struct ForState {
    condition: Option<Expr>,
    update: Option<Expr>,
    body: Box<Stmt>,
    per_iteration: Arc<Vec<String>>,
}
#[derive(Clone, Debug)]
struct AsyncOfState {
    binding: ForBinding,
    body: Box<Stmt>,
    source: Value,
    iterator: AsyncIteratorRecord,
    count: usize,
}
impl AsyncOfState {
    fn trace(&self, values: &mut Vec<Value>) {
        values.push(self.source.clone());
        self.iterator.trace_values(values);
    }
}
#[derive(Clone, Debug)]
struct OfState {
    binding: ForBinding,
    body: Box<Stmt>,
    source: Value,
    iterator: IteratorRecord,
    count: usize,
}
/// A bounded key snapshot, matching the synchronous interpreter: deletions are
/// checked when visited, newly added keys are not appended, and changes to
/// enumerability/prototypes during iteration are not fully dynamic ES semantics.
#[derive(Clone, Debug)]
struct InState {
    binding: ForBinding,
    body: Box<Stmt>,
    source: Value,
    keys: Arc<Vec<String>>,
    index: usize,
}
#[derive(Clone, Debug)]
enum Completion {
    Return(Value),
    Throw(JsError),
    Break(Option<String>),
    Continue(Option<String>),
}
impl Completion {
    fn trace(&self, values: &mut Vec<Value>) {
        match self {
            Self::Return(value) => values.push(value.clone()),
            Self::Throw(error) => values.extend(error.thrown.iter().cloned()),
            Self::Break(_) | Self::Continue(_) => {}
        }
    }
}
impl Frame {
    fn depth(&self) -> usize {
        match self {
            Self::Evaluate(_, _, action) | Self::Apply(_, _, action) => 1 + action.depth(),
            _ => 1,
        }
    }
}
impl Machine {
    fn depth(&self) -> usize {
        self.frames.iter().map(Frame::depth).sum::<usize>()
            + self
                .pending
                .as_ref()
                .map_or(0, |(_, action)| 1 + action.depth())
    }
    fn push(&mut self, frame: Frame) -> Result<(), JsError> {
        if self.depth() + frame.depth() > LIMIT {
            return Err(JsError::resource("Continuation frame limit exceeded"));
        }
        self.frames.push(frame);
        Ok(())
    }
}
impl Runtime {
    pub(super) fn resumable_execution_active(&self) -> bool {
        !self.resumables.running.is_empty()
    }
    pub(super) fn resumable_start(
        &mut self,
        owner: usize,
        body: Arc<Vec<Stmt>>,
        scope: usize,
        strict: bool,
        kind: FunctionKind,
    ) -> Result<(), JsError> {
        validate_body(&body, kind)?;
        if self.resumables.machines.len() + self.resumables.running.len() >= LIMIT {
            return Err(JsError::resource("Suspended function limit exceeded"));
        }
        if self.resumable_exists(owner) {
            return Err(JsError::runtime("Continuation already exists"));
        }
        self.resumables.machines.insert(
            owner,
            Machine {
                owner,
                scope,
                strict,
                kind,
                body,
                started: false,
                frames: Vec::new(),
                pending: None,
                abrupt: None,
            },
        );
        Ok(())
    }
    pub(super) fn resumable_exists(&self, owner: usize) -> bool {
        self.resumables.machines.contains_key(&owner) || self.resumables.running.contains(&owner)
    }
    pub(super) fn resumable_cancel(&mut self, owner: usize) {
        self.resumables.machines.remove(&owner);
        self.cancel_async_await(owner);
    }
    pub(super) fn resumable_resume(
        &mut self,
        owner: usize,
        input: ResumeInput,
        host: &mut impl Host,
    ) -> Result<Outcome, JsError> {
        if self.resumables.running.contains(&owner) {
            return Err(self.property_type_error("Generator is already executing"));
        }
        let mut machine = self
            .resumables
            .machines
            .remove(&owner)
            .ok_or_else(|| JsError::runtime("Missing continuation"))?;
        self.resumables.running.insert(owner);
        let strict = self.strict;
        self.strict = machine.strict;
        let result = self.run_continuation(&mut machine, input, host);
        self.strict = strict;
        self.resumables.running.remove(&owner);
        if matches!(
            &result,
            Ok(Outcome::Await(_) | Outcome::Yield(_) | Outcome::YieldResult(_))
        ) {
            self.resumables.machines.insert(owner, machine);
        }
        result
    }
    fn run_continuation(
        &mut self,
        machine: &mut Machine,
        input: ResumeInput,
        host: &mut impl Host,
    ) -> Result<Outcome, JsError> {
        self.tick()?;
        if !machine.started {
            // Abrupt completion of suspended-start never enters the body.
            match input {
                ResumeInput::Throw(value) => return Err(thrown(value)),
                ResumeInput::Return(value) => return Ok(Outcome::Completed(value)),
                ResumeInput::Normal(_) => {}
            }
            machine.started = true;
            self.prepare(&machine.body, machine.scope, host)?;
            machine.push(Frame::Sequence(machine.body.clone(), 0, machine.scope))?;
        } else {
            let pending = machine.pending.take();
            let result = match pending {
                Some((scope, Action::DelegateResume { state, next })) => {
                    self.continuation_delegate(machine, scope, *state, *next, input, host)
                }
                Some((scope, Action::AsyncDelegateResume { state, next })) => {
                    self.continuation_async_delegate(machine, scope, *state, *next, input, host)
                }
                Some((_, Action::AsyncOfClose(_, completion))) => match input {
                    ResumeInput::Normal(value) => {
                        let close = self.async_result_object(&value);
                        Self::complete_async_close(machine, completion, close).map(|()| None)
                    }
                    ResumeInput::Throw(value) => {
                        Self::complete_async_close(machine, completion, Err(thrown(value)))
                            .map(|()| None)
                    }
                    ResumeInput::Return(value) => {
                        machine.abrupt = Some(Completion::Return(value));
                        Ok(None)
                    }
                },
                pending => match input {
                    ResumeInput::Throw(value) => {
                        machine.abrupt = Some(Completion::Throw(thrown(value)));
                        Ok(None)
                    }
                    ResumeInput::Return(value) => {
                        machine.abrupt = Some(Completion::Return(value));
                        Ok(None)
                    }
                    ResumeInput::Normal(value) => match pending {
                        Some((scope, action)) => {
                            self.continuation_action(machine, scope, action, value, host)
                        }
                        None => Ok(None),
                    },
                },
            };
            if let Some(outcome) = Self::continuation_result(machine, result)? {
                return Ok(outcome);
            }
        }
        loop {
            self.tick()?;
            // Complete uncaught abrupts here, outside error-to-throw conversion.
            if machine.frames.is_empty() {
                if let Some(completion) = machine.abrupt.take() {
                    return Self::finish_completion(completion);
                }
                return Ok(Outcome::Completed(Value::Undefined));
            }
            let result = if machine.abrupt.is_some() {
                self.unwind_continuation(machine, host)
            } else if let Some(frame) = machine.frames.pop() {
                self.continuation_frame(machine, frame, host)
            } else {
                return Ok(Outcome::Completed(Value::Undefined));
            };
            if let Some(outcome) = Self::continuation_result(machine, result)? {
                return Ok(outcome);
            }
        }
    }
    fn continuation_result(
        machine: &mut Machine,
        result: Result<Option<Outcome>, JsError>,
    ) -> Result<Option<Outcome>, JsError> {
        match result {
            Err(error) if error.kind == ErrorKind::Resource => Err(error),
            Err(error) => {
                machine.pending = None;
                machine.abrupt = Some(Completion::Throw(error));
                Ok(None)
            }
            result => result,
        }
    }
    fn finish_completion(completion: Completion) -> Result<Outcome, JsError> {
        match completion {
            Completion::Return(value) => Ok(Outcome::Completed(value)),
            Completion::Throw(error) => Err(error),
            Completion::Break(_) | Completion::Continue(_) => Err(JsError::runtime(
                "Unsupported continuation control transfer",
            )),
        }
    }
    fn continuation_frame(
        &mut self,
        machine: &mut Machine,
        frame: Frame,
        host: &mut impl Host,
    ) -> Result<Option<Outcome>, JsError> {
        match frame {
            Frame::Evaluate(expression, scope, action) => {
                return self.continuation_expression(machine, &expression, scope, action, host);
            }
            Frame::Apply(value, scope, action) => {
                return self.continuation_action(machine, scope, action, value, host);
            }
            Frame::Sequence(body, index, scope) => {
                if let Some(statement) = body.get(index).cloned() {
                    machine.push(Frame::Sequence(body, index + 1, scope))?;
                    machine.push(Frame::Statement(statement, scope))?;
                }
            }
            Frame::Variables(kind, values, index, scope) => {
                if let Some((name, initial)) = values.get(index).cloned() {
                    machine.push(Frame::Variables(kind, values, index + 1, scope))?;
                    if kind == Declaration::Var && initial.is_none() {
                        return Ok(None);
                    }
                    let action = Action::Bind(kind, name);
                    return match initial {
                        Some(expr) => {
                            self.continuation_expression(machine, &expr, scope, action, host)
                        }
                        None => {
                            self.continuation_action(machine, scope, action, Value::Undefined, host)
                        }
                    };
                }
            }
            Frame::Loop(condition, body, scope) => {
                let action = Action::LoopCondition(condition.clone(), body);
                return self.continuation_expression(machine, &condition, scope, action, host);
            }
            Frame::ForStart(state, scope) => {
                let scope = self.continuation_iteration_scope(&state, scope)?;
                machine.push(Frame::ForTest(state, scope))?;
            }
            Frame::ForTest(state, scope) => {
                if let Some(condition) = state.condition.clone() {
                    return self.continuation_expression(
                        machine,
                        &condition,
                        scope,
                        Action::ForCondition(state),
                        host,
                    );
                }
                return self.continuation_action(
                    machine,
                    scope,
                    Action::ForCondition(state),
                    Value::Bool(true),
                    host,
                );
            }
            Frame::ForNext(state, scope) => {
                return self.continuation_for_next(machine, state, scope, host);
            }
            Frame::SwitchReady(clauses, value, scope) => {
                // CaseBlock declarations share one environment. Prepare it only
                // after the discriminant has completed, before case evaluation.
                let child = self.scope(scope, false)?;
                let mut statements = Vec::new();
                for (_, body) in &clauses {
                    for statement in body {
                        self.tick()?;
                        if statements.len() >= 65_536 {
                            return Err(JsError::resource("Switch statement budget exceeded"));
                        }
                        statements.push(statement.clone());
                    }
                }
                self.prepare(&statements, child, host)?;
                let mut selected = None;
                let mut default = None;
                let mut offset = 0;
                for (test, body) in &clauses {
                    self.tick()?;
                    if let Some(test) = test {
                        if crate::strict_equal(&value, &self.eval(test, child, host)?) {
                            selected = Some(offset);
                            break;
                        }
                    } else {
                        default = Some(offset);
                    }
                    offset += body.len();
                }
                if let Some(start) = selected.or(default) {
                    // The sequence cursor is the only fallthrough state. Cases
                    // and the discriminant are never evaluated on resumption.
                    machine.push(Frame::SwitchBoundary(child))?;
                    machine.push(Frame::Sequence(Arc::new(statements), start, child))?;
                }
            }
            Frame::SwitchBoundary(_) => {}
            Frame::ForEachReady(binding, body, of, source, scope) => {
                if of {
                    let iterator = self.iterator(source.clone(), host)?;
                    machine.push(Frame::OfNext(
                        OfState {
                            binding,
                            body,
                            source,
                            iterator,
                            count: 0,
                        },
                        scope,
                    ))?;
                } else {
                    let keys = self.enumerable_keys(&source)?;
                    if keys.len() > 4096 {
                        return Err(JsError::resource("For-in snapshot key limit exceeded"));
                    }
                    machine.push(Frame::InNext(
                        InState {
                            binding,
                            body,
                            source,
                            keys: Arc::new(keys),
                            index: 0,
                        },
                        scope,
                    ))?;
                }
            }
            Frame::OfNext(mut state, scope) => {
                // An IteratorStep failure does not enter the body and does not
                // run IteratorClose. Binding/body abrupts do, via OfBody.
                if let Some(value) = self.iterator_next(&mut state.iterator, host)? {
                    state.count += 1;
                    if state.count > 4096 {
                        return Err(JsError::resource("For-of iteration item limit exceeded"));
                    }
                    let binding = state.binding.clone();
                    let body = state.body.clone();
                    machine.push(Frame::OfBody(state, scope))?;
                    let iteration = self.iteration_scope(&binding, value, scope, host)?;
                    machine.push(Frame::Statement(*body, iteration))?;
                }
            }
            Frame::OfBody(state, scope) => machine.push(Frame::OfNext(state, scope))?,
            Frame::AsyncOfNext(mut state, scope) => {
                self.tick()?;
                let result = self.async_iterator_next(&mut state.iterator, machine.owner, host)?;
                return self.continuation_suspend(
                    machine,
                    scope,
                    Action::AsyncOfResult(state),
                    result,
                    true,
                );
            }
            Frame::AsyncOfBody(state, scope) => machine.push(Frame::AsyncOfNext(state, scope))?,
            Frame::InNext(mut state, scope) => {
                if let Some(key) = state.keys.get(state.index).cloned() {
                    state.index += 1;
                    if !matches!(&state.source, Value::String(_) | Value::Utf16(_))
                        && !self.has_property(&state.source, &key, host)?
                    {
                        machine.push(Frame::InNext(state, scope))?;
                    } else {
                        let value = self.string(key)?;
                        let binding = state.binding.clone();
                        let body = state.body.clone();
                        machine.push(Frame::InNext(state, scope))?;
                        let iteration = self.iteration_scope(&binding, value, scope, host)?;
                        machine.push(Frame::Statement(*body, iteration))?;
                    }
                }
            }
            Frame::Handler { finally, scope, .. } => {
                self.continuation_finally(machine, finally, scope, None, host)?
            }
            Frame::Finally(saved, _) => machine.abrupt = saved,
            Frame::Statement(statement, scope) => match statement {
                Stmt::Block(body) => self.continuation_block(machine, body, scope, host)?,
                Stmt::Variables(kind, values) => {
                    machine.push(Frame::Variables(kind, Arc::new(values), 0, scope))?
                }
                Stmt::Expression(expr) => {
                    return self.continuation_expression(
                        machine,
                        &expr,
                        scope,
                        Action::Discard,
                        host,
                    );
                }
                Stmt::Return(None) => machine.abrupt = Some(Completion::Return(Value::Undefined)),
                Stmt::Return(Some(expr)) => {
                    // Await before creating the abrupt completion: rejection must
                    // still reach this return statement's catch and finally.
                    let action = if machine.kind == FunctionKind::AsyncGenerator {
                        Action::Await(Box::new(Action::Return))
                    } else {
                        Action::Return
                    };
                    return self.continuation_expression(machine, &expr, scope, action, host);
                }
                Stmt::Throw(expr) => {
                    return self.continuation_expression(
                        machine,
                        &expr,
                        scope,
                        Action::Throw,
                        host,
                    );
                }
                Stmt::Break(label) => machine.abrupt = Some(Completion::Break(label)),
                Stmt::Continue(label) => machine.abrupt = Some(Completion::Continue(label)),
                Stmt::If(expr, yes, no) => {
                    return self.continuation_expression(
                        machine,
                        &expr,
                        scope,
                        Action::Branch(yes, no),
                        host,
                    );
                }
                Stmt::While(condition, body) => {
                    machine.push(Frame::Loop(condition, body, scope))?
                }
                Stmt::DoWhile(body, condition) => {
                    machine.push(Frame::Loop(condition, body.clone(), scope))?;
                    machine.push(Frame::Statement(*body, scope))?;
                }
                Stmt::For(init, condition, update, body) => {
                    let mut per_iteration = Vec::new();
                    let loop_scope = match init.as_deref() {
                        Some(Stmt::Variables(kind, values)) if *kind != Declaration::Var => {
                            if *kind == Declaration::Let {
                                per_iteration.extend(values.iter().map(|(name, _)| name.clone()));
                            }
                            self.scope(scope, false)?
                        }
                        _ => scope,
                    };
                    if let Some(init) = &init {
                        self.prepare(std::slice::from_ref(init.as_ref()), loop_scope, host)?;
                    }
                    let state = ForState {
                        condition,
                        update,
                        body,
                        per_iteration: Arc::new(per_iteration),
                    };
                    machine.push(Frame::ForStart(state, loop_scope))?;
                    if let Some(init) = init {
                        machine.push(Frame::Statement(*init, loop_scope))?;
                    }
                }
                Stmt::ForAwaitOf(binding, source, body) => {
                    let source_scope = self.continuation_source_scope(&binding, scope)?;
                    return self.continuation_expression(
                        machine,
                        &source,
                        source_scope,
                        Action::AsyncForSource(binding, body, scope),
                        host,
                    );
                }
                Stmt::ForOf(binding, source, body) => {
                    return self.continuation_for_each_source(
                        machine,
                        ForEachSource {
                            binding,
                            source,
                            body,
                            of: true,
                        },
                        scope,
                        host,
                    );
                }
                Stmt::ForIn(binding, source, body) => {
                    return self.continuation_for_each_source(
                        machine,
                        ForEachSource {
                            binding,
                            source,
                            body,
                            of: false,
                        },
                        scope,
                        host,
                    );
                }
                Stmt::Switch(discriminant, clauses) => {
                    return self.continuation_expression(
                        machine,
                        &discriminant,
                        scope,
                        Action::Switch(clauses),
                        host,
                    );
                }
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    machine.push(Frame::Handler {
                        catch,
                        diagnostic: None,
                        finally,
                        scope,
                    })?;
                    self.continuation_block(machine, body, scope, host)?;
                }
                other => match self.statement(&other, scope, host)? {
                    Flow::Return(value) => machine.abrupt = Some(Completion::Return(value)),
                    Flow::Break(label) => machine.abrupt = Some(Completion::Break(label)),
                    Flow::Continue(label) => machine.abrupt = Some(Completion::Continue(label)),
                    Flow::Empty | Flow::Normal(_) => {}
                },
            },
        }
        Ok(None)
    }
    fn continuation_source_scope(
        &mut self,
        binding: &ForBinding,
        scope: usize,
    ) -> Result<usize, JsError> {
        match binding {
            ForBinding::Declaration(kind, name) if *kind != Declaration::Var => {
                let child = self.scope(scope, false)?;
                self.declare(child, name, *kind == Declaration::Let, false)?;
                Ok(child)
            }
            ForBinding::Pattern(kind, pattern) if *kind != Declaration::Var => {
                let child = self.scope(scope, false)?;
                self.declare_pattern(pattern, *kind, child)?;
                Ok(child)
            }
            _ => Ok(scope),
        }
    }
    fn async_result_object(&mut self, result: &Value) -> Result<(), JsError> {
        if matches!(
            result,
            Value::Object(_)
                | Value::Array(_)
                | Value::Function(_)
                | Value::Native(_)
                | Value::Host(_)
        ) {
            Ok(())
        } else {
            Err(self.property_type_error("Async iterator result must be an object"))
        }
    }
    fn complete_async_close(
        machine: &mut Machine,
        completion: Completion,
        close: Result<(), JsError>,
    ) -> Result<(), JsError> {
        match close {
            Err(error) if error.kind == ErrorKind::Resource => return Err(error),
            Err(error) if !matches!(&completion, Completion::Throw(_)) => return Err(error),
            _ => {}
        }
        if !matches!(&completion, Completion::Break(None)) {
            machine.abrupt = Some(completion);
        }
        Ok(())
    }
    fn continuation_for_each_source(
        &mut self,
        machine: &mut Machine,
        loop_source: ForEachSource,
        scope: usize,
        host: &mut impl Host,
    ) -> Result<Option<Outcome>, JsError> {
        let ForEachSource {
            binding,
            source,
            body,
            of,
        } = loop_source;
        self.tick()?;
        // Source evaluation observes the lexical loop binding's TDZ. Iteration
        // scopes are subsequently created under the original outer scope.
        let source_scope = self.continuation_source_scope(&binding, scope)?;
        self.continuation_expression(
            machine,
            &source,
            source_scope,
            Action::ForEachSource(binding, body, of, scope),
            host,
        )
    }
    fn continuation_iteration_scope(
        &mut self,
        state: &ForState,
        previous: usize,
    ) -> Result<usize, JsError> {
        self.copy_iteration_scope(previous, &state.per_iteration)
    }
    fn continuation_for_next(
        &mut self,
        machine: &mut Machine,
        state: ForState,
        scope: usize,
        host: &mut impl Host,
    ) -> Result<Option<Outcome>, JsError> {
        self.tick()?;
        let next = self.continuation_iteration_scope(&state, scope)?;
        let update = state.update.clone();
        machine.push(Frame::ForTest(state, next))?;
        if let Some(update) = update {
            return self.continuation_expression(machine, &update, next, Action::Discard, host);
        }
        Ok(None)
    }
    fn continuation_block(
        &mut self,
        machine: &mut Machine,
        body: Vec<Stmt>,
        scope: usize,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        let child = self.scope(scope, false)?;
        self.prepare(&body, child, host)?;
        machine.push(Frame::Sequence(Arc::new(body), 0, child))
    }
    fn continuation_finally(
        &mut self,
        machine: &mut Machine,
        body: Option<Vec<Stmt>>,
        scope: usize,
        saved: Option<Completion>,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        if let Some(body) = body {
            machine.push(Frame::Finally(saved, scope))?;
            self.continuation_block(machine, body, scope, host)?;
        } else {
            machine.abrupt = saved;
        }
        Ok(())
    }
    fn unwind_continuation(
        &mut self,
        machine: &mut Machine,
        host: &mut impl Host,
    ) -> Result<Option<Outcome>, JsError> {
        let completion = machine
            .abrupt
            .take()
            .ok_or_else(|| JsError::runtime("Missing abrupt completion"))?;
        let Some(frame) = machine.frames.pop() else {
            return Self::finish_completion(completion).map(Some);
        };
        match frame {
            Frame::SwitchBoundary(_) => {
                if !matches!(&completion, Completion::Break(None)) {
                    machine.abrupt = Some(completion);
                }
            }
            Frame::Loop(condition, body, scope) => match completion {
                Completion::Break(None) => {}
                Completion::Continue(None) => machine.push(Frame::Loop(condition, body, scope))?,
                completion => machine.abrupt = Some(completion),
            },
            Frame::ForNext(state, scope) => match completion {
                Completion::Break(None) => {}
                Completion::Continue(None) => {
                    return self.continuation_for_next(machine, state, scope, host);
                }
                completion => machine.abrupt = Some(completion),
            },
            Frame::OfBody(mut state, scope) => {
                if matches!(&completion, Completion::Continue(None)) {
                    machine.push(Frame::OfNext(state, scope))?;
                } else {
                    let close = self.iterator_close(&mut state.iterator, host);
                    match close {
                        Err(error) if error.kind == ErrorKind::Resource => return Err(error),
                        // IteratorClose preserves an incoming throw over an
                        // ordinary error from getting/calling return.
                        Err(error) if !matches!(&completion, Completion::Throw(_)) => {
                            return Err(error);
                        }
                        _ => {}
                    }
                    if !matches!(&completion, Completion::Break(None)) {
                        machine.abrupt = Some(completion);
                    }
                }
            }
            Frame::AsyncOfBody(mut state, scope) => {
                if matches!(&completion, Completion::Continue(None)) {
                    machine.push(Frame::AsyncOfNext(state, scope))?;
                } else {
                    match self.async_iterator_return(&mut state.iterator, machine.owner, host) {
                        Ok(Some(result)) => {
                            return self.continuation_suspend(
                                machine,
                                scope,
                                Action::AsyncOfClose(state, completion),
                                result,
                                true,
                            );
                        }
                        Ok(None) => Self::complete_async_close(machine, completion, Ok(()))?,
                        Err(error) => Self::complete_async_close(machine, completion, Err(error))?,
                    }
                }
            }
            Frame::InNext(state, scope) => match completion {
                Completion::Break(None) => {}
                Completion::Continue(None) => machine.push(Frame::InNext(state, scope))?,
                completion => machine.abrupt = Some(completion),
            },
            Frame::Handler {
                catch,
                diagnostic,
                finally,
                scope,
            } => {
                if let (Completion::Throw(error), Some((binding, body))) = (&completion, catch) {
                    // Catch is consumed before preparing its scope. Its own
                    // errors enter finally, never the same catch again.
                    machine.push(Frame::Handler {
                        catch: None,
                        diagnostic: None,
                        finally,
                        scope,
                    })?;
                    let value = self.exception_value(error)?;
                    if let Some(Frame::Handler { diagnostic, .. }) = machine.frames.last_mut() {
                        *diagnostic = Some(Box::new(CatchDiagnostic::capture(error, &value)));
                    }
                    let child = self.scope(scope, false)?;
                    if let Some(name) = binding {
                        self.declare(child, &name, true, true)?;
                        self.scopes[child]
                            .bindings
                            .get_mut(&name)
                            .ok_or_else(|| JsError::runtime("Missing catch binding"))?
                            .value = value;
                    }
                    self.prepare(&body, child, host)?;
                    machine.push(Frame::Sequence(Arc::new(body), 0, child))?;
                } else {
                    let mut completion = completion;
                    if let (Some(diagnostic), Completion::Throw(error)) =
                        (diagnostic, &mut completion)
                    {
                        diagnostic.attach(error);
                    }
                    self.continuation_finally(machine, finally, scope, Some(completion), host)?;
                }
            }
            // A new abrupt from finally overrides its saved completion.
            Frame::Finally(_, _)
            | Frame::Sequence(..)
            | Frame::Statement(..)
            | Frame::Evaluate(..)
            | Frame::Apply(..)
            | Frame::Variables(..)
            | Frame::ForStart(..)
            | Frame::ForTest(..)
            | Frame::ForEachReady(..)
            | Frame::AsyncOfNext(..)
            | Frame::OfNext(..)
            | Frame::SwitchReady(..) => machine.abrupt = Some(completion),
        }
        Ok(None)
    }
    fn continuation_expression(
        &mut self,
        machine: &mut Machine,
        expr: &Expr,
        scope: usize,
        action: Action,
        host: &mut impl Host,
    ) -> Result<Option<Outcome>, JsError> {
        self.tick()?;
        if machine.depth() + 1 + action.depth() > LIMIT {
            return Err(JsError::resource(
                "Continuation expression depth limit exceeded",
            ));
        }
        // Every expression continuation is scheduled, not recursively executed.
        // Frame + action depth share the statement continuation's bounded budget.
        match expr {
            Expr::Array(items) => {
                // Reject source width before evaluating even the first element.
                if items.len() > 4096 {
                    return Err(JsError::resource("Array element limit exceeded"));
                }
                self.continuation_literal(
                    machine,
                    scope,
                    Action::ArrayElement {
                        items: Arc::new(items.clone()),
                        index: 0,
                        values: Vec::new(),
                        holes: BTreeSet::new(),
                        next: Box::new(action),
                    },
                )?;
                return Ok(None);
            }
            Expr::Template(parts) => {
                self.continuation_literal(
                    machine,
                    scope,
                    Action::TemplatePart {
                        parts: Arc::new(parts.clone()),
                        index: 0,
                        units: Vec::new(),
                        next: Box::new(action),
                    },
                )?;
                return Ok(None);
            }
            Expr::Unary(operator, target)
                if operator == "delete" && ordinary_member_reference(target) =>
            {
                let Expr::Member(base, key) = target.as_ref() else {
                    unreachable!("Ordinary member reference");
                };
                machine.push(Frame::Evaluate(
                    *base.clone(),
                    scope,
                    Action::ReferenceBase {
                        key: key.clone(),
                        operation: ReferenceOperation::Delete,
                        next: Box::new(action),
                    },
                ))?;
                return Ok(None);
            }
            Expr::Update(target, increment, prefix) if ordinary_member_reference(target) => {
                let Expr::Member(base, key) = target.as_ref() else {
                    unreachable!("Ordinary member reference");
                };
                machine.push(Frame::Evaluate(
                    *base.clone(),
                    scope,
                    Action::ReferenceBase {
                        key: key.clone(),
                        operation: ReferenceOperation::Update {
                            increment: *increment,
                            prefix: *prefix,
                        },
                        next: Box::new(action),
                    },
                ))?;
                return Ok(None);
            }
            Expr::Member(base, key) if !matches!(base.as_ref(), Expr::Super) => {
                machine.push(Frame::Evaluate(
                    *base.clone(),
                    scope,
                    Action::MemberBase {
                        key: key.clone(),
                        next: Box::new(action),
                    },
                ))?;
                return Ok(None);
            }
            Expr::Call(callee, args) if resumable_call(callee, args) => {
                let arguments = Arc::new(args.clone());
                if let Expr::Member(base, key) = callee.as_ref() {
                    machine.push(Frame::Evaluate(
                        *base.clone(),
                        scope,
                        Action::CallMemberBase {
                            key: key.clone(),
                            arguments,
                            next: Box::new(action),
                        },
                    ))?;
                } else {
                    let context = if let Expr::Name(name) = callee.as_ref() {
                        Some(name.clone())
                    } else {
                        None
                    };
                    machine.push(Frame::Evaluate(
                        *callee.clone(),
                        scope,
                        Action::CallCallee {
                            context,
                            arguments,
                            next: Box::new(action),
                        },
                    ))?;
                }
                return Ok(None);
            }
            Expr::YieldDelegate(operand) => {
                machine.push(Frame::Evaluate(
                    *operand.clone(),
                    scope,
                    Action::DelegateStart(Box::new(action)),
                ))?;
                return Ok(None);
            }
            Expr::Await(operand) => {
                machine.push(Frame::Evaluate(
                    *operand.clone(),
                    scope,
                    Action::Await(Box::new(action)),
                ))?;
                return Ok(None);
            }
            Expr::Yield(operand) => {
                let action = Action::Yield(Box::new(action));
                match operand {
                    Some(operand) => {
                        machine.push(Frame::Evaluate(*operand.clone(), scope, action))?
                    }
                    None => machine.push(Frame::Apply(Value::Undefined, scope, action))?,
                }
                return Ok(None);
            }
            Expr::Binary(operator, left, right) if resumable_binary(operator) => {
                let action = Action::BinaryLeft {
                    operator: operator.clone(),
                    right: right.clone(),
                    next: Box::new(action),
                };
                machine.push(Frame::Evaluate(*left.clone(), scope, action))?;
                return Ok(None);
            }
            Expr::Conditional(condition, yes, no) => {
                let action = Action::Conditional {
                    yes: yes.clone(),
                    no: no.clone(),
                    next: Box::new(action),
                };
                machine.push(Frame::Evaluate(*condition.clone(), scope, action))?;
                return Ok(None);
            }
            Expr::Unary(operator, operand)
                if resumable_unary(operator)
                    && !(operator == "typeof" && matches!(operand.as_ref(), Expr::Name(_))) =>
            {
                let action = Action::Unary {
                    operator: operator.clone(),
                    next: Box::new(action),
                };
                machine.push(Frame::Evaluate(*operand.clone(), scope, action))?;
                return Ok(None);
            }
            _ => {}
        }
        if let Expr::Assign(operator, target, right) = expr
            && resumable_assignment(operator, target)
        {
            if let Expr::Member(base, key) = target.as_ref()
                && !matches!(base.as_ref(), Expr::Super)
            {
                machine.push(Frame::Evaluate(
                    *base.clone(),
                    scope,
                    Action::ReferenceBase {
                        key: key.clone(),
                        operation: ReferenceOperation::Assign {
                            operator: operator.clone(),
                            right: right.clone(),
                        },
                        next: Box::new(action),
                    },
                ))?;
                return Ok(None);
            }
            // Resolve once, before evaluating any part of the RHS.
            // GetValue for a compound operation precedes the RHS, but all
            // coercion and the write are deferred until normal resumption.
            let place = self.place(target, scope, host)?;
            let previous = if operator == "=" {
                None
            } else {
                Some(self.read_place(&place, host)?)
            };
            let action = Action::Assign {
                place,
                previous,
                operator: operator.clone(),
                next: Box::new(action),
            };
            machine.push(Frame::Evaluate(*right.clone(), scope, action))?;
            return Ok(None);
        }
        let value = self.eval(expr, scope, host)?;
        machine.push(Frame::Apply(value, scope, action))?;
        Ok(None)
    }
    /// One action owns the partial literal regardless of its width. No spread
    /// iterator exists until its operand resumes, or after expansion finishes.
    fn continuation_literal(
        &mut self,
        machine: &mut Machine,
        scope: usize,
        action: Action,
    ) -> Result<(), JsError> {
        match action {
            Action::ArrayElement {
                items,
                index,
                values,
                holes,
                next,
            } => {
                if let Some(item) = items.get(index) {
                    let expr = match item {
                        Expr::Spread(operand) => Some(*operand.clone()),
                        Expr::Hole => None,
                        item => Some(item.clone()),
                    };
                    let action = Action::ArrayElement {
                        items,
                        index,
                        values,
                        holes,
                        next,
                    };
                    match expr {
                        Some(expr) => machine.push(Frame::Evaluate(expr, scope, action))?,
                        None => machine.push(Frame::Apply(Value::Undefined, scope, action))?,
                    }
                } else {
                    let value = self.literal_array_finish(values, holes)?;
                    machine.push(Frame::Apply(value, scope, *next))?;
                }
            }
            Action::TemplatePart {
                parts,
                index,
                units,
                next,
            } => {
                if let Some(part) = parts.get(index).cloned() {
                    machine.push(Frame::Evaluate(
                        part,
                        scope,
                        Action::TemplatePart {
                            parts,
                            index,
                            units,
                            next,
                        },
                    ))?;
                } else {
                    let value = self.literal_template_finish(units)?;
                    machine.push(Frame::Apply(value, scope, *next))?;
                }
            }
            _ => return Err(JsError::runtime("Missing literal continuation")),
        }
        Ok(())
    }
    fn continuation_call_arguments(
        &mut self,
        machine: &mut Machine,
        scope: usize,
        target: PreparedCall,
        arguments: Arc<Vec<Expr>>,
        next: Action,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        if let Some(first) = arguments.first() {
            machine.push(Frame::Evaluate(
                first.clone(),
                scope,
                Action::CallArgument {
                    target,
                    arguments,
                    index: 0,
                    values: Vec::new(),
                    next: Box::new(next),
                },
            ))?;
        } else {
            let value = self.continuation_invoke_call(target, Vec::new(), &next, host)?;
            machine.push(Frame::Apply(value, scope, next))?;
        }
        Ok(())
    }
    fn continuation_invoke_call(
        &mut self,
        target: PreparedCall,
        arguments: Vec<Value>,
        next: &Action,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        // The action has left the machine while user code executes. Preserve
        // both the captured target and the enclosing expression's temporaries.
        let mut retained = arguments.clone();
        target.trace_values(&mut retained);
        next.trace(&mut retained, &mut Vec::new());
        let roots = self.continuation_retain_values(retained)?;
        let result = self.invoke_prepared_call(target, arguments, host);
        for root in roots {
            self.release_value(root);
        }
        result
    }
    fn continuation_action(
        &mut self,
        machine: &mut Machine,
        scope: usize,
        action: Action,
        value: Value,
        host: &mut impl Host,
    ) -> Result<Option<Outcome>, JsError> {
        self.tick()?;
        match action {
            Action::ArrayElement {
                items,
                index,
                mut values,
                mut holes,
                next,
            } => {
                let result = if matches!(items.get(index), Some(Expr::Spread(_))) {
                    let mut retained = values.clone();
                    retained.push(value.clone());
                    next.trace(&mut retained, &mut Vec::new());
                    let roots = self.continuation_retain_values(retained)?;
                    let result = self.literal_array_spread(&mut values, value, host);
                    for root in roots {
                        self.release_value(root);
                    }
                    result
                } else {
                    self.literal_array_append(
                        &mut values,
                        &mut holes,
                        value,
                        matches!(items.get(index), Some(Expr::Hole)),
                    )
                };
                result?;
                self.continuation_literal(
                    machine,
                    scope,
                    Action::ArrayElement {
                        items,
                        index: index + 1,
                        values,
                        holes,
                        next,
                    },
                )?;
                return Ok(None);
            }
            Action::TemplatePart {
                parts,
                index,
                mut units,
                next,
            } => {
                let mut retained = vec![value.clone()];
                next.trace(&mut retained, &mut Vec::new());
                let roots = self.continuation_retain_values(retained)?;
                // Coerce now, before the next interpolation's effects.
                let result = self.literal_template_append(&mut units, value, host);
                for root in roots {
                    self.release_value(root);
                }
                result?;
                self.continuation_literal(
                    machine,
                    scope,
                    Action::TemplatePart {
                        parts,
                        index: index + 1,
                        units,
                        next,
                    },
                )?;
                return Ok(None);
            }
            Action::ReferenceBase {
                key,
                operation,
                next,
            } => {
                machine.push(Frame::Evaluate(
                    *key,
                    scope,
                    Action::ReferenceKey {
                        base: value,
                        operation,
                        next,
                    },
                ))?;
                return Ok(None);
            }
            Action::ReferenceKey {
                base,
                operation,
                next,
            } => {
                let mut retained = vec![base.clone(), value.clone()];
                next.trace(&mut retained, &mut Vec::new());
                let roots = self.continuation_retain_values(retained)?;
                let result = (|| {
                    // Evaluate the raw key first, but reject nullish bases
                    // before key coercion, GetValue or the assignment RHS.
                    let place = self.ordinary_member_place_values(base, value, host)?;
                    match operation {
                        ReferenceOperation::Delete => {
                            // Delete consumes the reference, never its GetValue.
                            let value = self.delete_ordinary_place(place, host)?;
                            machine.push(Frame::Apply(value, scope, *next))?;
                        }
                        ReferenceOperation::Assign { operator, right } => {
                            let previous = if operator == "=" {
                                None
                            } else {
                                Some(self.read_place(&place, host)?)
                            };
                            machine.push(Frame::Evaluate(
                                *right,
                                scope,
                                Action::Assign {
                                    place,
                                    previous,
                                    operator,
                                    next,
                                },
                            ))?;
                        }
                        ReferenceOperation::Update { increment, prefix } => {
                            let value = self.update_place(place, increment, prefix, host)?;
                            machine.push(Frame::Apply(value, scope, *next))?;
                        }
                    }
                    Ok::<(), JsError>(())
                })();
                for root in roots {
                    self.release_value(root);
                }
                result?;
                return Ok(None);
            }
            Action::MemberBase { key, next } => {
                machine.push(Frame::Evaluate(
                    *key,
                    scope,
                    Action::MemberKey { base: value, next },
                ))?;
                return Ok(None);
            }
            Action::MemberKey { base, next } => {
                let mut retained = vec![base.clone(), value.clone()];
                next.trace(&mut retained, &mut Vec::new());
                let roots = self.continuation_retain_values(retained)?;
                let result = self.read_member_values(base, value, host);
                for root in roots {
                    self.release_value(root);
                }
                machine.push(Frame::Apply(result?, scope, *next))?;
                return Ok(None);
            }
            Action::CallMemberBase {
                key,
                arguments,
                next,
            } => {
                machine.push(Frame::Evaluate(
                    *key,
                    scope,
                    Action::CallMemberKey {
                        base: value,
                        arguments,
                        next,
                    },
                ))?;
                return Ok(None);
            }
            Action::CallMemberKey {
                base,
                arguments,
                next,
            } => {
                let mut retained = vec![base.clone(), value.clone()];
                next.trace(&mut retained, &mut Vec::new());
                let roots = self.continuation_retain_values(retained)?;
                let result = self.prepare_member_call_values(base, value, host);
                for root in roots {
                    self.release_value(root);
                }
                self.continuation_call_arguments(machine, scope, result?, arguments, *next, host)?;
                return Ok(None);
            }
            Action::CallCallee {
                context,
                arguments,
                next,
            } => {
                let target = PreparedCall::Callable {
                    callable: value,
                    receiver: Value::Undefined,
                    context,
                };
                self.continuation_call_arguments(machine, scope, target, arguments, *next, host)?;
                return Ok(None);
            }
            Action::CallArgument {
                target,
                arguments,
                index,
                mut values,
                next,
            } => {
                values.push(value);
                let index = index + 1;
                if let Some(argument) = arguments.get(index) {
                    if values.len() >= 32 {
                        return Err(JsError::resource("Call argument limit exceeded"));
                    }
                    machine.push(Frame::Evaluate(
                        argument.clone(),
                        scope,
                        Action::CallArgument {
                            target,
                            arguments,
                            index,
                            values,
                            next,
                        },
                    ))?;
                } else {
                    let value = self.continuation_invoke_call(target, values, &next, host)?;
                    machine.push(Frame::Apply(value, scope, *next))?;
                }
                return Ok(None);
            }
            Action::DelegateStart(next) => {
                if machine.kind == FunctionKind::AsyncGenerator {
                    let mut values = vec![value.clone()];
                    next.trace(&mut values, &mut Vec::new());
                    let roots = self.continuation_retain_values(values)?;
                    let iterator = self.async_iterator(value.clone(), host);
                    for root in roots {
                        self.release_value(root);
                    }
                    let state = AsyncDelegateState {
                        source: value,
                        iterator: iterator?,
                        steps: 0,
                    };
                    return self.continuation_async_delegate(
                        machine,
                        scope,
                        state,
                        *next,
                        ResumeInput::Normal(Value::Undefined),
                        host,
                    );
                }
                let iterator = self.iterator(value.clone(), host)?;
                let state = DelegateState {
                    source: value,
                    iterator,
                    last_result: None,
                    steps: 0,
                };
                return self.continuation_delegate(
                    machine,
                    scope,
                    state,
                    *next,
                    ResumeInput::Normal(Value::Undefined),
                    host,
                );
            }
            Action::DelegateResume { state, next } => {
                return self.continuation_delegate(
                    machine,
                    scope,
                    *state,
                    *next,
                    ResumeInput::Normal(value),
                    host,
                );
            }
            Action::AsyncDelegateResume { state, next } => {
                return self.continuation_async_delegate(
                    machine,
                    scope,
                    *state,
                    *next,
                    ResumeInput::Normal(value),
                    host,
                );
            }
            Action::AsyncDelegateResult {
                state,
                next,
                returning,
            } => {
                self.async_result_object(&value)?;
                let roots = self.async_delegate_roots(&state, &next, &value)?;
                let result = (|| {
                    let done = self.get(value.clone(), "done", host)?.truthy();
                    let value = self.get(value, "value", host)?;
                    if done {
                        if returning {
                            machine.abrupt = Some(Completion::Return(value));
                        } else {
                            machine.push(Frame::Apply(value, scope, *next))?;
                        }
                        Ok(None)
                    } else {
                        // AsyncGeneratorYield does not await its argument.
                        self.continuation_suspend(
                            machine,
                            scope,
                            Action::AsyncDelegateResume { state, next },
                            value,
                            false,
                        )
                    }
                })();
                for root in roots {
                    self.release_value(root);
                }
                return result;
            }
            Action::AsyncDelegateMissingThrowClose { .. } => {
                // Close validates only the awaited object, not done or value.
                self.async_result_object(&value)?;
                return Err(self.property_type_error("Delegated iterator has no throw method"));
            }
            Action::AsyncDelegateReturnAbsent { .. } => {
                machine.abrupt = Some(Completion::Return(value));
            }
            Action::Await(next) => {
                return self.continuation_suspend(machine, scope, *next, value, true);
            }
            Action::Yield(next) => {
                if machine.kind == FunctionKind::AsyncGenerator {
                    return self.continuation_suspend(
                        machine,
                        scope,
                        Action::AsyncYieldReady(next),
                        value,
                        true,
                    );
                }
                return self.continuation_suspend(machine, scope, *next, value, false);
            }
            Action::AsyncYieldReady(next) => {
                return self.continuation_suspend(machine, scope, *next, value, false);
            }
            Action::BinaryLeft {
                operator,
                right,
                next,
            } => {
                let short = operator == "&&" && !value.truthy()
                    || operator == "||" && value.truthy()
                    || operator == "??" && !matches!(&value, Value::Null | Value::Undefined);
                if short {
                    machine.push(Frame::Apply(value, scope, *next))?;
                } else if matches!(operator.as_str(), "&&" | "||" | "??" | ",") {
                    machine.push(Frame::Evaluate(*right, scope, *next))?;
                } else {
                    let action = Action::BinaryRight {
                        operator,
                        left: value,
                        next,
                    };
                    machine.push(Frame::Evaluate(*right, scope, action))?;
                }
            }
            Action::BinaryRight {
                operator,
                left,
                next,
            } => {
                let value = self.binary_value(&operator, left, value, host)?;
                machine.push(Frame::Apply(value, scope, *next))?;
            }
            Action::Conditional { yes, no, next } => {
                let branch = if value.truthy() { yes } else { no };
                machine.push(Frame::Evaluate(*branch, scope, *next))?;
            }
            Action::Unary { operator, next } => {
                let value = self.unary_value(&operator, value, host)?;
                machine.push(Frame::Apply(value, scope, *next))?;
            }
            Action::Assign {
                place,
                previous,
                operator,
                next,
            } => {
                let mut retained = vec![value.clone()];
                place.trace_values(&mut retained, &mut Vec::new());
                retained.extend(previous.iter().cloned());
                next.trace(&mut retained, &mut Vec::new());
                let roots = self.continuation_retain_values(retained)?;
                let result = (|| {
                    let value = match previous {
                        Some(left) => {
                            let binary = operator.strip_suffix('=').ok_or_else(|| {
                                JsError::runtime("Invalid suspended assignment operator")
                            })?;
                            self.binary(binary, left, value, host)?
                        }
                        None => value,
                    };
                    self.write_place(place, value.clone(), host)?;
                    Ok::<Value, JsError>(value)
                })();
                for root in roots {
                    self.release_value(root);
                }
                machine.push(Frame::Apply(result?, scope, *next))?;
            }
            Action::Discard => {}
            Action::Return => machine.abrupt = Some(Completion::Return(value)),
            Action::Throw => machine.abrupt = Some(Completion::Throw(thrown(value))),
            Action::LoopCondition(condition, body) => {
                if value.truthy() {
                    machine.push(Frame::Loop(condition, body.clone(), scope))?;
                    machine.push(Frame::Statement(*body, scope))?;
                }
            }
            Action::Switch(clauses) => {
                machine.push(Frame::SwitchReady(clauses, value, scope))?;
            }
            Action::ForEachSource(binding, body, of, outer) => {
                machine.push(Frame::ForEachReady(binding, body, of, value, outer))?;
            }
            Action::AsyncForSource(binding, body, outer) => {
                let iterator = self.async_iterator(value.clone(), host)?;
                machine.push(Frame::AsyncOfNext(
                    AsyncOfState {
                        binding,
                        body,
                        source: value,
                        iterator,
                        count: 0,
                    },
                    outer,
                ))?;
            }
            Action::AsyncOfResult(mut state) => {
                self.async_result_object(&value)?;
                let done = self.get(value.clone(), "done", host)?.truthy();
                if !done {
                    // Native async iteration deliberately does not await value.
                    let value = self.get(value, "value", host)?;
                    state.count += 1;
                    if state.count > 4096 {
                        return Err(JsError::resource(
                            "For-await-of iteration item limit exceeded",
                        ));
                    }
                    let binding = state.binding.clone();
                    let body = state.body.clone();
                    // Binding failures must close too; next/result failures must not.
                    machine.push(Frame::AsyncOfBody(state, scope))?;
                    let iteration = self.iteration_scope(&binding, value, scope, host)?;
                    machine.push(Frame::Statement(*body, iteration))?;
                }
            }
            Action::AsyncOfClose(_, completion) => {
                let close = self.async_result_object(&value);
                Self::complete_async_close(machine, completion, close)?;
            }
            Action::ForCondition(state) => {
                if value.truthy() {
                    let body = state.body.clone();
                    machine.push(Frame::ForNext(state, scope))?;
                    machine.push(Frame::Statement(*body, scope))?;
                }
            }
            Action::Branch(yes, no) => {
                let next = if value.truthy() { Some(yes) } else { no };
                if let Some(next) = next {
                    machine.push(Frame::Statement(*next, scope))?;
                }
            }
            Action::Bind(kind, name) => {
                let target = if kind == Declaration::Var {
                    self.function_scope(scope)
                } else {
                    scope
                };
                if !self.scopes[target].bindings.contains_key(&name) {
                    self.declare(
                        target,
                        &name,
                        kind != Declaration::Const,
                        kind == Declaration::Var,
                    )?;
                }
                let binding = self.scopes[target]
                    .bindings
                    .get_mut(&name)
                    .ok_or_else(|| JsError::runtime("Missing continuation binding"))?;
                binding.value = value;
                binding.initialized = true;
            }
        }
        Ok(None)
    }
    /// Pending actions trace these same values across suspension. Retain the
    /// evaluated temporaries separately while getters and methods execute.
    fn async_delegate_roots(
        &mut self,
        state: &AsyncDelegateState,
        next: &Action,
        value: &Value,
    ) -> Result<Vec<u64>, JsError> {
        let mut values = vec![state.source.clone(), value.clone()];
        state.iterator.trace_values(&mut values);
        next.trace(&mut values, &mut Vec::new());
        self.continuation_retain_values(values)
    }
    fn continuation_retain_values(&mut self, values: Vec<Value>) -> Result<Vec<u64>, JsError> {
        let mut roots = Vec::new();
        let mut retained = BTreeSet::new();
        for value in values {
            // Strings, BigInts and other primitives are owned Rust values, not
            // GC heap edges. A wide numeric literal needs no external tokens.
            let id = match &value {
                Value::Array(id) | Value::Object(id) | Value::Function(id) => *id,
                _ => continue,
            };
            // Nested literal actions can contain thousands of references to
            // one object. Each heap identity needs only one temporary root.
            if !retained.insert(id) {
                continue;
            }
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
        Ok(roots)
    }
    fn continuation_async_delegate(
        &mut self,
        machine: &mut Machine,
        scope: usize,
        mut state: AsyncDelegateState,
        next: Action,
        input: ResumeInput,
        host: &mut impl Host,
    ) -> Result<Option<Outcome>, JsError> {
        self.tick()?;
        if machine.depth() + 2 + next.depth() > LIMIT {
            return Err(JsError::resource(
                "Delegation continuation depth limit exceeded",
            ));
        }
        if state.steps >= 4096 {
            return Err(JsError::resource("Delegated iterator step limit exceeded"));
        }
        state.steps += 1;
        let (operation, value) = match input {
            ResumeInput::Normal(value) => (AsyncIteratorOperation::Next, value),
            ResumeInput::Throw(value) => (AsyncIteratorOperation::Throw, value),
            ResumeInput::Return(value) => (AsyncIteratorOperation::Return, value),
        };
        let roots = self.async_delegate_roots(&state, &next, &value)?;
        let result = (|| {
            let result = self.async_iterator_operation(
                &mut state.iterator,
                machine.owner,
                operation,
                Some(value.clone()),
                host,
            )?;
            let next = Box::new(next);
            match result {
                Some(result) => self.continuation_suspend(
                    machine,
                    scope,
                    Action::AsyncDelegateResult {
                        state: Box::new(state),
                        next,
                        returning: matches!(operation, AsyncIteratorOperation::Return),
                    },
                    result,
                    true,
                ),
                None if matches!(operation, AsyncIteratorOperation::Return) => {
                    // Required even after the public return request was awaited.
                    self.continuation_suspend(
                        machine,
                        scope,
                        Action::AsyncDelegateReturnAbsent {
                            state: Box::new(state),
                            next,
                        },
                        value,
                        true,
                    )
                }
                None => {
                    // Missing throw closes normally: close errors take priority.
                    let close = self.async_iterator_operation(
                        &mut state.iterator,
                        machine.owner,
                        AsyncIteratorOperation::Return,
                        None,
                        host,
                    )?;
                    match close {
                        Some(result) => self.continuation_suspend(
                            machine,
                            scope,
                            Action::AsyncDelegateMissingThrowClose {
                                state: Box::new(state),
                                next,
                            },
                            result,
                            true,
                        ),
                        None => {
                            Err(self.property_type_error("Delegated iterator has no throw method"))
                        }
                    }
                }
            }
        })();
        for root in roots {
            self.release_value(root);
        }
        result
    }
    /// Synchronous yield* protocol: custom results are passed through unchanged.
    /// Only a done=true result has value read by this delegation path.
    fn continuation_delegate(
        &mut self,
        machine: &mut Machine,
        scope: usize,
        mut state: DelegateState,
        next: Action,
        input: ResumeInput,
        host: &mut impl Host,
    ) -> Result<Option<Outcome>, JsError> {
        self.tick()?;
        if machine.depth() + 2 + next.depth() > LIMIT {
            return Err(JsError::resource(
                "Delegation continuation depth limit exceeded",
            ));
        }
        if state.steps >= 4096 {
            return Err(JsError::resource("Delegated iterator step limit exceeded"));
        }
        state.steps += 1;
        state.last_result = None;
        let returning = matches!(&input, ResumeInput::Return(_));
        let custom = match &state.iterator {
            IteratorRecord::Custom { object, next, .. } => Some((object.clone(), next.clone())),
            _ => None,
        };
        let result = match input {
            ResumeInput::Normal(value) => {
                if let Some((object, method)) = custom {
                    // Even the first call receives exactly one undefined argument.
                    self.call(method, vec![value], object, host)?
                } else {
                    let item = self.iterator_next(&mut state.iterator, host)?;
                    let done = item.is_none();
                    self.ordinary_object(BTreeMap::from([
                        ("value".into(), item.unwrap_or(Value::Undefined)),
                        ("done".into(), Value::Bool(done)),
                    ]))?
                }
            }
            ResumeInput::Throw(value) => {
                let method = match &custom {
                    Some((object, _)) => self.get(object.clone(), "throw", host)?,
                    None => Value::Undefined,
                };
                if matches!(method, Value::Undefined | Value::Null) {
                    // Missing throw closes with a NORMAL completion: close
                    // errors win, otherwise the missing-method TypeError wins.
                    self.iterator_close(&mut state.iterator, host)?;
                    return Err(self.property_type_error("Delegated iterator has no throw method"));
                }
                if !self.is_callable_value(&method, host) {
                    return Err(
                        self.property_type_error("Delegated iterator throw is not callable")
                    );
                }
                let object = custom
                    .ok_or_else(|| JsError::runtime("Missing delegate object"))?
                    .0;
                self.call(method, vec![value], object, host)?
            }
            ResumeInput::Return(value) => {
                let method = match &custom {
                    Some((object, _)) => self.get(object.clone(), "return", host)?,
                    None => Value::Undefined,
                };
                if matches!(method, Value::Undefined | Value::Null) {
                    machine.abrupt = Some(Completion::Return(value));
                    return Ok(None);
                }
                if !self.is_callable_value(&method, host) {
                    return Err(
                        self.property_type_error("Delegated iterator return is not callable")
                    );
                }
                let object = custom
                    .ok_or_else(|| JsError::runtime("Missing delegate object"))?
                    .0;
                self.call(method, vec![value], object, host)?
            }
        };
        if !matches!(
            &result,
            Value::Object(_)
                | Value::Array(_)
                | Value::Function(_)
                | Value::Native(_)
                | Value::Host(_)
        ) {
            return Err(self.property_type_error("Delegated iterator result must be an object"));
        }
        let done = self.get(result.clone(), "done", host)?.truthy();
        if done {
            let value = self.get(result, "value", host)?;
            if returning {
                machine.abrupt = Some(Completion::Return(value));
            } else {
                // In particular, delegate.throw(done=true) is NORMAL yield*
                // completion, so the enclosing expression/body continues.
                machine.push(Frame::Apply(value, scope, next))?;
            }
            return Ok(None);
        }
        state.last_result = Some(result.clone());
        machine.pending = Some((
            scope,
            Action::DelegateResume {
                state: Box::new(state),
                next: Box::new(next),
            },
        ));
        Ok(Some(Outcome::YieldResult(result)))
    }
    fn continuation_suspend(
        &mut self,
        machine: &mut Machine,
        scope: usize,
        next: Action,
        value: Value,
        awaiting: bool,
    ) -> Result<Option<Outcome>, JsError> {
        self.tick()?;
        if machine.depth() + 1 + next.depth() > LIMIT {
            return Err(JsError::resource(
                "Continuation expression depth limit exceeded",
            ));
        }
        // Operand evaluation is now complete. The caller owns the produced
        // value; normal resumption feeds the next action, abrupt resumption
        // discards it (including any enclosing suspension or pending write).
        machine.pending = Some((scope, next));
        Ok(Some(if awaiting {
            Outcome::Await(value)
        } else {
            Outcome::Yield(value)
        }))
    }
    pub(super) fn resumable_trace(&self, owner: usize) -> (Vec<Value>, Vec<usize>) {
        let Some(machine) = self.resumables.machines.get(&owner) else {
            return (Vec::new(), Vec::new());
        };
        let mut scopes = vec![machine.scope];
        let mut values = Vec::new();
        if let Some(completion) = &machine.abrupt {
            completion.trace(&mut values);
        }
        for frame in &machine.frames {
            if let Frame::Finally(Some(completion), _) = frame {
                completion.trace(&mut values);
            }
            match frame {
                Frame::Evaluate(_, _, action) => action.trace(&mut values, &mut scopes),
                Frame::Apply(value, _, action) => {
                    values.push(value.clone());
                    action.trace(&mut values, &mut scopes);
                }
                Frame::OfNext(state, _) | Frame::OfBody(state, _) => {
                    values.push(state.source.clone());
                    state.iterator.trace_values(&mut values);
                }
                Frame::AsyncOfNext(state, _) | Frame::AsyncOfBody(state, _) => {
                    state.trace(&mut values)
                }
                Frame::InNext(state, _) => values.push(state.source.clone()),
                Frame::ForEachReady(_, _, _, value, _) | Frame::SwitchReady(_, value, _) => {
                    values.push(value.clone())
                }
                _ => {}
            }
            scopes.push(match frame {
                Frame::Sequence(_, _, scope)
                | Frame::Statement(_, scope)
                | Frame::Evaluate(_, scope, _)
                | Frame::Apply(_, scope, _)
                | Frame::Variables(_, _, _, scope)
                | Frame::Loop(_, _, scope)
                | Frame::ForStart(_, scope)
                | Frame::ForTest(_, scope)
                | Frame::ForNext(_, scope)
                | Frame::OfNext(_, scope)
                | Frame::OfBody(_, scope)
                | Frame::AsyncOfNext(_, scope)
                | Frame::AsyncOfBody(_, scope)
                | Frame::InNext(_, scope)
                | Frame::ForEachReady(_, _, _, _, scope)
                | Frame::SwitchReady(_, _, scope)
                | Frame::SwitchBoundary(scope)
                | Frame::Handler { scope, .. }
                | Frame::Finally(_, scope) => *scope,
            });
        }
        if let Some((scope, action)) = &machine.pending {
            scopes.push(*scope);
            action.trace(&mut values, &mut scopes);
        }
        // Expressions contain parser literals, not evaluated heap temporaries. The
        // suspension's produced value is held by the caller/promise reaction.
        (values, scopes)
    }
    pub(super) fn resumable_sweep(&mut self, live: &[bool]) {
        self.resumables.machines.retain(|owner, machine| {
            if live.get(*owner) != Some(&true) {
                return false;
            }
            for frame in &mut machine.frames {
                if let Frame::Handler {
                    diagnostic: Some(diagnostic),
                    ..
                } = frame
                {
                    diagnostic.clear_dead_identity(live);
                }
            }
            true
        });
    }
}
fn thrown(value: Value) -> JsError {
    let mut error = JsError::runtime("Uncaught exception in suspended function");
    error.thrown = Some(value);
    error
}

/// Reject unsupported shapes before executing any part of this function. Nested
/// function bodies are boundaries and must be validated when those functions are
/// created. This also rejects suspension hidden in patterns and class fields.
pub(super) fn validate_body(body: &[Stmt], kind: FunctionKind) -> Result<(), JsError> {
    let mut validator = Validator { kind, work: 0 };
    for statement in body {
        validator.statement(statement, true, 0)?;
    }
    Ok(())
}
struct Validator {
    kind: FunctionKind,
    work: usize,
}
impl Validator {
    fn step(&mut self, depth: usize) -> Result<(), JsError> {
        self.work += 1;
        if depth > LIMIT || self.work > 65_536 {
            return Err(JsError::resource("Continuation validation budget exceeded"));
        }
        Ok(())
    }
    fn expression(&mut self, expr: &Expr, direct: bool, depth: usize) -> Result<(), JsError> {
        self.step(depth)?;
        let d = depth + 1;
        match expr {
            Expr::Await(value) => {
                if !direct
                    || !matches!(
                        self.kind,
                        FunctionKind::Async | FunctionKind::AsyncGenerator
                    )
                {
                    return Err(unsupported());
                }
                self.expression(value, true, d)?;
            }
            Expr::YieldDelegate(value) => {
                if !direct
                    || !matches!(
                        self.kind,
                        FunctionKind::Generator | FunctionKind::AsyncGenerator
                    )
                {
                    return Err(unsupported());
                }
                self.expression(value, true, d)?;
            }
            Expr::Yield(value) => {
                if !direct
                    || !matches!(
                        self.kind,
                        FunctionKind::Generator | FunctionKind::AsyncGenerator
                    )
                {
                    return Err(unsupported());
                }
                if let Some(value) = value {
                    self.expression(value, true, d)?;
                }
            }
            Expr::Function { .. } => {}
            Expr::Class(class) => self.class(class, d)?,
            Expr::Array(values) => {
                for value in values {
                    if let Expr::Spread(operand) = value {
                        self.step(d)?;
                        self.expression(operand, direct, d + 1)?;
                    } else {
                        self.expression(value, direct, d)?;
                    }
                }
            }
            Expr::Template(values) => {
                for value in values {
                    self.expression(value, direct, d)?;
                }
            }
            Expr::Object(values) => {
                for (_, value) in values {
                    self.expression(value, false, d)?;
                }
            }
            Expr::Assign(operator, target, right) => {
                let allowed = direct && resumable_assignment(operator, target);
                self.expression(target, allowed && ordinary_member_reference(target), d)?;
                self.expression(right, allowed, d)?;
            }
            Expr::Binary(operator, a, b) => {
                let allowed = direct && resumable_binary(operator);
                self.expression(a, allowed, d)?;
                self.expression(b, allowed, d)?;
            }
            Expr::Member(a, b) => {
                let allow = direct && !matches!(a.as_ref(), Expr::Super);
                self.expression(a, allow, d)?;
                self.expression(b, allow, d)?;
            }
            Expr::ImportCall { specifier, options } => {
                self.expression(specifier, false, d)?;
                if let Some(options) = options {
                    self.expression(options, false, d)?;
                }
            }
            Expr::Call(callee, args) => {
                let allow = direct && resumable_call(callee, args);
                self.expression(callee, allow, d)?;
                for arg in args {
                    self.expression(arg, allow, d)?;
                }
            }
            Expr::Construct(callee, args) => {
                self.expression(callee, false, d)?;
                for arg in args {
                    self.expression(arg, false, d)?;
                }
            }
            Expr::Unary(operator, value) => {
                let allow = resumable_unary(operator)
                    || (operator == "delete" && ordinary_member_reference(value));
                self.expression(value, direct && allow, d)?
            }
            Expr::Update(value, _, _) => {
                self.expression(value, direct && ordinary_member_reference(value), d)?
            }
            Expr::Spread(value) | Expr::PrivateMember(value, _) => {
                self.expression(value, false, d)?
            }
            Expr::Conditional(a, b, c) => {
                self.expression(a, direct, d)?;
                self.expression(b, direct, d)?;
                self.expression(c, direct, d)?;
            }
            Expr::ImportMeta
            | Expr::Literal(_)
            | Expr::RegExp(..)
            | Expr::Name(_)
            | Expr::Hole
            | Expr::Super => {}
        }
        Ok(())
    }
    fn class(&mut self, class: &ClassDefinition, depth: usize) -> Result<(), JsError> {
        self.step(depth)?;
        if let Some(base) = &class.base {
            self.expression(base, false, depth + 1)?;
        }
        for member in &class.members {
            if let Some(value) = &member.value {
                self.expression(value, false, depth + 1)?;
            }
        }
        Ok(())
    }
    fn pattern(&mut self, pattern: &Pattern, depth: usize) -> Result<(), JsError> {
        self.step(depth)?;
        let d = depth + 1;
        match pattern {
            Pattern::Name(_) => {}
            Pattern::Default(pattern, expr) => {
                self.pattern(pattern, d)?;
                self.expression(expr, false, d)?;
            }
            Pattern::Array(values, rest) => {
                for value in values.iter().flatten() {
                    self.pattern(value, d)?;
                }
                if let Some(rest) = rest {
                    self.pattern(rest, d)?;
                }
            }
            Pattern::Object(values, rest) => {
                for (_, value) in values {
                    self.pattern(value, d)?;
                }
                if let Some(rest) = rest {
                    self.pattern(rest, d)?;
                }
            }
        }
        Ok(())
    }
    fn binding(&mut self, binding: &ForBinding, depth: usize) -> Result<(), JsError> {
        match binding {
            ForBinding::Declaration(..) => Ok(()),
            ForBinding::Reference(expr) => self.expression(expr, false, depth),
            ForBinding::Pattern(_, pattern) => self.pattern(pattern, depth),
        }
    }
    fn statement(&mut self, statement: &Stmt, allow: bool, depth: usize) -> Result<(), JsError> {
        self.step(depth)?;
        let d = depth + 1;
        match statement {
            Stmt::Expression(expr) | Stmt::Throw(expr) | Stmt::Return(Some(expr)) => {
                self.expression(expr, allow, d)?
            }
            Stmt::Variables(_, values) => {
                for (_, value) in values {
                    if let Some(value) = value {
                        self.expression(value, allow, d)?;
                    }
                }
            }
            Stmt::PatternVariables(_, values) => {
                for (pattern, value) in values {
                    self.pattern(pattern, d)?;
                    if let Some(value) = value {
                        self.expression(value, false, d)?;
                    }
                }
            }
            Stmt::Block(body) => {
                for statement in body {
                    self.statement(statement, allow, d)?;
                }
            }
            Stmt::If(expr, yes, no) => {
                self.expression(expr, allow, d)?;
                self.statement(yes, allow, d)?;
                if let Some(no) = no {
                    self.statement(no, allow, d)?;
                }
            }
            Stmt::While(expr, body) | Stmt::DoWhile(body, expr) => {
                self.expression(expr, allow, d)?;
                self.statement(body, allow, d)?;
            }
            Stmt::For(init, condition, update, body) => {
                if let Some(init) = init {
                    if allow
                        && self.kind != FunctionKind::Ordinary
                        && !matches!(
                            init.as_ref(),
                            Stmt::Variables(..) | Stmt::Expression(_) | Stmt::Empty
                        )
                    {
                        return Err(JsError::syntax(
                            0,
                            "Resumable for initialization supports simple named declarations or an expression",
                        ));
                    }
                    self.statement(init, allow, d)?;
                }
                for expr in [condition, update].into_iter().flatten() {
                    self.expression(expr, allow, d)?;
                }
                self.statement(body, allow, d)?;
            }
            Stmt::ForAwaitOf(binding, expr, body) => {
                if !allow
                    || !matches!(
                        self.kind,
                        FunctionKind::Async | FunctionKind::AsyncGenerator
                    )
                {
                    return Err(unsupported());
                }
                self.binding(binding, d)?;
                self.expression(expr, allow, d)?;
                self.statement(body, allow, d)?;
            }
            Stmt::ForIn(binding, expr, body) | Stmt::ForOf(binding, expr, body) => {
                self.binding(binding, d)?;
                self.expression(expr, allow, d)?;
                self.statement(body, allow, d)?;
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                for statement in body {
                    self.statement(statement, allow, d)?;
                }
                if let Some((_, body)) = catch {
                    for statement in body {
                        self.statement(statement, allow, d)?;
                    }
                }
                if let Some(body) = finally {
                    for statement in body {
                        self.statement(statement, allow, d)?;
                    }
                }
            }
            Stmt::Switch(expr, cases) => {
                self.expression(expr, allow, d)?;
                for (expr, body) in cases {
                    if let Some(expr) = expr {
                        self.expression(expr, false, d)?;
                    }
                    for statement in body {
                        self.statement(statement, allow, d)?;
                    }
                }
            }
            Stmt::Label(_, body) => self.statement(body, false, d)?,
            Stmt::FunctionDefinition(_, expr) => self.expression(expr, false, d)?,
            Stmt::ClassDefinition(_, class) => self.class(class, d)?,
            Stmt::Empty | Stmt::Return(None) | Stmt::Break(_) | Stmt::Continue(_) => {}
        }
        Ok(())
    }
}
fn ordinary_member_reference(target: &Expr) -> bool {
    matches!(target, Expr::Member(base, _) if !matches!(base.as_ref(), Expr::Super))
}
fn resumable_assignment(operator: &str, target: &Expr) -> bool {
    matches!(
        target,
        Expr::Name(_) | Expr::Member(..) | Expr::PrivateMember(..)
    ) && matches!(
        operator,
        "=" | "+="
            | "-="
            | "*="
            | "/="
            | "%="
            | "**="
            | "&="
            | "|="
            | "^="
            | "<<="
            | ">>="
            | ">>>="
    )
}
fn resumable_binary(operator: &str) -> bool {
    matches!(
        operator,
        "+" | "-"
            | "*"
            | "/"
            | "%"
            | "**"
            | "<"
            | ">"
            | "<="
            | ">="
            | "=="
            | "!="
            | "==="
            | "!=="
            | "&"
            | "|"
            | "^"
            | "<<"
            | ">>"
            | ">>>"
            | "&&"
            | "||"
            | "??"
            | ","
            | "in"
            | "instanceof"
    )
}
fn resumable_unary(operator: &str) -> bool {
    matches!(operator, "+" | "-" | "~" | "!" | "void" | "typeof")
}
fn unsupported() -> JsError {
    JsError::syntax(
        0,
        "Unsupported suspension position: ordinary member reads and assignment/update/delete references, call callees/arguments, unary/binary/conditional expressions, assignment RHS, arrays/templates and nested await/yield operands are supported; special calls/import arguments, private/super references, generic delete operands, objects/classes and case tests are not",
    )
}

// These special references retain the synchronous evaluator's dedicated paths.
fn resumable_call(callee: &Expr, args: &[Expr]) -> bool {
    !args.iter().any(|arg| matches!(arg, Expr::Spread(_)))
        && !matches!(callee, Expr::Super | Expr::PrivateMember(..))
        && !matches!(callee, Expr::Name(name) if name == "eval")
        && !matches!(callee, Expr::Member(base, _) if matches!(base.as_ref(), Expr::Super))
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
            Err(JsError::runtime("No host"))
        }
        fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
            Err(JsError::runtime("No host"))
        }
        fn call(&mut self, _: u64, _: &str, _: &[Value]) -> Result<Value, JsError> {
            Err(JsError::runtime("No host"))
        }
    }
    fn machine(source: &str, kind: FunctionKind) -> Runtime {
        let statements = crate::parser::parse(source).unwrap();
        let Stmt::FunctionDefinition(_, Expr::Function { body, .. }) = &statements[0] else {
            panic!("Expected resumable declaration");
        };
        machine_body(body.clone(), kind)
    }
    fn machine_body(body: Arc<Vec<Stmt>>, kind: FunctionKind) -> Runtime {
        let mut runtime = Runtime::default();
        let owner = runtime
            .allocate(crate::Heap::Object(crate::Object {
                properties: BTreeMap::new(),
                non_enumerable: BTreeSet::new(),
                prototype: None,
                regexp: None,
            }))
            .unwrap();
        assert_eq!(owner, 0);
        runtime
            .resumable_start(0, body.clone(), 0, false, kind)
            .unwrap();
        runtime
    }
    fn next(runtime: &mut Runtime) -> Outcome {
        runtime
            .resumable_resume(0, ResumeInput::Normal(Value::Undefined), &mut Empty)
            .unwrap()
    }
    fn yielded(outcome: Outcome, number: f64) {
        assert!(matches!(outcome, Outcome::Yield(Value::Number(n)) if n == number));
    }
    fn completed(outcome: Outcome, number: f64) {
        assert!(matches!(outcome, Outcome::Completed(Value::Number(n)) if n == number));
    }
    fn resume_value(runtime: &mut Runtime, value: Value) -> Outcome {
        runtime
            .resumable_resume(0, ResumeInput::Normal(value), &mut Empty)
            .unwrap()
    }
    #[test]
    fn member_delete_suspends_without_getvalue_or_replaying_in_all_kinds() {
        for (header, suspend, kind) in [
            ("function*", "yield", FunctionKind::Generator),
            ("async function", "await", FunctionKind::Async),
            ("async function*", "await", FunctionKind::AsyncGenerator),
            ("async function*", "yield", FunctionKind::AsyncGenerator),
        ] {
            for (expression, pauses) in [
                (format!("delete ({suspend} base())[key()]"), 1),
                (format!("delete base()[{suspend} key()]"), 1),
                (format!("delete ({suspend} base())[{suspend} key()]"), 2),
            ] {
                let source = format!(
                    "{header} f(){{let log='';let obj={{}};Object.defineProperty(obj,'x',{{configurable:true,get:function(){{throw 99;}}}});function base(){{log+='b';return obj;}}function key(){{log+='k';return {{toString:function(){{log+='c';return 'x';}}}};}}let result={expression};return log+':'+result+':'+('x' in obj);}}"
                );
                let mut runtime = machine(&source, kind);
                let mut outcome = next(&mut runtime);
                for _ in 0..pauses {
                    if kind == FunctionKind::AsyncGenerator && suspend == "yield" {
                        let Outcome::Await(value) = outcome else {
                            panic!("Expected async-generator yield assimilation, got {outcome:?}");
                        };
                        outcome = resume_value(&mut runtime, value);
                    }
                    let value = match outcome {
                        Outcome::Yield(value) if suspend == "yield" => value,
                        Outcome::Await(value) if suspend == "await" => value,
                        other => panic!("Expected delete suspension, got {other:?}"),
                    };
                    // The consumer owns the yielded/awaited value; ephemeral keys
                    // must be external roots while collection runs between resumes.
                    runtime
                        .collect_garbage(&[Value::Object(0), value.clone()], &Empty)
                        .unwrap();
                    if let Value::Object(id) = &value {
                        assert!(
                            !matches!(runtime.heap.get(*id), Some(crate::Heap::Vacant)),
                            "Consumer value {value:?} collected in {header}: {expression}"
                        );
                    }
                    outcome = resume_value(&mut runtime, value);
                }
                if kind == FunctionKind::AsyncGenerator {
                    let Outcome::Await(value) = outcome else {
                        panic!("Expected async-generator return assimilation, got {outcome:?}");
                    };
                    outcome = resume_value(&mut runtime, value);
                }
                assert!(matches!(outcome, Outcome::Completed(value)
                    if value == Value::string("bkc:true:false")));
            }
        }
    }

    #[test]
    fn member_delete_keeps_symbol_reference_and_array_holes() {
        let mut runtime = machine(
            "function* f(){let key=Symbol('x');let obj={};obj[key]=4;let result=delete (yield obj)[yield key];let a=[1,2];let hole=delete a[yield 0];let absent=delete a[yield 9];return result&&!(key in obj)&&hole&&absent&&!(0 in a)&&a.length===2;}",
            FunctionKind::Generator,
        );
        let mut outcome = next(&mut runtime);
        for _ in 0..4 {
            let Outcome::Yield(value) = outcome else {
                panic!("Expected reference");
            };
            runtime
                .collect_garbage(&[Value::Object(0)], &Empty)
                .unwrap();
            outcome = resume_value(&mut runtime, value);
        }
        assert!(matches!(outcome, Outcome::Completed(Value::Bool(true))));
    }

    #[test]
    fn member_delete_nullish_and_abrupt_keys_preserve_order() {
        for base in ["null", "undefined", "({x:1})"] {
            let source = format!(
                "function* f(){{let log='';let key={{toString:function(){{log+='c';throw 7;}}}};function keyExpression(){{log+='k';return key;}}try{{delete {base}[yield keyExpression()];}}catch(e){{return log;}}return 'bad';}}"
            );
            let mut runtime = machine(&source, FunctionKind::Generator);
            let Outcome::Yield(key) = next(&mut runtime) else {
                panic!("Expected key");
            };
            let expected = if base == "({x:1})" { "kc" } else { "k" };
            assert!(
                matches!(resume_value(&mut runtime, key), Outcome::Completed(value)
                if value == Value::string(expected))
            );
        }
    }

    #[test]
    fn member_delete_abrupt_resumption_never_deletes_and_runs_finally() {
        for phase in 0..2 {
            for returning in [false, true] {
                let mut runtime = machine(
                    "function* f(){let obj={x:1};try{delete (yield obj)[yield 'x'];}finally{yield obj.x;}}",
                    FunctionKind::Generator,
                );
                let mut outcome = next(&mut runtime);
                for _ in 0..phase {
                    let Outcome::Yield(value) = outcome else {
                        panic!("Expected reference");
                    };
                    outcome = resume_value(&mut runtime, value);
                }
                let input = if returning {
                    ResumeInput::Return(Value::Number(7.))
                } else {
                    ResumeInput::Throw(Value::Number(7.))
                };
                yielded(runtime.resumable_resume(0, input, &mut Empty).unwrap(), 1.);
                let result =
                    runtime.resumable_resume(0, ResumeInput::Normal(Value::Undefined), &mut Empty);
                if returning {
                    completed(result.unwrap(), 7.);
                } else {
                    assert_eq!(result.unwrap_err().thrown, Some(Value::Number(7.)));
                }
            }
        }
    }

    #[test]
    fn member_delete_async_rejection_preserves_property_and_finally() {
        for expression in [
            "delete (await Promise.reject(7))[await 'x']",
            "delete (await obj)[await Promise.reject(7)]",
        ] {
            let mut runtime = Runtime::default();
            let source = format!(
                "let result='';let obj={{x:1}};async function f(){{try{{{expression};}}catch(e){{result+=e;}}finally{{result+=':'+obj.x;}}}}f();"
            );
            runtime.execute(&source, &mut Empty).unwrap();
            assert_eq!(
                runtime.execute("result", &mut Empty).unwrap(),
                Value::string("7:1")
            );
        }
    }

    #[test]
    fn member_delete_validation_preserves_private_super_and_generic_boundaries() {
        let suspended = || Expr::Yield(Some(Box::new(Expr::Literal(Value::Number(1.)))));
        for target in [
            suspended(),
            Expr::PrivateMember(Box::new(suspended()), "x".into()),
            Expr::Member(Box::new(Expr::Super), Box::new(suspended())),
        ] {
            let expression = Expr::Unary("delete".into(), Box::new(target));
            assert_eq!(
                validate_body(&[Stmt::Return(Some(expression))], FunctionKind::Generator)
                    .unwrap_err()
                    .kind,
                ErrorKind::Syntax
            );
        }
    }

    #[test]
    fn member_delete_respects_strictness_after_resumption() {
        for (directive, expected) in [("", "false:1"), ("'use strict';", "TypeError:1")] {
            let mut runtime = Runtime::default();
            let source = format!(
                "function* f(){{{directive}let obj={{}};Object.defineProperty(obj,'x',{{value:1,configurable:false}});try{{return (delete obj[yield 'x'])+':'+obj.x;}}catch(e){{return e.name+':'+obj.x;}}}}let it=f();it.next();it.next('x').value;"
            );
            assert_eq!(
                runtime.execute(&source, &mut Empty).unwrap(),
                Value::string(expected)
            );
        }
    }

    #[test]
    fn member_assignment_suspends_base_key_rhs_in_all_resumable_kinds() {
        for (header, suspend, kind) in [
            ("function*", "yield", FunctionKind::Generator),
            ("async function", "await", FunctionKind::Async),
            ("async function*", "await", FunctionKind::AsyncGenerator),
        ] {
            let source = format!(
                "{header} f(){{let log='';let obj={{}};let old={{valueOf:function(){{log+='v';return 4;}}}};Object.defineProperty(obj,'x',{{get:function(){{log+='g';return old;}},set:function(v){{log+='s'+v;}}}});let key={{toString:function(){{log+='k';return 'x';}}}};function rhs(){{log+='r';return 3;}}let result=({suspend} obj)[{suspend} key]+={suspend} rhs();return log+':'+result;}}"
            );
            let mut runtime = machine(&source, kind);
            let mut outcome = next(&mut runtime);
            for expected_log in ["", "", "kgr"] {
                assert_eq!(
                    runtime.scopes[0].bindings["log"].value,
                    Value::string(expected_log)
                );
                let value = match outcome {
                    Outcome::Yield(value) if kind == FunctionKind::Generator => value,
                    Outcome::Await(value) if kind != FunctionKind::Generator => value,
                    other => panic!("Expected reference suspension, got {other:?}"),
                };
                runtime
                    .collect_garbage(&[Value::Object(0)], &Empty)
                    .unwrap();
                outcome = resume_value(&mut runtime, value);
            }
            if kind == FunctionKind::AsyncGenerator {
                let Outcome::Await(value) = outcome else {
                    panic!("Expected async-generator return assimilation, got {outcome:?}");
                };
                outcome = resume_value(&mut runtime, value);
            }
            assert!(
                matches!(&outcome, Outcome::Completed(value) if *value == Value::string("kgrvs7:7")),
                "{header}: {outcome:?}"
            );
        }
    }

    #[test]
    fn simple_member_assignment_omits_getter_and_preserves_destination() {
        let mut runtime = machine(
            "function* f(){let writes=0;let obj={};let original=obj;Object.defineProperty(obj,'x',{get:function(){throw 99;},set:function(v){writes=v;}});let result=(yield obj)[yield 'x']=yield 3;return result+writes;}",
            FunctionKind::Generator,
        );
        let Outcome::Yield(object) = next(&mut runtime) else {
            panic!("Expected base");
        };
        let Outcome::Yield(key) = resume_value(&mut runtime, object) else {
            panic!("Expected key");
        };
        yielded(resume_value(&mut runtime, key), 3.);
        runtime.scopes[0].bindings.get_mut("obj").unwrap().value = Value::Null;
        runtime
            .collect_garbage(&[Value::Object(0)], &Empty)
            .unwrap();
        completed(resume_value(&mut runtime, Value::Number(8.)), 16.);
    }

    #[test]
    fn member_reference_keeps_ephemeral_symbol_destination_across_gc() {
        let mut runtime = machine(
            "function* f(){let key=Symbol('x');function base(){let obj={};obj[key]={valueOf:function(){return 5;}};return obj;}return (yield base())[yield key]+=yield 2;}",
            FunctionKind::Generator,
        );
        let Outcome::Yield(base) = next(&mut runtime) else {
            panic!("Expected base");
        };
        let Outcome::Yield(key) = resume_value(&mut runtime, base) else {
            panic!("Expected key");
        };
        runtime
            .collect_garbage(&[Value::Object(0)], &Empty)
            .unwrap();
        yielded(resume_value(&mut runtime, key), 2.);
        runtime
            .collect_garbage(&[Value::Object(0)], &Empty)
            .unwrap();
        completed(resume_value(&mut runtime, Value::Number(2.)), 7.);
    }

    #[test]
    fn nullish_assignment_and_update_run_key_expression_but_not_coercion_or_rhs() {
        for expression in [
            "null[yield key]=rhs()",
            "null[yield key]+=rhs()",
            "null[yield key]++",
        ] {
            let source = format!(
                "function* f(){{let calls=0;let key={{toString:function(){{calls++;return 'x';}}}};function rhs(){{calls+=10;return 2;}}try{{{expression};}}catch(e){{return calls;}}return 99;}}"
            );
            let mut runtime = machine(&source, FunctionKind::Generator);
            let Outcome::Yield(key) = next(&mut runtime) else {
                panic!("Expected key");
            };
            completed(resume_value(&mut runtime, key), 0.);
        }
    }

    #[test]
    fn suspended_member_updates_convert_once_and_preserve_prefix_postfix_values() {
        for (initial, operation, expected) in [
            ("'4'", "obj[yield 'x']++", "4:5"),
            ("'4'", "++obj[yield 'x']", "5:5"),
            ("4n", "obj[yield 'x']--", "4:3"),
            ("4n", "--obj[yield 'x']", "3:3"),
            (
                "({valueOf:function(){return 4n;}})",
                "++obj[yield 'x']",
                "5:5",
            ),
            ("NaN", "obj[yield 'x']++", "NaN:NaN"),
        ] {
            let source = format!(
                "function* f(){{let obj={{x:{initial}}};let result={operation};return result+':'+obj.x;}}"
            );
            let mut runtime = machine(&source, FunctionKind::Generator);
            assert!(
                matches!(next(&mut runtime), Outcome::Yield(value) if value == Value::string("x"))
            );
            assert!(
                matches!(resume_value(&mut runtime, Value::string("x")), Outcome::Completed(value) if value == Value::string(expected))
            );
        }
        let mut runtime = machine(
            "function* f(){let obj={x:-0};let result=(yield obj)[yield 'x']++;return (1/result===-Infinity)&&(obj.x===1);}",
            FunctionKind::Generator,
        );
        let Outcome::Yield(base) = next(&mut runtime) else {
            panic!("Expected base");
        };
        let Outcome::Yield(key) = resume_value(&mut runtime, base) else {
            panic!("Expected key");
        };
        assert!(matches!(
            resume_value(&mut runtime, key),
            Outcome::Completed(Value::Bool(true))
        ));
    }

    #[test]
    fn abrupt_member_reference_resumption_discards_write_and_runs_finally() {
        for phase in 0..3 {
            for returning in [false, true] {
                let mut runtime = machine(
                    "function* f(){let writes=0;let obj={};Object.defineProperty(obj,'x',{get:function(){return 4;},set:function(v){writes++;}});try{(yield obj)[yield 'x']+=yield 3;}finally{yield writes;}return 99;}",
                    FunctionKind::Generator,
                );
                let mut outcome = next(&mut runtime);
                for _ in 0..phase {
                    let Outcome::Yield(value) = outcome else {
                        panic!("Expected suspension");
                    };
                    outcome = resume_value(&mut runtime, value);
                }
                let input = if returning {
                    ResumeInput::Return(Value::Number(7.))
                } else {
                    ResumeInput::Throw(Value::Number(7.))
                };
                yielded(runtime.resumable_resume(0, input, &mut Empty).unwrap(), 0.);
                let result =
                    runtime.resumable_resume(0, ResumeInput::Normal(Value::Undefined), &mut Empty);
                if returning {
                    completed(result.unwrap(), 7.);
                } else {
                    assert_eq!(result.unwrap_err().thrown, Some(Value::Number(7.)));
                }
            }
        }
    }

    #[test]
    fn member_reference_actions_count_depth_and_trace_nested_values() {
        let mut action = Action::Assign {
            place: Place::Member(Value::Object(10), "x".into()),
            previous: Some(Value::Object(11)),
            operator: "+=".into(),
            next: Box::new(Action::Discard),
        };
        for _ in 0..LIMIT {
            action = Action::ReferenceKey {
                base: Value::Object(12),
                operation: ReferenceOperation::Delete,
                next: Box::new(action),
            };
        }
        assert!(action.depth() > LIMIT);
        let mut values = Vec::new();
        action.trace(&mut values, &mut Vec::new());
        for id in [10, 11, 12] {
            assert!(values.contains(&Value::Object(id)));
        }
    }
    #[test]
    fn async_reference_rejection_abandons_each_pending_phase() {
        for expression in [
            "(await Promise.reject(7))[await 'x']+=await 3",
            "(await obj)[await Promise.reject(7)]+=await 3",
            "(await obj)[await 'x']+=await Promise.reject(7)",
            "++(await obj)[await Promise.reject(7)]",
        ] {
            let source = format!(
                "let result='';let writes=0;let obj={{}};Object.defineProperty(obj,'x',{{get:function(){{return 4;}},set:function(v){{writes++;}}}});async function f(){{try{{{expression};}}catch(e){{result+=e;}}finally{{result+=':'+writes;}}}}f();"
            );
            let mut runtime = Runtime::default();
            runtime.execute(&source, &mut Empty).unwrap();
            assert_eq!(
                runtime.execute("result", &mut Empty).unwrap(),
                Value::string("7:0")
            );
        }
    }

    #[test]
    fn resource_failure_discards_member_reference_without_catch_finally_or_write() {
        for expression in [
            "obj[yield 'x']+=yield 3",
            "obj[yield 'x']++",
            "delete obj[yield 'x']",
        ] {
            let source = format!(
                "function* f(){{let writes=0;let caught=0;let finished=0;let obj={{}};Object.defineProperty(obj,'x',{{get:function(){{return 4;}},set:function(v){{writes++;}}}});try{{{expression};}}catch(e){{caught++;}}finally{{finished++;}}}}"
            );
            let mut runtime = machine(&source, FunctionKind::Generator);
            assert!(matches!(next(&mut runtime), Outcome::Yield(_)));
            runtime.remaining = 0;
            let error = runtime
                .resumable_resume(0, ResumeInput::Normal(Value::string("x")), &mut Empty)
                .unwrap_err();
            assert_eq!(error.kind, ErrorKind::Resource);
            assert!(!runtime.resumable_exists(0));
            for name in ["writes", "caught", "finished"] {
                assert_eq!(runtime.scopes[0].bindings[name].value, Value::Number(0.));
            }
        }
    }

    #[test]
    fn reference_callback_failures_skip_later_phases() {
        for (setup, expression, expected) in [
            (
                "let key={toString:function(){log+='k';throw 7;}};",
                "obj[yield key]+=rhs()",
                "k:7",
            ),
            (
                "let key='x';Object.defineProperty(obj,'x',{get:function(){log+='g';throw 7;}});",
                "obj[yield key]+=rhs()",
                "g:7",
            ),
            (
                "let key='x';obj.x={valueOf:function(){log+='v';throw 7;}};",
                "obj[yield key]+=rhs()",
                "rv:7",
            ),
            (
                "let key='x';Object.defineProperty(obj,'x',{set:function(v){log+='s';throw 7;}});",
                "obj[yield key]=rhs()",
                "rs:7",
            ),
            (
                "let key='x';obj.x=Symbol('n');",
                "obj[yield key]++",
                ":type",
            ),
            (
                "let key='x';obj.x={valueOf:function(){log+='v';throw 7;}};",
                "++obj[yield key]",
                "v:7",
            ),
        ] {
            let source = format!(
                "function* f(){{let log='';let obj={{}};{setup}function rhs(){{log+='r';return 2;}}try{{{expression};}}catch(e){{return log+':'+(e===7?7:'type');}}return 'miss';}}"
            );
            let mut runtime = machine(&source, FunctionKind::Generator);
            let Outcome::Yield(key) = next(&mut runtime) else {
                panic!("Expected key");
            };
            assert!(
                matches!(resume_value(&mut runtime, key), Outcome::Completed(value) if value == Value::string(expected))
            );
        }
    }

    #[test]
    fn awaited_updates_use_shared_numeric_and_accessor_order() {
        for header in ["async function", "async function*"] {
            let kind = if header == "async function" {
                FunctionKind::Async
            } else {
                FunctionKind::AsyncGenerator
            };
            let source = format!(
                "{header} f(){{let log='';let obj={{}};Object.defineProperty(obj,'x',{{get:function(){{log+='g';return {{valueOf:function(){{log+='v';return 4n;}}}};}},set:function(v){{log+='s'+v;}}}});let result=++(await obj)[await 'x'];return log+':'+result;}}"
            );
            let mut runtime = machine(&source, kind);
            let Outcome::Await(base) = next(&mut runtime) else {
                panic!("Expected base");
            };
            let Outcome::Await(key) = resume_value(&mut runtime, base) else {
                panic!("Expected key");
            };
            runtime
                .collect_garbage(&[Value::Object(0)], &Empty)
                .unwrap();
            let mut outcome = resume_value(&mut runtime, key);
            if kind == FunctionKind::AsyncGenerator {
                let Outcome::Await(value) = outcome else {
                    panic!("Expected async-generator return assimilation, got {outcome:?}");
                };
                outcome = resume_value(&mut runtime, value);
            }
            assert!(
                matches!(&outcome, Outcome::Completed(value) if *value == Value::string("gvs5:5")),
                "{header}: {outcome:?}"
            );
        }
    }
    #[test]
    fn suspended_member_base_key_and_getter_run_once_in_order() {
        let mut runtime = machine(
            "function* f(){let log='';let obj={};Object.defineProperty(obj,'x',{get:function(){log+='g';return 7;}});let key={toString:function(){log+='c';return 'x';}};let answer=(yield 1)[yield 2];return log+':'+answer;}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        let object = runtime.scopes[0].bindings["obj"].value.clone();
        yielded(resume_value(&mut runtime, object), 2.);
        assert_eq!(runtime.scopes[0].bindings["log"].value, Value::string(""));
        let key = runtime.scopes[0].bindings["key"].value.clone();
        assert!(matches!(
            resume_value(&mut runtime, key),
            Outcome::Completed(value) if value == Value::string("cg:7")
        ));
    }

    #[test]
    fn suspended_member_call_captures_getter_before_arguments_and_keeps_receiver() {
        let mut runtime = machine(
            "function* f(){let reads=0;let obj={n:10};Object.defineProperty(obj,'m',{get:function(){reads++;return function(x){return this.n+x;};},configurable:true});let answer=(yield 1)[yield 2](yield 3);return answer+reads*100;}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        let object = runtime.scopes[0].bindings["obj"].value.clone();
        yielded(resume_value(&mut runtime, object), 2.);
        yielded(resume_value(&mut runtime, Value::string("m")), 3.);
        let owner_root = runtime.retain_value(Value::Object(0)).unwrap();
        runtime
            .execute(
                "Object.defineProperty(obj,'m',{value:function(){return 999;}});",
                &mut Empty,
            )
            .unwrap();
        runtime.release_value(owner_root);
        completed(resume_value(&mut runtime, Value::Number(4.)), 114.);
    }

    #[test]
    fn arbitrary_callee_suspends_before_arguments_and_uses_undefined_receiver() {
        let mut runtime = machine(
            "function* f(){let fn=function(x){'use strict';return (this===undefined)?x:99;};return (yield 1)(yield 2);}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        let callable = runtime.scopes[0].bindings["fn"].value.clone();
        yielded(resume_value(&mut runtime, callable), 2.);
        completed(resume_value(&mut runtime, Value::Number(7.)), 7.);

        let mut runtime = machine(
            "function* f(){return (yield 1)(yield 2);}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        yielded(resume_value(&mut runtime, Value::Number(0.)), 2.);
        assert_eq!(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Number(7.)), &mut Empty)
                .unwrap_err()
                .kind,
            ErrorKind::Runtime
        );
    }

    #[test]
    fn suspended_member_symbol_reads_and_calls_preserve_identity() {
        let mut runtime = machine(
            "function* f(){let key=Symbol('m');let obj={n:10};obj[key]=function(x){return this.n+x;};let fn=obj[yield 1];return obj[yield 2](yield 3)+(typeof fn==='function'?100:0);}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        let key = runtime.scopes[0].bindings["key"].value.clone();
        yielded(resume_value(&mut runtime, key.clone()), 2.);
        yielded(resume_value(&mut runtime, key), 3.);
        completed(resume_value(&mut runtime, Value::Number(4.)), 114.);
    }

    #[test]
    fn nullish_suspended_member_evaluates_key_but_skips_coercion_and_arguments() {
        for expression in ["null[yield 1]", "null[yield 1](yield 2)"] {
            let mut runtime = machine(
                &format!(
                    "function* f(){{let coerced=0;let key={{toString:function(){{coerced++;return 'x';}}}};try{{{expression};}}catch(e){{return coerced;}}}}"
                ),
                FunctionKind::Generator,
            );
            yielded(next(&mut runtime), 1.);
            let key = runtime.scopes[0].bindings["key"].value.clone();
            completed(resume_value(&mut runtime, key), 0.);
        }
    }

    #[test]
    fn member_key_throw_and_return_discard_getter_and_resume_finally() {
        for input in [
            ResumeInput::Throw(Value::Number(8.)),
            ResumeInput::Return(Value::Number(8.)),
        ] {
            let mut runtime = machine(
                "function* f(){let reads=0;let obj={};Object.defineProperty(obj,'x',{get:function(){reads++;return 1;}});try{return obj[yield 1];}finally{yield reads;}}",
                FunctionKind::Generator,
            );
            yielded(next(&mut runtime), 1.);
            yielded(
                runtime
                    .resumable_resume(0, input.clone(), &mut Empty)
                    .unwrap(),
                0.,
            );
            let result =
                runtime.resumable_resume(0, ResumeInput::Normal(Value::Undefined), &mut Empty);
            match input {
                ResumeInput::Throw(value) => assert_eq!(result.unwrap_err().thrown, Some(value)),
                _ => completed(result.unwrap(), 8.),
            }
        }
    }

    #[test]
    fn pending_member_base_and_captured_call_survive_collection_without_bindings() {
        let mut runtime = machine(
            "function* f(){return ({n:10,m:function(x){return this.n+x;}})[yield 1](yield 2);}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        runtime
            .collect_garbage(&[Value::Object(0)], &Empty)
            .unwrap();
        yielded(resume_value(&mut runtime, Value::string("m")), 2.);
        runtime
            .collect_garbage(&[Value::Object(0)], &Empty)
            .unwrap();
        completed(resume_value(&mut runtime, Value::Number(7.)), 17.);
    }

    #[test]
    fn resumed_callee_argument_limit_keeps_first_32_effects_only() {
        let arguments = vec!["n++"; 33].join(",");
        let source = format!("function* f(){{let n=0;return (yield 1)({arguments});}}");
        assert_eq!(
            crate::parser::parse(&source).unwrap_err().kind,
            ErrorKind::Resource
        );
        // The parser rejects oversized calls. Construct an AST independently to
        // keep exercising the runtime's defense and first-32 evaluation order.
        let arguments = vec!["n++"; 32].join(",");
        let statements = crate::parser::parse(&format!(
            "function* f(){{let n=0;return (yield 1)({arguments});}}"
        ))
        .unwrap();
        let Stmt::FunctionDefinition(_, Expr::Function { body, .. }) = &statements[0] else {
            panic!("Expected generator");
        };
        let mut body = body.as_ref().clone();
        let Stmt::Return(Some(Expr::Call(_, arguments))) = &mut body[1] else {
            panic!("Expected return call");
        };
        arguments.push(arguments[0].clone());
        let mut runtime = machine_body(Arc::new(body), FunctionKind::Generator);
        yielded(next(&mut runtime), 1.);
        let error = runtime
            .resumable_resume(0, ResumeInput::Normal(Value::Number(0.)), &mut Empty)
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert_eq!(runtime.scopes[0].bindings["n"].value, Value::Number(32.));
    }

    #[test]
    fn awaited_member_and_callee_resume_in_source_order() {
        let mut runtime = machine(
            "async function f(){let obj={m:function(x){return x+1;}};let fn=await (await obj)[await 'm'];return (await fn)(await 7);}",
            FunctionKind::Async,
        );
        let mut outcome = next(&mut runtime);
        let mut awaits = 0;
        loop {
            match outcome {
                Outcome::Await(value) => {
                    awaits += 1;
                    outcome = resume_value(&mut runtime, value);
                }
                Outcome::Completed(value) => {
                    assert_eq!(value, Value::Number(8.));
                    break;
                }
                other => panic!("Unexpected outcome: {other:?}"),
            }
        }
        assert_eq!(awaits, 5);
    }

    #[test]
    fn member_actions_count_depth_and_trace_enclosing_temporaries() {
        let action = Action::CallMemberKey {
            base: Value::Object(17),
            arguments: Arc::new(Vec::new()),
            next: Box::new(Action::MemberKey {
                base: Value::Object(18),
                next: Box::new(Action::Return),
            }),
        };
        assert_eq!(action.depth(), 3);
        let mut values = Vec::new();
        action.trace(&mut values, &mut Vec::new());
        assert_eq!(values, vec![Value::Object(17), Value::Object(18)]);
    }

    #[test]
    fn resumed_legacy_host_call_cannot_collect_the_running_machine() {
        #[derive(Default)]
        struct CollectingHost {
            calls: usize,
        }
        impl Host for CollectingHost {
            fn global(&mut self, name: &str) -> Option<Value> {
                (name == "bridge").then_some(Value::Host(1))
            }
            fn get(&mut self, _: u64, _: &str) -> Result<Value, JsError> {
                Ok(Value::Undefined)
            }
            fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
                Err(JsError::runtime("Unexpected host write"))
            }
            fn call(&mut self, _: u64, _: &str, _: &[Value]) -> Result<Value, JsError> {
                Err(JsError::runtime("Expected runtime-aware dispatch"))
            }
            fn call_with_runtime(
                &mut self,
                object: u64,
                method: &str,
                args: &[Value],
                this: &Value,
                runtime: &mut Runtime,
            ) -> Result<Value, JsError> {
                assert_eq!(object, 1);
                assert_eq!(method, "ping");
                assert!(args.is_empty());
                assert_eq!(this, &Value::Host(1));
                assert_eq!(runtime.eval_depth, 0);
                assert!(runtime.call_depth > 0);
                assert!(runtime.resumable_execution_active());
                let stats = runtime.collect_garbage(&[], self)?;
                assert_eq!(stats.reclaimed_objects, 0);
                assert_eq!(stats.reclaimed_scopes, 0);
                self.calls += 1;
                Ok(Value::Undefined)
            }
        }
        let mut runtime = Runtime::default();
        let mut host = CollectingHost::default();
        runtime
            .execute(
                "let answer=0;async function f(){let local={n:7};(await bridge).ping();return local.n;}f().then(function(value){answer=value;});",
                &mut host,
            )
            .unwrap();
        assert_eq!(host.calls, 1);
        assert_eq!(
            runtime.execute("answer", &mut host).unwrap(),
            Value::Number(7.)
        );
        assert!(!runtime.resumable_execution_active());
        assert_eq!(runtime.call_depth, 0);
        assert_eq!(runtime.eval_depth, 0);
    }

    #[test]
    fn suspended_members_preserve_generic_delete_and_special_call_boundaries() {
        for source in [
            "function* f(){delete (yield 1);}",
            "function* f(){let obj={};obj[{x:yield 1}]++;}",
            "function* f(){let obj={};delete obj[{x:yield 1}];}",
            "function* f(){return new (yield 1)();}",
            "function* f(){return (yield 1)(...[]);}",
            "function* f(){return eval(yield 1);}",
            "function* f(){return {x:(yield 1).x};}",
            "function* f(){switch(1){case (yield 1).x:break;}}",
        ] {
            // Function parsing validates suspension positions before exposing the AST.
            let error = crate::parser::parse_module(source).unwrap_err();
            assert_eq!(error.kind, ErrorKind::Syntax, "{source}: {error:?}");
        }
    }

    #[test]
    fn literal_array_preserves_holes_and_spreads_resumed_operand() {
        let mut runtime = machine(
            "function* f(){let a=[,yield 1,...(yield 2),,];return a.length*100+(0 in a)*10+(3 in a)+a[1]+a[2];}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        yielded(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Number(3.)), &mut Empty)
                .unwrap(),
            2.,
        );
        let mut source = Vec::new();
        let mut holes = BTreeSet::new();
        runtime
            .literal_array_append(&mut source, &mut holes, Value::Number(4.), false)
            .unwrap();
        let source = runtime.literal_array_finish(source, holes).unwrap();
        completed(
            runtime
                .resumable_resume(0, ResumeInput::Normal(source), &mut Empty)
                .unwrap(),
            407.,
        );
    }

    #[test]
    fn literal_template_coerces_each_part_before_evaluating_the_next() {
        let mut runtime = machine(
            "function* f(){let calls=0;let obj={toString:function(){calls++;return 'x';}};let s=`${yield 1}:${calls}:${yield 2}`;return s+':'+calls;}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        let object = runtime.scopes[0].bindings["obj"].value.clone();
        yielded(
            runtime
                .resumable_resume(0, ResumeInput::Normal(object), &mut Empty)
                .unwrap(),
            2.,
        );
        let result = runtime
            .resumable_resume(0, ResumeInput::Normal(Value::Number(7.)), &mut Empty)
            .unwrap();
        assert!(matches!(result, Outcome::Completed(value) if value == Value::string("x:1:7:1")));
    }

    #[test]
    fn literal_buffers_are_discarded_on_return_or_throw_without_iterator_close() {
        for input in [
            ResumeInput::Return(Value::Number(8.)),
            ResumeInput::Throw(Value::Number(8.)),
        ] {
            let mut runtime = machine(
                "function* f(){let opened=0,closed=0;let it={};it[Symbol.iterator]=function(){opened++;return {next:function(){return {done:true};},return:function(){closed++;return {};}};};try{let a=[{},...(yield 1),it];}catch(e){yield e;}finally{yield opened+closed;}return 9;}",
                FunctionKind::Generator,
            );
            yielded(next(&mut runtime), 1.);
            let throwing = matches!(&input, ResumeInput::Throw(_));
            let outcome = runtime.resumable_resume(0, input, &mut Empty).unwrap();
            if throwing {
                yielded(outcome, 8.);
                yielded(next(&mut runtime), 0.);
                completed(next(&mut runtime), 9.);
            } else {
                yielded(outcome, 0.);
                completed(next(&mut runtime), 8.);
            }
        }
    }

    #[test]
    fn nested_literal_partial_objects_remain_gc_roots() {
        let mut runtime = machine(
            "function* f(){let a=[{n:4},[yield 1],`${yield 2}`];return a[0].n+a[1][0]+a[2].length;}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        let retained = runtime
            .resumable_trace(0)
            .0
            .into_iter()
            .find_map(|value| match value {
                Value::Object(id) if id != 0 => Some(id),
                _ => None,
            })
            .unwrap();
        runtime
            .collect_garbage(&[Value::Object(0)], &Empty)
            .unwrap();
        assert!(!matches!(&runtime.heap[retained], crate::Heap::Vacant));
        yielded(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Number(3.)), &mut Empty)
                .unwrap(),
            2.,
        );
        runtime
            .collect_garbage(&[Value::Object(0)], &Empty)
            .unwrap();
        completed(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Number(8.)), &mut Empty)
                .unwrap(),
            8.,
        );
    }

    #[test]
    fn completed_array_spread_does_not_close_when_later_element_is_aborted() {
        let mut runtime = machine(
            "function* f(){let steps=0,closed=0;let it={};it[Symbol.iterator]=function(){return {next:function(){steps++;return {done:steps>2,value:steps};},return:function(){closed++;return {};}};};try{let a=[...it,yield 1];}finally{yield steps*10+closed;}}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        yielded(
            runtime
                .resumable_resume(0, ResumeInput::Return(Value::Number(9.)), &mut Empty)
                .unwrap(),
            30.,
        );
        completed(next(&mut runtime), 9.);
    }

    #[test]
    fn literal_array_does_not_replay_methods_or_getters() {
        let mut runtime = machine(
            "function* f(){let calls=0;let obj={};Object.defineProperty(obj,'m',{get:function(){calls++;return function(){calls++;return 3;};}});let a=[obj.m(),yield 1,obj.m()];return calls*10+a[0]+a[2];}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        completed(next(&mut runtime), 46.);
    }

    #[test]
    fn literal_width_limit_precedes_effects_and_width_does_not_consume_depth() {
        for width in [4096, 4097] {
            let mut runtime = machine("function* f(){}", FunctionKind::Generator);
            let mut items = vec![Expr::Hole; width];
            items[0] = Expr::Yield(Some(Box::new(Expr::Literal(Value::Number(1.)))));
            runtime.resumables.machines.get_mut(&0).unwrap().body =
                Arc::new(vec![Stmt::Return(Some(Expr::Array(items)))]);
            let result =
                runtime.resumable_resume(0, ResumeInput::Normal(Value::Undefined), &mut Empty);
            if width == 4096 {
                yielded(result.unwrap(), 1.);
                assert!(matches!(
                    next(&mut runtime),
                    Outcome::Completed(Value::Array(_))
                ));
            } else {
                assert_eq!(result.unwrap_err().kind, ErrorKind::Resource);
            }
        }
    }

    #[test]
    fn repeated_objects_across_nested_literal_actions_share_temporary_roots() {
        let repeated = "obj,".repeat(4095);
        let source = format!(
            "function* f(){{let calls=0;let obj={{toString:function(){{calls++;return 'x';}}}};let a=[obj,[{repeated}`${{yield 1}}`]];return a[1].length+calls;}}"
        );
        let mut runtime = machine(&source, FunctionKind::Generator);
        yielded(next(&mut runtime), 1.);
        runtime
            .collect_garbage(&[Value::Object(0)], &Empty)
            .unwrap();
        let object = runtime.scopes[0].bindings["obj"].value.clone();
        completed(
            runtime
                .resumable_resume(0, ResumeInput::Normal(object), &mut Empty)
                .unwrap(),
            4097.,
        );
    }

    #[test]
    fn literal_template_budget_is_enforced_after_resume() {
        let mut runtime = machine(
            "function* f(){return `${yield 1}x`;}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        let error = runtime
            .resumable_resume(
                0,
                ResumeInput::Normal(Value::string("a".repeat(65_536))),
                &mut Empty,
            )
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
    }

    #[test]
    fn literal_validation_counts_spread_nodes_and_preserves_forbidden_positions() {
        let mut expr = Expr::Yield(Some(Box::new(Expr::Literal(Value::Number(1.)))));
        for _ in 0..LIMIT {
            expr = Expr::Array(vec![Expr::Spread(Box::new(expr))]);
        }
        assert_eq!(
            validate_body(&[Stmt::Return(Some(expr))], FunctionKind::Generator)
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
        for source in [
            "function* f(){return new (yield 1)();}",
            "function* f(){return new F([yield 1]);}",
            "function* f(){let x={};delete x[{key:[yield 1]}];}",
            "function* f(){return {x:[yield 1]};}",
        ] {
            // Function parsing validates suspension positions before exposing the AST.
            let error = crate::parser::parse_module(source).unwrap_err();
            assert_eq!(error.kind, ErrorKind::Syntax, "{source}: {error:?}");
        }
    }
    #[test]
    fn loops_resume_without_replaying_and_unwind_break_continue() {
        let mut runtime = machine(
            "function* f(){let n=0; while(n<3){n++; try {yield n; if(n<3)continue; break;} finally {yield 10+n;}} return n;}",
            FunctionKind::Generator,
        );
        for n in [1., 11., 2., 12., 3., 13.] {
            yielded(next(&mut runtime), n);
        }
        completed(next(&mut runtime), 3.);
    }
    #[test]
    fn do_while_suspends_in_condition_and_runs_first_body() {
        let mut runtime = machine(
            "function* f(){let n=0; do {n++;} while(yield n); return n;}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        yielded(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Bool(true)), &mut Empty)
                .unwrap(),
            2.,
        );
        completed(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Bool(false)), &mut Empty)
                .unwrap(),
            2.,
        );
    }
    #[test]
    fn injected_return_suspends_in_finally_and_traces_saved_value() {
        let mut runtime = machine(
            "function* f(){try {yield 1;} finally {yield 2;}}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        let retained = runtime
            .allocate(crate::Heap::Object(crate::Object {
                properties: BTreeMap::new(),
                non_enumerable: BTreeSet::new(),
                prototype: None,
                regexp: None,
            }))
            .unwrap();
        let value = Value::Object(retained);
        yielded(
            runtime
                .resumable_resume(0, ResumeInput::Return(value.clone()), &mut Empty)
                .unwrap(),
            2.,
        );
        assert!(runtime.resumable_trace(0).0.contains(&value));
        runtime
            .collect_garbage(&[Value::Object(0)], &Empty)
            .unwrap();
        assert!(!matches!(&runtime.heap[retained], crate::Heap::Vacant));
        assert!(matches!(next(&mut runtime), Outcome::Completed(result) if result == value));
    }
    #[test]
    fn new_finally_abrupt_overrides_saved_return() {
        let mut runtime = machine(
            "function* f(){try {yield 1;} finally {yield 2; return 7;}}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        yielded(
            runtime
                .resumable_resume(0, ResumeInput::Return(Value::Number(9.)), &mut Empty)
                .unwrap(),
            2.,
        );
        completed(next(&mut runtime), 7.);
    }
    #[test]
    fn injected_throw_can_be_caught_then_finalized() {
        let mut runtime = machine(
            "function* f(){try {yield 1;} catch(e){yield e;} finally {yield 3;} return 4;}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        yielded(
            runtime
                .resumable_resume(0, ResumeInput::Throw(Value::Number(2.)), &mut Empty)
                .unwrap(),
            2.,
        );
        yielded(next(&mut runtime), 3.);
        completed(next(&mut runtime), 4.);
    }
    #[test]
    fn rejected_await_is_caught_and_finally_can_await() {
        let mut runtime = machine(
            "async function f(){try {await 1;} catch(e){return e;} finally {await 3;}}",
            FunctionKind::Async,
        );
        assert!(matches!(
            next(&mut runtime),
            Outcome::Await(Value::Number(1.))
        ));
        assert!(matches!(
            runtime
                .resumable_resume(0, ResumeInput::Throw(Value::Number(2.)), &mut Empty)
                .unwrap(),
            Outcome::Await(Value::Number(3.))
        ));
        completed(next(&mut runtime), 2.);
    }
    #[test]
    fn suspended_start_abrupt_never_enters_finally() {
        for input in [
            ResumeInput::Return(Value::Number(4.)),
            ResumeInput::Throw(Value::Number(4.)),
        ] {
            let mut runtime = machine(
                "function* f(){try {yield 1;} finally {yield 2;}}",
                FunctionKind::Generator,
            );
            match runtime.resumable_resume(0, input, &mut Empty) {
                Ok(Outcome::Completed(value)) => assert_eq!(value, Value::Number(4.)),
                Err(error) => assert_eq!(error.thrown, Some(Value::Number(4.))),
                other => panic!("Unexpected suspended-start result: {other:?}"),
            }
            assert!(!runtime.resumable_exists(0));
        }
    }
    #[test]
    fn resource_exhaustion_bypasses_handlers() {
        let mut runtime = machine(
            "function* f(){try {yield 1; while(true){}} catch(e){yield 2;} finally {yield 3;}}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        runtime.remaining = 32;
        let error = runtime
            .resumable_resume(0, ResumeInput::Normal(Value::Undefined), &mut Empty)
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert!(!runtime.resumable_exists(0));
    }
    #[test]
    fn synchronous_error_enters_catch_and_finally_throw_overrides() {
        let mut runtime = machine(
            "function* f(){try {missingName;} catch(e){yield 1;} finally {yield 2; throw 8;}}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        yielded(next(&mut runtime), 2.);
        let error = runtime
            .resumable_resume(0, ResumeInput::Normal(Value::Undefined), &mut Empty)
            .unwrap_err();
        assert_eq!(error.thrown, Some(Value::Number(8.)));
    }
    #[test]
    fn nested_finally_preserves_throw_across_suspension() {
        let mut runtime = machine(
            "function* f(){try {try {throw 5;} finally {yield 1;}} finally {yield 2;}}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        assert!(runtime.resumable_trace(0).0.contains(&Value::Number(5.)));
        yielded(next(&mut runtime), 2.);
        let error = runtime
            .resumable_resume(0, ResumeInput::Normal(Value::Undefined), &mut Empty)
            .unwrap_err();
        assert_eq!(error.thrown, Some(Value::Number(5.)));
    }
    #[test]
    fn async_arrow_binary_await_preserves_operand_result() {
        let mut runtime = Runtime::default();
        runtime
            .execute(
                "let answer=0;let f=async x=>1+await x;f(7).then(function(v){answer=v;});",
                &mut Empty,
            )
            .unwrap();
        assert_eq!(
            runtime.execute("answer", &mut Empty).unwrap(),
            Value::Number(8.)
        );
    }
    #[test]
    fn for_of_source_call_suspends_once_before_iteration() {
        let mut runtime = machine(
            "function* f(){let calls=0;function source(v){calls++;return [v,v+1];}for(let x of source(yield 1)){yield x;}return calls;}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        yielded(resume_value(&mut runtime, Value::Number(7.)), 7.);
        yielded(next(&mut runtime), 8.);
        completed(next(&mut runtime), 1.);
    }
    #[test]
    fn excluded_shapes_are_still_rejected() {
        for source in [
            "function* f(){for(let [i]=[0];i<1;i++){yield i;}}",
            "function* f(){label: while(true){yield 1;}}",
            "function* f(){switch(1){case (yield 1): break;}}",
            "function* f(){let x={a:yield 1};}",
        ] {
            // The parser invokes this validation before returning an AST.
            assert_eq!(
                crate::parser::parse(source).unwrap_err().kind,
                ErrorKind::Syntax,
                "{source}"
            );
        }
    }
    #[test]
    fn rejected_while_condition_runs_external_finally() {
        let mut runtime = machine(
            "async function f(){try {while(await 1){}} finally {await 2;}}",
            FunctionKind::Async,
        );
        assert!(matches!(
            next(&mut runtime),
            Outcome::Await(Value::Number(1.))
        ));
        assert!(matches!(
            runtime
                .resumable_resume(0, ResumeInput::Throw(Value::Number(9.)), &mut Empty)
                .unwrap(),
            Outcome::Await(Value::Number(2.))
        ));
        let error = runtime
            .resumable_resume(0, ResumeInput::Normal(Value::Undefined), &mut Empty)
            .unwrap_err();
        assert_eq!(error.thrown, Some(Value::Number(9.)));
    }
    #[test]
    fn for_let_copies_initial_and_each_iteration_environment() {
        let mut runtime = machine(
            "function* f(){let initial; let fs=[]; for(let i=0, get=()=>i;i<3;i++){initial=get; fs.push(()=>i); yield i;} return fs[0]()*100+fs[1]()*10+fs[2]()+initial()*1000;}",
            FunctionKind::Generator,
        );
        for n in [0., 1., 2.] {
            yielded(next(&mut runtime), n);
            runtime
                .collect_garbage(&[Value::Object(0)], &Empty)
                .unwrap();
        }
        completed(next(&mut runtime), 12.);
    }
    #[test]
    fn for_var_closures_share_function_binding() {
        let mut runtime = machine(
            "function* f(){let fs=[]; for(var i=0;i<3;i++){fs.push(()=>i); yield i;} return fs[0]()+fs[1]()+fs[2]();}",
            FunctionKind::Generator,
        );
        for n in [0., 1., 2.] {
            yielded(next(&mut runtime), n);
        }
        completed(next(&mut runtime), 9.);
    }
    #[test]
    fn for_continue_updates_after_finally_but_break_skips_update() {
        let mut runtime = machine(
            "function* f(){let updates=0; for(let i=0;i<4;updates++){try {yield i; i++; if(i===1)continue; break;} finally {yield 10+i;}} return updates;}",
            FunctionKind::Generator,
        );
        for n in [0., 11., 1., 12.] {
            yielded(next(&mut runtime), n);
        }
        completed(next(&mut runtime), 1.);
    }
    #[test]
    fn for_await_init_condition_update_resume_at_distinct_phases() {
        let mut runtime = machine(
            "async function f(){let starts=0; for(let i=await ++starts;await 2;await 3){await i;} return starts;}",
            FunctionKind::Async,
        );
        assert!(matches!(
            next(&mut runtime),
            Outcome::Await(Value::Number(1.))
        ));
        assert!(matches!(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Number(7.)), &mut Empty)
                .unwrap(),
            Outcome::Await(Value::Number(2.))
        ));
        assert!(matches!(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Bool(true)), &mut Empty)
                .unwrap(),
            Outcome::Await(Value::Number(7.))
        ));
        assert!(matches!(
            next(&mut runtime),
            Outcome::Await(Value::Number(3.))
        ));
        runtime
            .collect_garbage(&[Value::Object(0)], &Empty)
            .unwrap();
        assert!(matches!(
            next(&mut runtime),
            Outcome::Await(Value::Number(2.))
        ));
        completed(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Bool(false)), &mut Empty)
                .unwrap(),
            1.,
        );
    }
    #[test]
    fn for_update_rejection_enters_outer_finally_without_replaying_body() {
        let mut runtime = machine(
            "async function f(){try {for(let i=0;i<1;await 3){i++; await 1;}} finally {await 4;}}",
            FunctionKind::Async,
        );
        assert!(matches!(
            next(&mut runtime),
            Outcome::Await(Value::Number(1.))
        ));
        assert!(matches!(
            next(&mut runtime),
            Outcome::Await(Value::Number(3.))
        ));
        assert!(matches!(
            runtime
                .resumable_resume(0, ResumeInput::Throw(Value::Number(9.)), &mut Empty)
                .unwrap(),
            Outcome::Await(Value::Number(4.))
        ));
        let error = runtime
            .resumable_resume(0, ResumeInput::Normal(Value::Undefined), &mut Empty)
            .unwrap_err();
        assert_eq!(error.thrown, Some(Value::Number(9.)));
    }
    #[test]
    fn for_initial_let_tdz_is_catchable_before_first_body() {
        let mut runtime = machine(
            "function* f(){try {for(let i=i;i<1;i++){yield 1;}} catch(e){yield 2;} return 3;}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 2.);
        completed(next(&mut runtime), 3.);
    }
    #[test]
    fn for_yield_expression_init_test_and_update_are_not_replayed() {
        let mut runtime = machine(
            "function* f(){for(yield 1;yield 2;yield 3){yield 4;} return 5;}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        yielded(next(&mut runtime), 2.);
        yielded(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Bool(true)), &mut Empty)
                .unwrap(),
            4.,
        );
        yielded(next(&mut runtime), 3.);
        yielded(next(&mut runtime), 2.);
        completed(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Bool(false)), &mut Empty)
                .unwrap(),
            5.,
        );
    }
    fn custom_iterator_machine(tail: &str, close: &str) -> Runtime {
        let source = format!(
            "function* f(){{let n=0; let closed=0; let it={{next:function(){{n++; return {{value:n,done:n>3}};}},return:function(){{closed++; {close}}}}}; it[Symbol.iterator]=function(){{return this;}}; {tail}}}"
        );
        machine(&source, FunctionKind::Generator)
    }
    #[test]
    fn for_of_let_const_closures_and_captured_next_survive_gc() {
        let mut runtime = custom_iterator_machine(
            "let fs=[]; for(const x of it){delete it.next; fs.push(()=>x); yield x;} return fs[0]()*100+fs[1]()*10+fs[2]()+closed*1000;",
            "return {};",
        );
        for n in [1., 2., 3.] {
            yielded(next(&mut runtime), n);
            runtime
                .collect_garbage(&[Value::Object(0)], &Empty)
                .unwrap();
        }
        completed(next(&mut runtime), 123.);
    }
    #[test]
    fn for_of_continue_does_not_close_and_break_closes_once_after_finally() {
        let mut runtime = custom_iterator_machine(
            "for(let x of it){try {yield x; if(x===1)continue; break;} finally {yield 10+x;}} return closed*10+n;",
            "return {};",
        );
        for n in [1., 11., 2., 12.] {
            yielded(next(&mut runtime), n);
        }
        completed(next(&mut runtime), 12.);
    }
    #[test]
    fn for_of_return_injection_finishes_finally_before_iterator_close() {
        let mut runtime = custom_iterator_machine(
            "try {for(let x of it){try {yield x;} finally {yield closed;}}} finally {yield closed;}",
            "return {};",
        );
        yielded(next(&mut runtime), 1.);
        yielded(
            runtime
                .resumable_resume(0, ResumeInput::Return(Value::Number(8.)), &mut Empty)
                .unwrap(),
            0.,
        );
        yielded(next(&mut runtime), 1.);
        completed(next(&mut runtime), 8.);
    }
    #[test]
    fn for_of_throw_injection_preserves_original_over_close_error() {
        let mut runtime = custom_iterator_machine(
            "try {for(let x of it){yield x;}} catch(e){yield e;} return closed;",
            "throw 99;",
        );
        yielded(next(&mut runtime), 1.);
        yielded(
            runtime
                .resumable_resume(0, ResumeInput::Throw(Value::Number(7.)), &mut Empty)
                .unwrap(),
            7.,
        );
        completed(next(&mut runtime), 1.);
    }
    #[test]
    fn for_of_return_injection_is_replaced_by_close_error() {
        let mut runtime = custom_iterator_machine(
            "try {for(let x of it){yield x;}} catch(e){yield e;} return closed;",
            "throw 99;",
        );
        yielded(next(&mut runtime), 1.);
        yielded(
            runtime
                .resumable_resume(0, ResumeInput::Return(Value::Number(7.)), &mut Empty)
                .unwrap(),
            99.,
        );
        completed(next(&mut runtime), 1.);
    }
    #[test]
    fn for_of_binding_error_closes_but_next_error_does_not() {
        let mut runtime = custom_iterator_machine(
            "let obj=null; try {for(obj.x of it){yield 8;}} catch(e){yield closed;} return closed;",
            "return {};",
        );
        yielded(next(&mut runtime), 1.);
        completed(next(&mut runtime), 1.);
        let mut runtime = machine(
            "function* f(){let closed=0; let it={next:function(){throw 3;},return:function(){closed++;return {};}}; it[Symbol.iterator]=function(){return this;}; try {for(let x of it){yield x;}} catch(e){yield e;} return closed;}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 3.);
        completed(next(&mut runtime), 0.);
    }
    #[test]
    fn for_of_direct_await_source_is_evaluated_once() {
        let mut runtime = machine(
            "async function f(){let starts=0; function source(){starts++;return [1,2];} for(let x of await source()){await x;} return starts;}",
            FunctionKind::Async,
        );
        let Outcome::Await(source) = next(&mut runtime) else {
            panic!("Expected source suspension");
        };
        assert!(matches!(
            runtime
                .resumable_resume(0, ResumeInput::Normal(source), &mut Empty)
                .unwrap(),
            Outcome::Await(Value::Number(1.))
        ));
        runtime
            .collect_garbage(&[Value::Object(0)], &Empty)
            .unwrap();
        assert!(matches!(
            next(&mut runtime),
            Outcome::Await(Value::Number(2.))
        ));
        completed(next(&mut runtime), 1.);
    }
    #[test]
    fn for_in_snapshot_skips_deleted_and_excludes_added_keys() {
        let mut runtime = machine(
            "function* f(){let obj={a:1,b:2,c:3}; let fs=[]; for(let key in obj){fs.push(()=>key); yield key; if(key==='a'){delete obj.b; obj.d=4;}} return fs[0]()+fs[1]();}",
            FunctionKind::Generator,
        );
        assert!(matches!(next(&mut runtime), Outcome::Yield(value) if value == Value::string("a")));
        runtime
            .collect_garbage(&[Value::Object(0)], &Empty)
            .unwrap();
        assert!(matches!(next(&mut runtime), Outcome::Yield(value) if value == Value::string("c")));
        assert!(
            matches!(next(&mut runtime), Outcome::Completed(value) if value == Value::string("ac"))
        );
    }
    #[test]
    fn for_in_direct_yield_source_and_continue_break() {
        let mut runtime = machine(
            "function* f(){let count=0; for(const key in yield {a:1,b:2,c:3}){count++; yield count; if(count===1)continue; break;} return count;}",
            FunctionKind::Generator,
        );
        let Outcome::Yield(source) = next(&mut runtime) else {
            panic!("Expected source suspension");
        };
        yielded(
            runtime
                .resumable_resume(0, ResumeInput::Normal(source), &mut Empty)
                .unwrap(),
            1.,
        );
        yielded(next(&mut runtime), 2.);
        completed(next(&mut runtime), 2.);
    }
    #[test]
    fn for_each_source_observes_lexical_tdz() {
        for source in [
            "function* f(){let xs=[1]; try {for(let xs of xs){yield 9;}} catch(e){yield 2;} return 3;}",
            "function* f(){let xs={a:1}; try {for(let xs in xs){yield 9;}} catch(e){yield 2;} return 3;}",
        ] {
            let mut runtime = machine(source, FunctionKind::Generator);
            yielded(next(&mut runtime), 2.);
            completed(next(&mut runtime), 3.);
        }
    }
    #[test]
    fn resource_during_iterator_close_overrides_incoming_throw() {
        let mut runtime = custom_iterator_machine("for(let x of it){yield x;}", "while(true){}");
        yielded(next(&mut runtime), 1.);
        runtime.remaining = 64;
        let error = runtime
            .resumable_resume(0, ResumeInput::Throw(Value::Number(7.)), &mut Empty)
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert!(!runtime.resumable_exists(0));
    }
    #[test]
    fn for_of_resource_does_not_execute_iterator_return() {
        let mut runtime = custom_iterator_machine("for(let x of it){yield x;}", "return {};");
        yielded(next(&mut runtime), 1.);
        runtime.remaining = 0;
        let error = runtime
            .resumable_resume(0, ResumeInput::Normal(Value::Undefined), &mut Empty)
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert_eq!(
            runtime.scopes[0].bindings["closed"].value,
            Value::Number(0.)
        );
    }
    #[test]
    fn for_of_invalid_return_result_preserves_throw_but_replaces_break() {
        let mut runtime = custom_iterator_machine(
            "try {for(let x of it){yield x; throw 7;}} catch(e){yield e;} return closed;",
            "return 0;",
        );
        yielded(next(&mut runtime), 1.);
        yielded(next(&mut runtime), 7.);
        completed(next(&mut runtime), 1.);
        let mut runtime = custom_iterator_machine(
            "try {for(let x of it){yield x; break;}} catch(e){yield closed;} return closed;",
            "return 0;",
        );
        yielded(next(&mut runtime), 1.);
        yielded(next(&mut runtime), 1.);
        completed(next(&mut runtime), 1.);
    }
    #[test]
    fn for_of_patterns_and_for_in_await_body_use_iteration_scopes() {
        let mut runtime = machine(
            "function* f(){let fs=[]; for(let [x] of [[1],[2]]){fs.push(()=>x); yield x;} return fs[0]()*10+fs[1]();}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        yielded(next(&mut runtime), 2.);
        completed(next(&mut runtime), 12.);
        let mut runtime = machine(
            "async function f(){for(let key in {a:1,b:2}){await key;} return 3;}",
            FunctionKind::Async,
        );
        assert!(matches!(next(&mut runtime), Outcome::Await(value) if value == Value::string("a")));
        assert!(matches!(next(&mut runtime), Outcome::Await(value) if value == Value::string("b")));
        completed(next(&mut runtime), 3.);
    }
    #[test]
    fn assignment_compound_preserves_effect_order_without_replay() {
        let mut runtime = machine(
            "function* f(){let log=''; let result=0; let obj={}; let old={valueOf:function(){log+='C';return 5;}}; Object.defineProperty(obj,'x',{get:function(){log+='G';return old;},set:function(v){log+='S';result=v;}}); function key(){log+='K';return 'x';} function rhs(){log+='R';return 1;} obj[key()] += yield rhs(); return log+result;}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        assert_eq!(
            runtime.scopes[0].bindings["log"].value,
            Value::string("KGR")
        );
        let result = runtime
            .resumable_resume(0, ResumeInput::Normal(Value::Number(3.)), &mut Empty)
            .unwrap();
        assert!(matches!(result, Outcome::Completed(value) if value == Value::string("KGRCS8")));
    }
    #[test]
    fn simple_assignment_skips_getter_and_returns_rhs_through_initializer() {
        let mut runtime = machine(
            "function* f(){let writes=0; let obj={}; Object.defineProperty(obj,'x',{get:function(){throw 9;},set:function(v){writes=v;}}); let result=obj.x=yield 1; return result+writes;}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        completed(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Number(4.)), &mut Empty)
                .unwrap(),
            8.,
        );
    }
    #[test]
    fn suspended_assignment_retains_ephemeral_place_across_gc() {
        let mut runtime = machine(
            "function* f(){function target(){return {x:2};} return target().x += yield 1;}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        let retained = runtime
            .resumable_trace(0)
            .0
            .into_iter()
            .find_map(|value| match value {
                Value::Object(id) if id != 0 => Some(id),
                _ => None,
            })
            .unwrap();
        runtime
            .collect_garbage(&[Value::Object(0)], &Empty)
            .unwrap();
        assert!(!matches!(&runtime.heap[retained], crate::Heap::Vacant));
        completed(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Number(3.)), &mut Empty)
                .unwrap(),
            5.,
        );
    }
    #[test]
    fn suspended_assignment_uses_captured_old_value_and_target_after_interleaving() {
        let mut runtime = machine(
            "function* f(){let obj={x:2}; let original=obj; let result=obj.x += yield 1; return result+original.x+obj.x;}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        let original = runtime.scopes[0].bindings["original"].value.clone();
        runtime
            .write_place(
                Place::Member(original, "x".into()),
                Value::Number(50.),
                &mut Empty,
            )
            .unwrap();
        let replacement = runtime
            .eval(
                &Expr::Object(vec![("x".into(), Expr::Literal(Value::Number(100.)))]),
                0,
                &mut Empty,
            )
            .unwrap();
        runtime.scopes[0].bindings.get_mut("obj").unwrap().value = replacement;
        completed(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Number(3.)), &mut Empty)
                .unwrap(),
            110.,
        );
    }
    #[test]
    fn rejected_assignment_await_skips_write_and_enters_finally() {
        let mut runtime = machine(
            "async function f(){let obj={x:2}; try {obj.x += await 1;} finally {await obj.x;} return obj.x;}",
            FunctionKind::Async,
        );
        assert!(matches!(
            next(&mut runtime),
            Outcome::Await(Value::Number(1.))
        ));
        assert!(matches!(
            runtime
                .resumable_resume(0, ResumeInput::Throw(Value::Number(7.)), &mut Empty)
                .unwrap(),
            Outcome::Await(Value::Number(2.))
        ));
        let error = runtime
            .resumable_resume(0, ResumeInput::Normal(Value::Undefined), &mut Empty)
            .unwrap_err();
        assert_eq!(error.thrown, Some(Value::Number(7.)));
        let Value::Object(id) = runtime.scopes[0].bindings["obj"].value else {
            panic!("Expected object");
        };
        assert!(
            matches!(&runtime.heap[id], crate::Heap::Object(obj) if obj.properties["x"] == Value::Number(2.))
        );
    }
    #[test]
    fn injected_return_discards_assignment_and_closes_iterator() {
        let mut runtime = custom_iterator_machine(
            "let obj={x:2}; try {for(let x of it){obj.x=yield x;}} finally {yield obj.x+closed*10;}",
            "return {};",
        );
        yielded(next(&mut runtime), 1.);
        yielded(
            runtime
                .resumable_resume(0, ResumeInput::Return(Value::Number(9.)), &mut Empty)
                .unwrap(),
            12.,
        );
        completed(next(&mut runtime), 9.);
    }
    #[test]
    fn setter_error_after_resume_is_caught_without_replaying_target() {
        let mut runtime = machine(
            "function* f(){let calls=0; let obj={}; Object.defineProperty(obj,'x',{set:function(v){throw v;}}); function target(){calls++;return obj;} try {target().x=yield 1;} catch(e){yield e;} return calls;}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        yielded(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Number(7.)), &mut Empty)
                .unwrap(),
            7.,
        );
        completed(next(&mut runtime), 1.);
    }
    #[test]
    fn for_update_assignment_await_writes_after_resume_then_retests() {
        let mut runtime = machine(
            "async function f(){for(let i=0;i<2;i += await 1){await i;} return 4;}",
            FunctionKind::Async,
        );
        assert!(matches!(
            next(&mut runtime),
            Outcome::Await(Value::Number(0.))
        ));
        assert!(matches!(
            next(&mut runtime),
            Outcome::Await(Value::Number(1.))
        ));
        assert!(matches!(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Number(1.)), &mut Empty)
                .unwrap(),
            Outcome::Await(Value::Number(1.))
        ));
        assert!(matches!(
            next(&mut runtime),
            Outcome::Await(Value::Number(1.))
        ));
        completed(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Number(1.)), &mut Empty)
                .unwrap(),
            4.,
        );
    }
    #[test]
    fn suspended_assignment_patterns_lhs_and_unsupported_rhs_are_rejected() {
        for source in [
            "function* f(){let a=0,b=0; a=b=new call(yield 1);}",
            "function* f(){let a; [a]=yield 1;}",
            "function* f(){let obj={}; obj[{x:yield 1}]=2;}",
            "function* f(){let a=0; a += {x:yield 1};}",
        ] {
            // Function parsing validates suspension positions before exposing the AST.
            let error = crate::parser::parse_module(source).unwrap_err();
            assert_eq!(error.kind, ErrorKind::Syntax, "{source}: {error:?}");
        }
    }
    #[test]
    fn for_in_utf16_lone_surrogate_can_suspend_in_body() {
        let body = Arc::new(vec![Stmt::ForIn(
            ForBinding::Declaration(Declaration::Let, "key".into()),
            Expr::Literal(Value::Utf16(Arc::from([0xd800u16, 0x61u16]))),
            Box::new(Stmt::Expression(Expr::Yield(Some(Box::new(Expr::Name(
                "key".into(),
            )))))),
        )]);
        let mut runtime = Runtime::default();
        runtime
            .resumable_start(0, body, 0, false, FunctionKind::Generator)
            .unwrap();
        assert!(matches!(next(&mut runtime), Outcome::Yield(value) if value == Value::string("0")));
        assert!(matches!(next(&mut runtime), Outcome::Yield(value) if value == Value::string("1")));
        assert!(matches!(
            next(&mut runtime),
            Outcome::Completed(Value::Undefined)
        ));
    }
    #[test]
    fn previous_object_is_rooted_even_after_original_property_changes() {
        let mut runtime = machine(
            "function* f(){let obj={x:{valueOf:function(){return 5;}}}; return obj.x += yield 1;}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        let target = runtime.scopes[0].bindings["obj"].value.clone();
        let previous = runtime
            .read_place(&Place::Member(target.clone(), "x".into()), &mut Empty)
            .unwrap();
        runtime
            .write_place(
                Place::Member(target, "x".into()),
                Value::Number(100.),
                &mut Empty,
            )
            .unwrap();
        assert!(runtime.resumable_trace(0).0.contains(&previous));
        runtime
            .collect_garbage(&[Value::Object(0)], &Empty)
            .unwrap();
        completed(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Number(3.)), &mut Empty)
                .unwrap(),
            8.,
        );
    }
    #[test]
    fn symbol_assignment_suspends_with_resolved_place() {
        let mut runtime = machine(
            "function* f(){let key=Symbol('k'); let obj={}; obj[key]=2; return obj[key] += yield 1;}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        runtime
            .collect_garbage(&[Value::Object(0)], &Empty)
            .unwrap();
        completed(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Number(3.)), &mut Empty)
                .unwrap(),
            5.,
        );
    }
    #[test]
    fn assignment_action_traces_private_class_super_receiver_and_inner_action() {
        let action = Action::Assign {
            place: Place::PrivateMember(Value::Object(10), 11, "secret".into()),
            previous: Some(Value::Object(12)),
            operator: "+=".into(),
            next: Box::new(Action::Assign {
                place: Place::SuperMember(Value::Object(13), Value::Object(14), "x".into()),
                previous: None,
                operator: "=".into(),
                next: Box::new(Action::ForEachSource(
                    ForBinding::Declaration(Declaration::Let, "x".into()),
                    Box::new(Stmt::Empty),
                    true,
                    15,
                )),
            }),
        };
        let mut values = Vec::new();
        let mut scopes = Vec::new();
        action.trace(&mut values, &mut scopes);
        for value in [
            Value::Object(10),
            Value::Function(11),
            Value::Object(12),
            Value::Object(13),
            Value::Object(14),
        ] {
            assert!(values.contains(&value));
        }
        assert!(scopes.contains(&15));
        let action = Action::Assign {
            place: Place::Binding(16, "x".into()),
            previous: None,
            operator: "=".into(),
            next: Box::new(Action::Discard),
        };
        action.trace(&mut values, &mut scopes);
        assert!(scopes.contains(&16));
    }
    #[test]
    fn switch_default_in_middle_does_not_preempt_later_match_or_replay_cases() {
        let mut runtime = machine(
            "function* f(){let log=''; function test(n){log+=n;return n;} switch(2){case test(1):yield 1; default:yield 9; case test(2):yield 2; case test(3):yield 3;} return log;}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 2.);
        yielded(next(&mut runtime), 3.);
        assert!(
            matches!(next(&mut runtime), Outcome::Completed(value) if value == Value::string("12"))
        );
    }

    #[test]
    fn switch_default_fallback_and_match_before_default_fall_through_once() {
        for (discriminant, expected) in [(0, vec![9., 2.]), (1, vec![1., 9., 2.])] {
            let source = format!(
                "function* f(){{switch({discriminant}){{case 1:yield 1; default:yield 9; case 2:yield 2;}} return 4;}}"
            );
            let mut runtime = machine(&source, FunctionKind::Generator);
            for value in expected {
                yielded(next(&mut runtime), value);
            }
            completed(next(&mut runtime), 4.);
        }
    }

    #[test]
    fn switch_discriminant_yield_precedes_shared_lexical_environment() {
        let mut runtime = machine(
            "function* f(){let x=2; switch(yield x){case 2:let x=3; yield x; break; default:yield 9;} return x;}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 2.);
        yielded(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Number(2.)), &mut Empty)
                .unwrap(),
            3.,
        );
        completed(next(&mut runtime), 2.);
    }

    #[test]
    fn switch_case_tests_observe_tdz_from_other_clause() {
        let mut runtime = machine(
            "function* f(){try {switch(1){case x:yield 9; break; default:let x=1; yield 8;}} catch(e){yield 2;} return 3;}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 2.);
        completed(next(&mut runtime), 3.);
    }

    #[test]
    fn switch_shared_lexical_values_survive_gc_and_fallthrough() {
        let mut runtime = machine(
            "function* f(){switch(0){case 0:let obj={value:5}; yield 1; case 1:yield obj.value; return obj.value;}}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        runtime
            .collect_garbage(&[Value::Object(0)], &Empty)
            .unwrap();
        yielded(next(&mut runtime), 5.);
        completed(next(&mut runtime), 5.);
    }

    #[test]
    fn switch_break_is_local_but_continue_crosses_switch_and_finally() {
        let mut runtime = machine(
            "function* f(){for(let i=0;i<3;i++){switch(i){case 0:try {yield 0;continue;} finally {yield 10;} case 1:try {yield 1;break;} finally {yield 11;} default:yield 2;} yield 100+i;} return 3;}",
            FunctionKind::Generator,
        );
        for value in [0., 10., 1., 11., 101., 2., 102.] {
            yielded(next(&mut runtime), value);
        }
        completed(next(&mut runtime), 3.);
    }

    #[test]
    fn switch_await_discriminant_and_body_resume_without_reselection() {
        let mut runtime = machine(
            "async function f(){let calls=0; function test(){calls++;return 2;} switch(await 1){case test():await 3; break; default:await 9;} return calls;}",
            FunctionKind::Async,
        );
        assert!(matches!(
            next(&mut runtime),
            Outcome::Await(Value::Number(1.))
        ));
        assert!(matches!(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Number(2.)), &mut Empty)
                .unwrap(),
            Outcome::Await(Value::Number(3.))
        ));
        completed(next(&mut runtime), 1.);
    }

    #[test]
    fn rejected_switch_discriminant_goes_to_finally_without_case_effects() {
        let mut runtime = machine(
            "async function f(){let calls=0; function test(){calls++;return 2;} try {switch(await 1){case test():await 3;}} finally {await calls;}}",
            FunctionKind::Async,
        );
        assert!(matches!(
            next(&mut runtime),
            Outcome::Await(Value::Number(1.))
        ));
        assert!(matches!(
            runtime
                .resumable_resume(0, ResumeInput::Throw(Value::Number(7.)), &mut Empty)
                .unwrap(),
            Outcome::Await(Value::Number(0.))
        ));
        let error = runtime
            .resumable_resume(0, ResumeInput::Normal(Value::Undefined), &mut Empty)
            .unwrap_err();
        assert_eq!(error.thrown, Some(Value::Number(7.)));
    }

    #[test]
    fn switch_injected_return_unwinds_iterator_before_external_finally() {
        let mut runtime = custom_iterator_machine(
            "try {for(let x of it){switch(x){case 1:yield x;break;}}} finally {yield closed;}",
            "return {};",
        );
        yielded(next(&mut runtime), 1.);
        yielded(
            runtime
                .resumable_resume(0, ResumeInput::Return(Value::Number(7.)), &mut Empty)
                .unwrap(),
            1.,
        );
        completed(next(&mut runtime), 7.);
    }

    #[test]
    fn switch_matching_uses_strict_equality_and_empty_clauses_fall_through() {
        let mut runtime = machine(
            "function* f(){switch('1'){case 1:yield 9;break; case '1':; let n=2; case 3:yield n;break; default:yield 8;} return 4;}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 2.);
        completed(next(&mut runtime), 4.);
    }

    #[test]
    fn switch_case_and_labeled_switch_suspensions_remain_rejected() {
        for source in [
            "function* f(){switch(1){case yield 1:break;}}",
            "function* f(){label:switch(1){case 1:yield 1;}}",
        ] {
            // Function parsing validates suspension positions before exposing the AST.
            let error = crate::parser::parse_module(source).unwrap_err();
            assert_eq!(error.kind, ErrorKind::Syntax, "{source}: {error:?}");
        }
    }
    #[test]
    fn binary_operands_resume_left_to_right_exactly_once() {
        let mut runtime = machine(
            "function* f(){let calls=0; function operand(){calls++;return calls;} let result=(yield operand())+(yield operand()); return result+calls*100;}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        yielded(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Number(4.)), &mut Empty)
                .unwrap(),
            2.,
        );
        completed(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Number(5.)), &mut Empty)
                .unwrap(),
            209.,
        );
    }

    #[test]
    fn binary_coercion_occurs_after_both_operands_not_before_suspension() {
        let mut runtime = machine(
            "function* f(){let log=''; function left(){log+='L';return {valueOf:function(){log+='C';return 4;}};} function right(){log+='R';return 1;} let result=left()+(yield right()); return log+result;}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        assert_eq!(runtime.scopes[0].bindings["log"].value, Value::string("LR"));
        let result = runtime
            .resumable_resume(0, ResumeInput::Normal(Value::Number(3.)), &mut Empty)
            .unwrap();
        assert!(matches!(result, Outcome::Completed(value) if value == Value::string("LRC7")));
    }

    #[test]
    fn short_circuit_skips_suspension_and_returns_original_value() {
        for (expression, expected) in [
            ("false && (yield 1)", Value::Bool(false)),
            ("true || (yield 1)", Value::Bool(true)),
            ("0 ?? (yield 1)", Value::Number(0.)),
        ] {
            let source = format!("function* f(){{return {expression};}}");
            let mut runtime = machine(&source, FunctionKind::Generator);
            assert!(matches!(next(&mut runtime), Outcome::Completed(value) if value == expected));
        }
    }

    #[test]
    fn conditional_boolean_nesting_selects_only_required_branch() {
        let source = "function* f(){return (yield 1)?((yield 2)&&(yield 3)):(yield 4);}";
        let mut runtime = machine(source, FunctionKind::Generator);
        yielded(next(&mut runtime), 1.);
        yielded(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Bool(true)), &mut Empty)
                .unwrap(),
            2.,
        );
        completed(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Number(0.)), &mut Empty)
                .unwrap(),
            0.,
        );
        let mut runtime = machine(source, FunctionKind::Generator);
        yielded(next(&mut runtime), 1.);
        yielded(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Bool(false)), &mut Empty)
                .unwrap(),
            4.,
        );
        completed(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Number(8.)), &mut Empty)
                .unwrap(),
            8.,
        );
    }

    #[test]
    fn binary_left_temporary_survives_gc_while_right_is_suspended() {
        let mut runtime = machine(
            "function* f(){function left(){return {valueOf:function(){return 4;}};} return left()+(yield 1);}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        let retained = runtime
            .resumable_trace(0)
            .0
            .into_iter()
            .find_map(|value| match value {
                Value::Object(id) if id != 0 => Some(id),
                _ => None,
            })
            .unwrap();
        runtime
            .collect_garbage(&[Value::Object(0)], &Empty)
            .unwrap();
        assert!(!matches!(&runtime.heap[retained], crate::Heap::Vacant));
        completed(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Number(3.)), &mut Empty)
                .unwrap(),
            7.,
        );
    }

    #[test]
    fn in_rejects_primitive_rhs_before_coercing_suspended_left_key() {
        let mut runtime = machine(
            "function* f(){let calls=0; let key={toString:function(){calls++;return 'x';}}; try {return key in (yield 1);} catch(e){return calls;}}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        completed(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Number(3.)), &mut Empty)
                .unwrap(),
            0.,
        );
    }

    #[test]
    fn instanceof_after_suspension_uses_existing_operator() {
        let mut runtime = machine(
            "function* f(){function C(){} let obj=new C(); return obj instanceof (yield C);}",
            FunctionKind::Generator,
        );
        let Outcome::Yield(constructor) = next(&mut runtime) else {
            panic!("Expected constructor");
        };
        assert!(matches!(
            runtime
                .resumable_resume(0, ResumeInput::Normal(constructor), &mut Empty)
                .unwrap(),
            Outcome::Completed(Value::Bool(true))
        ));
    }

    #[test]
    fn unary_operations_resume_with_existing_coercion_and_typeof_name_fallback() {
        for (operator, expected) in [
            ("+", Value::Number(2.)),
            ("-", Value::Number(-2.)),
            ("~", Value::Number(-3.)),
            ("!", Value::Bool(false)),
            ("void", Value::Undefined),
            ("typeof", Value::string("number")),
        ] {
            let source = format!("function* f(){{return {operator} (yield 1);}}");
            let mut runtime = machine(&source, FunctionKind::Generator);
            yielded(next(&mut runtime), 1.);
            assert!(
                matches!(runtime.resumable_resume(0, ResumeInput::Normal(Value::Number(2.)), &mut Empty).unwrap(), Outcome::Completed(value) if value == expected)
            );
        }
        let mut runtime = machine(
            "function* f(){return typeof absentName;}",
            FunctionKind::Generator,
        );
        assert!(
            matches!(next(&mut runtime), Outcome::Completed(value) if value == Value::string("undefined"))
        );
    }

    #[test]
    fn coercion_throw_after_resume_reaches_catch_and_finally() {
        let mut runtime = machine(
            "function* f(){let left={valueOf:function(){throw 7;}}; try {return left+(yield 1);} catch(e){yield e;} finally {yield 9;} return 3;}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        yielded(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Number(2.)), &mut Empty)
                .unwrap(),
            7.,
        );
        yielded(next(&mut runtime), 9.);
        completed(next(&mut runtime), 3.);
    }

    #[test]
    fn abrupt_resume_discards_expression_and_closes_iterator() {
        let mut runtime = custom_iterator_machine(
            "try {for(let x of it){let result=1+(yield x);}} finally {yield closed;}",
            "return {};",
        );
        yielded(next(&mut runtime), 1.);
        yielded(
            runtime
                .resumable_resume(0, ResumeInput::Return(Value::Number(7.)), &mut Empty)
                .unwrap(),
            1.,
        );
        completed(next(&mut runtime), 7.);
    }

    #[test]
    fn await_numeric_comparison_and_comma_keep_result_order() {
        let mut runtime = machine(
            "async function f(){return (await 1, (await 2)*3 >= 6);}",
            FunctionKind::Async,
        );
        assert!(matches!(
            next(&mut runtime),
            Outcome::Await(Value::Number(1.))
        ));
        assert!(matches!(
            next(&mut runtime),
            Outcome::Await(Value::Number(2.))
        ));
        assert!(matches!(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Number(2.)), &mut Empty)
                .unwrap(),
            Outcome::Completed(Value::Bool(true))
        ));
    }

    #[test]
    fn continuation_budget_counts_operand_actions_and_statement_frames_together() {
        let mut machine = Machine {
            owner: 0,
            scope: 0,
            strict: false,
            kind: FunctionKind::Generator,
            body: Arc::new(Vec::new()),
            started: true,
            frames: Vec::new(),
            pending: None,
            abrupt: None,
        };
        for _ in 0..254 {
            machine.push(Frame::Statement(Stmt::Empty, 0)).unwrap();
        }
        let action = Action::Unary {
            operator: "!".into(),
            next: Box::new(Action::Discard),
        };
        let error = machine
            .push(Frame::Evaluate(Expr::Literal(Value::Bool(true)), 0, action))
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
    }

    #[test]
    fn unsupported_expression_boundaries_reject_before_effects() {
        for source in [
            "function* f(){eval(yield 1);}",
            "function* f(){return {x:yield 1};}",
            "function* f(){let x=0; x=new call(yield 1);}",
            "function* f(){return delete (yield 1);}",
            "function* f(){return yield call(...(yield 1));}",
        ] {
            // Function parsing validates suspension positions before exposing the AST.
            let error = crate::parser::parse_module(source).unwrap_err();
            assert_eq!(error.kind, ErrorKind::Syntax, "{source}: {error:?}");
        }
    }
    #[test]
    fn nested_yield_uses_inner_resume_value_then_suspends_outer_once() {
        let mut runtime = machine(
            "function* f(){return yield ((yield 1)+2);}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        yielded(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Number(3.)), &mut Empty)
                .unwrap(),
            5.,
        );
        completed(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Number(7.)), &mut Empty)
                .unwrap(),
            7.,
        );
    }

    #[test]
    fn nested_await_produces_separate_suspensions_even_for_primitive_values() {
        let mut runtime = machine(
            "async function f(){return await ((await 1)+2);}",
            FunctionKind::Async,
        );
        assert!(matches!(
            next(&mut runtime),
            Outcome::Await(Value::Number(1.))
        ));
        assert!(matches!(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Number(3.)), &mut Empty)
                .unwrap(),
            Outcome::Await(Value::Number(5.))
        ));
        completed(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Number(7.)), &mut Empty)
                .unwrap(),
            7.,
        );
    }

    #[test]
    fn compound_assignment_multiple_suspensions_capture_place_and_getter_once() {
        let mut runtime = machine(
            "function* f(){let log='';let result=0;let obj={};Object.defineProperty(obj,'x',{get:function(){log+='G';return 10;},set:function(v){log+='S';result=v;}});function key(){log+='K';return 'x';}function rhs(){log+='R';return 1;}obj[key()] += (yield rhs())+(yield rhs());return log+result;}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        assert_eq!(
            runtime.scopes[0].bindings["log"].value,
            Value::string("KGR")
        );
        yielded(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Number(3.)), &mut Empty)
                .unwrap(),
            1.,
        );
        assert_eq!(
            runtime.scopes[0].bindings["log"].value,
            Value::string("KGRR")
        );
        assert!(
            matches!(runtime.resumable_resume(0, ResumeInput::Normal(Value::Number(4.)), &mut Empty).unwrap(), Outcome::Completed(value) if value == Value::string("KGRRS17"))
        );
    }

    #[test]
    fn chained_assignment_writes_inner_then_outer_after_rhs_completion() {
        let mut runtime = machine(
            "function* f(){let a=0,b=3;a=b += (yield 1)+2;return a+b;}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        assert_eq!(runtime.scopes[0].bindings["a"].value, Value::Number(0.));
        assert_eq!(runtime.scopes[0].bindings["b"].value, Value::Number(3.));
        completed(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Number(4.)), &mut Empty)
                .unwrap(),
            18.,
        );
    }

    #[test]
    fn return_at_inner_yield_skips_outer_yield_and_pending_assignment() {
        let mut runtime = machine(
            "function* f(){let obj={x:2};try {obj.x=yield (yield 1);} finally {yield obj.x;}return obj.x;}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        yielded(
            runtime
                .resumable_resume(0, ResumeInput::Return(Value::Number(7.)), &mut Empty)
                .unwrap(),
            2.,
        );
        completed(next(&mut runtime), 7.);
    }

    #[test]
    fn throw_at_inner_yield_enters_catch_without_executing_outer_yield() {
        let mut runtime = machine(
            "function* f(){try {yield (yield 1);} catch(e){yield e;}finally {yield 9;}return 3;}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        yielded(
            runtime
                .resumable_resume(0, ResumeInput::Throw(Value::Number(7.)), &mut Empty)
                .unwrap(),
            7.,
        );
        yielded(next(&mut runtime), 9.);
        completed(next(&mut runtime), 3.);
    }

    #[test]
    fn rejected_inner_await_skips_outer_await_and_write_but_runs_finally() {
        let mut runtime = machine(
            "async function f(){let obj={x:2};try {obj.x+=await (await 1);}finally {await obj.x;}}",
            FunctionKind::Async,
        );
        assert!(matches!(
            next(&mut runtime),
            Outcome::Await(Value::Number(1.))
        ));
        assert!(matches!(
            runtime
                .resumable_resume(0, ResumeInput::Throw(Value::Number(7.)), &mut Empty)
                .unwrap(),
            Outcome::Await(Value::Number(2.))
        ));
        let error = runtime
            .resumable_resume(0, ResumeInput::Normal(Value::Undefined), &mut Empty)
            .unwrap_err();
        assert_eq!(error.thrown, Some(Value::Number(7.)));
    }

    #[test]
    fn nested_suspension_traces_assignment_previous_value_across_both_yields() {
        let mut runtime = machine(
            "function* f(){let obj={x:{valueOf:function(){return 5;}}};return obj.x += yield ((yield 1)+2);}",
            FunctionKind::Generator,
        );
        yielded(next(&mut runtime), 1.);
        let target = runtime.scopes[0].bindings["obj"].value.clone();
        let previous = runtime
            .read_place(&Place::Member(target.clone(), "x".into()), &mut Empty)
            .unwrap();
        runtime
            .write_place(
                Place::Member(target, "x".into()),
                Value::Number(100.),
                &mut Empty,
            )
            .unwrap();
        assert!(runtime.resumable_trace(0).0.contains(&previous));
        runtime
            .collect_garbage(&[Value::Object(0)], &Empty)
            .unwrap();
        yielded(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Number(3.)), &mut Empty)
                .unwrap(),
            5.,
        );
        assert!(runtime.resumable_trace(0).0.contains(&previous));
        runtime
            .collect_garbage(&[Value::Object(0)], &Empty)
            .unwrap();
        completed(
            runtime
                .resumable_resume(0, ResumeInput::Normal(Value::Number(4.)), &mut Empty)
                .unwrap(),
            9.,
        );
    }

    #[test]
    fn conditional_rhs_assignment_short_circuits_before_any_suspension() {
        let mut runtime = machine(
            "function* f(){let x=0;x=true?4:(yield 1);return x;}",
            FunctionKind::Generator,
        );
        completed(next(&mut runtime), 4.);
    }

    #[test]
    fn nested_suspension_actions_are_charged_in_the_shared_depth_budget() {
        let mut machine = Machine {
            owner: 0,
            scope: 0,
            strict: false,
            kind: FunctionKind::Generator,
            body: Arc::new(Vec::new()),
            started: true,
            frames: Vec::new(),
            pending: None,
            abrupt: None,
        };
        for _ in 0..253 {
            machine.push(Frame::Statement(Stmt::Empty, 0)).unwrap();
        }
        let action = Action::Await(Box::new(Action::Yield(Box::new(Action::Discard))));
        let error = machine
            .push(Frame::Evaluate(Expr::Literal(Value::Number(1.)), 0, action))
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
    }

    #[test]
    fn import_call_arguments_and_other_reference_boundaries_still_reject_suspension() {
        for source in [
            "async function f(){return import(await 'x');}",
            "async function f(){return import('x',await 1);}",
            "function* f(){let obj={};obj[{x:yield 1}]=yield 2;}",
            "function* f(){let x;[x]=(yield 1)+2;}",
            "function* f(){return yield call(...(yield 1));}",
        ] {
            // Function parsing validates suspension positions before exposing the AST.
            let error = crate::parser::parse_module(source).unwrap_err();
            assert_eq!(error.kind, ErrorKind::Syntax, "{source}: {error:?}");
        }
    }
    #[test]
    fn return_from_inner_yield_discards_outer_operand_and_closes_iterator() {
        let mut runtime = custom_iterator_machine(
            "try {for(let x of it){let result=yield (yield x);}}finally {yield closed;}",
            "return {};",
        );
        yielded(next(&mut runtime), 1.);
        yielded(
            runtime
                .resumable_resume(0, ResumeInput::Return(Value::Number(7.)), &mut Empty)
                .unwrap(),
            1.,
        );
        completed(next(&mut runtime), 7.);
    }
    #[test]
    fn for_await_distinguishes_async_values_from_sync_promise_values() {
        let mut runtime = Runtime::default();
        runtime.execute("let answer=''; let p=Promise.resolve(7); let it={next:function(){return {done:false,value:p};}}; it[Symbol.asyncIterator]=function(){return this;}; async function f(){for await(let x of it){answer=answer+(x===p);break;} for await(let x of [p]){answer=answer+':'+x;}} f();", &mut Empty).unwrap();
        assert_eq!(
            runtime.execute("answer", &mut Empty).unwrap(),
            Value::string("true:7")
        );
    }
    #[test]
    fn for_await_captures_next_and_only_sync_reads_terminal_value() {
        let mut runtime = Runtime::default();
        runtime.execute("let reads=0; let calls=0; let terminal={done:true}; Object.defineProperty(terminal,'value',{get:function(){reads++;return 0;}}); let a={next:function(){calls++;this.next=function(){throw 9;};return calls===1?{done:false,value:1}:terminal;}}; a[Symbol.asyncIterator]=function(){return this;}; let s={next:function(){return terminal;}}; s[Symbol.iterator]=function(){return this;}; async function f(){for await(let x of a){} for await(let x of s){}} f();", &mut Empty).unwrap();
        assert_eq!(
            runtime.execute("reads+':'+calls", &mut Empty).unwrap(),
            Value::string("1:2")
        );
    }
    #[test]
    fn for_await_closes_between_inner_and_outer_finally() {
        let mut runtime = Runtime::default();
        runtime.execute("let log=''; let it={next:function(){return {done:false,value:1};},return:function(){log+='r';return Promise.resolve({done:true});}}; it[Symbol.asyncIterator]=function(){return this;}; async function f(){try{for await(let x of it){try{break;}finally{await 0;log+='i';}}}finally{log+='o';}} f();", &mut Empty).unwrap();
        assert_eq!(
            runtime.execute("log", &mut Empty).unwrap(),
            Value::string("iro")
        );
    }
    #[test]
    fn for_await_close_rejection_preserves_throw_but_replaces_break() {
        let mut runtime = Runtime::default();
        runtime.execute("let log=''; let it={next:function(){return {done:false,value:1};},return:function(){return Promise.reject('close');}}; it[Symbol.asyncIterator]=function(){return this;}; async function f(){try{for await(let x of it){throw 'body';}}catch(e){log+=e;} try{for await(let x of it){break;}}catch(e){log+=':'+e;}} f();", &mut Empty).unwrap();
        assert_eq!(
            runtime.execute("log", &mut Empty).unwrap(),
            Value::string("body:close")
        );
    }
    #[test]
    fn for_await_sync_rejected_yield_closes_generator_finally() {
        let mut runtime = Runtime::default();
        runtime.execute("let log=''; function* g(){try{yield Promise.reject('bad');}finally{log+='closed';}} async function f(){try{for await(let x of g()){log+='body';}}catch(e){log+=':'+e;}} f();", &mut Empty).unwrap();
        assert_eq!(
            runtime.execute("log", &mut Empty).unwrap(),
            Value::string("closed:bad")
        );
    }
    #[test]
    fn for_await_binding_failure_closes_but_step_failure_does_not() {
        let mut runtime = Runtime::default();
        runtime.execute("let closed=0; let it={next:function(){return {done:false,value:null};},return:function(){closed++;return {};}}; it[Symbol.asyncIterator]=function(){return this;}; async function f(){try{for await(let {x} of it){}}catch(e){} it.next=function(){throw 2;};try{for await(let x of it){}}catch(e){}} f();", &mut Empty).unwrap();
        assert_eq!(
            runtime.execute("closed", &mut Empty).unwrap(),
            Value::Number(1.)
        );
    }
    #[test]
    fn for_await_pending_next_keeps_iterator_and_binding_alive_across_gc() {
        let mut runtime = Runtime::default();
        runtime.execute("let release; let answer=0; async function f(){let it={next:function(){return new Promise(function(resolve){release=resolve;});},return:function(){return {};}}; it[Symbol.asyncIterator]=function(){return this;}; for await(let x of it){answer=x;break;}} f();", &mut Empty).unwrap();
        runtime.collect_garbage(&[], &Empty).unwrap();
        runtime
            .execute("release({done:false,value:42})", &mut Empty)
            .unwrap();
        assert_eq!(
            runtime.execute("answer", &mut Empty).unwrap(),
            Value::Number(42.)
        );
    }
    #[test]
    fn for_await_continue_skips_close_and_return_waits_for_it() {
        let mut runtime = Runtime::default();
        runtime.execute("let calls=0;let closed=0;let answer=0;let it={next:function(){calls++;return {value:calls,done:false};},return:function(){closed++;return Promise.resolve({});}};it[Symbol.asyncIterator]=function(){return this;};async function f(){for await(let x of it){if(x<2)continue;return x;}} f().then(function(x){answer=x+closed;});", &mut Empty).unwrap();
        assert_eq!(
            runtime
                .execute("answer+':'+closed+':'+calls", &mut Empty)
                .unwrap(),
            Value::string("3:1:2")
        );
    }
    #[test]
    fn for_await_resource_budget_aborts_without_calling_return() {
        let mut runtime = machine(
            "async function f(){let it={next:function(){return {done:false,value:1};},return:function(){closed++;return {};}};it[Symbol.asyncIterator]=function(){return this;};for await(let x of it){}}",
            FunctionKind::Async,
        );
        runtime.declare(0, "closed", true, true).unwrap();
        runtime.scopes[0].bindings.get_mut("closed").unwrap().value = Value::Number(0.);
        let Outcome::Await(result) = next(&mut runtime) else {
            panic!("Expected next suspension");
        };
        let Some((_, Action::AsyncOfResult(state))) =
            &mut runtime.resumables.machines.get_mut(&0).unwrap().pending
        else {
            panic!("Expected result action");
        };
        state.count = 4096;
        let error = runtime
            .resumable_resume(0, ResumeInput::Normal(result), &mut Empty)
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert!(!runtime.resumable_exists(0));
        assert_eq!(
            runtime.scopes[0].bindings["closed"].value,
            Value::Number(0.)
        );
    }
    #[test]
    fn for_await_fatal_direct_thenable_disables_saved_resolver() {
        let mut runtime = Runtime::default();
        let error = runtime.execute("let saved; let entered=0; let it={next:function(){return {then:function(resolve){saved=resolve;while(true){}}};}};it[Symbol.asyncIterator]=function(){return this;};async function f(){for await(let x of it){entered++;break;}}f();", &mut Empty).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert!(runtime.resumables.machines.is_empty());
        // Only the regression harness restores work to probe the stale resolver.
        // Production Runtime never replenishes a failed page's instruction budget.
        runtime.remaining = runtime.limits.instructions;
        runtime
            .execute("saved({done:false,value:1});", &mut Empty)
            .unwrap();
        assert_eq!(
            runtime.execute("entered", &mut Empty).unwrap(),
            Value::Number(0.)
        );
    }
    #[test]
    fn for_await_cancellation_discards_adaptation_and_never_enters_body() {
        let mut runtime = Runtime::default();
        let promise = runtime.execute("let release;let entered=0;let value=new Promise(function(resolve){release=resolve;});async function f(){for await(let x of [value]){entered++;}}f();", &mut Empty).unwrap();
        let Value::Object(owner) = promise else {
            panic!("Expected async promise");
        };
        assert!(runtime.resumable_exists(owner));
        runtime.resumable_cancel(owner);
        runtime.execute("release(1)", &mut Empty).unwrap();
        assert!(!runtime.resumable_exists(owner));
        assert_eq!(
            runtime.execute("entered", &mut Empty).unwrap(),
            Value::Number(0.)
        );
    }

    fn async_generator_resume(runtime: &mut Runtime, input: ResumeInput) -> Outcome {
        runtime.resumable_resume(0, input, &mut Empty).unwrap()
    }

    fn fulfill_direct_delegate(runtime: &mut Runtime, outcome: Outcome) -> Outcome {
        let Outcome::Await(result) = outcome else {
            panic!("Expected delegated method result await");
        };
        async_generator_resume(runtime, ResumeInput::Normal(result))
    }

    #[test]
    fn async_delegate_selects_async_and_passes_one_initial_argument() {
        let mut runtime = machine(
            "async function* f(){let it={next:function(){return {done:false,value:arguments.length};}};it[Symbol.asyncIterator]=function(){return this;};it[Symbol.iterator]=function(){throw 99;};yield* it;}",
            FunctionKind::AsyncGenerator,
        );
        let result = next(&mut runtime);
        yielded(fulfill_direct_delegate(&mut runtime, result), 1.);
    }

    #[test]
    fn async_delegate_preserves_direct_value_identity_without_another_await() {
        let mut runtime = machine(
            "async function* f(){let value={then:function(){throw 99;}};let it={next:function(){return {done:false,value:value};}};it[Symbol.asyncIterator]=function(){return this;};yield* it;}",
            FunctionKind::AsyncGenerator,
        );
        let Outcome::Await(result) = next(&mut runtime) else {
            panic!("Expected method await");
        };
        let value = runtime.get(result.clone(), "value", &mut Empty).unwrap();
        let outcome = async_generator_resume(&mut runtime, ResumeInput::Normal(result));
        assert!(matches!(outcome, Outcome::Yield(actual) if actual == value));
    }

    #[test]
    fn async_delegate_throw_done_true_continues_expression() {
        let mut runtime = machine(
            "async function* f(){let it={next:function(){return {done:false,value:1};},throw:function(x){return {done:true,value:x};}};it[Symbol.asyncIterator]=function(){return this;};let answer=yield* it;yield answer+1;}",
            FunctionKind::AsyncGenerator,
        );
        let result = next(&mut runtime);
        yielded(fulfill_direct_delegate(&mut runtime, result), 1.);
        let result = async_generator_resume(&mut runtime, ResumeInput::Throw(Value::Number(7.)));
        awaited(fulfill_direct_delegate(&mut runtime, result), 8.);
    }

    #[test]
    fn async_delegate_reads_done_before_value_and_return_true_does_not_await_value() {
        let mut runtime = machine(
            "async function* f(){let order=0;let it={next:function(){return {done:false,value:1};},return:function(){let result={};Object.defineProperty(result,'done',{enumerable:true,configurable:true,get:function(){order=1;return true;}});Object.defineProperty(result,'value',{enumerable:true,configurable:true,get:function(){if(order!==1)throw 99;return {then:function(){throw 98;}};}});return result;}};it[Symbol.asyncIterator]=function(){return this;};yield* it;throw 97;}",
            FunctionKind::AsyncGenerator,
        );
        let result = next(&mut runtime);
        yielded(fulfill_direct_delegate(&mut runtime, result), 1.);
        let result = async_generator_resume(&mut runtime, ResumeInput::Return(Value::Number(7.)));
        assert!(matches!(
            fulfill_direct_delegate(&mut runtime, result),
            Outcome::Completed(Value::Object(_))
        ));
    }

    #[test]
    fn async_delegate_survives_gc_between_method_await_and_yield_resume() {
        let mut runtime = machine(
            "async function* f(){let source={};source[Symbol.asyncIterator]=function(){return {next:function(){return {done:false,value:7};}};};yield* source;}",
            FunctionKind::AsyncGenerator,
        );
        let Outcome::Await(result) = next(&mut runtime) else {
            panic!("Expected result await");
        };
        runtime
            .collect_garbage(&[Value::Object(0), result.clone()], &Empty)
            .unwrap();
        yielded(
            async_generator_resume(&mut runtime, ResumeInput::Normal(result)),
            7.,
        );
        runtime
            .collect_garbage(&[Value::Object(0)], &Empty)
            .unwrap();
        let result = next(&mut runtime);
        yielded(fulfill_direct_delegate(&mut runtime, result), 7.);
    }

    #[test]
    fn async_delegate_return_false_discards_old_return_on_next() {
        let mut runtime = machine(
            "async function* f(){let n=0;let it={next:function(){n++;return {done:n>1,value:3};},return:function(x){return {done:false,value:x};}};it[Symbol.asyncIterator]=function(){return this;};yield* it;yield 9;}",
            FunctionKind::AsyncGenerator,
        );
        let result = next(&mut runtime);
        yielded(fulfill_direct_delegate(&mut runtime, result), 3.);
        let result = async_generator_resume(&mut runtime, ResumeInput::Return(Value::Number(7.)));
        yielded(fulfill_direct_delegate(&mut runtime, result), 7.);
        let result = next(&mut runtime);
        awaited(fulfill_direct_delegate(&mut runtime, result), 9.);
    }

    #[test]
    fn async_delegate_absent_return_awaits_and_unwinds_finally() {
        let mut runtime = machine(
            "async function* f(){let it={next:function(){return {done:false,value:1};}};it[Symbol.asyncIterator]=function(){return this;};try{yield* it;}finally{yield 9;}}",
            FunctionKind::AsyncGenerator,
        );
        let result = next(&mut runtime);
        yielded(fulfill_direct_delegate(&mut runtime, result), 1.);
        awaited(
            async_generator_resume(&mut runtime, ResumeInput::Return(Value::Number(7.))),
            7.,
        );
        awaited(
            async_generator_resume(&mut runtime, ResumeInput::Normal(Value::Number(8.))),
            9.,
        );
        yielded(
            async_generator_resume(&mut runtime, ResumeInput::Normal(Value::Number(9.))),
            9.,
        );
        completed(next(&mut runtime), 8.);
    }

    #[test]
    fn async_delegate_internal_rejection_does_not_call_delegated_throw() {
        let mut runtime = machine(
            "async function* f(){let it={next:function(){return {};},throw:function(){throw 99;}};it[Symbol.asyncIterator]=function(){return this;};try{yield* it;}catch(e){yield e;}}",
            FunctionKind::AsyncGenerator,
        );
        assert!(matches!(next(&mut runtime), Outcome::Await(_)));
        awaited(
            async_generator_resume(&mut runtime, ResumeInput::Throw(Value::Number(7.))),
            7.,
        );
    }

    #[test]
    fn async_delegate_missing_throw_close_rejection_wins() {
        let mut runtime = machine(
            "async function* f(){let it={next:function(){return {done:false,value:1};},return:function(){return {};}};it[Symbol.asyncIterator]=function(){return this;};try{yield* it;}catch(e){yield e;}}",
            FunctionKind::AsyncGenerator,
        );
        let result = next(&mut runtime);
        yielded(fulfill_direct_delegate(&mut runtime, result), 1.);
        assert!(matches!(
            async_generator_resume(&mut runtime, ResumeInput::Throw(Value::Number(2.))),
            Outcome::Await(_)
        ));
        awaited(
            async_generator_resume(&mut runtime, ResumeInput::Throw(Value::Number(7.))),
            7.,
        );
    }

    #[test]
    fn async_delegate_missing_throw_close_ignores_result_getters() {
        let mut runtime = machine(
            "async function* f(){let it={next:function(){return {done:false,value:1};},return:function(){if(arguments.length!==0)throw 98;let result={};Object.defineProperty(result,'done',{enumerable:true,configurable:true,get:function(){throw 97;}});Object.defineProperty(result,'value',{enumerable:true,configurable:true,get:function(){throw 96;}});return result;}};it[Symbol.asyncIterator]=function(){return this;};try{yield* it;}catch(e){yield e.name==='TypeError'?7:99;}}",
            FunctionKind::AsyncGenerator,
        );
        let result = next(&mut runtime);
        yielded(fulfill_direct_delegate(&mut runtime, result), 1.);
        let result = async_generator_resume(&mut runtime, ResumeInput::Throw(Value::Number(2.)));
        awaited(fulfill_direct_delegate(&mut runtime, result), 7.);
    }

    #[test]
    fn async_delegate_budget_aborts_before_return_or_cleanup() {
        let mut runtime = machine(
            "async function* f(){let it={next:function(){return {done:false,value:1};},return:function(){throw 99;}};it[Symbol.asyncIterator]=function(){return this;};try{yield* it;}finally{throw 98;}}",
            FunctionKind::AsyncGenerator,
        );
        let result = next(&mut runtime);
        yielded(fulfill_direct_delegate(&mut runtime, result), 1.);
        let Some((_, Action::AsyncDelegateResume { state, .. })) =
            &mut runtime.resumables.machines.get_mut(&0).unwrap().pending
        else {
            panic!("Expected delegate resumption");
        };
        state.steps = 4096;
        let error = runtime
            .resumable_resume(0, ResumeInput::Return(Value::Number(7.)), &mut Empty)
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert!(!runtime.resumable_exists(0));
    }

    #[test]
    fn async_delegate_all_pending_phases_trace_source_next_and_continuation() {
        let state = Box::new(AsyncDelegateState {
            source: Value::Object(17),
            iterator: AsyncIteratorRecord::Direct {
                object: Value::Object(18),
                next: Value::Function(19),
            },
            steps: 1,
        });
        let next = Box::new(Action::Assign {
            place: Place::Binding(23, "x".into()),
            previous: Some(Value::Object(20)),
            operator: "=".into(),
            next: Box::new(Action::Discard),
        });
        for action in [
            Action::AsyncDelegateResume {
                state: state.clone(),
                next: next.clone(),
            },
            Action::AsyncDelegateResult {
                state: state.clone(),
                next: next.clone(),
                returning: false,
            },
            Action::AsyncDelegateMissingThrowClose {
                state: state.clone(),
                next: next.clone(),
            },
            Action::AsyncDelegateReturnAbsent { state, next },
        ] {
            assert_eq!(action.depth(), 3);
            let mut values = Vec::new();
            let mut scopes = Vec::new();
            action.trace(&mut values, &mut scopes);
            assert!(values.contains(&Value::Object(17)));
            assert!(values.contains(&Value::Object(18)));
            assert!(values.contains(&Value::Function(19)));
            assert!(values.contains(&Value::Object(20)));
            assert!(scopes.contains(&23));
        }
    }

    fn awaited(outcome: Outcome, number: f64) {
        assert!(matches!(outcome, Outcome::Await(Value::Number(n)) if n == number));
    }

    #[test]
    fn async_generator_yield_awaits_before_exposing_value_and_next_is_not_awaited() {
        let mut runtime = machine(
            "async function* f(){let sent = yield 1; yield sent;}",
            FunctionKind::AsyncGenerator,
        );
        awaited(next(&mut runtime), 1.);
        yielded(
            async_generator_resume(&mut runtime, ResumeInput::Normal(Value::Number(2.))),
            2.,
        );
        // The sent value becomes the operand of the second yield, not a
        // separate await on the first yield expression's completion.
        awaited(
            async_generator_resume(&mut runtime, ResumeInput::Normal(Value::Number(3.))),
            3.,
        );
        yielded(
            async_generator_resume(&mut runtime, ResumeInput::Normal(Value::Number(4.))),
            4.,
        );
        assert!(matches!(
            next(&mut runtime),
            Outcome::Completed(Value::Undefined)
        ));
    }

    #[test]
    fn async_generator_bare_yield_awaits_but_bare_return_and_fallthrough_do_not() {
        for source in [
            "async function* f(){yield; return;}",
            "async function* f(){yield;}",
        ] {
            let mut runtime = machine(source, FunctionKind::AsyncGenerator);
            assert!(matches!(
                next(&mut runtime),
                Outcome::Await(Value::Undefined)
            ));
            assert!(matches!(
                next(&mut runtime),
                Outcome::Yield(Value::Undefined)
            ));
            assert!(matches!(
                next(&mut runtime),
                Outcome::Completed(Value::Undefined)
            ));
        }
    }

    #[test]
    fn async_generator_rejected_yield_operand_reaches_catch_before_finally() {
        let mut runtime = machine(
            "async function* f(){try{yield 1;}catch(e){yield e;}finally{yield 9;}}",
            FunctionKind::AsyncGenerator,
        );
        awaited(next(&mut runtime), 1.);
        awaited(
            async_generator_resume(&mut runtime, ResumeInput::Throw(Value::Number(7.))),
            7.,
        );
        yielded(
            async_generator_resume(&mut runtime, ResumeInput::Normal(Value::Number(7.))),
            7.,
        );
        awaited(next(&mut runtime), 9.);
        yielded(
            async_generator_resume(&mut runtime, ResumeInput::Normal(Value::Number(9.))),
            9.,
        );
        assert!(matches!(
            next(&mut runtime),
            Outcome::Completed(Value::Undefined)
        ));
    }

    #[test]
    fn async_generator_return_operand_awaits_inside_try_and_finally_preserves_return() {
        let mut runtime = machine(
            "async function* f(){try{return 1;}catch(e){return e + 1;}finally{yield 9;}}",
            FunctionKind::AsyncGenerator,
        );
        awaited(next(&mut runtime), 1.);
        awaited(
            async_generator_resume(&mut runtime, ResumeInput::Throw(Value::Number(6.))),
            7.,
        );
        awaited(
            async_generator_resume(&mut runtime, ResumeInput::Normal(Value::Number(8.))),
            9.,
        );
        yielded(
            async_generator_resume(&mut runtime, ResumeInput::Normal(Value::Number(9.))),
            9.,
        );
        completed(next(&mut runtime), 8.);
    }

    #[test]
    fn async_generator_injected_return_is_not_awaited_again() {
        let mut runtime = machine(
            "async function* f(){try{yield 1;}finally{yield 2;}}",
            FunctionKind::AsyncGenerator,
        );
        awaited(next(&mut runtime), 1.);
        yielded(
            async_generator_resume(&mut runtime, ResumeInput::Normal(Value::Number(1.))),
            1.,
        );
        awaited(
            async_generator_resume(&mut runtime, ResumeInput::Return(Value::Number(8.))),
            2.,
        );
        yielded(
            async_generator_resume(&mut runtime, ResumeInput::Normal(Value::Number(2.))),
            2.,
        );
        completed(next(&mut runtime), 8.);
    }

    #[test]
    fn async_generator_nested_await_yield_and_return_have_distinct_continuations() {
        let mut runtime = machine(
            "async function* f(){return yield await 1;}",
            FunctionKind::AsyncGenerator,
        );
        awaited(next(&mut runtime), 1.);
        awaited(
            async_generator_resume(&mut runtime, ResumeInput::Normal(Value::Number(2.))),
            2.,
        );
        yielded(
            async_generator_resume(&mut runtime, ResumeInput::Normal(Value::Number(3.))),
            3.,
        );
        awaited(
            async_generator_resume(&mut runtime, ResumeInput::Normal(Value::Number(4.))),
            4.,
        );
        completed(
            async_generator_resume(&mut runtime, ResumeInput::Normal(Value::Number(5.))),
            5.,
        );
    }

    #[test]
    fn async_generator_validator_preserves_unsupported_suspension_boundaries() {
        // Delegation is supported; container suspension boundaries remain restricted.
        let delegate = Stmt::Expression(Expr::YieldDelegate(Box::new(Expr::Literal(
            Value::Undefined,
        ))));
        assert!(validate_body(&[delegate], FunctionKind::AsyncGenerator).is_ok());
        for source in [
            "async function* f(){yield {x:await 1};}",
            "async function* f(){return {x: yield 1};}",
        ] {
            // Function parsing validates suspension positions before exposing the AST.
            let error = crate::parser::parse_module(source).unwrap_err();
            assert_eq!(error.kind, ErrorKind::Syntax, "{source}: {error:?}");
        }
    }

    #[test]
    fn async_yield_ready_counts_and_traces_its_pending_expression() {
        let action = Action::AsyncYieldReady(Box::new(Action::BinaryRight {
            operator: "+".into(),
            left: Value::Object(17),
            next: Box::new(Action::Return),
        }));
        assert_eq!(action.depth(), 3);
        let mut values = Vec::new();
        let mut scopes = Vec::new();
        action.trace(&mut values, &mut scopes);
        assert!(
            values
                .iter()
                .any(|value| matches!(value, Value::Object(17)))
        );
    }

    #[test]
    fn async_generator_for_await_body_accepts_both_suspension_kinds() {
        let statements = crate::parser::parse(
            "async function* f(){for await(let value of [1]){yield await value;}}",
        )
        .unwrap();
        let Stmt::FunctionDefinition(_, Expr::Function { body, .. }) = &statements[0] else {
            panic!("Expected async generator");
        };
        validate_body(body, FunctionKind::AsyncGenerator).unwrap();
        assert!(validate_body(body, FunctionKind::Generator).is_err());
        assert!(validate_body(body, FunctionKind::Async).is_err());
    }
}
