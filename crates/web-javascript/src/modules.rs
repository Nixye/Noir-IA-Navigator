//! Static, pre-registered module graphs. URL strings are identities, never I/O.
//! Namespace values are bounded, null-prototype accessor objects, not the full
//! ECMAScript Module Namespace exotic object (notably descriptor semantics).
use crate::{Declaration, ErrorKind, Expr, Flow, Host, JsError, Runtime, Stmt, Value, parser};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use url::Url;

const MAX_MODULES: usize = 64;
const MAX_SOURCE: usize = 128 * 1024;
const MAX_TOTAL_SOURCE: usize = 512 * 1024;
const MAX_DEPTH: usize = 64;
pub(crate) const MAX_ENTRIES: usize = 256;

#[derive(Clone, Debug, Default)]
pub(super) struct ModuleState {
    records: BTreeMap<String, ModuleRecord>,
    aliases: BTreeMap<(usize, String), (usize, String)>,
    source_bytes: usize,
}

#[derive(Clone, Debug)]
struct ModuleRecord {
    source: String,
    source_url: String,
    parsed: parser::Module,
    dependencies: Vec<String>,
    scope: Option<usize>,
    namespace: Option<Value>,
    meta: Option<Value>,
    status: Status,
}

#[derive(Clone, Debug)]
enum Status {
    Registered,
    Linked,
    Evaluating,
    Evaluated,
    Failed(JsError),
}

/// Dynamic imports reject load/link failures before attaching the evaluation
/// continuation. A cached body failure still belongs to evaluation.
pub(super) enum ModuleExecution {
    LinkFailed(JsError),
    Evaluated(Result<Value, JsError>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Resolution {
    Missing,
    Ambiguous,
    Binding(usize, String),
}

#[derive(Default)]
struct Evaluation {
    next: usize,
    indices: BTreeMap<String, usize>,
    low: BTreeMap<String, usize>,
    stack: Vec<String>,
}

fn absolute_id(id: &str) -> Result<String, JsError> {
    if id.len() > 8192 {
        return Err(JsError::resource("Module URL exceeds 8192 bytes"));
    }
    let url =
        Url::parse(id).map_err(|_| JsError::syntax(0, "Module ID must be an absolute URL"))?;
    if !matches!(url.scheme(), "http" | "https" | "file") {
        return Err(JsError::syntax(0, "Unsupported module URL scheme"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(JsError::syntax(
            0,
            "Module URLs must not contain credentials",
        ));
    }
    if url.as_str().len() > 8192 {
        return Err(JsError::resource(
            "Normalized module URL exceeds 8192 bytes",
        ));
    }
    Ok(url.into())
}

fn resolve_request(base: &str, request: &str) -> Result<String, JsError> {
    if request.len() > 8192 {
        return Err(JsError::resource("Module request exceeds 8192 bytes"));
    }
    if Url::parse(request).is_ok() {
        return absolute_id(request);
    }
    if !request.starts_with("./") && !request.starts_with("../") && !request.starts_with('/') {
        return Err(JsError::syntax(
            0,
            "Bare module specifiers require an import map (unsupported)",
        ));
    }
    let base = Url::parse(base).map_err(|_| JsError::syntax(0, "Invalid module base URL"))?;
    let url = base
        .join(request)
        .map_err(|_| JsError::syntax(0, "Invalid relative module URL"))?;
    absolute_id(url.as_str())
}

impl Runtime {
    /// Supplies already-authorized bytes. This method never opens a URL or path.
    /// A successfully linked module's source and identity cannot be replaced.
    pub fn register_module(&mut self, id: &str, source: &str) -> Result<(), JsError> {
        self.register_module_with_source_url(id, source, id)
    }

    /// The host supplies the real source/base URL separately for synthetic
    /// identities such as inline modules. Neither URL grants I/O authority.
    pub fn register_module_with_source_url(
        &mut self,
        id: &str,
        source: &str,
        source_url: &str,
    ) -> Result<(), JsError> {
        let id = absolute_id(id)?;
        let source_url = absolute_id(source_url)?;
        if source.len() > MAX_SOURCE {
            return Err(JsError::resource("Module source exceeds 128 KiB"));
        }
        let old_bytes = if let Some(record) = self.modules.records.get(&id) {
            if record.source == source && record.source_url == source_url {
                return Ok(());
            }
            if !matches!(record.status, Status::Registered) {
                return Err(JsError::runtime("Cannot replace a linked module"));
            }
            record.source.len()
        } else {
            if self.modules.records.len() >= MAX_MODULES {
                return Err(JsError::resource("Module registry exceeds 64 modules"));
            }
            0
        };
        let total = self.modules.source_bytes - old_bytes + source.len();
        if total > MAX_TOTAL_SOURCE {
            return Err(JsError::resource("Module source registry exceeds 512 KiB"));
        }
        self.tick()?;
        let parsed = parser::parse_module(source)?;
        bounded_entries(&parsed)?;
        // Imports are lexical declarations for duplicate/var conflict checking.
        let mut validation = parsed.body.clone();
        for import in &parsed.imports {
            validation.push(Stmt::Variables(
                Declaration::Const,
                vec![(import.local.clone(), None)],
            ));
        }
        crate::dynamic_code::validate_list(&validation, &[], true)?;
        crate::strict_validation::validate(&validation, true)?;
        let dependencies = parsed
            .requests
            .iter()
            .map(|request| resolve_request(&source_url, request))
            .collect::<Result<Vec<_>, _>>()?;
        self.charge_string(source.len())?;
        self.modules.records.insert(
            id,
            ModuleRecord {
                source: source.into(),
                source_url,
                parsed,
                dependencies,
                scope: None,
                namespace: None,
                meta: None,
                status: Status::Registered,
            },
        );
        self.modules.source_bytes = total;
        Ok(())
    }

    /// Links the entire available graph before evaluating any module body.
    /// Returns the stable namespace object. Evaluations and failures are cached.
    pub fn execute_module(&mut self, id: &str, host: &mut impl Host) -> Result<Value, JsError> {
        self.ensure_executable()?;
        let id = absolute_id(id)?;
        self.collect_garbage(&[], host)?;
        let result = self.execute_module_core(&id, host);
        if !result
            .as_ref()
            .is_err_and(|error| error.kind == ErrorKind::Resource)
        {
            self.drain_execution_jobs(host)?;
        }
        result
    }

    // Internal job entry: no GC safe point or nested microtask checkpoint.
    pub(super) fn execute_module_core(
        &mut self,
        id: &str,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        match self.execute_module_staged(id, host) {
            ModuleExecution::LinkFailed(error) => Err(error),
            ModuleExecution::Evaluated(result) => result,
        }
    }

    pub(super) fn execute_module_staged(
        &mut self,
        id: &str,
        host: &mut impl Host,
    ) -> ModuleExecution {
        if let Err(error) = self.capture_global_object(host) {
            return ModuleExecution::LinkFailed(error);
        }
        let id = id.to_owned();
        let previous_strict = self.strict;
        self.strict = true;
        let mut evaluation_started = false;
        let result = (|| {
            if let Some(record) = self.modules.records.get(&id) {
                match &record.status {
                    Status::Failed(error) => {
                        evaluation_started = true;
                        return Err(error.clone());
                    }
                    Status::Evaluated => {
                        evaluation_started = true;
                        return Ok(record.namespace.clone().unwrap());
                    }
                    _ => {}
                }
            } else {
                // A missing root needs no transactional clone of the entire VM.
                self.tick()?;
                return Err(JsError::runtime("Module is not registered"));
            }
            // Linking only allocates internal declarations. Restore partially
            // instantiated records on failure, while preserving charged work.
            let before = self.clone();
            if let Err(error) = self.link_module_graph(&id, host) {
                self.rollback_to(before);
                return Err(error);
            }
            evaluation_started = true;
            let mut evaluation = Evaluation::default();
            if let Err(error) = self.evaluate_module_graph(&id, host, &mut evaluation, 0) {
                for pending in evaluation.stack {
                    self.modules.records.get_mut(&pending).unwrap().status =
                        Status::Failed(error.clone());
                }
                if !matches!(self.modules.records[&id].status, Status::Evaluated) {
                    self.modules.records.get_mut(&id).unwrap().status =
                        Status::Failed(error.clone());
                }
                return Err(error);
            }
            Ok(self.modules.records[&id].namespace.clone().unwrap())
        })();
        self.strict = previous_strict;
        if evaluation_started {
            ModuleExecution::Evaluated(result)
        } else {
            ModuleExecution::LinkFailed(result.expect_err("link stage cannot return a namespace"))
        }
    }

    /// Capture lexical identity before any operand or conversion can execute JS.
    fn import_referrer(&mut self, mut scope: usize) -> Result<String, JsError> {
        loop {
            self.tick()?;
            if let Some((id, _)) = self
                .modules
                .records
                .iter()
                .find(|(_, record)| record.scope == Some(scope))
            {
                return Ok(id.clone());
            }
            match self.scopes[scope].parent {
                Some(parent) => scope = parent,
                None => {
                    return Err(JsError::syntax(
                        0,
                        "Dynamic import requires a module lexical context",
                    ));
                }
            }
        }
    }

    pub(super) fn eval_import_meta(&mut self, scope: usize) -> Result<Value, JsError> {
        let id = self.import_referrer(scope)?;
        if let Some(meta) = &self.modules.records[&id].meta {
            return Ok(meta.clone());
        }
        let url = self.string(self.modules.records[&id].source_url.clone())?;
        let meta = self.object(BTreeMap::from([("url".into(), url)]), None)?;
        self.modules.records.get_mut(&id).unwrap().meta = Some(meta.clone());
        Ok(meta)
    }

    fn resolve_registered_request(&self, id: &str, request: &str) -> Result<String, JsError> {
        let record = self
            .modules
            .records
            .get(id)
            .ok_or_else(|| JsError::runtime("Unregistered module dependency"))?;
        resolve_request(&record.source_url, request)
    }

    /// Capture lexical identity before either operand. The URL remains an
    /// identity for pre-registered modules, never an authority for network I/O.
    pub(super) fn prepare_import_referrer(&mut self, scope: usize) -> Result<String, JsError> {
        let owner = self.import_referrer(scope)?;
        let length = self.modules.records[&owner].source_url.len();
        self.charge_string(length)?;
        let mut referrer = String::new();
        referrer
            .try_reserve_exact(length)
            .map_err(|_| JsError::resource("Import referrer storage unavailable"))?;
        if referrer.capacity() > length {
            self.charge_string(referrer.capacity() - length)?;
        }
        referrer.push_str(&self.modules.records[&owner].source_url);
        Ok(referrer)
    }

    /// Operand evaluation is complete before promise creation. Conversion and
    /// options access are explicit guarded reentrant bridges; a fatal Resource
    /// remains fatal through settle_async_promise instead of becoming rejection.
    pub(super) fn import_call_values(
        &mut self,
        referrer: String,
        specifier: Value,
        options: Value,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let promise = self.new_async_promise()?;
        let Value::Object(owner) = promise else {
            unreachable!()
        };
        let prepared = (|| {
            let primitive =
                self.operator_primitive(specifier, crate::coercion::Hint::String, host)?;
            if matches!(primitive, Value::Symbol(..)) {
                return Err(self.property_type_error("Cannot convert a Symbol to a string"));
            }
            let text = self
                .conversion_builtin("String", &[primitive], false, host)?
                .unwrap();
            let request = match text {
                Value::String(text) => text.to_string(),
                Value::Utf16(units) => String::from_utf16(&units).map_err(|_| {
                    self.property_type_error(
                        "Module specifiers with unpaired UTF-16 surrogates are not supported",
                    )
                })?,
                _ => unreachable!(),
            };
            if !matches!(options, Value::Undefined) {
                if !matches!(
                    options,
                    Value::Object(_)
                        | Value::Array(_)
                        | Value::Function(_)
                        | Value::Native(_)
                        | Value::Host(_)
                ) {
                    return Err(self.property_type_error("Import options must be an object"));
                }
                let attributes = self.get(options, "with", host)?;
                // Explicit subset boundary: no attribute object is enumerated.
                // Do not silently skip getters or key ordering.
                if !matches!(attributes, Value::Undefined) {
                    return Err(
                        self.property_type_error("Dynamic import attributes are not supported")
                    );
                }
            }
            resolve_request(&referrer, &request)
        })();
        match prepared {
            Ok(id) => self.enqueue_module_import(owner, id)?,
            Err(error) => self.settle_async_promise(owner, Err(error), host)?,
        }
        Ok(Value::Object(owner))
    }

    fn module_graph(
        &mut self,
        id: &str,
        seen: &mut BTreeSet<String>,
        order: &mut Vec<String>,
        depth: usize,
    ) -> Result<(), JsError> {
        self.tick()?;
        if seen.contains(id) {
            return Ok(());
        }
        if depth >= MAX_DEPTH {
            return Err(JsError::resource("Module graph depth exceeds 64"));
        }
        let record = self
            .modules
            .records
            .get(id)
            .ok_or_else(|| JsError::runtime("Module is not registered"))?;
        let dependencies = record.dependencies.clone();
        seen.insert(id.into());
        order.push(id.into());
        for dependency in dependencies {
            self.module_graph(&dependency, seen, order, depth + 1)?;
        }
        Ok(())
    }

    fn link_module_graph(&mut self, id: &str, host: &mut impl Host) -> Result<(), JsError> {
        let mut order = Vec::new();
        self.module_graph(id, &mut BTreeSet::new(), &mut order, 0)?;
        let fresh: Vec<String> = order
            .iter()
            .filter(|id| matches!(self.modules.records[*id].status, Status::Registered))
            .cloned()
            .collect();
        // Every module environment exists before any import is resolved.
        for id in &fresh {
            let scope = self.scope(0, true)?;
            self.modules.records.get_mut(id).unwrap().scope = Some(scope);
            self.declare(scope, "this", false, true)?;
            let parsed = self.modules.records[id].parsed.clone();
            self.prepare(&parsed.body, scope, host)?;
            for import in &parsed.imports {
                self.declare(scope, &import.local, false, true)?;
                self.scopes[scope]
                    .bindings
                    .get_mut(&import.local)
                    .unwrap()
                    .lexical = true;
            }
        }
        // Resolve all explicit exports, including unused reexports. Missing or
        // ambiguous named imports are link errors, never delayed body effects.
        for id in &fresh {
            let parsed = self.modules.records[id].parsed.clone();
            for export in &parsed.exports {
                self.require_module_export(id, &export.exported)?;
            }
            let scope = self.modules.records[id].scope.unwrap();
            for import in &parsed.imports {
                let dependency = self.resolve_registered_request(id, &import.source)?;
                if import.imported != "*" {
                    let target = self.require_module_export(&dependency, &import.imported)?;
                    self.modules
                        .aliases
                        .insert((scope, import.local.clone()), target);
                }
            }
        }
        // Enumerate the finite set of potential names. Resolution below filters
        // missing/cyclic and ambiguous star exports without shadowing explicit ones.
        let names: BTreeSet<String> = order
            .iter()
            .flat_map(|id| {
                self.modules.records[id]
                    .parsed
                    .exports
                    .iter()
                    .map(|export| export.exported.clone())
            })
            .collect();
        for id in &fresh {
            let mut exports = BTreeMap::new();
            for name in &names {
                if let Resolution::Binding(scope, local) =
                    self.resolve_module_export(id, name, &mut BTreeSet::new(), 0)?
                {
                    if exports.len() >= 128 {
                        return Err(JsError::resource("Module namespace exceeds 128 exports"));
                    }
                    let getter = self.make_function(
                        Arc::new(vec![]),
                        Arc::new(vec![Stmt::Return(Some(Expr::Name(local)))]),
                        scope,
                        true,
                    )?;
                    exports.insert(name.clone(), getter);
                }
            }
            let namespace = self.object(
                exports
                    .keys()
                    .map(|name| (name.clone(), Value::Undefined))
                    .collect(),
                None,
            )?;
            let Value::Object(owner) = namespace else {
                unreachable!()
            };
            for (name, getter) in exports {
                self.property_metadata.entry(owner).or_default().insert(
                    name,
                    crate::descriptors::PropertyMetadata {
                        writable: false,
                        enumerable: true,
                        configurable: false,
                        accessor: Some((getter, Value::Undefined)),
                    },
                );
            }
            self.reflection.non_extensible.insert(owner);
            self.modules.records.get_mut(id).unwrap().namespace = Some(Value::Object(owner));
        }
        for id in &fresh {
            let parsed = self.modules.records[id].parsed.clone();
            let scope = self.modules.records[id].scope.unwrap();
            for import in parsed
                .imports
                .iter()
                .filter(|import| import.imported == "*")
            {
                let dependency = self.resolve_registered_request(id, &import.source)?;
                let value = self.modules.records[&dependency].namespace.clone().unwrap();
                self.scopes[scope]
                    .bindings
                    .get_mut(&import.local)
                    .unwrap()
                    .value = value;
            }
            self.modules.records.get_mut(id).unwrap().status = Status::Linked;
        }
        Ok(())
    }

    fn require_module_export(&mut self, id: &str, name: &str) -> Result<(usize, String), JsError> {
        match self.resolve_module_export(id, name, &mut BTreeSet::new(), 0)? {
            Resolution::Binding(scope, local) => Ok((scope, local)),
            Resolution::Missing => Err(JsError::syntax(
                0,
                format!("Module export is missing: {name}"),
            )),
            Resolution::Ambiguous => Err(JsError::syntax(
                0,
                format!("Module export is ambiguous: {name}"),
            )),
        }
    }

    fn resolve_module_export(
        &mut self,
        id: &str,
        name: &str,
        seen: &mut BTreeSet<(String, String)>,
        depth: usize,
    ) -> Result<Resolution, JsError> {
        self.tick()?;
        if !seen.insert((id.into(), name.into())) {
            return Ok(Resolution::Missing);
        }
        if depth >= MAX_DEPTH {
            return Err(JsError::resource(
                "Module export resolution depth exceeds 64",
            ));
        }
        let record = self
            .modules
            .records
            .get(id)
            .ok_or_else(|| JsError::runtime("Unregistered module dependency"))?;
        let parsed = record.parsed.clone();
        let scope = record
            .scope
            .ok_or_else(|| JsError::runtime("Module environment is not linked"))?;
        if let Some(export) = parsed.exports.iter().find(|export| export.exported == name) {
            if let Some(import) = parsed
                .imports
                .iter()
                .find(|import| import.local == export.local)
                && import.imported != "*"
            {
                let dependency = self.resolve_registered_request(id, &import.source)?;
                return self.resolve_module_export(&dependency, &import.imported, seen, depth + 1);
            }
            return Ok(if self.scopes[scope].bindings.contains_key(&export.local) {
                Resolution::Binding(scope, export.local.clone())
            } else {
                Resolution::Missing
            });
        }
        if name == "default" {
            return Ok(Resolution::Missing);
        }
        let mut found = Resolution::Missing;
        for request in &parsed.star_exports {
            let dependency = self.resolve_registered_request(id, request)?;
            let candidate = self.resolve_module_export(&dependency, name, seen, depth + 1)?;
            if candidate == Resolution::Ambiguous {
                return Ok(candidate);
            }
            if candidate == Resolution::Missing {
                continue;
            }
            if found != Resolution::Missing && found != candidate {
                return Ok(Resolution::Ambiguous);
            }
            found = candidate;
        }
        Ok(found)
    }

    fn evaluate_module_graph(
        &mut self,
        id: &str,
        host: &mut impl Host,
        evaluation: &mut Evaluation,
        depth: usize,
    ) -> Result<(), JsError> {
        match &self.modules.records[id].status {
            Status::Evaluated | Status::Evaluating => return Ok(()),
            Status::Failed(error) => return Err(error.clone()),
            _ => {}
        }
        self.tick()?;
        if depth >= MAX_DEPTH {
            return Err(JsError::resource("Module evaluation depth exceeds 64"));
        }
        let index = evaluation.next;
        evaluation.next += 1;
        evaluation.indices.insert(id.into(), index);
        evaluation.low.insert(id.into(), index);
        evaluation.stack.push(id.into());
        self.modules.records.get_mut(id).unwrap().status = Status::Evaluating;
        let dependencies = self.modules.records[id].dependencies.clone();
        for dependency in dependencies {
            let back_edge = matches!(self.modules.records[&dependency].status, Status::Evaluating);
            self.evaluate_module_graph(&dependency, host, evaluation, depth + 1)?;
            if matches!(self.modules.records[&dependency].status, Status::Evaluating) {
                let dependency_index = if back_edge {
                    evaluation.indices[&dependency]
                } else {
                    evaluation.low[&dependency]
                };
                let low = evaluation.low[id].min(dependency_index);
                evaluation.low.insert(id.into(), low);
            }
        }
        let body = self.modules.records[id].parsed.body.clone();
        let scope = self.modules.records[id].scope.unwrap();
        for statement in &body {
            match self.statement(statement, scope, host)? {
                Flow::Normal(_) | Flow::Empty => {}
                _ => return Err(JsError::syntax(0, "Invalid module completion")),
            }
        }
        if evaluation.low[id] == index {
            while let Some(member) = evaluation.stack.pop() {
                self.modules.records.get_mut(&member).unwrap().status = Status::Evaluated;
                if member == id {
                    break;
                }
            }
        }
        Ok(())
    }

    /// Call with the actual lexical owner found by lookup_binding, so local
    /// shadowing remains normal. Assignment still targets the immutable import.
    pub(super) fn module_read_binding(
        &self,
        scope: usize,
        name: &str,
    ) -> Option<Result<Value, JsError>> {
        let (target, local) = self.modules.aliases.get(&(scope, name.into()))?;
        let binding = &self.scopes[*target].bindings[local];
        Some(if binding.initialized {
            Ok(binding.value.clone())
        } else {
            Err(JsError::runtime(format!(
                "Cannot access {name} before initialization"
            )))
        })
    }

    pub(super) fn module_cached_roots(&self) -> (Vec<Value>, Vec<usize>) {
        let mut values = Vec::new();
        let mut scopes = Vec::new();
        for record in self.modules.records.values() {
            if let Some(scope) = record.scope {
                scopes.push(scope);
            }
            if let Some(namespace) = &record.namespace {
                values.push(namespace.clone());
            }
            if let Some(meta) = &record.meta {
                values.push(meta.clone());
            }
            if let Status::Failed(error) = &record.status {
                values.extend(error.thrown.iter().cloned());
            }
        }
        (values, scopes)
    }
}

/// Parses static requests only. No URL or file is opened by this helper.
pub fn module_requests(source: &str) -> Result<Vec<String>, JsError> {
    if source.len() > MAX_SOURCE {
        return Err(JsError::resource("Module source exceeds 128 KiB"));
    }
    let module = parser::parse_module(source).map_err(|mut error| {
        error.mark_input_source_offset();
        error
    })?;
    bounded_entries(&module)?;
    Ok(module.requests)
}

fn bounded_entries(module: &parser::Module) -> Result<(), JsError> {
    if module.requests.len() > MAX_ENTRIES
        || module.imports.len() > MAX_ENTRIES
        || module.exports.len() > MAX_ENTRIES
        || module.star_exports.len() > MAX_ENTRIES
    {
        return Err(JsError::resource(
            "Module requests, imports or exports exceed 256",
        ));
    }
    Ok(())
}

/// Resolves only supported URL-like specifiers; bare names need an import map.
pub fn resolve_module_specifier(referrer: &str, specifier: &str) -> Result<String, JsError> {
    resolve_request(&absolute_id(referrer)?, specifier)
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

    fn register(vm: &mut Runtime, name: &str, source: &str) {
        vm.register_module(&format!("https://example.test/{name}.js"), source)
            .unwrap();
    }
    fn execute(vm: &mut Runtime, name: &str) -> Result<Value, JsError> {
        vm.execute_module(&format!("https://example.test/{name}.js"), &mut Empty)
    }
    fn get(vm: &mut Runtime, namespace: &Value, name: &str) -> Value {
        vm.get(namespace.clone(), name, &mut Empty).unwrap()
    }

    #[test]
    fn import_meta_is_stable_mutable_null_prototype_and_rooted() {
        let mut vm = Runtime::default();
        register(
            &mut vm,
            "meta",
            "export const meta=import.meta; const d=Object.getOwnPropertyDescriptor(import.meta,'url'); export const valid=Object.getPrototypeOf(import.meta)===null && Object.isExtensible(import.meta) && d.writable && d.enumerable && d.configurable && import.meta.resolve===undefined; export function read(){return import.meta;} export function remove(){delete import.meta.url; return import.meta.url;}",
        );
        let ns = execute(&mut vm, "meta").unwrap();
        assert_eq!(get(&mut vm, &ns, "valid"), Value::Bool(true));
        let meta = get(&mut vm, &ns, "meta");
        assert_eq!(
            get(&mut vm, &meta, "url"),
            Value::string("https://example.test/meta.js")
        );
        vm.collect_garbage(&[], &Empty).unwrap();
        let read = get(&mut vm, &ns, "read");
        assert_eq!(vm.invoke(read, vec![], &mut Empty).unwrap(), meta);
        let remove = get(&mut vm, &ns, "remove");
        assert_eq!(
            vm.invoke(remove, vec![], &mut Empty).unwrap(),
            Value::Undefined
        );
        assert_eq!(get(&mut vm, &meta, "url"), Value::Undefined);
    }

    #[test]
    fn explicit_source_url_controls_imports_without_exposing_synthetic_identity() {
        let mut vm = Runtime::default();
        register(&mut vm, "real/dep", "export const x=7;");
        let source = "import {x} from './dep.js'; export const value=x; export const initial=import.meta.url; export const meta=import.meta; import.meta.url='https://wrong.test/'; export const pending=import('./dep.js');";
        vm.register_module_with_source_url(
            "https://example.test/internal/root.js#inline",
            source,
            "https://example.test/real/page.html#original",
        )
        .unwrap();
        let ns = vm
            .execute_module("https://example.test/internal/root.js#inline", &mut Empty)
            .unwrap();
        assert_eq!(
            get(&mut vm, &ns, "initial"),
            Value::string("https://example.test/real/page.html#original")
        );
        assert_eq!(get(&mut vm, &ns, "value"), Value::Number(7.));
        let pending = get(&mut vm, &ns, "pending");
        assert!(matches!(
            vm.promise_status(&pending),
            Some(crate::PromiseInspection::Fulfilled(_))
        ));
        assert!(
            vm.register_module_with_source_url(
                "https://example.test/internal/root.js#inline",
                source,
                "https://example.test/other/"
            )
            .is_err()
        );
        vm.register_module_with_source_url(
            "https://example.test/internal/second.js",
            "export const meta=import.meta;",
            "https://example.test/real/page.html#original",
        )
        .unwrap();
        let second = vm
            .execute_module("https://example.test/internal/second.js", &mut Empty)
            .unwrap();
        let first_meta = get(&mut vm, &ns, "meta");
        let second_meta = get(&mut vm, &second, "meta");
        assert_ne!(first_meta, second_meta);
        assert_eq!(
            get(&mut vm, &second_meta, "url"),
            Value::string("https://example.test/real/page.html#original")
        );
        assert!(
            vm.register_module_with_source_url(
                "https://example.test/invalid.js",
                "",
                "data:text/javascript,"
            )
            .is_err()
        );
    }

    #[test]
    fn dynamic_import_is_fifo_fresh_and_reuses_namespace_and_evaluation() {
        let mut vm = Runtime::default();
        vm.execute("var order=''; var count=0;", &mut Empty)
            .unwrap();
        register(&mut vm, "dep", "count++; order+='D'; export const value=7;");
        register(
            &mut vm,
            "root",
            "queueMicrotask(()=>{order+='A';}); export const first=import('./dep.js'); export const second=import('./dep.js'); first.then(()=>{order+='R';}); queueMicrotask(()=>{order+='B';}); order+='S';",
        );
        let ns = execute(&mut vm, "root").unwrap();
        let first = get(&mut vm, &ns, "first");
        let second = get(&mut vm, &ns, "second");
        assert_ne!(first, second);
        let dep = execute(&mut vm, "dep").unwrap();
        assert_eq!(
            vm.promise_status(&first),
            Some(crate::PromiseInspection::Fulfilled(dep.clone()))
        );
        assert_eq!(
            vm.promise_status(&second),
            Some(crate::PromiseInspection::Fulfilled(dep))
        );
        assert_eq!(vm.global_value("count"), Some(Value::Number(1.)));
        assert_eq!(vm.global_value("order"), Some(Value::string("SADBR")));
    }

    #[test]
    fn dynamic_import_preserves_sync_operands_and_sync_conversion_order() {
        let mut vm = Runtime::default();
        register(&mut vm, "dep", "export const x=1;");
        register(
            &mut vm,
            "root",
            "export let order=''; export let caught=false; try { import(missing); } catch(e) { caught=true; } function options(){order+='O'; const result={};Object.defineProperty(result,'with',{get:function(){order+='W';return undefined;},enumerable:true,configurable:true});return result;} const spec={toString(){order+='S'; return './dep.js';}}; export const good=import(spec,options()); order+='E'; export const rejected=import({toString(){throw 17;}}); export const attributes=import('./dep.js',{with:{}}); export const primitive=import('./dep.js',null);",
        );
        let ns = execute(&mut vm, "root").unwrap();
        assert_eq!(get(&mut vm, &ns, "caught"), Value::Bool(true));
        assert_eq!(get(&mut vm, &ns, "order"), Value::string("OSWE"));
        let rejected = get(&mut vm, &ns, "rejected");
        assert_eq!(
            vm.promise_status(&rejected),
            Some(crate::PromiseInspection::Rejected(Value::Number(17.)))
        );
        for name in ["attributes", "primitive"] {
            let promise = get(&mut vm, &ns, name);
            assert!(matches!(
                vm.promise_status(&promise),
                Some(crate::PromiseInspection::Rejected(_))
            ));
        }
    }

    #[test]
    fn dynamic_import_distinguishes_link_failure_from_evaluation_failure_order() {
        for (target, expected, body_runs) in [
            (None, "RQ", 0.),
            (
                Some("import {missing} from './dep.js'; attempts++;"),
                "RQ",
                0.,
            ),
            (Some("attempts++; throw 17;"), "QR", 1.),
        ] {
            let mut vm = Runtime::default();
            vm.execute("var order=''; var attempts=0;", &mut Empty)
                .unwrap();
            register(&mut vm, "dep", "export const present=1;");
            if let Some(source) = target {
                register(&mut vm, "target", source);
            }
            let source = "import('./target.js').catch(()=>{order+='R';}); queueMicrotask(()=>queueMicrotask(()=>{order+='Q';}));";
            register(&mut vm, "root", source);
            execute(&mut vm, "root").unwrap();
            assert_eq!(vm.global_value("order"), Some(Value::string(expected)));
            assert_eq!(vm.global_value("attempts"), Some(Value::Number(body_runs)));
            // Cached evaluation failure retains its continuation stage; link
            // failures remain pre-evaluation and cannot run the module body.
            vm.execute("order='';", &mut Empty).unwrap();
            register(&mut vm, "again", source);
            execute(&mut vm, "again").unwrap();
            assert_eq!(vm.global_value("order"), Some(Value::string(expected)));
            assert_eq!(vm.global_value("attempts"), Some(Value::Number(body_runs)));
        }
    }

    #[test]
    fn evaluated_dynamic_import_waits_for_evaluation_continuation() {
        let mut vm = Runtime::default();
        vm.execute("var order='';", &mut Empty).unwrap();
        register(&mut vm, "dep", "export const x=1;");
        execute(&mut vm, "dep").unwrap();
        register(
            &mut vm,
            "root",
            "import('./dep.js').then(()=>{order+='I';}); queueMicrotask(()=>queueMicrotask(()=>{order+='Q';}));",
        );
        execute(&mut vm, "root").unwrap();
        assert_eq!(vm.global_value("order"), Some(Value::string("QI")));
    }

    #[test]
    fn dynamic_import_keeps_lexical_base_after_return_and_gc() {
        let mut vm = Runtime::default();
        vm.register_module("https://example.test/dir/dep.js", "export const x=42;")
            .unwrap();
        vm.register_module(
            "https://example.test/dir/root.js",
            "export function load(){return import('./dep.js');}",
        )
        .unwrap();
        let ns = vm
            .execute_module("https://example.test/dir/root.js", &mut Empty)
            .unwrap();
        let load = get(&mut vm, &ns, "load");
        register(&mut vm, "unrelated", "export const x=0;");
        execute(&mut vm, "unrelated").unwrap();
        vm.collect_garbage(&[], &Empty).unwrap();
        let promise = vm.invoke(load, vec![], &mut Empty).unwrap();
        let Some(crate::PromiseInspection::Fulfilled(dep)) = vm.promise_status(&promise) else {
            panic!("import did not fulfill")
        };
        assert_eq!(get(&mut vm, &dep, "x"), Value::Number(42.));
    }

    #[test]
    fn dynamic_import_caches_failure_and_assimilates_namespace_then() {
        let mut vm = Runtime::default();
        vm.execute("var attempts=0;", &mut Empty).unwrap();
        register(&mut vm, "bad", "attempts++; throw {reason:9};");
        register(
            &mut vm,
            "thenable",
            "export function then(resolve){resolve(23);}",
        );
        register(
            &mut vm,
            "root",
            "export const a=import('./bad.js'); export const b=import('./bad.js'); export const c=import('./thenable.js'); export const missing=import('./private-not-registered.js');",
        );
        let ns = execute(&mut vm, "root").unwrap();
        let a = get(&mut vm, &ns, "a");
        let b = get(&mut vm, &ns, "b");
        assert_ne!(a, b);
        assert_eq!(vm.promise_status(&a), vm.promise_status(&b));
        assert!(matches!(
            vm.promise_status(&a),
            Some(crate::PromiseInspection::Rejected(_))
        ));
        assert_eq!(vm.global_value("attempts"), Some(Value::Number(1.)));
        let c = get(&mut vm, &ns, "c");
        assert_eq!(
            vm.promise_status(&c),
            Some(crate::PromiseInspection::Fulfilled(Value::Number(23.)))
        );
        let missing = get(&mut vm, &ns, "missing");
        let Some(crate::PromiseInspection::Rejected(reason)) = vm.promise_status(&missing) else {
            panic!("missing import did not reject")
        };
        let message = get(&mut vm, &reason, "message");
        assert!(!message.as_str().unwrap().contains("private-not-registered"));
    }

    #[test]
    fn queued_dynamic_import_roots_its_unreturned_promise() {
        let mut vm = Runtime::default();
        register(&mut vm, "dep", "export const x=5;");
        register(
            &mut vm,
            "root",
            "export function load(){return import('./dep.js');}",
        );
        let ns = execute(&mut vm, "root").unwrap();
        let load = get(&mut vm, &ns, "load");
        // Internal call deliberately leaves the job pending until after GC.
        let promise = vm.call(load, vec![], Value::Undefined, &mut Empty).unwrap();
        assert_eq!(
            vm.promise_status(&promise),
            Some(crate::PromiseInspection::Pending)
        );
        vm.collect_garbage(&[], &Empty).unwrap();
        vm.drain_promise_jobs(&mut Empty).unwrap();
        assert!(matches!(
            vm.promise_status(&promise),
            Some(crate::PromiseInspection::Fulfilled(_))
        ));
    }

    #[test]
    fn dynamic_import_resource_exhaustion_is_fatal_not_a_rejection() {
        let mut vm = Runtime::new(crate::Limits {
            instructions: 10_000,
            ..crate::Limits::default()
        });
        register(&mut vm, "dep", "while(true){}");
        register(
            &mut vm,
            "root",
            "export let caught=false; import('./dep.js').catch(()=>{caught=true;});",
        );
        assert_eq!(
            execute(&mut vm, "root").unwrap_err().kind,
            ErrorKind::Resource
        );
    }

    #[test]
    fn imports_and_namespaces_are_live_and_evaluate_once() {
        let mut vm = Runtime::default();
        register(
            &mut vm,
            "dep",
            "export let count = 1; export function bump() { count++; }",
        );
        register(
            &mut vm,
            "root",
            "import { count, bump } from './dep.js'; import * as ns from './dep.js'; bump(); export function read() { return count + ns.count; } export const top = this;",
        );
        let namespace = execute(&mut vm, "root").unwrap();
        let read = get(&mut vm, &namespace, "read");
        assert_eq!(
            vm.invoke(read.clone(), vec![], &mut Empty).unwrap(),
            Value::Number(4.)
        );
        assert_eq!(get(&mut vm, &namespace, "top"), Value::Undefined);
        assert_eq!(execute(&mut vm, "root").unwrap(), namespace);
        assert_eq!(
            vm.invoke(read, vec![], &mut Empty).unwrap(),
            Value::Number(4.)
        );
        assert!(vm.global_value("count").is_none());
        let dep = execute(&mut vm, "dep").unwrap();
        let bump = get(&mut vm, &dep, "bump");
        vm.invoke(bump, vec![], &mut Empty).unwrap();
        assert_eq!(get(&mut vm, &dep, "count"), Value::Number(3.));
        assert!(
            vm.register_module("https://example.test/dep.js", "export let count=99;")
                .is_err()
        );
        vm.collect_garbage(&[], &Empty).unwrap();
        assert_eq!(get(&mut vm, &dep, "count"), Value::Number(3.));
    }

    #[test]
    fn link_errors_precede_all_body_effects_and_can_be_repaired() {
        let mut vm = Runtime::default();
        vm.execute("var effect=0;", &mut Empty).unwrap();
        register(&mut vm, "dep", "effect++; export const present=1;");
        register(
            &mut vm,
            "root",
            "import { missing } from './dep.js'; effect++;",
        );
        assert!(execute(&mut vm, "root").is_err());
        assert_eq!(vm.global_value("effect"), Some(Value::Number(0.)));
        register(&mut vm, "dep", "effect++; export const missing=1;");
        execute(&mut vm, "root").unwrap();
        assert_eq!(vm.global_value("effect"), Some(Value::Number(2.)));
    }

    #[test]
    fn star_ambiguity_is_omitted_but_named_import_is_an_error() {
        let mut vm = Runtime::default();
        register(&mut vm, "a", "export const x=1; export default 2;");
        register(&mut vm, "b", "export const x=2;");
        register(
            &mut vm,
            "star",
            "export * from './a.js'; export * from './b.js';",
        );
        register(&mut vm, "bad", "import { x } from './star.js';");
        assert_eq!(execute(&mut vm, "bad").unwrap_err().kind, ErrorKind::Syntax);
        let namespace = execute(&mut vm, "star").unwrap();
        assert_eq!(get(&mut vm, &namespace, "x"), Value::Undefined);
        assert_eq!(get(&mut vm, &namespace, "default"), Value::Undefined);
        register(
            &mut vm,
            "good",
            "export * from './a.js'; export { x } from './b.js';",
        );
        let namespace = execute(&mut vm, "good").unwrap();
        assert_eq!(get(&mut vm, &namespace, "x"), Value::Number(2.));
    }

    #[test]
    fn cycles_hoist_functions_and_preserve_tdz_and_error_identity() {
        let mut vm = Runtime::default();
        register(
            &mut vm,
            "a",
            "import { y } from './b.js'; export function one() { return 1; } export const x=y;",
        );
        register(
            &mut vm,
            "b",
            "import { one } from './a.js'; export const y=one();",
        );
        let namespace = execute(&mut vm, "a").unwrap();
        assert_eq!(get(&mut vm, &namespace, "x"), Value::Number(1.));
        register(
            &mut vm,
            "c",
            "import { y } from './d.js'; export const x=y;",
        );
        register(
            &mut vm,
            "d",
            "import { x } from './c.js'; export const y=x;",
        );
        assert!(
            execute(&mut vm, "c")
                .unwrap_err()
                .message
                .contains("initialization")
        );
        assert!(execute(&mut vm, "d").is_err());
        register(&mut vm, "error", "throw { reason: 7 };");
        let first = execute(&mut vm, "error").unwrap_err();
        vm.collect_garbage(&[], &Empty).unwrap();
        let second = execute(&mut vm, "error").unwrap_err();
        assert!(first.thrown.is_some());
        assert_eq!(first.thrown, second.thrown);
    }

    #[test]
    fn completed_cycle_members_cache_the_cycle_failure() {
        let mut vm = Runtime::default();
        register(&mut vm, "a", "import './b.js'; throw 123;");
        register(&mut vm, "b", "import './a.js'; export const okay=1;");
        let error = execute(&mut vm, "a").unwrap_err();
        assert_eq!(error.thrown, Some(Value::Number(123.)));
        assert_eq!(execute(&mut vm, "b").unwrap_err().thrown, error.thrown);
    }

    #[test]
    fn imports_reject_writes_and_namespace_integrity_is_honest() {
        let mut vm = Runtime::default();
        register(&mut vm, "dep", "export let x=1;");
        register(&mut vm, "bad", "import { x } from './dep.js'; x=3;");
        assert!(execute(&mut vm, "bad").is_err());
        register(
            &mut vm,
            "check",
            "import * as ns from './dep.js'; const d=Object.getOwnPropertyDescriptor(ns,'x'); export const good=typeof d.get==='function' && d.set===undefined && !d.configurable && d.enumerable && !Object.isExtensible(ns) && Object.getPrototypeOf(ns)===null; export function overwrite() { ns.x=3; }",
        );
        let namespace = execute(&mut vm, "check").unwrap();
        assert_eq!(get(&mut vm, &namespace, "good"), Value::Bool(true));
        let overwrite = get(&mut vm, &namespace, "overwrite");
        assert!(vm.invoke(overwrite, vec![], &mut Empty).is_err());
    }

    #[test]
    fn url_resolution_and_budgets_are_bounded_and_have_no_io() {
        assert_eq!(
            resolve_module_specifier("https://example.test/a/b.js", "../x.js").unwrap(),
            "https://example.test/x.js"
        );
        assert!(resolve_module_specifier("https://example.test/", "package").is_err());
        assert!(
            resolve_module_specifier("https://example.test/", "data:text/javascript,1").is_err()
        );
        let mut vm = Runtime::default();
        assert_eq!(
            vm.register_module("file:///missing.js", &" ".repeat(MAX_SOURCE + 1))
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
        register(&mut vm, "missing", "import './never-registered.js';");
        assert!(
            execute(&mut vm, "missing")
                .unwrap_err()
                .message
                .contains("not registered")
        );
        let mut vm = Runtime::new(crate::Limits {
            instructions: 100,
            ..crate::Limits::default()
        });
        register(
            &mut vm,
            "loop",
            "try { while(true) {} } catch (e) { } export const done=true;",
        );
        assert_eq!(
            execute(&mut vm, "loop").unwrap_err().kind,
            ErrorKind::Resource
        );
        assert_eq!(
            execute(&mut vm, "loop").unwrap_err().kind,
            ErrorKind::Resource
        );
    }
}
