//! Original, bounded JavaScript interpreter for Noir's documented language subset.
//! The host contract exposes values and calls only; this crate has no I/O APIs.
mod array_from;
mod array_methods;
mod async_generators;
mod async_iteration;
mod bigint;
pub use bigint::BigInt;
mod callables;
mod class_runtime;
mod coercion;
mod crypto;
mod descriptors;
mod dynamic_code;
mod exception_diagnostics;
pub use exception_diagnostics::ExceptionDiagnostic;
mod finalization_registry;
mod gc;
#[cfg(test)]
mod host_constructors;
mod json;
mod language;
mod literal_builders;
mod math;
mod modules;
mod native_symbol;
mod numeric_globals;
mod parser;
mod prepared_call;
mod promises;
mod properties;
mod reflect_api;
mod reflection;
mod regexp;
mod regexp_runtime;
mod resumable;
mod resumable_functions;
mod runtime_support;
mod stdlib;
mod strict_validation;
mod strings;
mod wasm;
mod weak_collections;
mod weak_refs;
pub use gc::CollectionStats;
pub use modules::{module_requests, resolve_module_specifier};
use parser::{Declaration, Expr, ForBinding, FunctionKind, Stmt};
pub use promises::PromiseInspection;
use regexp_runtime::regexp_source;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::Arc,
};

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    BigInt(Arc<BigInt>),
    String(Arc<str>),
    Utf16(Arc<[u16]>),
    Symbol(u64, Arc<str>),
    Array(usize),
    Object(usize),
    Function(usize),
    Native(&'static str),
    Host(u64),
}
impl Value {
    pub fn string(value: impl Into<String>) -> Self {
        Self::String(Arc::from(value.into()))
    }
    pub fn as_str(&self) -> Option<&str> {
        if let Self::String(s) = self {
            Some(s)
        } else {
            None
        }
    }
    pub fn as_number(&self) -> Option<f64> {
        if let Self::Number(n) = self {
            Some(*n)
        } else {
            None
        }
    }
    pub fn as_host(&self) -> Option<u64> {
        if let Self::Host(id) = self {
            Some(*id)
        } else {
            None
        }
    }
    pub fn truthy(&self) -> bool {
        match self {
            Self::Undefined | Self::Null => false,
            Self::Bool(v) => *v,
            Self::Number(n) => *n != 0. && !n.is_nan(),
            Self::BigInt(n) => !n.is_zero(),
            Self::String(s) => !s.is_empty(),
            Self::Utf16(s) => !s.is_empty(),
            _ => true,
        }
    }
    pub fn primitive_text(&self) -> Result<String, JsError> {
        Ok(match self {
            Self::Undefined => "undefined".into(),
            Self::Null => "null".into(),
            Self::Bool(v) => v.to_string(),
            Self::String(s) => s.to_string(),
            Self::Utf16(s) => String::from_utf16(s).map_err(|_| {
                JsError::runtime("Unpaired UTF-16 string cannot cross this UTF-8 host boundary")
            })?,
            Self::BigInt(n) => n.text(10),
            Self::Number(n) => {
                if n.is_nan() {
                    "NaN".into()
                } else if *n == f64::INFINITY {
                    "Infinity".into()
                } else if *n == f64::NEG_INFINITY {
                    "-Infinity".into()
                } else if *n == 0. {
                    "0".into()
                } else {
                    runtime_support::number_text(*n)
                }
            }
            Self::Symbol(..) => {
                return Err(JsError::runtime(
                    "Cannot implicitly convert a Symbol to a string",
                ));
            }
            _ => {
                return Err(JsError::runtime(
                    "Object-to-primitive coercion is not supported by this host API",
                ));
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    Syntax,
    Runtime,
    Resource,
}
#[derive(Debug, Clone)]
pub struct JsError {
    pub kind: ErrorKind,
    pub message: String,
    pub offset: Option<usize>,
    /// A JavaScript throw carries its original value, including object identity.
    /// Resource errors never use this field and are not catchable by page code.
    pub thrown: Option<Value>,
    diagnostics: Option<Box<exception_diagnostics::ErrorDiagnostics>>,
}
impl JsError {
    pub fn syntax(offset: usize, message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Syntax,
            message: message.into(),
            offset: Some(offset),
            thrown: None,
            diagnostics: None,
        }
    }
    pub fn runtime(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Runtime,
            message: message.into(),
            offset: None,
            thrown: None,
            diagnostics: None,
        }
    }
    pub fn resource(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Resource,
            message: message.into(),
            offset: None,
            thrown: None,
            diagnostics: None,
        }
    }
}
impl fmt::Display for JsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)?;
        if let Some(offset) = self.offset {
            write!(f, " at byte {offset}")?;
        }
        self.fmt_diagnostics(f)
    }
}
impl std::error::Error for JsError {}

pub trait Host {
    /// Trusted, frozen top-level policy. This is not a complete secure-context implementation.
    fn allows_subtle_crypto(&self) -> bool {
        false
    }
    /// Reports an uncaught microtask exception without stopping later jobs.
    /// Hosts may expose this diagnostic in their existing page console.
    fn report_microtask_error(&mut self, _error: &JsError) {}
    fn utc_now_millis(&self) -> Option<i64> {
        None
    }
    fn trace_roots(&self) -> Vec<Value> {
        Vec::new()
    }
    fn global(&mut self, name: &str) -> Option<Value>;
    fn get(&mut self, object: u64, property: &str) -> Result<Value, JsError>;
    /// Tests host metadata, never by evaluating a getter or comparing its value.
    fn has_property(&mut self, _object: u64, _property: &str) -> Result<bool, JsError> {
        Err(JsError::runtime(
            "Host property existence is not implemented",
        ))
    }
    fn set(&mut self, object: u64, property: &str, value: Value) -> Result<(), JsError>;
    fn call(&mut self, object: u64, method: &str, args: &[Value]) -> Result<Value, JsError>;
    fn call_with_this(
        &mut self,
        object: u64,
        method: &str,
        args: &[Value],
        _this: &Value,
    ) -> Result<Value, JsError> {
        self.call(object, method, args)
    }
    fn is_callable(&self, _object: u64) -> bool {
        false
    }
    /// Explicit opt-in to construction, independent of ordinary callability.
    fn is_constructor(&self, _object: u64) -> bool {
        false
    }
    /// Constructs a host object without using the ordinary call hook. Returned
    /// VM references must belong to this runtime; retained values must be traced
    /// through trace_roots or retained with Runtime::retain_value.
    fn construct_with_runtime(
        &mut self,
        _object: u64,
        _args: &[Value],
        _runtime: &mut Runtime,
    ) -> Result<Value, JsError> {
        Err(JsError::runtime("Host value is not a constructor"))
    }
    /// Allows host APIs to read bounded VM values such as listener dictionaries.
    /// The host still receives only the authority exposed by this runtime.
    fn call_with_runtime(
        &mut self,
        object: u64,
        method: &str,
        args: &[Value],
        this: &Value,
        _runtime: &mut Runtime,
    ) -> Result<Value, JsError> {
        self.call_with_this(object, method, args, this)
    }
    /// Identity of the host's global object, if it supplies one. All extensible
    /// properties live in this VM's bounded heap, never in privileged services.
    fn global_object(&self) -> Option<u64> {
        None
    }
    fn global_accessor(&self, _property: &str) -> bool {
        false
    }
}

// Native interpreter frames require a hard ceiling independent of embedder policy.
const MAX_CALL_DEPTH: usize = 16;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub instructions: u64,
    pub heap_objects: usize,
    pub scopes: usize,
    pub string_bytes: usize,
    /// Active calls, clamped to the native interpreter ceiling of 16.
    pub call_depth: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            instructions: 100_000,
            heap_objects: 4096,
            scopes: 4096,
            string_bytes: 4 * 1024 * 1024,
            call_depth: MAX_CALL_DEPTH,
        }
    }
}

#[derive(Debug, Clone)]
struct Binding {
    value: Value,
    mutable: bool,
    initialized: bool,
    global_property: bool,
    lexical: bool,
}
#[derive(Debug, Clone)]
struct Scope {
    parent: Option<usize>,
    function: bool,
    bindings: BTreeMap<String, Binding>,
}
#[derive(Debug, Clone)]
struct Function {
    params: Arc<Vec<String>>,
    body: Arc<Vec<Stmt>>,
    scope: usize,
    arrow: bool,
    lexical_this: Value,
    properties: BTreeMap<String, Value>,
    strict: bool,
    parameters: Option<Arc<Vec<parser::Parameter>>>,
    constructible: bool,
    kind: FunctionKind,
    method: bool,
}
impl Function {
    fn has_prototype(&self) -> bool {
        matches!(
            self.kind,
            FunctionKind::Generator | FunctionKind::AsyncGenerator
        ) || (!self.arrow && !self.method && self.kind == FunctionKind::Ordinary)
    }
}
#[derive(Debug, Clone)]
struct Object {
    properties: BTreeMap<String, Value>,
    non_enumerable: BTreeSet<String>,
    prototype: Option<Value>,
    regexp: Option<Arc<regexp::Compiled>>,
}
#[derive(Debug, Clone)]
enum Heap {
    Vacant,
    Array(Vec<Value>),
    Object(Object),
    Function(Function),
}
#[derive(Debug, Clone)]
pub struct Runtime {
    limits: Limits,
    remaining: u64,
    string_bytes: usize,
    scopes: Vec<Scope>,
    heap: Vec<Heap>,
    call_depth: usize,
    eval_depth: usize,
    conversion_depth: usize,
    binding_count: usize,
    global_properties: Option<usize>,
    regexp_count: usize,
    regexp_pattern_bytes: usize,
    regexp_work: usize,
    array_properties: BTreeMap<usize, BTreeMap<String, Value>>,
    array_holes: BTreeMap<usize, BTreeSet<usize>>,
    array_prototype: Option<usize>,
    array_non_enumerable: BTreeMap<usize, BTreeSet<String>>,
    array_slots: usize,
    json_namespace: Option<usize>,
    property_metadata: BTreeMap<usize, BTreeMap<String, descriptors::PropertyMetadata>>,
    deleted_synthetics: BTreeMap<usize, BTreeSet<String>>,
    dynamic_depth: usize,
    dynamic_source_bytes: usize,
    bound_functions: BTreeMap<usize, callables::BoundFunction>,
    stdlib: stdlib::StdlibState,
    wasm: wasm::WasmState,
    crypto: crypto::CryptoState,
    language: language::LanguageState,
    strict: bool,
    free_heap: Vec<usize>,
    free_scopes: Vec<usize>,
    property_order: BTreeMap<usize, Vec<String>>,
    symbol_properties: BTreeMap<usize, BTreeMap<u64, Value>>,
    symbol_metadata: BTreeMap<usize, BTreeMap<u64, descriptors::PropertyMetadata>>,
    symbol_order: BTreeMap<usize, Vec<u64>>,
    promises: promises::PromiseState,
    weak_refs: weak_refs::WeakRefState,
    weak_collections: weak_collections::WeakCollectionState,
    finalization_registries: finalization_registry::FinalizationRegistryState,
    gc_roots: BTreeMap<u64, Value>,
    next_gc_root: u64,
    reflection: reflection::ReflectionState,
    modules: modules::ModuleState,
    resumables: resumable::ResumableState,
    resumable_functions: resumable_functions::ResumableFunctions,
    async_generators: async_generators::AsyncGenerators,
}
#[derive(Debug)]
enum Flow {
    Empty,
    Normal(Value),
    Return(Value),
    Break(Option<String>),
    Continue(Option<String>),
}
#[derive(Clone, Debug)]
enum Place {
    Binding(usize, String),
    Global(u64, String),
    Member(Value, String),
    SymbolMember(Value, u64),
    PrivateMember(Value, usize, String),
    SuperMember(Value, Value, String),
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new(Limits::default())
    }
}
impl Runtime {
    pub fn new(mut limits: Limits) -> Self {
        limits.call_depth = limits.call_depth.min(MAX_CALL_DEPTH);
        Self {
            remaining: limits.instructions,
            limits,
            string_bytes: 0,
            scopes: vec![Scope {
                parent: None,
                function: true,
                bindings: BTreeMap::new(),
            }],
            heap: vec![],
            call_depth: 0,
            eval_depth: 0,
            conversion_depth: 0,
            binding_count: 0,
            global_properties: None,
            regexp_count: 0,
            regexp_pattern_bytes: 0,
            regexp_work: 0,
            array_properties: BTreeMap::new(),
            array_holes: BTreeMap::new(),
            array_prototype: None,
            array_non_enumerable: BTreeMap::new(),
            array_slots: 0,
            json_namespace: None,
            property_metadata: BTreeMap::new(),
            deleted_synthetics: BTreeMap::new(),
            dynamic_depth: 0,
            dynamic_source_bytes: 0,
            bound_functions: BTreeMap::new(),
            stdlib: stdlib::StdlibState::default(),
            wasm: wasm::WasmState::default(),
            crypto: crypto::CryptoState::default(),
            language: language::LanguageState::default(),
            strict: false,
            free_heap: Vec::new(),
            free_scopes: Vec::new(),
            property_order: BTreeMap::new(),
            symbol_properties: BTreeMap::new(),
            symbol_metadata: BTreeMap::new(),
            symbol_order: BTreeMap::new(),
            promises: promises::PromiseState::default(),
            weak_refs: weak_refs::WeakRefState::default(),
            weak_collections: weak_collections::WeakCollectionState::default(),
            finalization_registries: finalization_registry::FinalizationRegistryState::default(),
            gc_roots: BTreeMap::new(),
            next_gc_root: 0,
            reflection: reflection::ReflectionState::default(),
            modules: modules::ModuleState::default(),
            resumables: resumable::ResumableState::default(),
            resumable_functions: resumable_functions::ResumableFunctions::default(),
            async_generators: async_generators::AsyncGenerators::default(),
        }
    }
    pub fn remaining_instructions(&self) -> u64 {
        self.remaining
    }
    /// Parsing completes before any script effect. Errors never skip syntax.
    pub fn execute(&mut self, source: &str, host: &mut impl Host) -> Result<Value, JsError> {
        self.collect_garbage(&[], host)?;
        let input_offset = self.call_depth == 0
            && self.eval_depth == 0
            && self.dynamic_depth == 0
            && !self.resumable_execution_active();
        let statements = parser::parse(source).map_err(|mut error| {
            if input_offset {
                error.mark_input_source_offset();
            }
            error
        })?;
        dynamic_code::validate_list(&statements, &[], true)?;
        strict_validation::validate(&statements, false)?;
        self.charge_string(source.len())?;
        if !self.scopes[0].bindings.contains_key("this")
            && let Some(value) = host.global("this")
        {
            self.declare(0, "this", false, true)?;
            self.scopes[0].bindings.get_mut("this").unwrap().value = value;
        }
        let previous = self.strict;
        self.strict = strict_validation::strict_directive(&statements);
        let result = self.block(&statements, 0, host);
        self.strict = previous;
        let result = result.and_then(|flow| match flow {
            Flow::Normal(v) => Ok(v),
            Flow::Empty => Ok(Value::Undefined),
            _ => Err(JsError::runtime("Invalid top-level completion")),
        });
        if !result
            .as_ref()
            .is_err_and(|error| error.kind == ErrorKind::Resource)
        {
            self.drain_execution_jobs(host)?;
        }
        result
    }
    /// Host scheduling boundary: ordinary microtasks precede cleanup work.
    fn drain_execution_jobs(&mut self, host: &mut impl Host) -> Result<(), JsError> {
        let mut microtask_work = 0;
        self.drain_promise_jobs_with_budget(host, &mut microtask_work)?;
        self.drain_finalization_registry_cleanup_with_budget(host, &mut microtask_work)
    }
    /// Used by the DOM event bridge. Callback values never leave the renderer.
    pub fn invoke(
        &mut self,
        callback: Value,
        args: Vec<Value>,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let mut roots = args.clone();
        roots.push(callback.clone());
        self.collect_garbage(&roots, host)?;
        let result = self.call(callback, args, Value::Undefined, host);
        if !result
            .as_ref()
            .is_err_and(|error| error.kind == ErrorKind::Resource)
            && self.call_depth == 0
        {
            self.drain_execution_jobs(host)?;
        }
        result
    }
    pub fn invoke_with_this(
        &mut self,
        callback: Value,
        args: Vec<Value>,
        this: Value,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let mut roots = args.clone();
        roots.push(callback.clone());
        roots.push(this.clone());
        self.collect_garbage(&roots, host)?;
        let result = self.call(callback, args, this, host);
        if !result
            .as_ref()
            .is_err_and(|error| error.kind == ErrorKind::Resource)
            && self.call_depth == 0
        {
            self.drain_execution_jobs(host)?;
        }
        result
    }
    /// Restore globals/heap after a failed transaction while preserving consumed work.
    pub fn rollback_to(&mut self, mut previous: Self) {
        previous.remaining = previous.remaining.min(self.remaining);
        previous.dynamic_source_bytes =
            previous.dynamic_source_bytes.max(self.dynamic_source_bytes);
        previous.wasm_preserve_budgets(self);
        previous.crypto_preserve_budgets(self);
        *self = previous;
    }
    pub fn global_value(&self, name: &str) -> Option<Value> {
        self.scopes[0]
            .bindings
            .get(name)
            .filter(|b| b.initialized)
            .map(|b| b.value.clone())
            .or_else(|| self.global_own_property(name))
    }
    fn global_own_property(&self, name: &str) -> Option<Value> {
        if let Some(binding) = self.scopes[0]
            .bindings
            .get(name)
            .filter(|b| b.global_property && b.initialized)
        {
            return Some(binding.value.clone());
        }
        self.global_properties.and_then(|id| match &self.heap[id] {
            Heap::Object(object) => object.properties.get(name).cloned(),
            _ => None,
        })
    }
    fn tick(&mut self) -> Result<(), JsError> {
        self.remaining = self
            .remaining
            .checked_sub(1)
            .ok_or_else(|| JsError::resource("JavaScript instruction budget exceeded"))?;
        Ok(())
    }
    fn charge_string(&mut self, size: usize) -> Result<(), JsError> {
        self.string_bytes = self
            .string_bytes
            .checked_add(size)
            .ok_or_else(|| JsError::resource("String budget overflow"))?;
        if self.string_bytes > self.limits.string_bytes {
            Err(JsError::resource(
                "JavaScript string allocation budget exceeded",
            ))
        } else {
            Ok(())
        }
    }
    fn string(&mut self, value: String) -> Result<Value, JsError> {
        if value.len() > 65_536 {
            return Err(JsError::resource("JavaScript string exceeds 64 KiB"));
        }
        self.charge_string(value.len())?;
        Ok(Value::string(value))
    }
    fn host_value(&mut self, value: Value) -> Result<Value, JsError> {
        if let Value::Utf16(units) = &value {
            if units.len() > 65_536 {
                return Err(JsError::resource("Host UTF-16 string exceeds limit"));
            }
            self.charge_string(units.len().saturating_mul(2))?;
        }
        if let Value::String(text) = &value {
            if text.len() > 65_536 {
                return Err(JsError::resource("Host string exceeds 64 KiB"));
            }
            self.charge_string(text.len())?;
        }
        Ok(value)
    }
    fn allocate(&mut self, object: Heap) -> Result<usize, JsError> {
        if self.heap.len() >= self.limits.heap_objects && self.free_heap.is_empty() {
            return Err(JsError::resource("JavaScript heap object budget exceeded"));
        }
        if let Heap::Array(values) = &object {
            self.charge_array_slots(values.len())?;
        }
        self.allocate_reserved(object)
    }
    /// Store an object whose array slots (if any) have already been charged.
    /// Literal builders reserve each slot before their potentially suspended push.
    fn allocate_reserved(&mut self, object: Heap) -> Result<usize, JsError> {
        if self.heap.len() >= self.limits.heap_objects && self.free_heap.is_empty() {
            return Err(JsError::resource("JavaScript heap object budget exceeded"));
        }
        let id = if let Some(id) = self.free_heap.pop() {
            self.heap[id] = object;
            id
        } else {
            let id = self.heap.len();
            self.heap.push(object);
            id
        };
        Ok(id)
    }
    fn check_array_slots(&self, additional: usize) -> Result<(), JsError> {
        if additional > 4096 || self.array_slots.saturating_add(additional) > 65_536 {
            return Err(JsError::resource(
                "JavaScript aggregate array slot budget exceeded",
            ));
        }
        Ok(())
    }
    fn charge_array_slots(&mut self, additional: usize) -> Result<(), JsError> {
        self.check_array_slots(additional)?;
        self.remaining = self
            .remaining
            .checked_sub(additional as u64)
            .ok_or_else(|| JsError::resource("Array allocation instruction budget exceeded"))?;
        self.array_slots += additional;
        Ok(())
    }
    fn scope(&mut self, parent: usize, function: bool) -> Result<usize, JsError> {
        if self.scopes.len() >= self.limits.scopes && self.free_scopes.is_empty() {
            return Err(JsError::resource(
                "JavaScript lexical scope budget exceeded",
            ));
        }
        let scope = Scope {
            parent: Some(parent),
            function,
            bindings: BTreeMap::new(),
        };
        let id = if let Some(id) = self.free_scopes.pop() {
            self.scopes[id] = scope;
            id
        } else {
            let id = self.scopes.len();
            self.scopes.push(scope);
            id
        };
        Ok(id)
    }
    fn function_scope(&self, mut scope: usize) -> usize {
        while !self.scopes[scope].function {
            scope = self.scopes[scope].parent.unwrap_or(0);
        }
        scope
    }
    fn copy_iteration_scope(
        &mut self,
        previous: usize,
        names: &[String],
    ) -> Result<usize, JsError> {
        self.tick()?;
        if names.is_empty() {
            return Ok(previous);
        }
        let parent = self.scopes[previous]
            .parent
            .ok_or_else(|| JsError::runtime("Missing for-loop environment parent"))?;
        let next = self.scope(parent, false)?;
        for name in names {
            self.tick()?;
            let binding = self.scopes[previous]
                .bindings
                .get(name)
                .cloned()
                .ok_or_else(|| JsError::runtime("Missing per-iteration binding"))?;
            self.declare(next, name, binding.mutable, binding.initialized)?;
            self.scopes[next].bindings.insert(name.clone(), binding);
        }
        Ok(next)
    }
    fn lookup_binding(&self, mut scope: usize, name: &str) -> Option<usize> {
        loop {
            if self.scopes[scope].bindings.contains_key(name) {
                return Some(scope);
            }
            scope = self.scopes[scope].parent?;
        }
    }
    fn read(&mut self, scope: usize, name: &str, host: &mut impl Host) -> Result<Value, JsError> {
        if let Some(id) = self.lookup_binding(scope, name) {
            if let Some(value) = self.module_read_binding(id, name) {
                return value;
            }
            let b = &self.scopes[id].bindings[name];
            if !b.initialized {
                return Err(JsError::runtime(format!(
                    "Cannot access {name} before initialization"
                )));
            }
            // Global var bindings are object properties, not lexical shadows.
            if b.global_property
                && host.global_accessor(name)
                && let Some(global) = host.global_object()
            {
                let value = host.get(global, name)?;
                return self.host_value(value);
            }
            return Ok(b.value.clone());
        }
        if host.global_accessor(name)
            && let Some(global) = host.global_object()
        {
            let value = host.get(global, name)?;
            return self.host_value(value);
        }
        if host.global_object().is_some()
            && let Some(value) = self.global_own_property(name)
        {
            return Ok(value);
        }
        if name == "crypto" {
            return self.crypto_namespace(host);
        }
        if name == "WebAssembly" {
            return self.wasm_namespace();
        }
        if name == "JSON" {
            return self.json_namespace();
        }
        if name == "Reflect" {
            return self.reflect_namespace();
        }
        if name == "Math" {
            return self.math_namespace();
        }
        intrinsic_global(name)
            .or_else(|| host.global(name))
            .ok_or_else(|| JsError::runtime(format!("{name} is not defined")))
    }
    fn declare(
        &mut self,
        scope: usize,
        name: &str,
        mutable: bool,
        initialized: bool,
    ) -> Result<(), JsError> {
        if self.binding_count >= 16_384 {
            return Err(JsError::resource(
                "JavaScript total binding budget exceeded",
            ));
        }
        if self.scopes[scope].bindings.len() >= 1024 {
            return Err(JsError::resource("Bindings per scope exceed 1024"));
        }
        if self.scopes[scope].bindings.contains_key(name) {
            return Err(JsError::runtime(format!(
                "Identifier {name} already declared"
            )));
        }
        self.scopes[scope].bindings.insert(
            name.into(),
            Binding {
                value: Value::Undefined,
                mutable,
                initialized,
                global_property: false,
                lexical: !initialized,
            },
        );
        self.binding_count += 1;
        Ok(())
    }
    fn collect_vars(
        &mut self,
        statements: &[Stmt],
        scope: usize,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        for statement in statements {
            self.tick()?;
            match statement {
                Stmt::PatternVariables(Declaration::Var, values) => {
                    for (pattern, _) in values {
                        let mut names = Vec::new();
                        pattern.names(&mut names);
                        self.collect_vars(
                            &[Stmt::Variables(
                                Declaration::Var,
                                names.into_iter().map(|name| (name, None)).collect(),
                            )],
                            scope,
                            host,
                        )?;
                    }
                }
                Stmt::Variables(Declaration::Var, values) => {
                    for (name, _) in values {
                        let target = self.function_scope(scope);
                        let new = !self.scopes[target].bindings.contains_key(name);
                        if new {
                            self.declare(target, name, true, true)?;
                        }
                        if target == 0 {
                            if !new && !self.scopes[0].bindings[name].global_property {
                                return Err(JsError::runtime(format!(
                                    "Global lexical binding conflicts with var {name}"
                                )));
                            }
                            // Namespace markers only recognize names; global var
                            // bindings must retain the actual lazy namespace object.
                            let initial = match self.global_own_property(name) {
                                Some(value) => Some(value),
                                None if name == "crypto" => Some(self.crypto_namespace(host)?),
                                None if name == "WebAssembly" => Some(self.wasm_namespace()?),
                                None if name == "JSON" => Some(self.json_namespace()?),
                                None if name == "Math" => Some(self.math_namespace()?),
                                None if name == "Reflect" => Some(self.reflect_namespace()?),
                                None => intrinsic_global(name).or_else(|| host.global(name)),
                            };
                            let binding = self.scopes[0].bindings.get_mut(name).unwrap();
                            binding.global_property = true;
                            if new && let Some(value) = initial {
                                binding.value = value;
                            }
                        }
                    }
                }
                Stmt::Block(body) => self.collect_vars(body, scope, host)?,
                Stmt::If(_, yes, no) => {
                    self.collect_vars(std::slice::from_ref(yes.as_ref()), scope, host)?;
                    if let Some(no) = no {
                        self.collect_vars(std::slice::from_ref(no.as_ref()), scope, host)?;
                    }
                }
                Stmt::While(_, body) => {
                    self.collect_vars(std::slice::from_ref(body.as_ref()), scope, host)?
                }
                Stmt::DoWhile(body, _) => {
                    self.collect_vars(std::slice::from_ref(body.as_ref()), scope, host)?
                }
                Stmt::Label(_, body) => {
                    self.collect_vars(std::slice::from_ref(body.as_ref()), scope, host)?
                }
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    self.collect_vars(body, scope, host)?;
                    if let Some((_, body)) = catch {
                        self.collect_vars(body, scope, host)?;
                    }
                    if let Some(body) = finally {
                        self.collect_vars(body, scope, host)?;
                    }
                }
                Stmt::Switch(_, clauses) => {
                    for (_, body) in clauses {
                        self.collect_vars(body, scope, host)?;
                    }
                }
                Stmt::For(init, _, _, body) => {
                    if let Some(init) = init {
                        self.collect_vars(std::slice::from_ref(init.as_ref()), scope, host)?;
                    }
                    self.collect_vars(std::slice::from_ref(body.as_ref()), scope, host)?;
                }
                Stmt::ForIn(binding, _, body)
                | Stmt::ForOf(binding, _, body)
                | Stmt::ForAwaitOf(binding, _, body) => {
                    if let ForBinding::Declaration(Declaration::Var, name) = binding {
                        self.collect_vars(
                            &[Stmt::Variables(
                                Declaration::Var,
                                vec![(name.clone(), None)],
                            )],
                            scope,
                            host,
                        )?;
                    }
                    if let ForBinding::Pattern(Declaration::Var, pattern) = binding {
                        self.declare_pattern(pattern, Declaration::Var, scope)?;
                    }
                    self.collect_vars(std::slice::from_ref(body.as_ref()), scope, host)?;
                }
                _ => {}
            }
        }
        Ok(())
    }
    fn prepare(
        &mut self,
        statements: &[Stmt],
        scope: usize,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        self.collect_vars(statements, scope, host)?;
        for statement in statements {
            match statement {
                Stmt::PatternVariables(kind, values) if *kind != Declaration::Var => {
                    for (pattern, _) in values {
                        self.declare_pattern(pattern, *kind, scope)?;
                    }
                }
                Stmt::ClassDefinition(name, _) => {
                    self.declare(scope, name, true, false)?;
                }
                Stmt::FunctionDefinition(name, expr) => {
                    if scope == 0
                        && self.scopes[0]
                            .bindings
                            .get(name)
                            .is_some_and(|binding| !binding.global_property)
                    {
                        return Err(JsError::syntax(
                            0,
                            format!("Global lexical binding conflicts with function {name}"),
                        ));
                    }
                    let function = self.eval(expr, scope, host)?;
                    let name_value = self.string(name.clone())?;
                    self.set_unchecked(function.clone(), "name", name_value, host)?;
                    if let Value::Function(id) = function {
                        self.property_metadata.entry(id).or_default().insert(
                            "name".into(),
                            descriptors::PropertyMetadata {
                                writable: false,
                                enumerable: false,
                                configurable: true,
                                accessor: None,
                            },
                        );
                    }
                    if !self.scopes[scope].bindings.contains_key(name) {
                        self.declare(scope, name, true, true)?;
                    }
                    let binding = self.scopes[scope].bindings.get_mut(name).unwrap();
                    binding.value = function;
                    binding.global_property = scope == 0;
                }
                Stmt::Variables(kind, values) if *kind != Declaration::Var => {
                    for (name, _) in values {
                        self.declare(scope, name, *kind == Declaration::Let, false)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    fn block(
        &mut self,
        statements: &[Stmt],
        scope: usize,
        host: &mut impl Host,
    ) -> Result<Flow, JsError> {
        self.prepare(statements, scope, host)?;
        let mut value = None;
        for statement in statements {
            match self.statement(statement, scope, host)? {
                Flow::Normal(v) => value = Some(v),
                Flow::Empty => {}
                other => return Ok(other),
            }
        }
        Ok(value.map_or(Flow::Empty, Flow::Normal))
    }
    fn statement(
        &mut self,
        statement: &Stmt,
        scope: usize,
        host: &mut impl Host,
    ) -> Result<Flow, JsError> {
        self.statement_labeled(statement, scope, host, &[])
    }
    fn statement_labeled(
        &mut self,
        statement: &Stmt,
        scope: usize,
        host: &mut impl Host,
        labels: &[String],
    ) -> Result<Flow, JsError> {
        self.tick()?;
        match statement {
            Stmt::Empty | Stmt::FunctionDefinition(..) => Ok(Flow::Empty),
            Stmt::ClassDefinition(name, definition) => {
                let value = self.eval_class(definition, scope, host)?;
                let binding = self.scopes[scope]
                    .bindings
                    .get_mut(name)
                    .ok_or_else(|| JsError::runtime("Missing class binding"))?;
                binding.value = value;
                binding.initialized = true;
                Ok(Flow::Empty)
            }
            Stmt::PatternVariables(kind, values) => {
                for (pattern, initial) in values {
                    if *kind == Declaration::Var && initial.is_none() {
                        continue;
                    }
                    let value = match initial {
                        Some(expr) => self.eval(expr, scope, host)?,
                        None => Value::Undefined,
                    };
                    self.bind_declared_pattern(pattern, value, *kind, scope, host)?;
                }
                Ok(Flow::Empty)
            }
            Stmt::Expression(expr) => Ok(Flow::Normal(self.eval(expr, scope, host)?)),
            Stmt::Variables(kind, values) => {
                for (name, initial) in values {
                    let target = if *kind == Declaration::Var {
                        self.function_scope(scope)
                    } else {
                        scope
                    };
                    if !self.scopes[target].bindings.contains_key(name) {
                        self.declare(
                            target,
                            name,
                            *kind != Declaration::Const,
                            *kind == Declaration::Var,
                        )?;
                    }
                    let value = match initial {
                        Some(expr) => self.eval(expr, scope, host)?,
                        None => Value::Undefined,
                    };
                    let binding = self.scopes[target].bindings.get_mut(name).unwrap();
                    if initial.is_some() || *kind != Declaration::Var {
                        binding.value = value;
                    }
                    binding.initialized = true;
                }
                Ok(Flow::Empty)
            }
            Stmt::Block(statements) => {
                let child = self.scope(scope, false)?;
                self.block(statements, child, host)
            }
            Stmt::If(condition, yes, no) => {
                if self.eval(condition, scope, host)?.truthy() {
                    self.statement(yes, scope, host)
                } else if let Some(no) = no {
                    self.statement(no, scope, host)
                } else {
                    Ok(Flow::Normal(Value::Undefined))
                }
            }
            Stmt::While(condition, body) => {
                while self.eval(condition, scope, host)?.truthy() {
                    match self.statement(body, scope, host)? {
                        Flow::Break(None) => break,
                        Flow::Continue(Some(label)) if labels.contains(&label) => {}
                        other @ (Flow::Break(Some(_)) | Flow::Continue(Some(_))) => {
                            return Ok(other);
                        }
                        Flow::Return(value) => return Ok(Flow::Return(value)),
                        _ => {}
                    }
                }
                Ok(Flow::Normal(Value::Undefined))
            }
            Stmt::DoWhile(body, condition) => {
                loop {
                    match self.statement(body, scope, host)? {
                        Flow::Break(None) => break,
                        Flow::Continue(Some(label)) if labels.contains(&label) => {}
                        other @ (Flow::Break(Some(_)) | Flow::Continue(Some(_))) => {
                            return Ok(other);
                        }
                        Flow::Return(value) => return Ok(Flow::Return(value)),
                        _ => {}
                    }
                    if !self.eval(condition, scope, host)?.truthy() {
                        break;
                    }
                }
                Ok(Flow::Normal(Value::Undefined))
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                let child = self.scope(scope, false)?;
                let mut result = self.block(body, child, host);
                // Exhaustion is a host boundary, not an exception a page can
                // swallow or override by returning from finally.
                if result
                    .as_ref()
                    .is_err_and(|error| error.kind == ErrorKind::Resource)
                {
                    return result;
                }
                if let Err(error) = result.as_ref()
                    && error.kind != ErrorKind::Resource
                    && let Some((binding, body)) = catch
                {
                    let value = self.exception_value(error)?;
                    let diagnostic = exception_diagnostics::CatchDiagnostic::capture(error, &value);
                    let child = self.scope(scope, false)?;
                    if let Some(name) = binding {
                        self.declare(child, name, true, true)?;
                        self.scopes[child].bindings.get_mut(name).unwrap().value = value;
                    }
                    result = self.block(body, child, host);
                    if let Err(error) = &mut result {
                        diagnostic.attach(error);
                    }
                }
                if result
                    .as_ref()
                    .is_err_and(|error| error.kind == ErrorKind::Resource)
                {
                    return result;
                }
                if let Some(body) = finally {
                    let child = self.scope(scope, false)?;
                    let final_result = self.block(body, child, host);
                    if !matches!(final_result, Ok(Flow::Normal(_) | Flow::Empty)) {
                        return final_result;
                    }
                }
                result
            }
            Stmt::Switch(discriminant, clauses) => {
                let value = self.eval(discriminant, scope, host)?;
                let child = self.scope(scope, false)?;
                let statements = clauses
                    .iter()
                    .flat_map(|(_, body)| body.iter().cloned())
                    .collect::<Vec<_>>();
                self.prepare(&statements, child, host)?;
                let mut start = None;
                let mut default = None;
                for (index, (test, _)) in clauses.iter().enumerate() {
                    self.tick()?;
                    if let Some(test) = test {
                        if strict_equal(&value, &self.eval(test, child, host)?) {
                            start = Some(index);
                            break;
                        }
                    } else {
                        default = Some(index);
                    }
                }
                let mut last = Value::Undefined;
                if let Some(start) = start.or(default) {
                    for (_, statements) in &clauses[start..] {
                        for statement in statements {
                            match self.statement(statement, child, host)? {
                                Flow::Normal(value) => last = value,
                                Flow::Break(None) => return Ok(Flow::Normal(last)),
                                Flow::Empty => {}
                                flow => return Ok(flow),
                            }
                        }
                    }
                }
                Ok(Flow::Normal(last))
            }
            Stmt::For(initial, condition, update, body) => {
                let mut child = self.scope(scope, false)?;
                let mut per_iteration = Vec::new();
                if let Some(initial) = initial {
                    match initial.as_ref() {
                        Stmt::Variables(Declaration::Let, values) => {
                            per_iteration.extend(values.iter().map(|(name, _)| name.clone()))
                        }
                        Stmt::PatternVariables(Declaration::Let, values) => {
                            for (pattern, _) in values {
                                pattern.names(&mut per_iteration);
                            }
                        }
                        _ => {}
                    }
                    self.prepare(std::slice::from_ref(initial.as_ref()), child, host)?;
                    self.statement(initial, child, host)?;
                }
                child = self.copy_iteration_scope(child, &per_iteration)?;
                loop {
                    self.tick()?;
                    if let Some(condition) = condition
                        && !self.eval(condition, child, host)?.truthy()
                    {
                        break;
                    }
                    match self.statement(body, child, host)? {
                        Flow::Break(None) => break,
                        Flow::Continue(Some(label)) if labels.contains(&label) => {}
                        other @ (Flow::Break(Some(_)) | Flow::Continue(Some(_))) => {
                            return Ok(other);
                        }
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        _ => {}
                    }
                    child = self.copy_iteration_scope(child, &per_iteration)?;
                    if let Some(update) = update {
                        self.eval(update, child, host)?;
                    }
                }
                Ok(Flow::Normal(Value::Undefined))
            }
            Stmt::ForIn(binding, source, body) => {
                let source_scope = if let ForBinding::Declaration(kind, name) = binding
                    && *kind != Declaration::Var
                {
                    let child = self.scope(scope, false)?;
                    self.declare(child, name, *kind == Declaration::Let, false)?;
                    child
                } else if let ForBinding::Pattern(kind, pattern) = binding
                    && *kind != Declaration::Var
                {
                    let child = self.scope(scope, false)?;
                    self.declare_pattern(pattern, *kind, child)?;
                    child
                } else {
                    scope
                };
                let object = self.eval(source, source_scope, host)?;
                let keys = self.enumerable_keys(&object)?;
                for key in keys {
                    self.tick()?;
                    // Deleted properties are skipped. New properties are not
                    // appended to this bounded enumeration snapshot.
                    if !matches!(object, Value::String(_) | Value::Utf16(_))
                        && !self.has_property(&object, &key, host)?
                    {
                        continue;
                    }
                    let value = self.string(key)?;
                    let iteration = self.iteration_scope(binding, value, scope, host)?;
                    match self.statement(body, iteration, host)? {
                        Flow::Break(None) => break,
                        Flow::Continue(Some(label)) if labels.contains(&label) => {}
                        other @ (Flow::Break(Some(_)) | Flow::Continue(Some(_))) => {
                            return Ok(other);
                        }
                        Flow::Return(value) => return Ok(Flow::Return(value)),
                        _ => {}
                    }
                }
                Ok(Flow::Normal(Value::Undefined))
            }
            Stmt::ForAwaitOf(..) => Err(JsError::syntax(
                0,
                "For-await-of requires an async resumable function frame",
            )),
            Stmt::ForOf(binding, source, body) => {
                let source_scope = match binding {
                    ForBinding::Declaration(kind, name) if *kind != Declaration::Var => {
                        let child = self.scope(scope, false)?;
                        self.declare(child, name, *kind == Declaration::Let, false)?;
                        child
                    }
                    ForBinding::Pattern(kind, pattern) if *kind != Declaration::Var => {
                        let child = self.scope(scope, false)?;
                        self.declare_pattern(pattern, *kind, child)?;
                        child
                    }
                    _ => scope,
                };
                let value = self.eval(source, source_scope, host)?;
                let mut iterator = self.iterator(value, host)?;
                let mut count = 0usize;
                let mut last = Value::Undefined;
                let result = (|| {
                    while let Some(value) = self.iterator_next(&mut iterator, host)? {
                        count += 1;
                        if count > 4096 {
                            return Err(JsError::resource("For-of iteration item limit exceeded"));
                        }
                        let iteration = self.iteration_scope(binding, value, scope, host)?;
                        match self.statement(body, iteration, host)? {
                            Flow::Break(None) => {
                                self.iterator_close(&mut iterator, host)?;
                                return Ok(Flow::Normal(last));
                            }
                            Flow::Continue(None) => {}
                            Flow::Continue(Some(label)) if labels.contains(&label) => {}
                            Flow::Normal(value) => last = value,
                            Flow::Empty => {}
                            other => {
                                self.iterator_close(&mut iterator, host)?;
                                return Ok(other);
                            }
                        }
                    }
                    Ok(Flow::Normal(last))
                })();
                if result
                    .as_ref()
                    .is_err_and(|error| error.kind != ErrorKind::Resource)
                {
                    self.iterator_close(&mut iterator, host)?;
                }
                result
            }
            Stmt::Return(value) => Ok(Flow::Return(match value {
                Some(expr) => self.eval(expr, scope, host)?,
                None => Value::Undefined,
            })),
            Stmt::Throw(expr) => {
                let value = self.eval(expr, scope, host)?;
                let mut error = JsError::runtime(format!(
                    "Uncaught {}",
                    value
                        .primitive_text()
                        .unwrap_or_else(|_| "[object Object]".into())
                ));
                error.thrown = Some(value);
                Err(error)
            }
            Stmt::Label(name, body) => {
                let mut labels = labels.to_vec();
                labels.push(name.clone());
                match self.statement_labeled(body, scope, host, &labels)? {
                    Flow::Break(Some(label)) if label == *name => {
                        Ok(Flow::Normal(Value::Undefined))
                    }
                    other => Ok(other),
                }
            }
            Stmt::Break(label) => Ok(Flow::Break(label.clone())),
            Stmt::Continue(label) => Ok(Flow::Continue(label.clone())),
        }
    }
    fn unary_value(
        &mut self,
        op: &str,
        value: Value,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let value = match op {
            "+" => self.operator_number(value, host)?,
            "-" | "~" => self.operator_numeric(value, host)?,
            _ => value,
        };
        match op {
            "!" => Ok(Value::Bool(!value.truthy())),
            "-" if matches!(value, Value::BigInt(_)) => {
                let Value::BigInt(n) = &value else {
                    unreachable!()
                };
                Ok(Value::BigInt(Arc::new(n.negated())))
            }
            "+" if matches!(value, Value::BigInt(_)) => {
                Err(self.property_type_error("Cannot convert BigInt to Number implicitly"))
            }
            "~" if matches!(value, Value::BigInt(_)) => {
                let Value::BigInt(n) = &value else {
                    unreachable!()
                };
                Ok(Value::BigInt(Arc::new(
                    n.bitwise(&BigInt::parse("-1")?, "^")?,
                )))
            }
            "-" => Ok(Value::Number(-self.number(&value))),
            "+" => Ok(Value::Number(self.number(&value))),
            "~" => Ok(Value::Number((!to_int32(self.number(&value))) as f64)),
            "void" => Ok(Value::Undefined),
            "typeof" => self.string(
                match value {
                    Value::Undefined => "undefined",
                    Value::Host(id) if host.is_callable(id) => "function",
                    Value::Null | Value::Object(_) | Value::Array(_) | Value::Host(_) => "object",
                    Value::Bool(_) => "boolean",
                    Value::Number(_) => "number",
                    Value::BigInt(_) => "bigint",
                    Value::String(_) | Value::Utf16(_) => "string",
                    Value::Symbol(..) => "symbol",
                    Value::Function(_) | Value::Native(_) => "function",
                }
                .into(),
            ),
            _ => Err(JsError::runtime("Unsupported unary operator")),
        }
    }
    fn binary_value(
        &mut self,
        op: &str,
        left: Value,
        right: Value,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        if op == "in" {
            if !matches!(
                right,
                Value::Object(_)
                    | Value::Array(_)
                    | Value::Function(_)
                    | Value::Native(_)
                    | Value::Host(_)
            ) {
                return Err(self.property_type_error("Right operand of in must be an object"));
            }
            let left = self.coerce_property_key(left, host)?;
            if let Value::Symbol(key, _) = left {
                return self.has_symbol(&right, key, host).map(Value::Bool);
            }
            let key = self.text(&left, 0)?;
            return self.has_property(&right, &key, host).map(Value::Bool);
        }
        if op == "instanceof" {
            return self.instance_of(left, right, host).map(Value::Bool);
        }
        if matches!(op, "&&" | "||" | "??" | ",") {
            Ok(right)
        } else {
            self.binary(op, left, right, host)
        }
    }
    fn eval(
        &mut self,
        expression: &Expr,
        scope: usize,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        self.tick()?;
        self.eval_depth += 1;
        if self.eval_depth > 64 {
            self.eval_depth -= 1;
            return Err(JsError::resource("Expression evaluation depth exceeded"));
        }
        let result = self.eval_inner(expression, scope, host);
        self.eval_depth -= 1;
        result
    }
    fn eval_inner(
        &mut self,
        expression: &Expr,
        scope: usize,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        match expression {
            Expr::ImportMeta => self.eval_import_meta(scope),
            Expr::ImportCall { specifier, options } => {
                self.eval_import_call(specifier, options.as_deref(), scope, host)
            }
            Expr::Literal(value) => Ok(value.clone()),
            Expr::RegExp(pattern, flags) => self.make_regexp(pattern, flags),
            Expr::Name(name) => self.read(scope, name, host),
            Expr::Array(values) => {
                if values.len() > 4096 {
                    return Err(JsError::resource("Array limit exceeded"));
                }
                let mut output = Vec::new();
                let mut holes = BTreeSet::new();
                for expr in values {
                    match expr {
                        Expr::Spread(expr) => {
                            let source = self.eval(expr, scope, host)?;
                            self.literal_array_spread(&mut output, source, host)?;
                        }
                        Expr::Hole => self.literal_array_append(
                            &mut output,
                            &mut holes,
                            Value::Undefined,
                            true,
                        )?,
                        expr => {
                            let value = self.eval(expr, scope, host)?;
                            self.literal_array_append(&mut output, &mut holes, value, false)?;
                        }
                    }
                }
                self.literal_array_finish(output, holes)
            }
            Expr::Object(pairs) => {
                if pairs.len() > 128 {
                    return Err(JsError::resource("Object property limit exceeded"));
                }
                let object = self.ordinary_object(BTreeMap::new())?;
                for (name, expr) in pairs {
                    if let Expr::Spread(expr) = expr {
                        let source = self.eval(expr, scope, host)?;
                        self.copy_properties(object.clone(), source, &BTreeSet::new(), host)?;
                    } else {
                        let value = self.eval(expr, scope, host)?;
                        if matches!(expr, Expr::Function { method: true, .. }) {
                            let function_name = self.string(name.clone())?;
                            self.set_unchecked(value.clone(), "name", function_name, host)?;
                            if let Value::Function(id) = &value {
                                self.property_metadata.entry(*id).or_default().insert(
                                    "name".into(),
                                    descriptors::PropertyMetadata {
                                        writable: false,
                                        enumerable: false,
                                        configurable: true,
                                        accessor: None,
                                    },
                                );
                            }
                        }
                        self.create_data_property(&object, name, value, host)?;
                    }
                }
                Ok(object)
            }
            Expr::Function {
                name,
                params,
                body,
                arrow,
                parameters,
                kind,
                method,
            } => {
                if let Some(name) = name {
                    let child = self.scope(scope, false)?;
                    self.declare(child, name, false, true)?;
                    let function =
                        self.make_function(params.clone(), body.clone(), child, *arrow)?;
                    self.attach_parameters(&function, parameters.clone())?;
                    self.configure_resumable_function(&function, *kind, *method)?;
                    self.scopes[child].bindings.get_mut(name).unwrap().value = function.clone();
                    Ok(function)
                } else {
                    let function =
                        self.make_function(params.clone(), body.clone(), scope, *arrow)?;
                    self.attach_parameters(&function, parameters.clone())?;
                    self.configure_resumable_function(&function, *kind, *method)?;
                    Ok(function)
                }
            }
            Expr::Await(_) | Expr::Yield(_) | Expr::YieldDelegate(_) => Err(JsError::syntax(
                0,
                "Suspension requires a resumable function frame",
            )),
            Expr::Member(object, property) => {
                if matches!(object.as_ref(), Expr::Super) {
                    return self.super_get(property, scope, host);
                }
                let object = self.eval(object, scope, host)?;
                let key = self.eval(property, scope, host)?;
                self.read_member_values(object, key, host)
            }
            Expr::PrivateMember(object, name) => {
                let value = self.eval(object, scope, host)?;
                let class = self.private_class(scope)?;
                self.private_get(value, class, name, host)
            }
            Expr::Class(definition) => self.eval_class(definition, scope, host),
            Expr::Template(parts) => {
                let mut units = Vec::new();
                for part in parts {
                    let value = self.eval(part, scope, host)?;
                    self.literal_template_append(&mut units, value, host)?;
                }
                self.literal_template_finish(units)
            }
            Expr::Spread(_) | Expr::Hole | Expr::Super => {
                Err(JsError::runtime("Invalid standalone language expression"))
            }
            Expr::Call(callee, args) => {
                if matches!(callee.as_ref(), Expr::Super) {
                    let args = self.argument_values(args, scope, host)?;
                    return self.super_construct(args, scope, host);
                }
                if let Expr::Member(base, key) = callee.as_ref()
                    && matches!(base.as_ref(), Expr::Super)
                {
                    let function = self.super_get(key, scope, host)?;
                    let this = self.read(scope, "this", host)?;
                    let args = self.argument_values(args, scope, host)?;
                    return self.call(function, args, this, host);
                }
                if let Expr::PrivateMember(object, name) = callee.as_ref() {
                    let object = self.eval(object, scope, host)?;
                    let class = self.private_class(scope)?;
                    let function = self.private_get(object.clone(), class, name, host)?;
                    let args = self.argument_values(args, scope, host)?;
                    return self.call(function, args, object, host);
                }
                // Keep direct eval in the calling lexical environment.
                if matches!(callee.as_ref(), Expr::Name(name) if name == "eval") {
                    let callable = self.eval(callee, scope, host)?;
                    let args = self.argument_values(args, scope, host)?;
                    return if callable == Value::Native("eval") {
                        self.dynamic_eval(&args, scope, true, host)
                    } else {
                        self.call(callable, args, Value::Undefined, host)
                            .map_err(|error| callables::error_context(error, "eval"))
                    };
                }
                let target = self.prepare_call(callee, scope, host)?;
                let args = self.argument_values(args, scope, host)?;
                self.invoke_prepared_call(target, args, host)
            }
            Expr::Construct(callee, args) => {
                let callable = self.eval(callee, scope, host)?;
                let args = self.argument_values(args, scope, host)?;
                self.construct(callable, args, host)
            }
            Expr::Unary(op, expr) => {
                if op == "delete" {
                    return self.delete_expression(expr, scope, host);
                }
                if op == "typeof"
                    && let Expr::Name(name) = expr.as_ref()
                    && self.lookup_binding(scope, name).is_none()
                    && self.global_own_property(name).is_none()
                    && host.global(name).is_none()
                    && intrinsic_global(name).is_none()
                {
                    return self.string("undefined".into());
                }
                let value = self.eval(expr, scope, host)?;
                self.unary_value(op, value, host)
            }
            Expr::Binary(op, left, right) => {
                let left = self.eval(left, scope, host)?;
                if op == "&&" && !left.truthy()
                    || op == "||" && left.truthy()
                    || op == "??" && !matches!(left, Value::Undefined | Value::Null)
                {
                    return Ok(left);
                }
                let right = self.eval(right, scope, host)?;
                self.binary_value(op, left, right, host)
            }
            Expr::Assign(op, target, expression) => {
                if op == "=" && matches!(target.as_ref(), Expr::Array(_) | Expr::Object(_)) {
                    let value = self.eval(expression, scope, host)?;
                    self.assign_destructure(target, value.clone(), scope, host)?;
                    return Ok(value);
                }
                let place = self.place(target, scope, host)?;
                let previous = if op != "=" {
                    Some(self.read_place(&place, host)?)
                } else {
                    None
                };
                let right = self.eval(expression, scope, host)?;
                let value = if let Some(left) = previous {
                    self.binary(&op[..op.len() - 1], left, right, host)?
                } else {
                    right
                };
                self.write_place(place, value.clone(), host)?;
                Ok(value)
            }
            Expr::Update(target, increment, prefix) => {
                let place = self.place(target, scope, host)?;
                self.update_place(place, *increment, *prefix, host)
            }
            Expr::Conditional(condition, yes, no) => {
                if self.eval(condition, scope, host)?.truthy() {
                    self.eval(yes, scope, host)
                } else {
                    self.eval(no, scope, host)
                }
            }
        }
    }
    fn make_function(
        &mut self,
        params: Arc<Vec<String>>,
        body: Arc<Vec<Stmt>>,
        scope: usize,
        arrow: bool,
    ) -> Result<Value, JsError> {
        let strict = self.strict || strict_validation::strict_directive(&body);
        let lexical_this = self
            .lookup_binding(scope, "this")
            .map(|id| self.scopes[id].bindings["this"].value.clone())
            .unwrap_or(Value::Undefined);
        Ok(Value::Function(self.allocate(Heap::Function(
            Function {
                params,
                body,
                scope,
                arrow,
                lexical_this,
                properties: BTreeMap::new(),
                strict,
                parameters: None,
                constructible: !arrow,
                kind: FunctionKind::Ordinary,
                method: false,
            },
        ))?))
    }
    fn place(
        &mut self,
        target: &Expr,
        scope: usize,
        host: &mut impl Host,
    ) -> Result<Place, JsError> {
        match target {
            Expr::Name(name) => {
                if let Some(id) = self.lookup_binding(scope, name) {
                    Ok(Place::Binding(id, name.clone()))
                } else if let Some(id) = host.global_object() {
                    if self.strict
                        && self.global_own_property(name).is_none()
                        && intrinsic_global(name).is_none()
                        && !host.has_property(id, name)?
                    {
                        return Err(JsError::runtime(format!(
                            "Assignment to undeclared identifier {name}"
                        )));
                    }
                    Ok(Place::Global(id, name.clone()))
                } else {
                    Err(JsError::runtime(format!(
                        "Assignment to undeclared identifier {name}"
                    )))
                }
            }
            Expr::Member(object, property) => {
                if matches!(object.as_ref(), Expr::Super) {
                    let base = self.super_base(scope, host)?;
                    let receiver = self.read(scope, "this", host)?;
                    let key = self.eval(property, scope, host)?;
                    let key = self.coerce_property_key(key, host)?;
                    if matches!(key, Value::Symbol(..)) {
                        return Err(
                            self.property_type_error("Symbol super assignment is not implemented")
                        );
                    }
                    return Ok(Place::SuperMember(base, receiver, self.text(&key, 0)?));
                }
                let object = self.eval(object, scope, host)?;
                let key = self.eval(property, scope, host)?;
                self.ordinary_member_place_values(object, key, host)
            }
            Expr::PrivateMember(object, name) => {
                let object = self.eval(object, scope, host)?;
                let class = self.private_class(scope)?;
                Ok(Place::PrivateMember(object, class, name.clone()))
            }
            _ => Err(JsError::runtime("Invalid assignment target")),
        }
    }
    fn read_place(&mut self, place: &Place, host: &mut impl Host) -> Result<Value, JsError> {
        match place {
            Place::Binding(id, name) => {
                if let Some(value) = self.module_read_binding(*id, name) {
                    return value;
                }
                let binding = &self.scopes[*id].bindings[name];
                if !binding.initialized {
                    return Err(JsError::runtime("Access before initialization"));
                }
                Ok(binding.value.clone())
            }
            Place::Member(object, key) => self.get(object.clone(), key, host),
            Place::SymbolMember(object, key) => self.get_symbol(object.clone(), *key, host),
            Place::PrivateMember(object, class, name) => {
                self.private_get(object.clone(), *class, name, host)
            }
            Place::SuperMember(base, receiver, key) => {
                self.get_with_receiver(base.clone(), key, receiver.clone(), host)
            }
            Place::Global(_, name) => self.read(0, name, host),
        }
    }
    fn write_place(
        &mut self,
        place: Place,
        value: Value,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        match place {
            Place::Binding(id, name) => {
                let binding = self.scopes[id].bindings.get_mut(&name).unwrap();
                if !binding.initialized {
                    return Err(JsError::runtime("Assignment before initialization"));
                }
                if !binding.mutable {
                    return Err(JsError::runtime(format!("Assignment to constant {name}")));
                }
                binding.value = value;
                Ok(())
            }
            Place::Member(object, property) => self.set(object, &property, value, host),
            Place::SymbolMember(object, key) => self.set_symbol(object, key, value, host),
            Place::PrivateMember(object, class, name) => {
                self.private_set(object, class, &name, value, host)
            }
            Place::SuperMember(base, receiver, key) => {
                self.super_set(base, receiver, &key, value, host)
            }
            Place::Global(id, name) => self.set(Value::Host(id), &name, value, host),
        }
    }
    fn get(
        &mut self,
        object: Value,
        property: &str,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        self.get_with_receiver(object.clone(), property, object, host)
    }
    fn get_with_receiver(
        &mut self,
        object: Value,
        property: &str,
        receiver: Value,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        if object == Value::Native("Symbol") {
            let storage = self.symbol_constructor_storage()?;
            return self.get_with_receiver(storage, property, receiver, host);
        }
        if object == Value::Native("Function.prototype") {
            if let Some(descriptor) = self.function_prototype_descriptor(property)? {
                return Ok(descriptor.value);
            }
            let prototype = self.object_prototype_value()?;
            return self.get_with_receiver(prototype, property, receiver, host);
        }
        if let Some(value) = self.wasm_get(&object, property)? {
            return Ok(value);
        }
        if let Some(value) = self.reflection_get(&object, property, host)? {
            return Ok(value);
        }
        if let Some(value) = self.stdlib_get(&object, property)? {
            return Ok(value);
        }
        if let Some(value) = self.promise_get(&object, property)? {
            return Ok(value);
        }
        if let Some(value) = self.weak_ref_get(&object, property)? {
            return Ok(value);
        }
        if let Some(value) = self.weak_collection_get(&object, property)? {
            return Ok(value);
        }
        if let Some(value) = self.finalization_registry_get(&object, property)? {
            return Ok(value);
        }
        match object {
            Value::Host(id) => {
                if host.is_callable(id)
                    && let Some(method) = callables::callable_method(property)
                {
                    return Ok(method);
                }
                if Some(id) == host.global_object() {
                    if host.global_accessor(property) {
                        let value = host.get(id, property)?;
                        return self.host_value(value);
                    }
                    if let Some(value) = self.global_own_property(property) {
                        return Ok(value);
                    }
                    if property == "crypto" {
                        return self.crypto_namespace(host);
                    }
                    if property == "WebAssembly" {
                        return self.wasm_namespace();
                    }
                    if property == "JSON" {
                        return self.json_namespace();
                    }
                    if property == "Reflect" {
                        return self.reflect_namespace();
                    }
                    if property == "Math" {
                        return self.math_namespace();
                    }
                    if let Some(value) = intrinsic_global(property) {
                        return Ok(value);
                    }
                }
                let value = host.get(id, property)?;
                self.host_value(value)
            }
            Value::Array(id) => {
                if let Some(value) = self.read_accessor(id, property, receiver.clone(), host)? {
                    return Ok(value);
                }
                let Heap::Array(values) = &self.heap[id] else {
                    return Err(JsError::runtime("Invalid array reference"));
                };
                if property == "length" {
                    Ok(Value::Number(values.len() as f64))
                } else if let Some(index) = properties::array_index(property)
                    && self.array_has_index(id, index, values.len())
                {
                    Ok(values[index].clone())
                } else {
                    if let Some(value) = self
                        .array_properties
                        .get(&id)
                        .and_then(|properties| properties.get(property))
                        .cloned()
                    {
                        return Ok(value);
                    }
                    let prototype = if Some(id) == self.array_prototype {
                        self.object_prototype_value()?
                    } else {
                        self.array_prototype_value()?
                    };
                    self.get_with_receiver(prototype, property, receiver, host)
                }
            }
            Value::Object(id) => {
                let mut current = Value::Object(id);
                for _ in 0..64 {
                    self.tick()?;
                    match current {
                        Value::Object(id) => {
                            if let Some(value) =
                                self.read_accessor(id, property, receiver.clone(), host)?
                            {
                                return Ok(value);
                            }
                            let Some(Heap::Object(object)) = self.heap.get(id) else {
                                return Err(JsError::runtime("Invalid object reference"));
                            };
                            if let Some(value) = object.properties.get(property) {
                                return Ok(value.clone());
                            }
                            current = object.prototype.clone().unwrap_or(Value::Null);
                        }
                        Value::Null => return Ok(Value::Undefined),
                        other => return self.get_with_receiver(other, property, receiver, host),
                    }
                }
                Err(JsError::resource("Prototype lookup depth exceeded"))
            }
            Value::Function(id) => {
                if let Some(value) = self.read_accessor(id, property, receiver, host)? {
                    return Ok(value);
                }
                let Some(Heap::Function(function)) = self.heap.get(id) else {
                    return Err(JsError::runtime("Invalid function reference"));
                };
                if let Some(value) = function.properties.get(property) {
                    return Ok(value.clone());
                }
                if property == "length" && !self.synthetic_deleted(id, property) {
                    return Ok(Value::Number(function.params.len() as f64));
                }
                if property == "prototype" && function.has_prototype() {
                    if function.properties.len() >= 128 {
                        return Err(JsError::resource("Function property limit exceeded"));
                    }
                    self.charge_string("prototype".len())?;
                    let prototype = self.ordinary_object(BTreeMap::from([(
                        "constructor".into(),
                        Value::Function(id),
                    )]))?;
                    self.hide_properties(&prototype, &["constructor"]);
                    let Heap::Function(function) = &mut self.heap[id] else {
                        unreachable!()
                    };
                    function
                        .properties
                        .insert("prototype".into(), prototype.clone());
                    return Ok(prototype);
                }
                if let Some(base) = self.class_static_parent(id) {
                    return self.get_with_receiver(base, property, Value::Function(id), host);
                }
                Ok(callables::callable_method(property).unwrap_or(Value::Undefined))
            }
            Value::Native(name) => {
                if name == "Array" && property == "prototype" {
                    return self.array_prototype_value();
                }
                if let Some(method) = callables::callable_method(property) {
                    return Ok(method);
                }
                if property == "length" {
                    return Ok(Value::Number(callables::native_length(name) as f64));
                }
                if property == "name" {
                    return self.string(if name == "Function.prototype" {
                        String::new()
                    } else {
                        name.rsplit('.').next().unwrap_or(name).into()
                    });
                }
                Ok(intrinsic_property(name, property).unwrap_or(Value::Undefined))
            }
            object @ (Value::String(_) | Value::Utf16(_)) => Ok(self
                .string_get_property(&object, property)?
                .unwrap_or(Value::Undefined)),
            Value::Symbol(..) => {
                let prototype = self.symbol_prototype()?;
                self.get_with_receiver(prototype, property, receiver, host)
            }
            Value::Null | Value::Undefined => Err(JsError::runtime(
                "Cannot access property of null or undefined",
            )),
            _ => Err(JsError::runtime(format!("Unsupported property {property}"))),
        }
    }
    fn set(
        &mut self,
        object: Value,
        property: &str,
        value: Value,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        if let Some(success) = self.stdlib_set(&object, property, &value, host)? {
            return if success || !self.strict {
                Ok(())
            } else {
                Err(self.property_type_error("Cannot assign to read-only native property"))
            };
        }
        match self.assignment_action(&object, property, value.clone(), host)? {
            descriptors::Assignment::Store => self.set_unchecked(object, property, value, host),
            descriptors::Assignment::Handled => Ok(()),
            descriptors::Assignment::Blocked => {
                if self.strict {
                    Err(self.property_type_error("Cannot assign to read-only property"))
                } else {
                    Ok(())
                }
            }
        }
    }
    fn set_unchecked(
        &mut self,
        object: Value,
        property: &str,
        value: Value,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        if property.len() > 1024 {
            return Err(JsError::resource("Property name exceeds limit"));
        }
        match object {
            Value::Native("Symbol") => {
                let storage = self.symbol_constructor_storage()?;
                self.set_unchecked(storage, property, value, host)
            }
            Value::Host(id) if Some(id) == host.global_object() => {
                if matches!(property, "undefined" | "NaN" | "Infinity") {
                    return Ok(());
                }
                if host.global_accessor(property) {
                    return host.set(id, property, value);
                }
                if self.scopes[0]
                    .bindings
                    .get(property)
                    .is_some_and(|b| b.global_property)
                {
                    return self.write_place(Place::Binding(0, property.to_owned()), value, host);
                }
                let properties = if let Some(id) = self.global_properties {
                    id
                } else {
                    let id = self.allocate(Heap::Object(Object {
                        properties: BTreeMap::new(),
                        non_enumerable: BTreeSet::new(),
                        prototype: None,
                        regexp: None,
                    }))?;
                    self.global_properties = Some(id);
                    id
                };
                self.set(Value::Object(properties), property, value, host)
            }
            Value::Host(id) => host.set(id, property, value),
            Value::Function(id) => {
                self.note_property_insertion(id, property);
                if matches!(self.heap.get(id),Some(Heap::Function(function)) if !function.properties.contains_key(property))
                {
                    self.charge_string(property.len())?;
                }
                let Some(Heap::Function(function)) = self.heap.get_mut(id) else {
                    return Err(JsError::runtime("Invalid function reference"));
                };
                if function.properties.len() >= 128 && !function.properties.contains_key(property) {
                    return Err(JsError::resource("Function property limit exceeded"));
                }
                function.properties.insert(property.into(), value);
                Ok(())
            }
            Value::Object(id) => {
                self.note_property_insertion(id, property);
                if matches!(self.heap.get(id),Some(Heap::Object(object)) if !object.properties.contains_key(property))
                {
                    self.charge_string(property.len())?;
                }
                let Heap::Object(object) = &mut self.heap[id] else {
                    return Err(JsError::runtime("Invalid object"));
                };
                let values = &mut object.properties;
                if object.regexp.is_some()
                    && matches!(
                        property,
                        "source"
                            | "flags"
                            | "global"
                            | "sticky"
                            | "unicode"
                            | "ignoreCase"
                            | "multiline"
                            | "dotAll"
                            | "hasIndices"
                    )
                {
                    return Ok(());
                }
                if values.len() >= 128 && !values.contains_key(property) {
                    return Err(JsError::resource("Object property limit exceeded"));
                }
                values.insert(property.into(), value);
                Ok(())
            }
            Value::Array(id) => {
                if property == "length" {
                    return self.set_array_length(id, value, host);
                }
                let Some(index) = properties::array_index(property) else {
                    self.note_property_insertion(id, property);
                    if !self
                        .array_properties
                        .get(&id)
                        .is_some_and(|properties| properties.contains_key(property))
                    {
                        self.charge_string(property.len())?;
                    }
                    let properties = self.array_properties.entry(id).or_default();
                    if properties.len() >= 128 && !properties.contains_key(property) {
                        return Err(JsError::resource("Array property limit exceeded"));
                    }
                    properties.insert(property.into(), value);
                    return Ok(());
                };
                if index >= 4096 {
                    return Err(JsError::resource("Array index exceeds limit"));
                }
                let Some(Heap::Array(values)) = self.heap.get(id) else {
                    return Err(JsError::runtime("Invalid array"));
                };
                let old_length = values.len();
                self.charge_array_slots((index + 1).saturating_sub(old_length))?;
                let Heap::Array(values) = &mut self.heap[id] else {
                    unreachable!()
                };
                values.resize(old_length.max(index + 1), Value::Undefined);
                values[index] = value;
                let holes = self.array_holes.entry(id).or_default();
                holes.extend(old_length..index);
                holes.remove(&index);
                Ok(())
            }
            _ => Err(self.property_type_error("This value has no supported writable properties")),
        }
    }
    fn call(
        &mut self,
        callable: Value,
        args: Vec<Value>,
        this: Value,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        self.tick()?;
        if args.len() > 32 {
            return Err(JsError::resource("Call argument limit exceeded"));
        }
        if self.call_depth >= self.limits.call_depth {
            return Err(JsError::resource("JavaScript call stack budget exceeded"));
        }
        self.call_depth += 1;
        let result = self.call_inner(callable, args, this, host);
        self.call_depth -= 1;
        result
    }
    fn call_inner(
        &mut self,
        callable: Value,
        args: Vec<Value>,
        this: Value,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        if let Value::Host(id) = callable {
            if !host.is_callable(id) {
                return Err(self.property_type_error("Value is not callable"));
            }
            let value = host.call_with_runtime(id, "call", &args, &this, self)?;
            return self.host_value(value);
        }
        if let Value::Native(name) = callable {
            if name == "Array.from" {
                return self.array_from(&args, this, host);
            }
            if name == "SubtleCrypto.digest" {
                return self.crypto_digest(&args, &this, host);
            }
            if let Some(value) = self.wasm_native_call(name, &args, false, host)? {
                return Ok(value);
            }
            if let Some(value) = self.numeric_global_call(name, &args, false, host)? {
                return Ok(value);
            }
            if let Some(value) = self.conversion_builtin(name, &args, false, host)? {
                return Ok(value);
            }
            if matches!(
                name,
                "BigInt.toString" | "BigInt.valueOf" | "BigInt.toLocaleString"
            ) {
                return self.bigint_method(name, &this, &args);
            }
            if let Some(value) = self.resumable_native_call(name, &args, this.clone(), host)? {
                return Ok(value);
            }
            if let Some(value) = self.reflect_call(name, &args, false, host)? {
                return Ok(value);
            }
            if let Some(value) = self.math_call(name, &args, false, host)? {
                return Ok(value);
            }
            if let Some(value) = self.reflection_call(name, &args, this.clone(), false, host)? {
                return Ok(value);
            }
            if let Some(value) = self.promise_call(name, &args, this.clone(), false, host)? {
                return Ok(value);
            }
            if let Some(value) = self.weak_ref_call(name, &args, this.clone(), false)? {
                return Ok(value);
            }
            if let Some(value) =
                self.finalization_registry_call(name, &args, this.clone(), false, host)?
            {
                return Ok(value);
            }
            if let Some(value) =
                self.weak_collection_call(name, &args, this.clone(), false, host)?
            {
                return Ok(value);
            }
            if let Some(value) = self.stdlib_call(name, &args, this.clone(), false, host)? {
                return Ok(value);
            }
            if name == "JSON.parse" {
                return self.json_parse_with_host(&args, host);
            }
            if name == "JSON.stringify" {
                return self.json_stringify(&args, host);
            }
            if name == "String.fromCharCode" {
                return self.string_from_char_code(&args);
            }
            if name == "String.fromCodePoint" {
                return self.string_from_code_point(&args);
            }
            if let Some(method) = name.strip_prefix("String.") {
                return self.string_method(&this, method, args, host);
            }
            if let Some(method) = name.strip_prefix("Function.")
                && matches!(method, "call" | "apply" | "bind")
            {
                return self.callable_adapter(method, this, args, host);
            }
            if name == "Function.prototype" {
                return Ok(Value::Undefined);
            }
            if name == "Object.hasOwn" {
                let target = args.first().cloned().unwrap_or(Value::Undefined);
                if matches!(target, Value::Null | Value::Undefined) {
                    return Err(
                        self.property_type_error("Object.hasOwn requires a non-null target")
                    );
                }
                let object = self.box_value(target)?;
                let key = self
                    .coerce_property_key(args.get(1).cloned().unwrap_or(Value::Undefined), host)?;
                let present = if let Value::Symbol(symbol, _) = key {
                    self.symbol_descriptor(&object, symbol)?.is_some()
                } else {
                    let name = self.text(&key, 0)?;
                    self.has_own_property(&object, &name)?
                };
                return Ok(Value::Bool(present));
            }
            if name == "eval" {
                return self.dynamic_eval(&args, 0, false, host);
            }
            if name == "Function" {
                return self.dynamic_function(&args, host);
            }
            if matches!(
                name,
                "Object.create"
                    | "Object.getOwnPropertyDescriptor"
                    | "Object.getOwnPropertyDescriptors"
                    | "Object.getOwnPropertyNames"
                    | "Object.defineProperty"
                    | "Object.defineProperties"
                    | "Object.keys"
                    | "Object.values"
                    | "Object.entries"
            ) {
                return self.descriptor_native(name, &args, host);
            }
            if let Some(property) = name.strip_prefix("Array.")
                && matches!(
                    property,
                    "push"
                        | "pop"
                        | "join"
                        | "forEach"
                        | "map"
                        | "slice"
                        | "concat"
                        | "filter"
                        | "reduce"
                        | "reduceRight"
                        | "find"
                        | "findIndex"
                        | "findLast"
                        | "findLastIndex"
                        | "every"
                        | "some"
                        | "indexOf"
                        | "lastIndexOf"
                        | "includes"
                        | "at"
                        | "reverse"
                        | "toReversed"
                        | "shift"
                        | "unshift"
                        | "splice"
                        | "toSpliced"
                        | "sort"
                        | "toSorted"
                        | "fill"
                        | "copyWithin"
                        | "flat"
                        | "flatMap"
                        | "toString"
                )
            {
                let Value::Array(id) = this else {
                    return Err(JsError::runtime("Array method requires an array receiver"));
                };
                return self.array_method(id, property, args, host);
            }
            if matches!(name, "RegExp.test" | "RegExp.exec" | "RegExp.toString") {
                let Value::Object(id) = this else {
                    return Err(JsError::runtime("RegExp method requires a RegExp receiver"));
                };
                let compiled = self.regexp(id)?;
                if name == "RegExp.toString" {
                    return self.string(format!(
                        "/{}/{}",
                        regexp_source(&compiled.source),
                        compiled.flags
                    ));
                }
                let text = self.text(args.first().unwrap_or(&Value::Undefined), 0)?;
                let result = self.regexp_exec(id, &text)?;
                return if name == "RegExp.test" {
                    Ok(Value::Bool(result.is_some()))
                } else {
                    match result {
                        Some(result) => self.regexp_array(result, &text),
                        None => Ok(Value::Null),
                    }
                };
            }
            return self.native(name, &args);
        }
        let Value::Function(id) = callable else {
            return Err(self.property_type_error("Value is not callable"));
        };
        if let Some(bound) = self.bound_functions.get(&id).cloned() {
            let args = self.prepend_bound(&bound, args)?;
            return self.call(bound.target, args, bound.this, host);
        }
        if self.is_class(id) {
            return Err(self.property_type_error("Class constructor requires new"));
        }
        if self.wasm.functions.contains_key(&id) {
            return self.wasm_invoke(id, &args, host);
        }
        self.invoke_function(id, args, this, None, host)
    }
    fn invoke_function(
        &mut self,
        id: usize,
        args: Vec<Value>,
        this: Value,
        construction: Option<usize>,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let Some(Heap::Function(function)) = self.heap.get(id) else {
            return Err(JsError::runtime("Invalid function reference"));
        };
        let function = function.clone();
        let async_promise = if function.kind == FunctionKind::Async {
            Some(self.new_async_promise()?)
        } else {
            None
        };
        let previous_strict = self.strict;
        self.strict = function.strict;
        let result = (|| {
            let scope = self.scope(function.scope, true)?;
            let derived = self.attach_constructor_scope(construction, scope)?;
            self.declare(scope, "this", false, !derived)?;
            self.scopes[scope].bindings.get_mut("this").unwrap().value = if function.arrow {
                if self.lookup_binding(function.scope, "this").is_some() {
                    self.read(function.scope, "this", host)?
                } else {
                    function.lexical_this
                }
            } else if !function.strict && matches!(this, Value::Undefined | Value::Null) {
                host.global("this").unwrap_or(Value::Undefined)
            } else {
                this
            };
            self.attach_method_scope(id, scope)?;
            let mut parameter_names = Vec::new();
            if let Some(parameters) = &function.parameters {
                for parameter in parameters.iter() {
                    parameter.pattern.names(&mut parameter_names);
                }
            } else {
                parameter_names.extend(function.params.iter().cloned());
            }
            if !function.arrow && !parameter_names.iter().any(|param| param == "arguments") {
                let arguments = Value::Array(self.allocate(Heap::Array(args.clone()))?);
                self.declare(scope, "arguments", true, true)?;
                self.scopes[scope]
                    .bindings
                    .get_mut("arguments")
                    .unwrap()
                    .value = arguments;
            }
            if let Some(parameters) = &function.parameters {
                for parameter in parameters.iter() {
                    self.declare_pattern(&parameter.pattern, Declaration::Let, scope)?;
                }
                for name in &parameter_names {
                    if let Some(binding) = self.scopes[scope].bindings.get_mut(name) {
                        binding.lexical = false;
                    }
                }
                for (index, parameter) in parameters.iter().enumerate() {
                    let value = if parameter.rest {
                        Value::Array(
                            self.allocate(Heap::Array(args.iter().skip(index).cloned().collect()))?,
                        )
                    } else {
                        args.get(index).cloned().unwrap_or(Value::Undefined)
                    };
                    self.bind_pattern(&parameter.pattern, value, scope, host)?;
                }
            } else {
                for (index, param) in function.params.iter().enumerate() {
                    self.declare(scope, param, true, true)?;
                    self.scopes[scope].bindings.get_mut(param).unwrap().value =
                        args.get(index).cloned().unwrap_or(Value::Undefined);
                }
            }
            if function.kind == FunctionKind::AsyncGenerator {
                return self.create_async_generator(
                    id,
                    function.body.clone(),
                    scope,
                    function.strict,
                    host,
                );
            }
            if function.kind == FunctionKind::Generator {
                return self.create_generator(
                    id,
                    function.body.clone(),
                    scope,
                    function.strict,
                    host,
                );
            }
            if let Some(Value::Object(owner)) = &async_promise {
                self.resumable_start(
                    *owner,
                    function.body.clone(),
                    scope,
                    function.strict,
                    FunctionKind::Async,
                )?;
                self.resume_async(*owner, true, Value::Undefined, host)?;
                return Ok(Value::Object(*owner));
            }
            let result = self.block(&function.body, scope, host);
            match result? {
                Flow::Return(value) => Ok(value),
                Flow::Normal(_) | Flow::Empty => Ok(Value::Undefined),
                _ => Err(JsError::runtime("Invalid function completion")),
            }
        })();
        self.strict = previous_strict;
        if let Some(Value::Object(owner)) = async_promise {
            if let Err(error) = result {
                self.settle_async_promise(owner, Err(error), host)?;
            }
            return Ok(Value::Object(owner));
        }
        result
    }
    fn object(
        &mut self,
        properties: BTreeMap<String, Value>,
        prototype: Option<Value>,
    ) -> Result<Value, JsError> {
        if properties.len() > 128 {
            return Err(JsError::resource("Object property limit exceeded"));
        }
        let keys = properties.keys().cloned().collect();
        let id = self.allocate(Heap::Object(Object {
            properties,
            non_enumerable: BTreeSet::new(),
            prototype,
            regexp: None,
        }))?;
        self.remember_property_order(id, keys);
        Ok(Value::Object(id))
    }
    fn exception_value(&mut self, error: &JsError) -> Result<Value, JsError> {
        if let Some(value) = &error.thrown {
            return Ok(value.clone());
        }
        let name = self.string(if error.kind == ErrorKind::Syntax {
            "SyntaxError".into()
        } else {
            "Error".into()
        })?;
        let message = self.string(error.message.clone())?;
        self.object(
            BTreeMap::from([("name".into(), name), ("message".into(), message)]),
            None,
        )
    }
    fn construct(
        &mut self,
        callable: Value,
        args: Vec<Value>,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        self.tick()?;
        if args.len() > 32 {
            return Err(JsError::resource("Constructor argument limit exceeded"));
        }
        if let Value::Function(id) = &callable
            && let Some(bound) = self.bound_functions.get(id).cloned()
        {
            return self.construct_bound(bound, args, host);
        }
        if let Value::Host(id) = callable {
            if !host.is_constructor(id) {
                return Err(self.property_type_error("Host value is not a constructor"));
            }
            if self.call_depth >= self.limits.call_depth {
                return Err(JsError::resource("JavaScript call stack budget exceeded"));
            }
            // Host callbacks can reenter the runtime. Keep their arguments live
            // by suppressing collection and use the ordinary call depth budget.
            self.call_depth += 1;
            let result = host.construct_with_runtime(id, &args, self);
            self.call_depth -= 1;
            let value = self.host_value(result?)?;
            let valid = match &value {
                Value::Object(id) => matches!(self.heap.get(*id), Some(Heap::Object(_))),
                Value::Array(id) => matches!(self.heap.get(*id), Some(Heap::Array(_))),
                Value::Function(id) => matches!(self.heap.get(*id), Some(Heap::Function(_))),
                Value::Host(_) | Value::Native(_) => true,
                _ => false,
            };
            return if valid {
                Ok(value)
            } else {
                Err(self.property_type_error("Host constructor must return a valid object"))
            };
        }
        if let Value::Native(name) = callable {
            if name.starts_with("Generator.") || name.starts_with("AsyncGenerator.") {
                return Err(self.property_type_error("Generator methods are not constructors"));
            }
            if name == "Array.from" {
                return Err(self.property_type_error("Array.from is not a constructor"));
            }
            if name == "SubtleCrypto.digest" {
                return Err(self.property_type_error("digest is not a constructor"));
            }
            if let Some(value) = self.wasm_native_call(name, &args, true, host)? {
                return Ok(value);
            }
            if let Some(value) = self.numeric_global_call(name, &args, true, host)? {
                return Ok(value);
            }
            if let Some(value) = self.conversion_builtin(name, &args, true, host)? {
                return Ok(value);
            }
            if let Some(value) = self.reflect_call(name, &args, true, host)? {
                return Ok(value);
            }
            if let Some(value) = self.math_call(name, &args, true, host)? {
                return Ok(value);
            }
            if name == "BigInt" {
                return Err(self.property_type_error("BigInt is not a constructor"));
            }
            if let Some(value) = self.reflection_call(name, &args, Value::Undefined, true, host)? {
                return Ok(value);
            }
            if let Some(value) = self.promise_call(name, &args, Value::Undefined, true, host)? {
                return Ok(value);
            }
            if let Some(value) = self.weak_ref_call(name, &args, Value::Undefined, true)? {
                return Ok(value);
            }
            if let Some(value) =
                self.finalization_registry_call(name, &args, Value::Undefined, true, host)?
            {
                return Ok(value);
            }
            if let Some(value) =
                self.weak_collection_call(name, &args, Value::Undefined, true, host)?
            {
                return Ok(value);
            }
            if let Some(value) = self.stdlib_call(name, &args, Value::Undefined, true, host)? {
                return Ok(value);
            }
            if name == "Function" {
                return self.dynamic_function(&args, host);
            }
            if name == "RegExp" {
                return self.regexp_constructor(&args, false);
            }
            if matches!(
                name,
                "Object"
                    | "Array"
                    | "Error"
                    | "TypeError"
                    | "RangeError"
                    | "ReferenceError"
                    | "SyntaxError"
            ) {
                return self.native(name, &args);
            }
            return Err(JsError::runtime(
                "This native function is not a supported constructor",
            ));
        }
        let Value::Function(id) = callable else {
            return Err(self.property_type_error("Value is not a constructor"));
        };
        if self.is_class(id) {
            return self.construct_class(id, args, id, host);
        }
        let Some(Heap::Function(function)) = self.heap.get(id) else {
            return Err(JsError::runtime("Invalid function reference"));
        };
        if !function.constructible {
            return Err(self.property_type_error("This function is not a constructor"));
        }
        let prototype = self.get(Value::Function(id), "prototype", host)?;
        let prototype = if matches!(
            prototype,
            Value::Object(_) | Value::Function(_) | Value::Host(_) | Value::Array(_)
        ) {
            Some(prototype)
        } else {
            Some(self.object_prototype_value()?)
        };
        let instance = self.object(BTreeMap::new(), prototype)?;
        let result = self.call(Value::Function(id), args, instance.clone(), host)?;
        if matches!(
            result,
            Value::Object(_) | Value::Array(_) | Value::Function(_) | Value::Host(_)
        ) {
            Ok(result)
        } else {
            Ok(instance)
        }
    }
    fn method(
        &mut self,
        object: Value,
        property: &str,
        args: Vec<Value>,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        self.tick()?;
        if self.is_callable_value(&object, host) && matches!(property, "call" | "apply" | "bind") {
            return self.callable_adapter(property, object, args, host);
        }
        if matches!(object, Value::String(_) | Value::Utf16(_)) {
            return self.string_method(&object, property, args, host);
        }
        match object.clone() {
            Value::Host(id) => {
                if args.len() > 32 {
                    return Err(JsError::resource("Call argument limit exceeded"));
                }
                if self.call_depth >= self.limits.call_depth {
                    return Err(JsError::resource("JavaScript call stack budget exceeded"));
                }
                // Legacy host methods can synchronously invoke page callbacks.
                // Keep one frame active until the entire method has returned,
                // preventing GC and nested microtask checkpoints between them.
                self.call_depth += 1;
                let result = host.call_with_runtime(id, property, &args, &Value::Host(id), self);
                self.call_depth -= 1;
                self.host_value(result?)
            }
            Value::Object(_) => {
                let function = self.get(object.clone(), property, host)?;
                self.call(function, args, object, host)
            }
            Value::Function(_) => {
                let function = self.get(object.clone(), property, host)?;
                self.call(function, args, object, host)
            }
            Value::Native(name) => {
                if let Some(Value::Native(method)) = intrinsic_property(name, property) {
                    self.call(Value::Native(method), args, object, host)
                } else {
                    Err(JsError::runtime(format!(
                        "Unsupported {name} method {property}"
                    )))
                }
            }
            Value::Array(_) => {
                let function = self.get(object.clone(), property, host)?;
                self.call(function, args, object, host)
            }
            Value::String(text) => {
                if matches!(property, "match" | "search" | "replace" | "split") {
                    return self.regexp_string_method(&text, property, args, host);
                }
                let argument = if let Some(value) = args.first() {
                    self.text(value, 0)?
                } else {
                    "undefined".into()
                };
                match property {
                    "toUpperCase" => self.string(text.to_uppercase()),
                    "toLowerCase" => self.string(text.to_lowercase()),
                    "trim" => self.string(text.trim().into()),
                    "includes" => Ok(Value::Bool(text.contains(&argument))),
                    "startsWith" => Ok(Value::Bool(text.starts_with(&argument))),
                    "endsWith" => Ok(Value::Bool(text.ends_with(&argument))),
                    "slice" => {
                        if !text.is_ascii() {
                            return Err(JsError::runtime(
                                "UTF-16 string slicing is not implemented",
                            ));
                        }
                        let length = text.len() as i64;
                        let index = |n: f64| {
                            let n = n as i64;
                            (if n < 0 {
                                (length + n).clamp(0, length)
                            } else {
                                n.min(length)
                            }) as usize
                        };
                        let start = index(args.first().map_or(0., |v| self.number(v)));
                        let end = index(args.get(1).map_or(length as f64, |v| self.number(v)));
                        self.string(text[start..end.max(start)].into())
                    }
                    _ => Err(JsError::runtime(format!(
                        "Unsupported string method {property}"
                    ))),
                }
            }
            Value::Number(number) if property == "toFixed" => {
                if args
                    .first()
                    .is_some_and(|value| matches!(value, Value::BigInt(_) | Value::Symbol(..)))
                {
                    return Err(self.property_type_error(
                        "Cannot implicitly convert BigInt or Symbol to a number",
                    ));
                }
                let digits = args.first().map_or(0., |v| self.number(v));
                if !(0.0..=20.0).contains(&digits) {
                    return Err(JsError::runtime("toFixed digits must be between 0 and 20"));
                }
                self.string(format!("{number:.precision$}", precision = digits as usize))
            }
            _ => Err(JsError::runtime(format!("Unsupported method {property}"))),
        }
    }
    fn native(&mut self, name: &str, args: &[Value]) -> Result<Value, JsError> {
        let first = args.first().cloned().unwrap_or(Value::Undefined);
        match name {
            "Number" | "String" | "BigInt" | "BigInt.asIntN" | "BigInt.asUintN" => {
                Err(self
                    .property_type_error("Conversion builtin requires the Host-aware call path"))
            }
            "JSON.parse" => self.json_parse(args),
            "RegExp" => self.regexp_constructor(args, true),
            "Error" | "TypeError" | "RangeError" | "ReferenceError" | "SyntaxError" => {
                let prototype = self.reflection_prototype(
                    crate::reflection::error_constructor(name).expect("matched error constructor"),
                )?;
                let error_name = self.string(name.into())?;
                let message = if matches!(first, Value::Undefined) {
                    self.string(String::new())?
                } else {
                    let text = self.text(&first, 0)?;
                    self.string(text)?
                };
                let error = self.object(
                    BTreeMap::from([("name".into(), error_name), ("message".into(), message)]),
                    Some(prototype),
                )?;
                self.hide_properties(&error, &["name", "message"]);
                Ok(error)
            }
            "Object" => match first {
                Value::Undefined | Value::Null => self.object(BTreeMap::new(), None),
                Value::Array(_) | Value::Object(_) | Value::Function(_) | Value::Host(_) => {
                    Ok(first)
                }
                _ => Err(JsError::runtime(
                    "Primitive wrapper objects are not implemented",
                )),
            },
            "Array" => {
                let values = if args.len() == 1
                    && let Value::Number(length) = first
                {
                    if !length.is_finite() || length < 0. || length.fract() != 0. {
                        return Err(JsError::runtime("Invalid array length"));
                    }
                    if length > 4096. {
                        return Err(JsError::resource("Array item limit exceeded"));
                    }
                    self.check_array_slots(length as usize)?;
                    vec![Value::Undefined; length as usize]
                } else {
                    args.to_vec()
                };
                let length = values.len();
                let id = self.allocate(Heap::Array(values))?;
                if args.len() == 1 && matches!(first, Value::Number(_)) && length > 0 {
                    self.array_holes.insert(id, (0..length).collect());
                }
                Ok(Value::Array(id))
            }
            "Array.isArray" => Ok(Value::Bool(matches!(first, Value::Array(_)))),
            "String.fromCharCode" => self.string_from_char_code(args),
            "Object.getPrototypeOf" => match first {
                Value::Native("Function.prototype") => self.object_prototype_value(),
                Value::BigInt(_) => self.reflection_prototype("BigInt"),
                Value::Function(_) => Ok(Value::Native("Function.prototype")),
                Value::Native(name) if name != "Function.prototype" => {
                    Ok(Value::Native("Function.prototype"))
                }
                Value::Object(id) => {
                    let Some(Heap::Object(object)) = self.heap.get(id) else {
                        return Err(JsError::runtime("Invalid object reference"));
                    };
                    Ok(object.prototype.clone().unwrap_or(Value::Null))
                }
                Value::Array(id) => {
                    if Some(id) == self.array_prototype {
                        self.object_prototype_value()
                    } else {
                        self.array_prototype_value()
                    }
                }
                _ => Err(JsError::runtime(
                    "Prototype inspection supports ordinary objects only",
                )),
            },
            "Object.hasOwn" => {
                Err(self.property_type_error("Object.hasOwn requires the Host-aware call path"))
            }
            "Boolean" => Ok(Value::Bool(first.truthy())),
            "parseInt"
            | "parseFloat"
            | "isNaN"
            | "isFinite"
            | "Number.isNaN"
            | "Number.isFinite"
            | "Number.isInteger"
            | "Number.isSafeInteger" => {
                Err(self.property_type_error("Numeric builtin requires the Host-aware call path"))
            }
            _ => Err(JsError::runtime("Unknown native function")),
        }
    }
    fn text(&mut self, value: &Value, depth: usize) -> Result<String, JsError> {
        self.tick()?;
        if depth > 8 {
            return Err(JsError::resource("Object conversion recursion exceeded"));
        }
        match value {
            Value::Array(id) => {
                let Some(Heap::Array(values)) = self.heap.get(*id) else {
                    return Err(JsError::runtime("Invalid array reference"));
                };
                let values = values.clone();
                let mut text = String::new();
                for (index, value) in values.iter().enumerate() {
                    if index > 0 {
                        text.push(',');
                    }
                    if !matches!(value, Value::Null | Value::Undefined) {
                        text.push_str(&self.text(value, depth + 1)?);
                    }
                    if text.len() > 65_536 {
                        return Err(JsError::resource("String conversion exceeds limit"));
                    }
                }
                Ok(text)
            }
            Value::Object(id) if self.regexp(*id).is_ok() => {
                let compiled = self.regexp(*id)?;
                Ok(format!(
                    "/{}/{}",
                    regexp_source(&compiled.source),
                    compiled.flags
                ))
            }
            Value::Object(id) if self.reflection.boxed.contains_key(id) => {
                let primitive = self.reflection.boxed[id].clone();
                if matches!(primitive, Value::BigInt(_)) {
                    return Err(self.property_type_error(
                        "BigInt wrapper coercion is not implemented; call toString explicitly",
                    ));
                }
                self.text(&primitive, depth + 1)
            }
            Value::Object(_) | Value::Host(_) => Ok("[object Object]".into()),
            Value::Function(_) | Value::Native(_) => Err(JsError::runtime(
                "Function source conversion is not supported",
            )),
            _ => value.primitive_text(),
        }
    }
    fn number(&self, value: &Value) -> f64 {
        match value {
            Value::Number(n) => *n,
            Value::Null => 0.,
            Value::Bool(v) => {
                if *v {
                    1.
                } else {
                    0.
                }
            }
            Value::String(s) => numeric_string(s),
            Value::Utf16(s) => String::from_utf16(s).map_or(f64::NAN, |text| numeric_string(&text)),
            Value::Object(id) => self
                .reflection
                .boxed
                .get(id)
                .map_or(f64::NAN, |value| self.number(value)),
            _ => f64::NAN,
        }
    }
    fn primitive_binary(&mut self, op: &str, left: Value, right: Value) -> Result<Value, JsError> {
        // Only primitive operands reach this side-effect-free calculation.
        if op == "+"
            && (matches!(left, Value::String(_) | Value::Utf16(_))
                || matches!(right, Value::String(_) | Value::Utf16(_)))
        {
            let mut text = self.string_units_coerce(&left)?;
            let tail = self.string_units_coerce(&right)?;
            if text.len().saturating_add(tail.len()) > 65_536 {
                return Err(JsError::resource("String concatenation exceeds limit"));
            }
            text.extend(tail);
            return self.utf16_string(text);
        }
        if matches!(left, Value::BigInt(_)) || matches!(right, Value::BigInt(_)) {
            return self.bigint_binary(op, &left, &right);
        }
        if !matches!(op, "===" | "!==" | "==" | "!=")
            && (matches!(left, Value::Symbol(..)) || matches!(right, Value::Symbol(..)))
        {
            return Err(self.property_type_error("Cannot convert a Symbol to a number"));
        }
        let a = self.number(&left);
        let b = self.number(&right);
        Ok(match op {
            "+" => Value::Number(a + b),
            "-" => Value::Number(a - b),
            "*" => Value::Number(a * b),
            "/" => Value::Number(a / b),
            "%" => Value::Number(a % b),
            "**" => Value::Number(a.powf(b)),
            "&" => Value::Number((to_int32(a) & to_int32(b)) as f64),
            "|" => Value::Number((to_int32(a) | to_int32(b)) as f64),
            "^" => Value::Number((to_int32(a) ^ to_int32(b)) as f64),
            "<<" => Value::Number(to_int32(a).wrapping_shl(to_uint32(b) & 31) as f64),
            ">>" => Value::Number((to_int32(a) >> (to_uint32(b) & 31)) as f64),
            ">>>" => Value::Number((to_uint32(a) >> (to_uint32(b) & 31)) as f64),
            "===" => Value::Bool(strict_equal(&left, &right)),
            "!==" => Value::Bool(!strict_equal(&left, &right)),
            "==" | "!=" => {
                let same = if matches!(
                    (&left, &right),
                    (Value::Null, Value::Undefined) | (Value::Undefined, Value::Null)
                ) {
                    true
                } else if std::mem::discriminant(&left) == std::mem::discriminant(&right)
                    || (matches!(left, Value::String(_) | Value::Utf16(_))
                        && matches!(right, Value::String(_) | Value::Utf16(_)))
                {
                    strict_equal(&left, &right)
                } else if matches!(
                    left,
                    Value::Number(_) | Value::String(_) | Value::Utf16(_) | Value::Bool(_)
                ) && matches!(
                    right,
                    Value::Number(_) | Value::String(_) | Value::Utf16(_) | Value::Bool(_)
                ) {
                    a == b
                } else {
                    false
                };
                Value::Bool(if op == "==" { same } else { !same })
            }
            "<" | ">" | "<=" | ">=" => {
                let order =
                    if let (Some(a), Some(b)) = (strings::units(&left), strings::units(&right)) {
                        Some(a.cmp(&b))
                    } else {
                        a.partial_cmp(&b)
                    };
                Value::Bool(match op {
                    "<" => order.is_some_and(|o| o.is_lt()),
                    ">" => order.is_some_and(|o| o.is_gt()),
                    "<=" => order.is_some_and(|o| o.is_le()),
                    _ => order.is_some_and(|o| o.is_ge()),
                })
            }
            _ => {
                return Err(JsError::runtime(format!(
                    "Unsupported binary operator {op}"
                )));
            }
        })
    }
}

fn intrinsic_global(name: &str) -> Option<Value> {
    if let Some(value) = stdlib::global(name)
        .or_else(|| promises::global(name))
        .or_else(|| weak_refs::global(name))
        .or_else(|| weak_collections::global(name))
        .or_else(|| finalization_registry::global(name))
    {
        return Some(value);
    }
    Some(match name {
        "undefined" => Value::Undefined,
        "NaN" => Value::Number(f64::NAN),
        "Infinity" => Value::Number(f64::INFINITY),
        "String" => Value::Native("String"),
        "Number" => Value::Native("Number"),
        "BigInt" => Value::Native("BigInt"),
        "Boolean" => Value::Native("Boolean"),
        "Object" => Value::Native("Object"),
        "Array" => Value::Native("Array"),
        "eval" => Value::Native("eval"),
        "Function" => Value::Native("Function"),
        "RegExp" => Value::Native("RegExp"),
        // Recognition for `typeof`; reads return the lazily allocated namespace.
        "JSON" => Value::Native("JSON"),
        "WebAssembly" => Value::Native("WebAssembly"),
        "Reflect" => Value::Native("Reflect"),
        "Math" => Value::Native("Math"),
        "Error" => Value::Native("Error"),
        "TypeError" => Value::Native("TypeError"),
        "RangeError" => Value::Native("RangeError"),
        "ReferenceError" => Value::Native("ReferenceError"),
        "SyntaxError" => Value::Native("SyntaxError"),
        "parseInt" => Value::Native("parseInt"),
        "parseFloat" => Value::Native("parseFloat"),
        "isNaN" => Value::Native("isNaN"),
        "isFinite" => Value::Native("isFinite"),
        _ => return None,
    })
}
fn intrinsic_property(name: &str, property: &str) -> Option<Value> {
    if name == "Number"
        && let Some(value) = numeric_globals::number_property(property)
    {
        return Some(value);
    }
    Some(Value::Native(match (name, property) {
        ("Function", "prototype") => "Function.prototype",
        ("BigInt", "asIntN") => "BigInt.asIntN",
        ("BigInt", "asUintN") => "BigInt.asUintN",
        ("Array", "isArray") => "Array.isArray",
        ("Array", "from") => "Array.from",
        ("String", "fromCharCode") => "String.fromCharCode",
        ("String", "fromCodePoint") => "String.fromCodePoint",
        ("Object", "create") => "Object.create",
        ("Object", "getPrototypeOf") => "Object.getPrototypeOf",
        ("Object", "hasOwn") => "Object.hasOwn",
        ("Object", "keys") => "Object.keys",
        ("Object", "values") => "Object.values",
        ("Object", "entries") => "Object.entries",
        ("Object", "getOwnPropertyDescriptor") => "Object.getOwnPropertyDescriptor",
        ("Object", "getOwnPropertyDescriptors") => "Object.getOwnPropertyDescriptors",
        ("Object", "getOwnPropertyNames") => "Object.getOwnPropertyNames",
        ("Object", "defineProperty") => "Object.defineProperty",
        ("Object", "defineProperties") => "Object.defineProperties",
        _ => return None,
    }))
}

fn numeric_string(text: &str) -> f64 {
    let text =
        text.trim_matches(|ch: char| (ch.is_whitespace() && ch != '\u{85}') || ch == '\u{feff}');
    if text.is_empty() {
        return 0.;
    }
    if matches!(text, "Infinity" | "+Infinity") {
        return f64::INFINITY;
    }
    if text == "-Infinity" {
        return f64::NEG_INFINITY;
    }
    for (prefixes, radix) in [(["0x", "0X"], 16), (["0b", "0B"], 2), (["0o", "0O"], 8)] {
        if let Some(digits) = text
            .strip_prefix(prefixes[0])
            .or_else(|| text.strip_prefix(prefixes[1]))
        {
            return numeric_globals::power_of_two_integer(digits.encode_utf16(), radix);
        }
    }
    // Rust also accepts "inf"; JavaScript StringNumericLiteral does not.
    if !text
        .bytes()
        .all(|ch| ch.is_ascii_digit() || matches!(ch, b'+' | b'-' | b'.' | b'e' | b'E'))
    {
        return f64::NAN;
    }
    text.parse().unwrap_or(f64::NAN)
}

fn to_uint32(number: f64) -> u32 {
    if !number.is_finite() || number == 0. {
        0
    } else {
        number.trunc().rem_euclid(4_294_967_296.) as u32
    }
}
fn to_int32(number: f64) -> i32 {
    to_uint32(number) as i32
}

fn strict_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => a == b,
        (Value::BigInt(a), Value::BigInt(b)) => a == b,
        (Value::Undefined, Value::Undefined) | (Value::Null, Value::Null) => true,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::String(a), Value::String(b)) => a == b,
        (Value::Utf16(a), Value::Utf16(b)) => a == b,
        (Value::String(a), Value::Utf16(b)) | (Value::Utf16(b), Value::String(a)) => {
            a.encode_utf16().eq(b.iter().copied())
        }
        (Value::Symbol(a, _), Value::Symbol(b, _)) => a == b,
        (Value::Array(a), Value::Array(b))
        | (Value::Object(a), Value::Object(b))
        | (Value::Function(a), Value::Function(b)) => a == b,
        (Value::Host(a), Value::Host(b)) => a == b,
        (Value::Native(a), Value::Native(b)) => a == b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn async_generator_functions_have_prototypes_but_are_not_constructors() {
        assert_eq!(run("async function* g(){yield 1;}const it=g();let errors=0;try{new g();}catch(e){if(e.name==='TypeError')errors++;}try{new it.next();}catch(e){if(e.name==='TypeError')errors++;}typeof g.prototype==='object'&&Object.getPrototypeOf(it)===g.prototype&&it[Symbol.asyncIterator]()===it&&typeof it[Symbol.iterator]==='undefined'&&it.next.length===1&&it.throw.length===1&&it.return.length===1&&errors===2;").unwrap(),Value::Bool(true));
    }
    #[test]
    fn async_generator_parameter_errors_are_synchronous_and_do_not_start_body() {
        assert_eq!(run("let entered=0;function fail(){throw 9;}async function* g(x=fail()){entered++;yield x;}let caught=false;try{g();}catch(e){caught=e===9;}caught&&entered===0;").unwrap(), Value::Bool(true));
    }
    #[test]
    fn delegated_missing_throw_accepts_native_function_as_close_result() {
        assert_eq!(run("const source={};source[Symbol.iterator]=()=>({next(){return {done:false};},return(){return Object;}});function* g(){yield* source;}const iterator=g();iterator.next();try{iterator.throw(1);false;}catch(e){e.message==='Delegated iterator has no throw method';}").unwrap(),Value::Bool(true));
        assert_eq!(run("const source={};source[Symbol.iterator]=()=>({next(){return Object;},return(){return Object;}});let count=0;for(const value of source){count++;break;}count===1;").unwrap(),Value::Bool(true));
    }
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
    fn run(source: &str) -> Result<Value, JsError> {
        Runtime::default().execute(source, &mut Empty)
    }
    #[test]
    fn empty_completions_do_not_end_switch_or_replace_try_completion() {
        for source in [
            "let n=0;switch(1){case 1:let x=2;;n=x;case 2:;n+=3;break;default:n=99}n===5;",
            "function f(){try{return 7}finally{}}f()===7;",
            "function f(){try{return 7}finally{let ignored=2}}f()===7;",
            "let caught=false;try{try{throw 17}finally{let ignored=3}}catch(e){caught=e===17}caught;",
        ] {
            assert_eq!(run(source).unwrap(), Value::Bool(true), "{source}");
        }
        assert_eq!(
            run("try{while(true){}}finally{let ignored=3}")
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
    }
    #[test]
    fn regexp_literals_division_and_control_contexts_are_distinct() {
        assert_eq!(
            run("const a={if:function(){return 12;}}; a.if()/3;").unwrap(),
            Value::Number(4.)
        );
        for source in [
            "12 / 3 / 2; if(true) /a/.test('a');",
            "function f(){return /a/.test('a');} f();",
            "if(false){} /a/.test('a');",
            "const x={value:12}; x.value/3===4 && /[\\/\\]]+/.test('/]');",
            "let f=function(){return 12;}; f()/3===4;",
            "const f=()=>{return /a/.test('a');}; f();",
            "const f=()=>{ {} /a/.test('a'); return true;}; f();",
            "label:{} /a/.test('a');",
            "function f(){switch(1){case 1:{} /a/.test('a');return true;}}f();",
            "const r= /a/; r.test('a');",
        ] {
            assert_eq!(run(source).unwrap(), Value::Bool(true), "{source}");
        }
        assert_eq!(
            run("let n=0; for(;n<1;n++) /x/.test('x'); n;").unwrap(),
            Value::Number(1.)
        );
        assert_eq!(run("({value:8}).value / 2;").unwrap(), Value::Number(4.));
        assert_eq!(
            run("const a=true?{n:1}:{n:2}; a.n/2;").unwrap(),
            Value::Number(0.5)
        );
        assert_eq!(
            run("const a={return:12}; a.return/3;").unwrap(),
            Value::Number(4.)
        );
    }
    #[test]
    fn regexp_test_exec_and_last_index_match_standard_state() {
        assert_eq!(
            run("/[a&&b]/.test('&') && /[a~~b]/.test('~') && !/[a&&b]/.test('c');").unwrap(),
            Value::Bool(true)
        );
        assert_eq!(run("/a{b/.test('a{b') && /a}/.test('a}') && /[[]/u.test('[') && /[^]/.test('\\n') && !/[]/.test('x');").unwrap(),Value::Bool(true));
        assert_eq!(
            run("/a{2,3}/.exec('aaa')[0];").unwrap(),
            Value::string("aaa")
        );
        assert_eq!(
            run("String(/a\\/b/gi);").unwrap(),
            Value::string("/a\\/b/gi")
        );
        assert_eq!(run("const r=/a(b)?/g;const a=r.exec('ab a');const x=a[0]+':'+a[1]+':'+a.index+':'+r.lastIndex;const b=r.exec('ab a');const y=b[1]===undefined;const z=r.exec('ab a')===null; x+':'+b.index+':'+y+':'+z+':'+r.lastIndex;").unwrap(),Value::string("ab:b:0:2:3:true:true:0"));
        assert_eq!(run("const r=/b/y;r.lastIndex=1;const a=r.test('ab');r.lastIndex=0;const b=r.test('ab');a+':'+b+':'+r.lastIndex;").unwrap(),Value::string("true:false:0"));
        assert_eq!(
            run("const r=/a/;r.lastIndex=17;r.test('a');r.lastIndex;").unwrap(),
            Value::Number(17.)
        );
        assert_eq!(run("const r=/a/g; const copy=new RegExp(r); RegExp(r)===r && copy!==r && copy.flags==='g';").unwrap(),Value::Bool(true));
        assert_eq!(
            run("/é/iu.test('É') && /a.b/s.test('a\\nb') && !/a.b/.test('a\\rb');").unwrap(),
            Value::Bool(true)
        );
    }
    #[test]
    fn regexp_string_operations_and_replacement_callbacks_are_real() {
        assert_eq!(
            run("'a1b22'.match(/\\d+/g).join(',');").unwrap(),
            Value::string("1,22")
        );
        assert_eq!(
            run("const r=/b/g;r.lastIndex=7; const n='abc'.search(r);n+':'+r.lastIndex;").unwrap(),
            Value::string("1:7")
        );
        assert_eq!(
            run("'abc'.match(/(?:)/g).length;").unwrap(),
            Value::Number(4.)
        );
        assert_eq!(
            run("'a1b22'.replace(/(\\d+)/g,(match,n,index)=>'['+n+':'+index+']');").unwrap(),
            Value::string("a[1:1]b[22:3]")
        );
        assert_eq!(
            run("'abc'.replace(/(b)/,'$$-$&-$1-$`-$\\\'');").unwrap(),
            Value::string("a$-b-b-a-cc")
        );
        assert_eq!(
            run("'a,b,,c'.split(/(,)/).join('|');").unwrap(),
            Value::string("a|,|b|,||,|c")
        );
        assert_eq!(
            run("'abc'.split(/(?:)/).join('|');").unwrap(),
            Value::string("a|b|c")
        );
        assert_eq!(run("''.split(/(?:)/).length;").unwrap(), Value::Number(0.));
        assert_eq!(
            run("'a--b--c'.split('--',2).join('|');").unwrap(),
            Value::string("a|b")
        );
        assert_eq!(
            run("'abc'.replace('b',()=>12);").unwrap(),
            Value::string("a12c")
        );
    }
    #[test]
    fn regexp_semantic_limits_are_explicit_and_resource_errors_uncatchable() {
        let mut vm = Runtime::default();
        assert_eq!(
            vm.execute("var effect=1; /(?<=a*)b/;", &mut Empty)
                .unwrap_err()
                .kind,
            ErrorKind::Syntax
        );
        assert!(vm.global_value("effect").is_none());
        assert_eq!(
            run("try {new RegExp('a{99999999}');} catch(error) {42;}")
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
        assert_eq!(
            run("try {let n=0;while(n<100){new RegExp('a');n++;}} catch(error){42;}")
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
        assert_eq!(run("/a$/.test('a\\n');").unwrap(), Value::Bool(false));
        assert!(run("/./.test('😀');").is_err());
        assert_eq!(run("/./u.exec('😀').index;").unwrap(), Value::Number(0.));
        assert!(run("/a/gg;").is_err());
        assert_eq!(
            run("new RegExp('(a)\\\\1').test('aa');").unwrap(),
            Value::Bool(true)
        );
        assert_eq!(run("/(?=a)a/.test('a');").unwrap(), Value::Bool(true));
        assert_eq!(
            run("new RegExp('(?<=a*)b');").unwrap_err().kind,
            ErrorKind::Runtime
        );
    }
    #[test]
    fn arithmetic_control_flow_functions_and_arrow_callbacks() {
        assert_eq!(run("function double(n){return n*2;} let sum=0; for(let i=0;i<5;i++){sum+=double(i);} sum;").unwrap(),Value::Number(20.));
        assert_eq!(
            run("const values=[1,2,3].map(x=>x*3); values.join('-');").unwrap(),
            Value::string("3-6-9")
        );
        assert_eq!(
            run("let n=0;while(n<4){n++;if(n===2){continue;}if(n===3){break;}}n;").unwrap(),
            Value::Number(3.)
        );
    }
    #[test]
    fn lexical_scopes_tdz_closures_and_constants() {
        assert_eq!(
            run("let n=3;{let n=4;}const f=(x)=>{return x+n;};f(2);").unwrap(),
            Value::Number(5.)
        );
        assert!(run("const a=1;a=2;").is_err());
        assert!(run("let a=1;{a;let a=2;}").is_err());
        assert_eq!(
            run("function maker(x){return y=>x+y;}let f=maker(8);f(2);").unwrap(),
            Value::Number(10.)
        );
    }
    #[test]
    fn objects_arrays_unicode_and_short_circuit() {
        assert_eq!(
            run("let o={title:'Ação',count:1};o.count+=2;o.title+' '+o.count;").unwrap(),
            Value::string("Ação 3")
        );
        assert_eq!(run("'🌙'.length;").unwrap(), Value::Number(2.));
        assert_eq!(run("true || unavailable();").unwrap(), Value::Bool(true));
        assert_eq!(
            run("typeof unavailable;").unwrap(),
            Value::string("undefined")
        );
    }
    #[test]
    fn call_and_assignment_evaluation_order_preserves_side_effects_once() {
        assert_eq!(
            run("let i=0;let a=[10,20];a[i++]+=5;a.join(',')+' '+i;").unwrap(),
            Value::string("15,20 1")
        );
        assert_eq!(
            run("let f=n=>10+n;function change(){f=n=>20;return 1;}f(change());").unwrap(),
            Value::Number(11.)
        );
        assert_eq!(
            run("let o={f:n=>10+n};function change(){o.f=n=>20;return 1;}o.f(change());").unwrap(),
            Value::Number(11.)
        );
        let long = format!("{}1;", "1+".repeat(1000));
        assert_eq!(run(&long).unwrap_err().kind, ErrorKind::Resource);
    }
    #[test]
    fn unsupported_syntax_is_rejected_before_any_execution() {
        // Classes and template interpolation are supported; these must execute.
        assert_eq!(
            run("class Example{}; new Example() instanceof Example;").unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            run("const value=3;`value ${value}`;").unwrap(),
            Value::string("value 3")
        );
        let mut runtime = Runtime::default();
        for script in [
            "let changed=1;({get value(){return 1;}});",
            "let changed=1;import x from 'x';",
            "let changed=1;({['key']:1});",
            "let changed=1;const x:number=3;",
        ] {
            assert_eq!(
                runtime.execute(script, &mut Empty).unwrap_err().kind,
                ErrorKind::Syntax
            );
            assert!(runtime.global_value("changed").is_none());
        }
    }
    #[test]
    fn call_depth_has_an_absolute_ceiling_and_releases_after_fatal_errors() {
        for requested in [0, 4, MAX_CALL_DEPTH, usize::MAX] {
            let mut vm = Runtime::new(Limits {
                call_depth: requested,
                ..Limits::default()
            });
            let effective = requested.min(MAX_CALL_DEPTH);
            assert_eq!(vm.limits.call_depth, effective);
            vm.execute("let entered=0;let caught=0;let finalized=0;function recurse(){entered++;return recurse()}", &mut Empty).unwrap();
            let before = vm.clone();
            let remaining = vm.remaining;
            let error = vm
                .execute(
                    "try{recurse()}catch(e){caught++}finally{finalized++}",
                    &mut Empty,
                )
                .unwrap_err();
            assert_eq!(error.kind, ErrorKind::Resource);
            assert!(error.thrown.is_none());
            assert_eq!(error.message, "JavaScript call stack budget exceeded");
            assert_eq!(
                vm.global_value("entered"),
                Some(Value::Number(effective as f64))
            );
            assert_eq!(vm.global_value("caught"), Some(Value::Number(0.)));
            assert_eq!(vm.global_value("finalized"), Some(Value::Number(0.)));
            assert_eq!(
                (vm.call_depth, vm.eval_depth, vm.conversion_depth),
                (0, 0, 0)
            );
            let spent = vm.remaining;
            assert!(spent < remaining);
            vm.rollback_to(before);
            assert_eq!(vm.remaining, spent);
            assert_eq!(vm.call_depth, 0);
            assert_eq!(vm.execute("1+1", &mut Empty).unwrap(), Value::Number(2.));
        }
    }

    #[test]
    fn call_depth_accepts_finite_recursion_at_the_ceiling() {
        for source in [
            "function f(n){if(n===1)return 7;return f(n-1)}f(16)",
            "function f(n){if(n===1)return 7;return g(n-1)}function g(n){if(n===1)return 7;return f(n-1)}f(16)",
            "let o={};let n=0;Object.defineProperty(o,'x',{get:function(){n++;if(n===16)return 7;return o.x}});o.x",
        ] {
            let mut vm = Runtime::default();
            assert_eq!(
                vm.execute(source, &mut Empty).unwrap(),
                Value::Number(7.),
                "{source}"
            );
            assert_eq!((vm.call_depth, vm.eval_depth), (0, 0));
        }
    }
    #[test]
    fn untrusted_execution_and_allocation_are_bounded() {
        assert_eq!(run("while(true){}").unwrap_err().kind, ErrorKind::Resource);
        assert_eq!(
            run("function recurse(){return recurse();}recurse();")
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
        let mut runtime = Runtime::new(Limits {
            instructions: 100,
            ..Limits::default()
        });
        assert_eq!(
            runtime
                .execute("let n=0;while(n<1000){n++;}", &mut Empty)
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
    }
}
