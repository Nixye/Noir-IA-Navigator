//! Host-only exception diagnostics. No snapshot retains a VM value, reads a
//! property, or invokes page coercion. Catch history is not JavaScript Error.cause.
//! This deliberately does not connect Promise rejections or overridden finally
//! completions. Large string/BigInt rethrows may conservatively add one bounded
//! previous-catch entry rather than retaining their payload just for equality.
use crate::{ErrorKind, JsError, Value};
use std::fmt;

const MAX_PREVIOUS: usize = 4;
const MAX_MESSAGE_BYTES: usize = 256;
const MAX_CONTEXTS: usize = 4;
const MAX_CONTEXT_BYTES: usize = 128;
const MAX_CONTEXT_TOTAL_BYTES: usize = 256;

/// A bounded, inert snapshot for host diagnostics, never a JavaScript object.
#[derive(Debug, Clone)]
pub struct ExceptionDiagnostic {
    pub kind: ErrorKind,
    pub message: String,
    pub offset: Option<usize>,
    pub contexts: Vec<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Default)]
pub(super) struct ErrorDiagnostics {
    contexts: Vec<String>,
    contexts_truncated: bool,
    previous: Vec<ExceptionDiagnostic>,
    history_truncated: bool,
    rethrow: Option<ExceptionDiagnostic>,
    input_source_offset: bool,
    offset_not_in_input: bool,
}

fn bounded_text(text: &str, limit: usize) -> (String, bool) {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_owned(), end != text.len())
}

impl ExceptionDiagnostic {
    fn capture(error: &JsError) -> Self {
        if let Some(original) = error.rethrow_diagnostic() {
            let mut original = original.clone();
            // Host layers can add fresh context while a rethrow propagates.
            // Keep it in later snapshots without duplicating identical labels.
            for context in error.diagnostic_contexts() {
                if original.contexts.contains(context) {
                    continue;
                }
                let used = original.contexts.iter().map(String::len).sum::<usize>();
                if original.contexts.len() >= MAX_CONTEXTS || used >= MAX_CONTEXT_TOTAL_BYTES {
                    original.truncated = true;
                    break;
                }
                let (context, truncated) = bounded_text(
                    context,
                    MAX_CONTEXT_BYTES.min(MAX_CONTEXT_TOTAL_BYTES - used),
                );
                original.contexts.push(context);
                original.truncated |= truncated;
            }
            original.truncated |= error
                .diagnostics
                .as_ref()
                .is_some_and(|d| d.contexts_truncated);
            return original;
        }
        let (message, truncated) = bounded_text(&error.message, MAX_MESSAGE_BYTES);
        Self {
            kind: error.kind.clone(),
            message,
            offset: error.offset,
            contexts: error.diagnostic_contexts().to_vec(),
            truncated: truncated
                || error
                    .diagnostics
                    .as_ref()
                    .is_some_and(|d| d.contexts_truncated),
        }
    }
}

impl fmt::Display for ExceptionDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)?;
        if let Some(offset) = self.offset {
            write!(f, " at byte {offset}")?;
        }
        for context in &self.contexts {
            write!(f, " [context: {context}]")?;
        }
        if self.truncated {
            write!(f, " [diagnostic truncated]")?;
        }
        Ok(())
    }
}

impl JsError {
    /// Adds inert host context without changing message, thrown value or any
    /// JavaScript property. Stores at most four labels, 128 UTF-8 bytes per label
    /// and 256 bytes in total. Callers must not evaluate getters/coercions or
    /// allocate an unbounded string to prepare a label.
    pub fn add_diagnostic_context(&mut self, context: &str) {
        let data = self.diagnostics.get_or_insert_default();
        let used = data.contexts.iter().map(String::len).sum::<usize>();
        if data.contexts.len() >= MAX_CONTEXTS || used >= MAX_CONTEXT_TOTAL_BYTES {
            data.contexts_truncated = true;
            return;
        }
        let (context, truncated) = bounded_text(
            context,
            MAX_CONTEXT_BYTES.min(MAX_CONTEXT_TOTAL_BYTES - used),
        );
        data.contexts.push(context);
        data.contexts_truncated |= truncated;
    }

    pub fn diagnostic_contexts(&self) -> &[String] {
        self.diagnostics.as_ref().map_or(&[], |d| &d.contexts)
    }

    /// Earlier errors whose catches completed with another throw, oldest first.
    /// At most four entries are kept; this does not represent Error.cause.
    pub fn previous_exceptions(&self) -> &[ExceptionDiagnostic] {
        self.diagnostics.as_ref().map_or(&[], |d| &d.previous)
    }

    pub fn exception_history_truncated(&self) -> bool {
        self.diagnostics
            .as_ref()
            .is_some_and(|d| d.history_truncated)
    }

    /// Original host diagnostic when the same caught value was thrown again.
    /// The propagated error's ordinary message and thrown value remain intact.
    pub fn rethrow_diagnostic(&self) -> Option<&ExceptionDiagnostic> {
        self.diagnostics.as_ref().and_then(|d| d.rethrow.as_ref())
    }

    /// An offset verified to belong to the input of an idle Runtime::execute or
    /// a direct module_requests parse.
    /// Dynamic source, JSON, validators and host reentry are intentionally absent.
    pub fn input_source_offset(&self) -> Option<usize> {
        self.diagnostics
            .as_ref()
            .filter(|d| d.input_source_offset)
            .and(self.offset)
    }

    pub(super) fn mark_input_source_offset(&mut self) {
        if self.offset.is_some() {
            let data = self.diagnostics.get_or_insert_default();
            data.input_source_offset = !data.offset_not_in_input;
        }
    }

    pub(super) fn without_input_source_offset(mut self) -> Self {
        if self.offset.is_some() {
            let data = self.diagnostics.get_or_insert_default();
            data.input_source_offset = false;
            data.offset_not_in_input = true;
        }
        self
    }

    pub(super) fn fmt_diagnostics(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Some(data) = &self.diagnostics else {
            return Ok(());
        };
        for context in &data.contexts {
            write!(f, " [context: {context}]")?;
        }
        if data.contexts_truncated {
            write!(f, " [context truncated]")?;
        }
        if let Some(original) = &data.rethrow {
            write!(f, " [rethrown diagnostic: {original}]")?;
        }
        for previous in &data.previous {
            write!(f, " [previously caught: {previous}]")?;
        }
        if data.history_truncated {
            write!(f, " [catch history truncated]")?;
        }
        Ok(())
    }
}

/// Identity only, never a strong VM/GC edge. String payloads are copied only
/// when small. Heap identities must be invalidated by the continuation sweep
/// before an unmarked slot can be used again.
#[derive(Debug, Clone)]
enum ThrownIdentity {
    Undefined,
    Null,
    Bool(bool),
    Number(u64),
    String(Vec<u16>),
    Symbol(u64),
    Array(usize),
    Object(usize),
    Function(usize),
    Native(&'static str),
    Host(u64),
}
impl ThrownIdentity {
    fn capture(value: &Value) -> Option<Self> {
        Some(match value {
            Value::Undefined => Self::Undefined,
            Value::Null => Self::Null,
            Value::Bool(value) => Self::Bool(*value),
            Value::Number(value) => Self::Number(value.to_bits()),
            Value::String(value) if value.len() <= MAX_MESSAGE_BYTES => {
                Self::String(value.encode_utf16().collect())
            }
            Value::Utf16(value) if value.len() <= MAX_MESSAGE_BYTES / 2 => {
                Self::String(value.to_vec())
            }
            Value::Symbol(id, _) => Self::Symbol(*id),
            Value::Array(id) => Self::Array(*id),
            Value::Object(id) => Self::Object(*id),
            Value::Function(id) => Self::Function(*id),
            Value::Native(name) => Self::Native(name),
            Value::Host(id) => Self::Host(*id),
            _ => return None,
        })
    }
    fn matches(&self, value: &Value) -> bool {
        match (self, value) {
            (Self::Undefined, Value::Undefined) | (Self::Null, Value::Null) => true,
            (Self::Bool(a), Value::Bool(b)) => a == b,
            (Self::Number(a), Value::Number(b)) => *a == b.to_bits(),
            (Self::String(a), Value::String(b)) => a.iter().copied().eq(b.encode_utf16()),
            (Self::String(a), Value::Utf16(b)) => a.as_slice() == b.as_ref(),
            (Self::Symbol(a), Value::Symbol(b, _)) => a == b,
            (Self::Array(a), Value::Array(b))
            | (Self::Object(a), Value::Object(b))
            | (Self::Function(a), Value::Function(b)) => a == b,
            (Self::Native(a), Value::Native(b)) => a == b,
            (Self::Host(a), Value::Host(b)) => a == b,
            _ => false,
        }
    }
    fn heap_id(&self) -> Option<usize> {
        match self {
            Self::Array(id) | Self::Object(id) | Self::Function(id) => Some(*id),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct CatchDiagnostic {
    original: ExceptionDiagnostic,
    previous: Vec<ExceptionDiagnostic>,
    truncated: bool,
    identity: Option<ThrownIdentity>,
}
impl CatchDiagnostic {
    pub(super) fn capture(error: &JsError, value: &Value) -> Self {
        Self {
            original: ExceptionDiagnostic::capture(error),
            previous: error.previous_exceptions().to_vec(),
            truncated: error.exception_history_truncated(),
            identity: ThrownIdentity::capture(value),
        }
    }

    pub(super) fn clear_dead_identity(&mut self, live: &[bool]) {
        if self
            .identity
            .as_ref()
            .and_then(ThrownIdentity::heap_id)
            .is_some_and(|id| live.get(id) != Some(&true))
        {
            self.identity = None;
        }
    }

    pub(super) fn attach(self, error: &mut JsError) {
        // Fatal exhaustion bypasses the language's catch/finally machinery.
        if error.kind == ErrorKind::Resource {
            return;
        }
        let same = self
            .identity
            .as_ref()
            .zip(error.thrown.as_ref())
            .is_some_and(|(identity, value)| identity.matches(value));
        let data = error.diagnostics.get_or_insert_default();
        let mut previous = Vec::with_capacity(MAX_PREVIOUS);
        let mut truncated = self.truncated || data.history_truncated;
        for entry in self
            .previous
            .into_iter()
            .chain((!same).then_some(self.original.clone()))
            .chain(std::mem::take(&mut data.previous))
        {
            if previous.len() < MAX_PREVIOUS {
                previous.push(entry);
            } else {
                truncated = true;
            }
        }
        if same {
            data.rethrow = Some(self.original);
        }
        data.previous = previous;
        data.history_truncated = truncated;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resumable::ResumeInput;
    use crate::{Heap, Host, Limits, Runtime};
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct Empty {
        calls: Vec<String>,
        microtask_errors: Vec<JsError>,
    }
    impl Host for Empty {
        fn global(&mut self, name: &str) -> Option<Value> {
            (name == "host").then_some(Value::Host(1))
        }
        fn get(&mut self, _: u64, _: &str) -> Result<Value, JsError> {
            Ok(Value::Undefined)
        }
        fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
            Ok(())
        }
        fn call(&mut self, _: u64, method: &str, _: &[Value]) -> Result<Value, JsError> {
            self.calls.push(method.into());
            if method == "fail" {
                let mut error = JsError::runtime("host original");
                error.add_diagnostic_context("receiver=host property=fail");
                Err(error)
            } else {
                Ok(Value::Undefined)
            }
        }
        fn report_microtask_error(&mut self, error: &JsError) {
            self.microtask_errors.push(error.clone());
        }
    }
    fn run(source: &str) -> Result<Value, JsError> {
        Runtime::default().execute(source, &mut Empty::default())
    }

    #[test]
    fn failing_google_style_reporter_keeps_first_error_without_defining_reporter() {
        let error =
            run("const _={};try{missingOriginal();}catch(e){_._DumpException(e);}").unwrap_err();
        assert_eq!(error.message, "Value is not callable (_DumpException)");
        assert_eq!(error.previous_exceptions().len(), 1);
        assert_eq!(
            error.previous_exceptions()[0].message,
            "missingOriginal is not defined"
        );
        assert!(
            error
                .to_string()
                .contains("previously caught: Runtime: missingOriginal is not defined")
        );
    }

    #[test]
    fn thrown_identity_message_name_and_cause_are_untouched() {
        for source in [
            "const first={};const next=new TypeError('visible');next.cause={};const cause=next.cause;let ok=false;try{try{throw first;}catch(e){throw next;}}catch(e){ok=e===next&&e.message==='visible'&&e.name==='TypeError'&&e.cause===cause;}ok;",
            "const first={};const next=new Error('visible');let ok=false;try{try{throw first;}catch(e){throw next;}}catch(e){ok=e===next&&!Object.hasOwn(e,'cause')&&e.message==='visible';}ok;",
            "const first={};let ok=false;try{try{throw first;}catch(e){throw 42;}}catch(e){ok=e===42;}ok;",
            "let seen;try{try{missingOriginal();}catch(e){const _={};_._DumpException(e);}}catch(e){seen=e;}seen.message==='Value is not callable'&&!Object.hasOwn(seen,'cause');",
        ] {
            assert_eq!(run(source).unwrap(), Value::Bool(true), "{source}");
        }
    }

    #[test]
    fn diagnostic_snapshots_never_read_getters_or_coerce_objects() {
        let mut vm = Runtime::default();
        let mut host = Empty::default();
        let error = vm.execute("let reads=0;const x={};Object.defineProperty(x,'message',{get(){reads++;throw 1;}});Object.defineProperty(x,'name',{get(){reads++;throw 2;}});Object.defineProperty(x,'cause',{get(){reads++;throw 3;}});x.toString=function(){reads++;throw 4;};x[Symbol.toPrimitive]=function(){reads++;throw 5;};try{throw x;}catch(e){const _={};_._DumpException(e);}", &mut host).unwrap_err();
        assert_eq!(error.previous_exceptions().len(), 1);
        assert_eq!(vm.execute("reads;", &mut host).unwrap(), Value::Number(0.));
    }

    #[test]
    fn same_value_rethrow_preserves_diagnostic_without_a_repeated_catch_chain() {
        let error = run("try{try{try{missingOriginal();}catch(e){throw e;}}catch(e){throw e;}}catch(e){throw e;}").unwrap_err();
        assert_eq!(error.message, "Uncaught [object Object]");
        assert!(error.previous_exceptions().is_empty());
        assert_eq!(
            error.rethrow_diagnostic().unwrap().message,
            "missingOriginal is not defined"
        );
        let error = run("try{try{missingOriginal();}catch(e){throw e;}}catch(e){const _={};_._DumpException(e);}").unwrap_err();
        assert_eq!(
            error.previous_exceptions()[0].message,
            "missingOriginal is not defined"
        );
        assert!(error.rethrow_diagnostic().is_none());
    }

    #[test]
    fn nested_catch_failures_keep_oldest_entries_and_report_truncation() {
        let mut source = "throw 'root';".to_owned();
        for index in 0..10 {
            source = format!("try{{{source}}}catch(e){{throw 'next{index}';}}");
        }
        let error = run(&source).unwrap_err();
        assert_eq!(error.message, "Uncaught next9");
        assert_eq!(error.previous_exceptions().len(), MAX_PREVIOUS);
        assert_eq!(error.previous_exceptions()[0].message, "Uncaught root");
        assert_eq!(error.previous_exceptions()[3].message, "Uncaught next2");
        assert!(error.exception_history_truncated());
        assert!(error.to_string().contains("catch history truncated"));
    }

    #[test]
    fn unicode_messages_and_contexts_are_bounded_without_slicing_utf8() {
        let mut first = JsError::runtime("é".repeat(10_000));
        first.add_diagnostic_context(&"🦀".repeat(200));
        first.add_diagnostic_context(&"é".repeat(200));
        first.add_diagnostic_context("discarded");
        assert_eq!(
            first
                .diagnostic_contexts()
                .iter()
                .map(String::len)
                .sum::<usize>(),
            MAX_CONTEXT_TOTAL_BYTES
        );
        let value = Value::Number(1.);
        let saved = CatchDiagnostic::capture(&first, &value);
        let mut second = JsError::runtime("new");
        saved.attach(&mut second);
        let snapshot = &second.previous_exceptions()[0];
        assert_eq!(snapshot.message.len(), MAX_MESSAGE_BYTES);
        assert!(snapshot.truncated);
        assert_eq!(snapshot.contexts.len(), 2);
        assert!(second.to_string().len() < 800);
        let mut items = JsError::runtime("labels");
        for _ in 0..10 {
            items.add_diagnostic_context("x");
        }
        assert_eq!(items.diagnostic_contexts().len(), MAX_CONTEXTS);
        assert!(items.to_string().contains("context truncated"));
    }

    #[test]
    fn host_context_is_copied_into_caught_snapshot() {
        let error = run("try{host.fail();}catch(e){const _={};_._DumpException(e);}").unwrap_err();
        assert_eq!(error.previous_exceptions()[0].message, "host original");
        assert_eq!(
            error.previous_exceptions()[0].contexts,
            ["receiver=host property=fail"]
        );
        assert!(error.to_string().contains("receiver=host property=fail"));
    }

    #[test]
    fn handled_errors_do_not_leak_history_and_finally_precedence_is_unchanged() {
        let error = run("try{throw 1;}catch(e){}throw 2;").unwrap_err();
        assert!(error.previous_exceptions().is_empty());
        let error = run("try{throw 1;}catch(e){throw 2;}finally{throw 3;}").unwrap_err();
        assert_eq!(error.thrown, Some(Value::Number(3.)));
        assert!(error.previous_exceptions().is_empty());
        let mut host = Empty::default();
        let mut vm = Runtime::default();
        let error = vm
            .execute(
                "try{throw 1;}catch(e){host.caught();throw 2;}finally{host.finished();}",
                &mut host,
            )
            .unwrap_err();
        assert_eq!(host.calls, ["caught", "finished"]);
        assert_eq!(error.thrown, Some(Value::Number(2.)));
        assert_eq!(error.previous_exceptions().len(), 1);
        assert_eq!(
            run("function f(){try{throw 1;}catch(e){throw 2;}finally{return 3;}}f();").unwrap(),
            Value::Number(3.)
        );
    }

    #[test]
    fn resource_failures_still_bypass_catch_and_finally() {
        for source in [
            "try{while(true){}}catch(e){host.caught();}finally{host.finished();}",
            "try{throw 1;}catch(e){while(true){}}finally{host.finished();}",
            "function* f(){try{throw 1;}catch(e){yield 0;while(true){}}finally{host.finished();}}const g=f();g.next();g.next();",
        ] {
            let mut vm = Runtime::new(Limits {
                instructions: 1000,
                ..Limits::default()
            });
            let mut host = Empty::default();
            let error = vm.execute(source, &mut host).unwrap_err();
            assert_eq!(error.kind, ErrorKind::Resource, "{source}");
            assert!(error.previous_exceptions().is_empty());
            assert!(host.calls.is_empty());
        }
    }

    #[test]
    fn suspended_catch_preserves_context_across_yield_and_rethrow() {
        let mut vm = Runtime::default();
        let mut host = Empty::default();
        vm.execute("function* f(){try{missingOriginal();}catch(e){yield 1;const _={};_._DumpException(e);}finally{host.finished();}}const g=f();g.next();", &mut host).unwrap();
        let error = vm.execute("g.next();", &mut host).unwrap_err();
        assert_eq!(
            error.previous_exceptions()[0].message,
            "missingOriginal is not defined"
        );
        assert_eq!(host.calls, ["finished"]);
        let error = run("function* f(){try{missingOriginal();}catch(e){yield 1;throw e;}}const g=f();g.next();g.next();").unwrap_err();
        assert!(error.previous_exceptions().is_empty());
        assert_eq!(
            error.rethrow_diagnostic().unwrap().message,
            "missingOriginal is not defined"
        );
    }

    #[test]
    fn async_catch_keeps_diagnostics_until_the_promise_settlement_boundary() {
        let mut vm = Runtime::default();
        let mut host = Empty::default();
        let promise = vm.execute("const gate=Promise.withResolvers();async function f(){try{missingOriginal();}catch(e){await gate.promise;const _={};_._DumpException(e);}}f();", &mut host).unwrap();
        let Value::Object(owner) = promise else {
            panic!("async function returns a promise")
        };
        let error = vm
            .resumable_resume(owner, ResumeInput::Normal(Value::Undefined), &mut host)
            .unwrap_err();
        assert_eq!(
            error.previous_exceptions()[0].message,
            "missingOriginal is not defined"
        );
    }

    #[test]
    fn suspended_diagnostics_do_not_keep_caught_objects_alive() {
        let mut vm = Runtime::default();
        let mut host = Empty::default();
        vm.execute("let weak;function* f(){try{let doomed={};weak=new WeakRef(doomed);throw doomed;}catch(e){e=null;yield 1;throw {};}}const g=f();g.next();", &mut host).unwrap();
        vm.collect_garbage(&[], &host).unwrap();
        assert_eq!(
            vm.execute("weak.deref()===undefined;", &mut host).unwrap(),
            Value::Bool(true)
        );
        let error = vm.execute("g.next();", &mut host).unwrap_err();
        assert_eq!(error.previous_exceptions().len(), 1);
        assert!(error.rethrow_diagnostic().is_none());
    }

    #[test]
    fn collected_catch_identity_never_matches_a_recycled_slot() {
        let mut vm = Runtime::default();
        let original = vm.object(BTreeMap::new(), None).unwrap();
        let Value::Object(id) = original else {
            panic!("object")
        };
        let mut first = JsError::runtime("first object");
        first.thrown = Some(Value::Object(id));
        let mut saved = CatchDiagnostic::capture(&first, &Value::Object(id));
        vm.collect_garbage(&[], &Empty::default()).unwrap();
        assert!(matches!(vm.heap[id], Heap::Vacant));
        saved.clear_dead_identity(&vec![false; vm.heap.len()]);
        let reused = vm.object(BTreeMap::new(), None).unwrap();
        assert_eq!(reused, Value::Object(id));
        let mut next = JsError::runtime("new object");
        next.thrown = Some(reused);
        saved.attach(&mut next);
        assert!(next.rethrow_diagnostic().is_none());
        assert_eq!(next.previous_exceptions()[0].message, "first object");
    }

    #[test]
    fn microtask_host_receives_catch_history_and_later_jobs_still_run() {
        let mut vm = Runtime::default();
        let mut host = Empty::default();
        vm.execute("queueMicrotask(()=>{try{host.fail();}catch(e){const _={};_._DumpException(e);}});queueMicrotask(()=>host.later());", &mut host).unwrap();
        assert_eq!(host.calls, ["fail", "later"]);
        assert_eq!(host.microtask_errors.len(), 1);
        assert_eq!(
            host.microtask_errors[0].previous_exceptions()[0].message,
            "host original"
        );
    }

    #[test]
    fn context_added_while_rethrow_propagates_is_not_lost_in_next_snapshot() {
        let first = JsError::runtime("original");
        let value = Value::Object(17);
        let saved = CatchDiagnostic::capture(&first, &value);
        let mut second = JsError::runtime("rethrown");
        second.thrown = Some(value.clone());
        saved.attach(&mut second);
        second.add_diagnostic_context("receiver=host property=next");
        let saved = CatchDiagnostic::capture(&second, &value);
        let mut third = JsError::runtime("last");
        saved.attach(&mut third);
        assert_eq!(third.previous_exceptions()[0].message, "original");
        assert_eq!(
            third.previous_exceptions()[0].contexts,
            ["receiver=host property=next"]
        );
    }

    #[test]
    fn module_preflight_marks_only_offsets_from_the_module_input() {
        assert!(
            crate::module_requests("export const x=;")
                .unwrap_err()
                .input_source_offset()
                .is_some()
        );
        for source in [
            "export const x = `template ${1 + }`;",
            "const x=1;export {x};export {x};",
        ] {
            assert_eq!(
                crate::module_requests(source)
                    .unwrap_err()
                    .input_source_offset(),
                None,
                "{source}"
            );
        }
    }
    #[test]
    fn only_direct_input_parse_errors_expose_input_source_offset() {
        let direct = run("let x=;").unwrap_err();
        assert!(direct.input_source_offset().is_some());
        for source in [
            "eval('let x=;');",
            "Function('return ; )');",
            "JSON.parse('{');",
            "'use strict';var eval=1;",
            "const value = `interpolation ${1 + }`;",
        ] {
            let error = run(source).unwrap_err();
            assert_eq!(error.input_source_offset(), None, "{source}");
        }
        let mut vm = Runtime {
            call_depth: 1,
            ..Runtime::default()
        };
        let reentered = vm.execute("let x=;", &mut Empty::default()).unwrap_err();
        assert_eq!(reentered.input_source_offset(), None);
    }
}
