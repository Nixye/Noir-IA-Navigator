//! Bounded, interpreter-only WebAssembly. No host imports, I/O, or JS dependency.
//!
//! Validation is distinct from instantiation eligibility. Only stateless numeric
//! modules may execute: a fresh Store per call is then observationally equivalent
//! and immutable compiled records can safely participate in VM rollback snapshots.
use std::{fmt, sync::Arc};
use wasmi::{
    CompilationMode, Config, EnforcedLimits, Engine, ExternType, Instance, Module, Store,
    StoreLimitsBuilder, Val,
};
use wasmi::{TrapCode, ValType};
use wasmparser::{Parser, Payload, ValType as ParserType};

pub const MAX_MODULE_BYTES: usize = 256 * 1024;
pub const MAX_FUNCTIONS: u32 = 256;
pub const MAX_TYPES: u32 = 128;
pub const MAX_EXPORTS: u32 = 128;
pub const MAX_GLOBALS: u32 = 128;
pub const MAX_PARAMS: usize = 32;
pub const MAX_LOCALS: u32 = 256;
pub const MAX_BODY_BYTES: usize = 16 * 1024;
pub const MAX_OPERATORS: usize = 32 * 1024;
pub const MAX_CONTROL_DEPTH: usize = 64;
pub const MAX_CALL_DEPTH: usize = 32;
pub const MAX_STACK_HEIGHT: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmErrorKind {
    Compile,
    Unsupported,
    Link,
    Runtime,
    Resource,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WasmError {
    pub kind: WasmErrorKind,
    pub message: String,
}
impl fmt::Display for WasmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for WasmError {}
fn error(kind: WasmErrorKind, message: impl ToString) -> WasmError {
    WasmError {
        kind,
        message: message.to_string(),
    }
}
fn malformed(e: impl ToString) -> WasmError {
    error(WasmErrorKind::Compile, e)
}
fn cap(ok: bool, message: &str) -> Result<(), WasmError> {
    if ok {
        Ok(())
    } else {
        Err(error(WasmErrorKind::Resource, message))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    I32,
    I64,
    F32,
    F64,
    Unsupported,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WasmValue {
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportKind {
    Function,
    Memory,
    Table,
    Global,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Export {
    pub function_index: Option<u32>,
    pub name: String,
    pub kind: ExportKind,
    pub params: Vec<ValueType>,
    pub results: Vec<ValueType>,
}
#[derive(Debug, Clone)]
pub struct CompiledModule {
    module: Arc<Module>,
    exports: Arc<[Export]>,
    incompatibility: Option<String>,
}
impl CompiledModule {
    pub fn exports(&self) -> &[Export] {
        &self.exports
    }
    pub fn incompatibility(&self) -> Option<&str> {
        self.incompatibility.as_deref()
    }
    pub fn ensure_instantiable(&self) -> Result<(), WasmError> {
        match &self.incompatibility {
            Some(reason) => Err(error(WasmErrorKind::Unsupported, reason)),
            None => Ok(()),
        }
    }
}
#[derive(Debug, Clone)]
pub struct ExecutionOutcome {
    pub result: Result<Option<WasmValue>, WasmError>,
    pub fuel_remaining: u64,
}

fn engine() -> Engine {
    let mut c = Config::default();
    c.consume_fuel(true)
        .enforced_limits(EnforcedLimits::strict())
        .compilation_mode(CompilationMode::Eager)
        .ignore_custom_sections(true)
        .set_min_stack_height(128)
        .set_max_stack_height(MAX_STACK_HEIGHT)
        .set_max_recursion_depth(MAX_CALL_DEPTH)
        .set_max_cached_stacks(0)
        .wasm_reference_types(false)
        .wasm_multi_value(false)
        .wasm_multi_memory(false)
        .wasm_tail_call(false)
        .wasm_extended_const(false)
        .wasm_custom_page_sizes(false)
        .wasm_wide_arithmetic(false);
    // Keep start and mutable-global validation enabled. Eligibility rejects them,
    // rather than falsely classifying valid core binaries as malformed.
    Engine::new(&c)
}

fn numeric(t: ParserType) -> bool {
    matches!(
        t,
        ParserType::I32 | ParserType::I64 | ParserType::F32 | ParserType::F64
    )
}
fn reject(reason: &mut Option<String>, message: &str) {
    if reason.is_none() {
        *reason = Some(message.to_owned());
    }
}

/// Bounded structural pass before wasmi validation/translation allocates IR.
/// Input is already byte-capped; counted sections are capped before iteration.
fn preflight(bytes: &[u8]) -> Result<Option<String>, WasmError> {
    cap(
        bytes.len() <= MAX_MODULE_BYTES,
        "WebAssembly module byte limit exceeded",
    )?;
    let mut reason = None;
    let mut operators = 0usize;
    for payload in Parser::new(0).parse_all(bytes) {
        match payload.map_err(malformed)? {
            Payload::Version { encoding, .. } if encoding != wasmparser::Encoding::Module => {
                return Err(malformed("WebAssembly components are unsupported"));
            }
            Payload::TypeSection(s) => {
                cap(s.count() <= MAX_TYPES, "WebAssembly type limit exceeded")?;
                // Avoid FuncType/RecGroup allocation until declared counts are capped.
                // The official BinaryReader decodes all primitive encodings here.
                let range = s.original_position()..s.range().end;
                let mut reader = wasmparser::BinaryReader::new(&bytes[range.clone()], range.start);
                for _ in 0..s.count() {
                    if reader.read_u8().map_err(malformed)? != 0x60 {
                        return Err(malformed(
                            "GC and recursive type groups are outside the configured feature set",
                        ));
                    }
                    for limit in [MAX_PARAMS, 1] {
                        let count = reader.read_var_u32().map_err(malformed)? as usize;
                        cap(count <= limit, "WebAssembly signature limit exceeded")?;
                        for _ in 0..count {
                            let ty: ParserType = reader.read().map_err(malformed)?;
                            if !numeric(ty) {
                                reject(
                                    &mut reason,
                                    "Reference and vector types cannot be instantiated",
                                );
                            }
                        }
                    }
                }
                if !reader.eof() {
                    return Err(malformed("Trailing bytes in WebAssembly type section"));
                }
            }
            Payload::ImportSection(s) => {
                cap(s.count() <= 128, "WebAssembly import limit exceeded")?;
                if s.count() != 0 {
                    reject(&mut reason, "Imports are not supported");
                }
            }
            Payload::FunctionSection(s) => cap(
                s.count() <= MAX_FUNCTIONS,
                "WebAssembly function limit exceeded",
            )?,
            Payload::MemorySection(s) => {
                cap(s.count() <= 1, "WebAssembly memory count limit exceeded")?;
                if s.count() != 0 {
                    reject(&mut reason, "Linear memory is not supported");
                }
            }
            Payload::TableSection(s) => {
                cap(s.count() <= 16, "WebAssembly table count limit exceeded")?;
                if s.count() != 0 {
                    reject(&mut reason, "Tables are not supported");
                }
            }
            Payload::GlobalSection(s) => {
                cap(
                    s.count() <= MAX_GLOBALS,
                    "WebAssembly global limit exceeded",
                )?;
                for g in s {
                    let g = g.map_err(malformed)?;
                    if g.ty.mutable || !numeric(g.ty.content_type) {
                        reject(
                            &mut reason,
                            "Mutable or reference globals are not supported",
                        );
                    }
                }
            }
            Payload::ExportSection(s) => {
                cap(
                    s.count() <= MAX_EXPORTS,
                    "WebAssembly export limit exceeded",
                )?;
                for e in s {
                    let e = e.map_err(malformed)?;
                    cap(
                        e.name.len() <= 256,
                        "WebAssembly export name limit exceeded",
                    )?;
                    if e.kind != wasmparser::ExternalKind::Func {
                        reject(&mut reason, "Only function exports are supported");
                    }
                }
            }
            Payload::StartSection { .. } => {
                reject(&mut reason, "Start functions are not supported")
            }
            Payload::ElementSection(s) => {
                cap(
                    s.count() <= 128,
                    "WebAssembly element segment limit exceeded",
                )?;
                if s.count() != 0 {
                    reject(&mut reason, "Element segments are not supported");
                }
            }
            Payload::DataSection(s) => {
                cap(s.count() <= 128, "WebAssembly data segment limit exceeded")?;
                if s.count() != 0 {
                    reject(&mut reason, "Data segments are not supported");
                }
            }
            Payload::DataCountSection { count, .. } => {
                cap(count <= 128, "WebAssembly data segment limit exceeded")?;
                if count != 0 {
                    reject(&mut reason, "Data segments are not supported");
                }
            }
            Payload::CodeSectionStart { count, .. } => cap(
                count <= MAX_FUNCTIONS,
                "WebAssembly code count limit exceeded",
            )?,
            Payload::CodeSectionEntry(body) => {
                cap(
                    body.range().len() <= MAX_BODY_BYTES,
                    "WebAssembly function body byte limit exceeded",
                )?;
                let mut locals = 0u32;
                for local in body.get_locals_reader().map_err(malformed)? {
                    let (count, ty) = local.map_err(malformed)?;
                    locals = locals.checked_add(count).ok_or_else(|| {
                        error(WasmErrorKind::Resource, "WebAssembly local count overflow")
                    })?;
                    cap(locals <= MAX_LOCALS, "WebAssembly local limit exceeded")?;
                    if !numeric(ty) {
                        reject(&mut reason, "Reference and vector locals are not supported");
                    }
                }
                let mut depth = 0usize;
                let mut reader = body.get_operators_reader().map_err(malformed)?;
                while !reader.eof() {
                    let op = reader.read().map_err(malformed)?;
                    operators += 1;
                    cap(
                        operators <= MAX_OPERATORS,
                        "WebAssembly operator limit exceeded",
                    )?;
                    match op {
                        wasmparser::Operator::Block { .. }
                        | wasmparser::Operator::Loop { .. }
                        | wasmparser::Operator::If { .. } => {
                            depth += 1;
                            cap(
                                depth <= MAX_CONTROL_DEPTH,
                                "WebAssembly control depth limit exceeded",
                            )?;
                        }
                        wasmparser::Operator::End => depth = depth.saturating_sub(1),
                        _ => {}
                    }
                }
            }
            Payload::TagSection(_) => reject(&mut reason, "Exception tags are not supported"),
            _ => {}
        }
    }
    Ok(reason)
}

/// Validates the configured core feature set under Noir's structural quotas.
/// Success does not imply the module can be instantiated by this first slice.
pub fn validate(bytes: &[u8]) -> Result<(), WasmError> {
    preflight(bytes)?;
    Module::validate(&engine(), bytes).map_err(compile_error)
}
fn value_type(t: ValType) -> ValueType {
    match t {
        ValType::I32 => ValueType::I32,
        ValType::I64 => ValueType::I64,
        ValType::F32 => ValueType::F32,
        ValType::F64 => ValueType::F64,
        _ => ValueType::Unsupported,
    }
}

fn compile_error(e: wasmi::Error) -> WasmError {
    if matches!(
        e.kind(),
        wasmi::errors::ErrorKind::UserLimits(_) | wasmi::errors::ErrorKind::ImplementationLimits(_)
    ) {
        error(WasmErrorKind::Resource, e)
    } else {
        malformed(e)
    }
}
pub fn compile(bytes: &[u8]) -> Result<CompiledModule, WasmError> {
    let mut incompatibility = preflight(bytes)?;
    let module = Module::new(&engine(), bytes).map_err(compile_error)?;
    let mut binary_exports = Vec::new();
    // Already validated and byte/count/name capped by the preflight.
    for payload in Parser::new(0).parse_all(bytes) {
        if let Payload::ExportSection(section) = payload.map_err(malformed)? {
            for e in section {
                let e = e.map_err(malformed)?;
                let function_index = (e.kind == wasmparser::ExternalKind::Func).then_some(e.index);
                binary_exports.push((e.name, function_index));
            }
        }
    }
    let mut exports = Vec::new();
    // Wasmi's map iterator does not preserve the binary declaration order.
    // Preserve it explicitly for the JavaScript exports object's own-key order.
    for (name, function_index) in binary_exports {
        let ty = module
            .get_export(name)
            .ok_or_else(|| malformed("Missing validated export metadata"))?;
        let (kind, params, results) = match ty {
            ExternType::Func(t) => (
                ExportKind::Function,
                t.params()
                    .iter()
                    .copied()
                    .map(value_type)
                    .collect::<Vec<_>>(),
                t.results()
                    .iter()
                    .copied()
                    .map(value_type)
                    .collect::<Vec<_>>(),
            ),
            ExternType::Memory(_) => (ExportKind::Memory, vec![], vec![]),
            ExternType::Table(_) => (ExportKind::Table, vec![], vec![]),
            ExternType::Global(_) => (ExportKind::Global, vec![], vec![]),
        };
        if params
            .iter()
            .chain(&results)
            .any(|t| matches!(t, ValueType::Unsupported))
        {
            reject(
                &mut incompatibility,
                "Export signatures support only i32, i64, f32 and f64",
            );
        }
        exports.push(Export {
            function_index,
            name: name.to_owned(),
            kind,
            params,
            results,
        });
    }
    Ok(CompiledModule {
        module: Arc::new(module),
        exports: exports.into(),
        incompatibility,
    })
}

fn runtime_error(e: wasmi::Error) -> WasmError {
    let kind = match e.as_trap_code() {
        Some(TrapCode::OutOfFuel | TrapCode::StackOverflow) => WasmErrorKind::Resource,
        _ => WasmErrorKind::Runtime,
    };
    error(kind, e)
}
/// Receives the caller's remaining budget, never creates a fresh allowance.
/// Returns remaining fuel on every path, including traps and resource failures.
pub fn execute(
    module: &CompiledModule,
    export: &str,
    args: &[WasmValue],
    fuel: u64,
) -> ExecutionOutcome {
    let mut store = Store::new(
        module.module.engine(),
        StoreLimitsBuilder::new()
            .memory_size(0)
            .table_elements(0)
            .memories(0)
            .tables(0)
            .instances(1)
            .build(),
    );
    store.limiter(|s| s);
    let result = (|| {
        module.ensure_instantiable()?;
        cap(
            args.len() <= MAX_PARAMS,
            "WebAssembly argument limit exceeded",
        )?;
        store.set_fuel(fuel).map_err(runtime_error)?;
        let instance = Instance::new(&mut store, &module.module, &[]).map_err(runtime_error)?;
        let function = instance
            .get_func(&store, export)
            .ok_or_else(|| error(WasmErrorKind::Link, "Unknown function export"))?;
        let ty = function.ty(&store);
        if args.len() != ty.params().len() {
            return Err(error(
                WasmErrorKind::Link,
                "WebAssembly argument count mismatch",
            ));
        }
        let inputs: Vec<Val> = args
            .iter()
            .map(|v| match *v {
                WasmValue::I32(n) => Val::I32(n),
                WasmValue::I64(n) => Val::I64(n),
                WasmValue::F32(n) => Val::F32(n.into()),
                WasmValue::F64(n) => Val::F64(n.into()),
            })
            .collect();
        if inputs.iter().zip(ty.params()).any(|(v, t)| v.ty() != *t) {
            return Err(error(
                WasmErrorKind::Link,
                "WebAssembly argument type mismatch",
            ));
        }
        let mut output: Vec<Val> = ty
            .results()
            .iter()
            .copied()
            .map(Val::default_for_ty)
            .collect();
        function
            .call(&mut store, &inputs, &mut output)
            .map_err(runtime_error)?;
        match output.first() {
            None => Ok(None),
            Some(Val::I32(n)) => Ok(Some(WasmValue::I32(*n))),
            Some(Val::I64(n)) => Ok(Some(WasmValue::I64(*n))),
            Some(Val::F32(n)) => Ok(Some(WasmValue::F32((*n).into()))),
            Some(Val::F64(n)) => Ok(Some(WasmValue::F64((*n).into()))),
            _ => Err(error(
                WasmErrorKind::Unsupported,
                "Unsupported WebAssembly result",
            )),
        }
    })();
    let fuel_remaining = if module.incompatibility.is_some() || args.len() > MAX_PARAMS {
        fuel
    } else {
        store.get_fuel().unwrap_or(0).min(fuel)
    };
    ExecutionOutcome {
        result,
        fuel_remaining,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Raw core-Wasm fixtures; no WAT parser/toolchain is part of this boundary.
    const HEADER: &[u8] = b"\0asm\x01\0\0\0";
    const ADD: &[u8] = b"\0asm\x01\0\0\0\x01\x07\x01\x60\x02\x7f\x7f\x01\x7f\x03\x02\x01\0\x07\x07\x01\x03add\0\0\x0a\x09\x01\x07\0\x20\0\x20\x01\x6a\x0b";
    fn single(body: &[u8], result: Option<u8>) -> Vec<u8> {
        let mut bytes = HEADER.to_vec();
        if let Some(ty) = result {
            bytes.extend([1, 5, 1, 0x60, 0, 1, ty]);
        } else {
            bytes.extend([1, 4, 1, 0x60, 0, 0]);
        }
        bytes.extend([3, 2, 1, 0, 7, 5, 1, 1, b'f', 0, 0]);
        bytes.extend([10, (body.len() + 3) as u8, 1, (body.len() + 1) as u8, 0]);
        bytes.extend(body);
        bytes
    }
    #[test]
    fn executes_add_and_immutable_clone_with_shared_budget() {
        let m = compile(ADD).unwrap();
        assert!(m.ensure_instantiable().is_ok());
        assert_eq!(m.exports()[0].params, [ValueType::I32, ValueType::I32]);
        let a = execute(&m, "add", &[WasmValue::I32(20), WasmValue::I32(22)], 100);
        assert_eq!(a.result, Ok(Some(WasmValue::I32(42))));
        assert!(a.fuel_remaining < 100);
        let b = execute(
            &m.clone(),
            "add",
            &[WasmValue::I32(i32::MAX), WasmValue::I32(1)],
            a.fuel_remaining,
        );
        assert_eq!(b.result, Ok(Some(WasmValue::I32(i32::MIN))));
        assert!(b.fuel_remaining < a.fuel_remaining);
    }
    #[test]
    fn real_loop_trap_and_recursion_are_bounded() {
        let looping = compile(&single(&[0x03, 0x40, 0x0c, 0, 0x0b, 0x0b], None)).unwrap();
        let outcome = execute(&looping, "f", &[], 100);
        assert_eq!(outcome.result.unwrap_err().kind, WasmErrorKind::Resource);
        assert!(outcome.fuel_remaining < 100);
        let trap = compile(&single(&[0, 0x0b], None)).unwrap();
        let outcome = execute(&trap, "f", &[], 100);
        assert_eq!(outcome.result.unwrap_err().kind, WasmErrorKind::Runtime);
        assert!(outcome.fuel_remaining < 100);
        let recursive = compile(&single(&[0x10, 0, 0x0b], None)).unwrap();
        assert_eq!(
            execute(&recursive, "f", &[], 100_000)
                .result
                .unwrap_err()
                .kind,
            WasmErrorKind::Resource
        );
    }
    #[test]
    fn float_exports_and_internal_calls_execute() {
        let f = compile(&single(&[0x43, 0, 0, 0x20, 0x40, 0x0b], Some(0x7d))).unwrap();
        assert_eq!(
            execute(&f, "f", &[], 100).result,
            Ok(Some(WasmValue::F32(2.5)))
        );
        let calls = b"\0asm\x01\0\0\0\x01\x05\x01\x60\0\x01\x7f\x03\x03\x02\0\0\x07\x05\x01\x01f\0\x01\x0a\x0b\x02\x04\0\x41\x2a\x0b\x04\0\x10\0\x0b";
        let m = compile(calls).unwrap();
        assert_eq!(
            execute(&m, "f", &[], 100).result,
            Ok(Some(WasmValue::I32(42)))
        );
    }
    #[test]
    fn valid_but_ineligible_modules_never_instantiate() {
        let sections: &[&[u8]] = &[
            &[5, 3, 1, 0, 1],                                        // memory 1
            &[4, 4, 1, 0x70, 0, 1],                                  // table funcref 1
            &[6, 6, 1, 0x7f, 1, 0x41, 0, 0x0b],                      // mutable i32 global
            &[1, 4, 1, 0x60, 0, 0, 2, 7, 1, 1, b'm', 1, b'f', 0, 0], // function import
            &[
                1, 4, 1, 0x60, 0, 0, 3, 2, 1, 0, 8, 1, 0, 10, 5, 1, 3, 0, 0, 0x0b,
            ], // trapping start
        ];
        for section in sections {
            let mut bytes = HEADER.to_vec();
            bytes.extend_from_slice(section);
            assert!(validate(&bytes).is_ok());
            let m = compile(&bytes).unwrap();
            assert!(m.incompatibility().is_some());
            let outcome = execute(&m, "f", &[], 100);
            assert_eq!(outcome.result.unwrap_err().kind, WasmErrorKind::Unsupported);
            assert_eq!(outcome.fuel_remaining, 100);
        }
    }
    #[test]
    fn i64_exports_preserve_exact_bits_and_wrapping_arithmetic() {
        let bytes = single(&[0x42, 0x7f, 0x0b], Some(0x7e));
        assert!(validate(&bytes).is_ok());
        let constant = compile(&bytes).unwrap();
        assert!(constant.ensure_instantiable().is_ok());
        assert_eq!(constant.exports()[0].results, [ValueType::I64]);
        assert_eq!(
            execute(&constant, "f", &[], 100).result,
            Ok(Some(WasmValue::I64(-1)))
        );
        // Same add fixture, with i64 parameters/result and i64.add.
        let mut bytes = ADD.to_vec();
        bytes[13] = 0x7e;
        bytes[14] = 0x7e;
        bytes[16] = 0x7e;
        let opcode = bytes.len() - 2;
        bytes[opcode] = 0x7c;
        let add = compile(&bytes).unwrap();
        assert!(add.ensure_instantiable().is_ok());
        assert_eq!(add.exports()[0].params, [ValueType::I64, ValueType::I64]);
        for (a, b, sum) in [
            (9_007_199_254_740_993, 2, 9_007_199_254_740_995),
            (i64::MAX, 1, i64::MIN),
            (i64::MIN, -1, i64::MAX),
            (-1, 1, 0),
        ] {
            let outcome = execute(&add, "add", &[WasmValue::I64(a), WasmValue::I64(b)], 100);
            assert_eq!(outcome.result, Ok(Some(WasmValue::I64(sum))));
            assert!(outcome.fuel_remaining < 100);
        }
        let wrong = execute(&add, "add", &[WasmValue::I32(1), WasmValue::I64(2)], 100);
        assert_eq!(wrong.result.unwrap_err().kind, WasmErrorKind::Link);
        assert_eq!(wrong.fuel_remaining, 100);
        let exhausted = execute(&add, "add", &[WasmValue::I64(1), WasmValue::I64(2)], 0);
        assert_eq!(exhausted.result.unwrap_err().kind, WasmErrorKind::Resource);
        assert_eq!(exhausted.fuel_remaining, 0);
    }
    #[test]
    fn malformed_truncated_duplicate_and_overlong_leb_fail() {
        for bytes in [
            b"invalid".as_slice(),
            &ADD[..ADD.len() - 1],
            b"\0asm\x01\0\0\0\x01\x80\x80\x80\x80\x80\0",
            b"\0asm\x01\0\0\0\x01\x01\0\x01\x01\0",
        ] {
            assert_eq!(validate(bytes).unwrap_err().kind, WasmErrorKind::Compile);
            assert_eq!(compile(bytes).unwrap_err().kind, WasmErrorKind::Compile);
        }
    }
    #[test]
    fn structural_limits_and_argument_failures_are_explicit() {
        assert_eq!(
            validate(&vec![0; MAX_MODULE_BYTES + 1]).unwrap_err().kind,
            WasmErrorKind::Resource
        );
        let mut types = HEADER.to_vec();
        types.extend([1, 2, 0x81, 1]);
        assert_eq!(compile(&types).unwrap_err().kind, WasmErrorKind::Resource);
        let m = compile(ADD).unwrap();
        let wrong = execute(&m, "add", &[WasmValue::F32(1.0), WasmValue::I32(1)], 100);
        assert_eq!(wrong.result.unwrap_err().kind, WasmErrorKind::Link);
        assert_eq!(wrong.fuel_remaining, 100);
        assert_eq!(
            execute(&m, "missing", &[], 100).result.unwrap_err().kind,
            WasmErrorKind::Link
        );
        let zero = execute(&m, "add", &[WasmValue::I32(1), WasmValue::I32(2)], 0);
        assert_eq!(zero.result.unwrap_err().kind, WasmErrorKind::Resource);
        assert_eq!(zero.fuel_remaining, 0);
    }
}

#[cfg(test)]
mod allocation_boundary_tests {
    use super::*;
    #[test]
    fn declared_type_vectors_are_capped_before_materialization() {
        // One recursive group with a malicious declared count: never allocate it.
        let recursive = b"\0asm\x01\0\0\0\x01\x07\x01\x4e\xff\xff\xff\xff\x0f";
        assert_eq!(
            validate(recursive).unwrap_err().kind,
            WasmErrorKind::Compile
        );
        // One ordinary function declares 33 parameters, without their bytes.
        let parameters = b"\0asm\x01\0\0\0\x01\x03\x01\x60\x21";
        assert_eq!(
            validate(parameters).unwrap_err().kind,
            WasmErrorKind::Resource
        );
        // Large result count is rejected before the general parser can reserve it.
        let results = b"\0asm\x01\0\0\0\x01\x04\x01\x60\0\x7f";
        assert_eq!(compile(results).unwrap_err().kind, WasmErrorKind::Resource);
    }
    #[test]
    fn passive_data_is_hidden_mutable_state_and_is_rejected() {
        let bytes = b"\0asm\x01\0\0\0\x0b\x04\x01\x01\x01x";
        assert!(validate(bytes).is_ok());
        let m = compile(bytes).unwrap();
        assert_eq!(
            m.ensure_instantiable().unwrap_err().kind,
            WasmErrorKind::Unsupported
        );
    }
    #[test]
    fn alias_exports_preserve_original_function_index_and_binary_order() {
        let bytes = b"\0asm\x01\0\0\0\x01\x04\x01\x60\0\0\x03\x02\x01\0\x07\x09\x02\x01z\0\0\x01a\0\0\x0a\x04\x01\x02\0\x0b";
        let m = compile(bytes).unwrap();
        assert_eq!(m.exports().len(), 2);
        assert_eq!(m.exports()[0].name, "z");
        assert_eq!(m.exports()[1].name, "a");
        assert_eq!(m.exports()[0].function_index, Some(0));
        assert_eq!(m.exports()[1].function_index, Some(0));
    }
}
