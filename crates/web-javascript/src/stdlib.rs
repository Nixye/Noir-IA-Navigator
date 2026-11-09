//! Bounded native data structures for the original interpreter. No page I/O.
use crate::descriptors::Definition;
use crate::{Heap, Host, JsError, Runtime, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

const MAX_ITEMS: usize = 4096;
const MAX_BUFFER: usize = 1024 * 1024;
const MAX_BUFFER_TOTAL: usize = 4 * MAX_BUFFER;
const MAX_SYMBOLS: usize = 4096;
const DATE_LIMIT: f64 = 8_640_000_000_000_000.;

#[derive(Clone, Copy, Debug)]
enum ElementKind {
    I8,
    U8,
    U8Clamped,
    I16,
    U16,
    I32,
    U32,
    F32,
    F64,
    BigI64,
    BigU64,
}
impl ElementKind {
    fn is_bigint(self) -> bool {
        matches!(self, Self::BigI64 | Self::BigU64)
    }
    fn size(self) -> usize {
        match self {
            Self::I8 | Self::U8 | Self::U8Clamped => 1,
            Self::I16 | Self::U16 => 2,
            Self::I32 | Self::U32 | Self::F32 => 4,
            Self::F64 | Self::BigI64 | Self::BigU64 => 8,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::I8 => "Int8Array",
            Self::U8 => "Uint8Array",
            Self::U8Clamped => "Uint8ClampedArray",
            Self::I16 => "Int16Array",
            Self::U16 => "Uint16Array",
            Self::I32 => "Int32Array",
            Self::U32 => "Uint32Array",
            Self::F32 => "Float32Array",
            Self::F64 => "Float64Array",
            Self::BigI64 => "BigInt64Array",
            Self::BigU64 => "BigUint64Array",
        }
    }
}
fn element_kind(name: &str) -> Option<ElementKind> {
    Some(match name {
        "Int8Array" => ElementKind::I8,
        "Uint8Array" => ElementKind::U8,
        "Uint8ClampedArray" => ElementKind::U8Clamped,
        "Int16Array" => ElementKind::I16,
        "Uint16Array" => ElementKind::U16,
        "Int32Array" => ElementKind::I32,
        "Uint32Array" => ElementKind::U32,
        "Float32Array" => ElementKind::F32,
        "Float64Array" => ElementKind::F64,
        "BigInt64Array" => ElementKind::BigI64,
        "BigUint64Array" => ElementKind::BigU64,
        _ => return None,
    })
}

#[derive(Clone, Debug)]
enum NativeObject {
    Date(f64),
    Map(Vec<Option<(Value, Value)>>),
    Set(Vec<Option<Value>>),
    Buffer(Vec<u8>),
    View {
        buffer: usize,
        offset: usize,
        length: usize,
        kind: Option<ElementKind>,
    },
    Iterator {
        target: Value,
        index: usize,
        mode: u8,
        done: bool,
    },
}

#[derive(Clone, Debug, Default)]
pub(super) struct StdlibState {
    objects: BTreeMap<usize, NativeObject>,
    prototypes: BTreeMap<&'static str, Value>,
    symbols: BTreeMap<u64, Arc<str>>,
    no_description: BTreeSet<u64>,
    registry: BTreeMap<String, u64>,
    well_known: BTreeMap<&'static str, u64>,
    next_symbol: u64,
    buffer_bytes: usize,
}

pub(super) fn global(name: &str) -> Option<Value> {
    Some(Value::Native(match name {
        "Date" => "Date",
        "Map" => "Map",
        "Set" => "Set",
        "Symbol" => "Symbol",
        "ArrayBuffer" => "ArrayBuffer",
        "DataView" => "DataView",
        "Int8Array" => "Int8Array",
        "Uint8Array" => "Uint8Array",
        "Uint8ClampedArray" => "Uint8ClampedArray",
        "Int16Array" => "Int16Array",
        "Uint16Array" => "Uint16Array",
        "Int32Array" => "Int32Array",
        "Uint32Array" => "Uint32Array",
        "Float32Array" => "Float32Array",
        "Float64Array" => "Float64Array",
        "BigInt64Array" => "BigInt64Array",
        "BigUint64Array" => "BigUint64Array",
        _ => return None,
    }))
}

fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => a == b || (a.is_nan() && b.is_nan()),
        _ => crate::strict_equal(a, b),
    }
}
fn canonical(value: Value) -> Value {
    if matches!(value,Value::Number(n) if n==0.) {
        Value::Number(0.)
    } else {
        value
    }
}
fn object_id(value: &Value) -> Option<usize> {
    if let Value::Object(id) = value {
        Some(*id)
    } else {
        None
    }
}
fn time_clip(value: f64) -> f64 {
    if value.is_finite() && value.abs() <= DATE_LIMIT {
        if value.trunc() == 0. {
            0.
        } else {
            value.trunc()
        }
    } else {
        f64::NAN
    }
}

impl Runtime {
    /// Snapshot actual BufferSource slots, never page-defined getters. All views
    /// use their byte range (DataView length is already measured in bytes).
    pub(super) fn wasm_buffer_source(
        &mut self,
        value: &Value,
        capacity: usize,
    ) -> Result<Vec<u8>, JsError> {
        self.snapshot_buffer_source(
            value,
            capacity,
            web_wasm::MAX_MODULE_BYTES,
            "WebAssembly",
            |_, _| Ok(()),
        )
    }

    pub(super) fn snapshot_buffer_source(
        &mut self,
        value: &Value,
        capacity: usize,
        individual_limit: usize,
        label: &str,
        reserve: impl FnOnce(&mut Self, usize) -> Result<(), JsError>,
    ) -> Result<Vec<u8>, JsError> {
        let Value::Object(id) = value else {
            return Err(self.property_type_error(&format!(
                "{label} requires an ArrayBuffer or ArrayBuffer view"
            )));
        };
        let (buffer, offset, length) = match self.stdlib.objects.get(id) {
            Some(NativeObject::Buffer(bytes)) => (*id, 0, bytes.len()),
            Some(NativeObject::View {
                buffer,
                offset,
                length,
                kind,
            }) => {
                let length = length
                    .checked_mul(kind.map_or(1, ElementKind::size))
                    .ok_or_else(|| JsError::resource("BufferSource byte length overflow"))?;
                (*buffer, *offset, length)
            }
            _ => {
                return Err(self.property_type_error(&format!(
                    "{label} requires an ArrayBuffer or ArrayBuffer view"
                )));
            }
        };
        if length > capacity || length > individual_limit {
            return Err(JsError::resource(format!(
                "{label} cumulative binary byte budget exceeded"
            )));
        }
        reserve(self, length)?;
        self.remaining = self
            .remaining
            .checked_sub(length as u64 + 1)
            .ok_or_else(|| JsError::resource(format!("{label} binary work budget exceeded")))?;
        let Some(NativeObject::Buffer(bytes)) = self.stdlib.objects.get(&buffer) else {
            return Err(self.property_type_error("Invalid BufferSource backing buffer"));
        };
        let end = offset
            .checked_add(length)
            .ok_or_else(|| JsError::resource("BufferSource range overflow"))?;
        let slice = bytes
            .get(offset..end)
            .ok_or_else(|| JsError::runtime("Invalid BufferSource range"))?;
        Ok(slice.to_vec())
    }

    pub(super) fn stdlib_is_exotic(&self, value: &Value) -> bool {
        object_id(value).is_some_and(|id| self.stdlib.objects.contains_key(&id))
    }
    fn stdlib_object(&mut self, name: &'static str, brand: NativeObject) -> Result<Value, JsError> {
        let prototype = self.stdlib_prototype(name)?;
        let value = self.object(BTreeMap::new(), Some(prototype))?;
        let Value::Object(id) = value else {
            unreachable!()
        };
        self.stdlib.objects.insert(id, brand);
        Ok(Value::Object(id))
    }
    fn stdlib_prototype(&mut self, name: &'static str) -> Result<Value, JsError> {
        if let Some(value) = self.stdlib.prototypes.get(name) {
            return Ok(value.clone());
        }
        let methods: &[(&str, &'static str)] = match name {
            "Map" => &[
                ("get", "Map.get"),
                ("set", "Map.set"),
                ("has", "Map.has"),
                ("delete", "Map.delete"),
                ("clear", "Map.clear"),
                ("forEach", "Map.forEach"),
                ("keys", "Map.keys"),
                ("values", "Map.values"),
                ("entries", "Map.entries"),
            ],
            "Set" => &[
                ("add", "Set.add"),
                ("has", "Set.has"),
                ("delete", "Set.delete"),
                ("clear", "Set.clear"),
                ("forEach", "Set.forEach"),
                ("keys", "Set.values"),
                ("values", "Set.values"),
                ("entries", "Set.entries"),
            ],
            "ArrayBuffer" => &[("slice", "ArrayBuffer.slice")],
            "Date" => &[
                ("getTime", "Date.getTime"),
                ("valueOf", "Date.getTime"),
                ("setTime", "Date.setTime"),
                ("toISOString", "Date.toISOString"),
                ("toJSON", "Date.toJSON"),
                ("toUTCString", "Date.toUTCString"),
                ("getUTCFullYear", "Date.getUTCFullYear"),
                ("getUTCMonth", "Date.getUTCMonth"),
                ("getUTCDate", "Date.getUTCDate"),
                ("getUTCDay", "Date.getUTCDay"),
                ("getUTCHours", "Date.getUTCHours"),
                ("getUTCMinutes", "Date.getUTCMinutes"),
                ("getUTCSeconds", "Date.getUTCSeconds"),
                ("getUTCMilliseconds", "Date.getUTCMilliseconds"),
                ("setUTCFullYear", "Date.setUTCFullYear"),
                ("setUTCMonth", "Date.setUTCMonth"),
                ("setUTCDate", "Date.setUTCDate"),
                ("setUTCHours", "Date.setUTCHours"),
                ("setUTCMinutes", "Date.setUTCMinutes"),
                ("setUTCSeconds", "Date.setUTCSeconds"),
                ("setUTCMilliseconds", "Date.setUTCMilliseconds"),
            ],
            "DataView" => &[
                ("getInt8", "DataView.getInt8"),
                ("getUint8", "DataView.getUint8"),
                ("getInt16", "DataView.getInt16"),
                ("getUint16", "DataView.getUint16"),
                ("getInt32", "DataView.getInt32"),
                ("getUint32", "DataView.getUint32"),
                ("getFloat32", "DataView.getFloat32"),
                ("getFloat64", "DataView.getFloat64"),
                ("getBigInt64", "DataView.getBigInt64"),
                ("getBigUint64", "DataView.getBigUint64"),
                ("setInt8", "DataView.setInt8"),
                ("setUint8", "DataView.setUint8"),
                ("setInt16", "DataView.setInt16"),
                ("setUint16", "DataView.setUint16"),
                ("setInt32", "DataView.setInt32"),
                ("setUint32", "DataView.setUint32"),
                ("setFloat32", "DataView.setFloat32"),
                ("setFloat64", "DataView.setFloat64"),
                ("setBigInt64", "DataView.setBigInt64"),
                ("setBigUint64", "DataView.setBigUint64"),
            ],
            "Iterator" => &[("next", "NativeIterator.next")],
            _ if element_kind(name).is_some() => &[
                ("set", "TypedArray.set"),
                ("subarray", "TypedArray.subarray"),
                ("slice", "TypedArray.slice"),
                ("fill", "TypedArray.fill"),
                ("at", "TypedArray.at"),
                ("includes", "TypedArray.includes"),
                ("indexOf", "TypedArray.indexOf"),
                ("join", "TypedArray.join"),
                ("values", "TypedArray.values"),
                ("keys", "TypedArray.keys"),
                ("entries", "TypedArray.entries"),
                ("forEach", "TypedArray.forEach"),
                ("map", "TypedArray.map"),
                ("reverse", "TypedArray.reverse"),
            ],
            _ => return Err(JsError::runtime("Unknown native prototype")),
        };
        let mut properties = BTreeMap::new();
        if name != "Iterator" {
            properties.insert(
                "constructor".into(),
                global(name).ok_or_else(|| JsError::runtime("Unknown native constructor"))?,
            );
        }
        properties.extend(
            methods
                .iter()
                .map(|(name, native)| (name.to_string(), Value::Native(native))),
        );
        let value = self.object(properties, None)?;
        let Value::Object(id) = value else {
            unreachable!()
        };
        if let Heap::Object(object) = &mut self.heap[id] {
            object
                .non_enumerable
                .extend(object.properties.keys().cloned());
        }
        self.stdlib.prototypes.insert(name, Value::Object(id));
        let getters: &[(&str, &'static str)] = match name {
            "Map" => &[("size", "Map.size")],
            "Set" => &[("size", "Set.size")],
            "ArrayBuffer" => &[
                ("byteLength", "ArrayBuffer.byteLength"),
                ("maxByteLength", "ArrayBuffer.maxByteLength"),
                ("resizable", "ArrayBuffer.resizable"),
            ],
            "DataView" => &[
                ("buffer", "DataView.buffer"),
                ("byteOffset", "DataView.byteOffset"),
                ("byteLength", "DataView.byteLength"),
            ],
            _ if element_kind(name).is_some() => &[
                ("buffer", "TypedArray.buffer"),
                ("byteOffset", "TypedArray.byteOffset"),
                ("byteLength", "TypedArray.byteLength"),
                ("length", "TypedArray.length"),
            ],
            _ => &[],
        };
        for (property, getter) in getters {
            self.charge_string(property.len())?;
            if let Heap::Object(object) = &mut self.heap[id] {
                object
                    .properties
                    .insert((*property).into(), Value::Undefined);
                object.non_enumerable.insert((*property).into());
            }
            self.property_metadata.entry(id).or_default().insert(
                (*property).into(),
                crate::descriptors::PropertyMetadata {
                    writable: false,
                    enumerable: false,
                    configurable: true,
                    accessor: Some((Value::Native(getter), Value::Undefined)),
                },
            );
        }
        if let Some(kind) = element_kind(name) {
            if let Heap::Object(object) = &mut self.heap[id] {
                object.properties.insert(
                    "BYTES_PER_ELEMENT".into(),
                    Value::Number(kind.size() as f64),
                );
                object.non_enumerable.insert("BYTES_PER_ELEMENT".into());
            }
            self.property_metadata.entry(id).or_default().insert(
                "BYTES_PER_ELEMENT".into(),
                crate::descriptors::PropertyMetadata {
                    writable: false,
                    enumerable: false,
                    configurable: false,
                    accessor: None,
                },
            );
        }
        let iterator = match name {
            "Map" => Some("Map.entries"),
            "Set" => Some("Set.values"),
            "Iterator" => Some("NativeIterator.identity"),
            _ if element_kind(name).is_some() => Some("TypedArray.values"),
            _ => None,
        };
        if let Some(method) = iterator {
            let symbol = self.well_known_symbol("iterator")?;
            let Value::Symbol(symbol, _) = symbol else {
                unreachable!()
            };
            self.symbol_properties
                .entry(id)
                .or_default()
                .insert(symbol, Value::Native(method));
            self.symbol_metadata.entry(id).or_default().insert(
                symbol,
                crate::descriptors::PropertyMetadata {
                    writable: true,
                    enumerable: false,
                    configurable: true,
                    accessor: None,
                },
            );
        }
        Ok(Value::Object(id))
    }
    fn stdlib_buffer(&mut self, size: usize) -> Result<Value, JsError> {
        if size > MAX_BUFFER || self.stdlib.buffer_bytes.saturating_add(size) > MAX_BUFFER_TOTAL {
            return Err(JsError::resource(
                "Native buffer allocation budget exceeded",
            ));
        }
        self.remaining = self
            .remaining
            .checked_sub(size.div_ceil(64) as u64)
            .ok_or_else(|| JsError::resource("Native buffer initialization work exceeded"))?;
        let buffer = self.stdlib_object("ArrayBuffer", NativeObject::Buffer(vec![0; size]))?;
        self.stdlib.buffer_bytes += size;
        Ok(buffer)
    }
    pub(super) fn buffer_from_bytes(&mut self, bytes: &[u8]) -> Result<Value, JsError> {
        let value = self.stdlib_buffer(bytes.len())?;
        let Value::Object(id) = value else {
            unreachable!()
        };
        let Some(NativeObject::Buffer(target)) = self.stdlib.objects.get_mut(&id) else {
            unreachable!()
        };
        target.copy_from_slice(bytes);
        Ok(value)
    }
    fn stdlib_number(&mut self, value: &Value) -> Result<f64, JsError> {
        if matches!(value, Value::Symbol(..) | Value::BigInt(_)) || self.is_bigint_wrapper(value) {
            Err(self.property_type_error("Cannot convert Symbol or BigInt to a number"))
        } else {
            Ok(self.number(value))
        }
    }

    pub(super) fn stdlib_get(
        &mut self,
        value: &Value,
        property: &str,
    ) -> Result<Option<Value>, JsError> {
        if let Value::Native(name) = value
            && global(name).is_some()
        {
            if property == "prototype" {
                if *name == "Symbol" {
                    return self.symbol_prototype().map(Some);
                }
                return self.stdlib_prototype(name).map(Some);
            }
            if property == "BYTES_PER_ELEMENT"
                && let Some(kind) = element_kind(name)
            {
                return Ok(Some(Value::Number(kind.size() as f64)));
            }
            if element_kind(name).is_some() && matches!(property, "from" | "of") {
                return Ok(Some(Value::Native(if property == "from" {
                    "TypedArray.from"
                } else {
                    "TypedArray.of"
                })));
            }
            let native = match (*name, property) {
                ("Date", "now") => Some("Date.now"),
                ("Date", "parse") => Some("Date.parse"),
                ("Date", "UTC") => Some("Date.UTC"),
                ("ArrayBuffer", "isView") => Some("ArrayBuffer.isView"),
                ("Symbol", "for") => Some("Symbol.for"),
                ("Symbol", "keyFor") => Some("Symbol.keyFor"),
                _ => None,
            };
            if let Some(native) = native {
                return Ok(Some(Value::Native(native)));
            }
            if *name == "Symbol"
                && matches!(
                    property,
                    "iterator"
                        | "asyncIterator"
                        | "toStringTag"
                        | "toPrimitive"
                        | "hasInstance"
                        | "species"
                        | "isConcatSpreadable"
                        | "match"
                        | "replace"
                        | "search"
                        | "split"
                        | "unscopables"
                )
            {
                let name = match property {
                    "iterator" => "iterator",
                    "asyncIterator" => "asyncIterator",
                    "toStringTag" => "toStringTag",
                    "toPrimitive" => "toPrimitive",
                    "hasInstance" => "hasInstance",
                    "species" => "species",
                    "isConcatSpreadable" => "isConcatSpreadable",
                    "match" => "match",
                    "replace" => "replace",
                    "search" => "search",
                    "split" => "split",
                    _ => "unscopables",
                };
                return self.well_known_symbol(name).map(Some);
            }
        }
        let Some(id) = object_id(value) else {
            return Ok(None);
        };
        if !matches!(
            self.stdlib.objects.get(&id),
            Some(NativeObject::View { kind: Some(_), .. })
        ) {
            return Ok(None);
        }
        if invalid_numeric_property(property) {
            return Ok(Some(Value::Undefined));
        }
        let numeric = matches!(
            self.stdlib.objects.get(&id),
            Some(NativeObject::View { kind: Some(_), .. })
        ) && canonical_index(property).is_some();
        if !numeric
            && matches!(self.heap.get(id),Some(Heap::Object(object)) if object.properties.contains_key(property))
        {
            return Ok(None);
        }
        self.charge_stdlib_inspection(id)?;
        let Some(brand) = self.stdlib.objects.get(&id).cloned() else {
            return Ok(None);
        };
        let result = match brand {
            NativeObject::View {
                buffer,
                offset,
                length,
                kind,
            } => match property {
                _ if kind.is_some() && canonical_index(property).is_some() => {
                    let index = canonical_index(property).unwrap();
                    Some(if index < length {
                        self.read_element(
                            buffer,
                            offset + index * kind.unwrap().size(),
                            kind.unwrap(),
                            true,
                        )?
                    } else {
                        Value::Undefined
                    })
                }
                _ => None,
            },
            _ => None,
        };
        Ok(result)
    }
    pub(super) fn stdlib_set(
        &mut self,
        value: &Value,
        property: &str,
        new: &Value,
        host: &mut impl Host,
    ) -> Result<Option<bool>, JsError> {
        let Some(id) = object_id(value) else {
            return Ok(None);
        };
        if !matches!(
            self.stdlib.objects.get(&id),
            Some(NativeObject::View { kind: Some(_), .. })
        ) {
            return Ok(None);
        }
        if invalid_numeric_property(property) {
            let Some(NativeObject::View {
                kind: Some(kind), ..
            }) = self.stdlib.objects.get(&id).cloned()
            else {
                unreachable!()
            };
            self.element_value(kind, new, host)?;
            return Ok(Some(true));
        }
        let numeric = matches!(
            self.stdlib.objects.get(&id),
            Some(NativeObject::View { kind: Some(_), .. })
        ) && canonical_index(property).is_some();
        if !numeric
            && matches!(self.heap.get(id),Some(Heap::Object(object)) if object.properties.contains_key(property))
        {
            return Ok(None);
        }
        self.charge_stdlib_inspection(id)?;
        let Some(brand) = self.stdlib.objects.get(&id).cloned() else {
            return Ok(None);
        };
        if let NativeObject::View {
            buffer,
            offset,
            length,
            kind: Some(kind),
        } = brand
            && let Some(index) = canonical_index(property)
        {
            let number = self.element_value(kind, new, host)?;
            if index < length {
                self.write_element(buffer, offset + index * kind.size(), kind, &number, true)?;
            }
            return Ok(Some(true));
        }
        Ok(None)
    }
    pub(super) fn stdlib_has(&self, value: &Value, property: &str) -> Option<bool> {
        if let Value::Native(name) = value {
            if global(name).is_some() && matches!(property, "prototype" | "BYTES_PER_ELEMENT") {
                return Some(property == "prototype" || element_kind(name).is_some());
            }
            if matches!(
                (*name, property),
                ("Date", "now" | "parse" | "UTC")
                    | ("ArrayBuffer", "isView")
                    | (
                        "Symbol",
                        "for"
                            | "keyFor"
                            | "iterator"
                            | "asyncIterator"
                            | "toStringTag"
                            | "toPrimitive"
                            | "hasInstance"
                            | "species"
                            | "isConcatSpreadable"
                            | "match"
                            | "replace"
                            | "search"
                            | "split"
                            | "unscopables"
                    )
            ) || element_kind(name).is_some() && matches!(property, "from" | "of")
            {
                return Some(true);
            }
        }
        let id = object_id(value)?;
        if matches!(
            self.stdlib.objects.get(&id),
            Some(NativeObject::View { kind: Some(_), .. })
        ) && invalid_numeric_property(property)
        {
            return Some(false);
        }
        match self.stdlib.objects.get(&id)? {
            NativeObject::View { length, kind, .. }
                if canonical_index(property).is_some() && kind.is_some() =>
            {
                Some(canonical_index(property).unwrap() < *length)
            }
            _ => None,
        }
    }
    pub(super) fn stdlib_delete(
        &self,
        value: &Value,
        property: &str,
    ) -> Result<Option<bool>, JsError> {
        let Some(id) = object_id(value) else {
            return Ok(None);
        };
        if matches!(
            self.stdlib.objects.get(&id),
            Some(NativeObject::View { kind: Some(_), .. })
        ) && invalid_numeric_property(property)
        {
            return Ok(Some(true));
        }
        if let Some(NativeObject::View {
            length,
            kind: Some(_),
            ..
        }) = self.stdlib.objects.get(&id)
            && let Some(index) = canonical_index(property)
        {
            return Ok(Some(index >= *length));
        }
        Ok(None)
    }

    pub(super) fn symbol_prototype(&mut self) -> Result<Value, JsError> {
        if let Some(value) = self.stdlib.prototypes.get("Symbol") {
            return Ok(value.clone());
        }
        let result = self.object(
            BTreeMap::from([
                ("constructor".into(), Value::Native("Symbol")),
                ("description".into(), Value::Undefined),
                ("toString".into(), Value::Native("Symbol.toString")),
                ("valueOf".into(), Value::Native("Symbol.valueOf")),
            ]),
            None,
        )?;
        self.hide_properties(
            &result,
            &["constructor", "description", "toString", "valueOf"],
        );
        let Value::Object(id) = &result else {
            unreachable!()
        };
        self.property_metadata.entry(*id).or_default().insert(
            "description".into(),
            crate::descriptors::PropertyMetadata {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: Some((Value::Native("Symbol.description"), Value::Undefined)),
            },
        );
        self.stdlib.prototypes.insert("Symbol", result.clone());
        Ok(result)
    }
    fn new_symbol(&mut self, description: String) -> Result<Value, JsError> {
        if self.stdlib.symbols.len() >= MAX_SYMBOLS {
            return Err(JsError::resource("Symbol identity budget exceeded"));
        }
        if description.len() > 65536 {
            return Err(JsError::resource("Symbol description exceeds limit"));
        }
        self.charge_string(description.len())?;
        self.stdlib.next_symbol = self
            .stdlib
            .next_symbol
            .checked_add(1)
            .ok_or_else(|| JsError::resource("Symbol identity overflow"))?;
        let id = self.stdlib.next_symbol;
        let text: Arc<str> = Arc::from(description);
        self.stdlib.symbols.insert(id, text.clone());
        Ok(Value::Symbol(id, text))
    }
    pub(super) fn symbol_value(&self, id: u64) -> Result<Value, JsError> {
        self.stdlib
            .symbols
            .get(&id)
            .cloned()
            .map(|description| Value::Symbol(id, description))
            .ok_or_else(|| JsError::runtime("Unknown Symbol identity"))
    }

    pub(super) fn well_known_symbol(&mut self, name: &str) -> Result<Value, JsError> {
        let name = match name {
            "iterator" => "iterator",
            "asyncIterator" => "asyncIterator",
            "toStringTag" => "toStringTag",
            "toPrimitive" => "toPrimitive",
            "hasInstance" => "hasInstance",
            "species" => "species",
            "isConcatSpreadable" => "isConcatSpreadable",
            "match" => "match",
            "replace" => "replace",
            "search" => "search",
            "split" => "split",
            "unscopables" => "unscopables",
            _ => return Err(JsError::runtime("Unknown well-known Symbol")),
        };
        if let Some(id) = self.stdlib.well_known.get(name) {
            return Ok(Value::Symbol(*id, self.stdlib.symbols[id].clone()));
        }
        let value = self.new_symbol(format!("Symbol.{name}"))?;
        if let Value::Symbol(id, _) = value {
            self.stdlib.well_known.insert(name, id);
        }
        Ok(value)
    }

    fn charge_stdlib_inspection(&mut self, id: usize) -> Result<(), JsError> {
        let work = match self.stdlib.objects.get(&id) {
            Some(NativeObject::Map(values)) => values.len(),
            Some(NativeObject::Set(values)) => values.len(),
            Some(NativeObject::Buffer(bytes)) => bytes.len().div_ceil(64),
            _ => 1,
        };
        self.remaining = self
            .remaining
            .checked_sub(work as u64)
            .ok_or_else(|| JsError::resource("Native data inspection work exceeded"))?;
        Ok(())
    }

    pub(super) fn stdlib_call(
        &mut self,
        name: &str,
        args: &[Value],
        this: Value,
        construct: bool,
        host: &mut impl Host,
    ) -> Result<Option<Value>, JsError> {
        let first = args.first().cloned().unwrap_or(Value::Undefined);
        if let Some(kind) = element_kind(name) {
            if !construct {
                return Err(self.property_type_error("Typed array constructor requires new"));
            }
            return self.new_typed_array(kind, args, host).map(Some);
        }
        if global(name).is_some() {
            return match name {
                "Symbol" => {
                    if construct {
                        return Err(self.property_type_error("Symbol is not a constructor"));
                    }
                    let text = if matches!(first, Value::Undefined) {
                        String::new()
                    } else {
                        self.text(&first, 0)?
                    };
                    let result = self.new_symbol(text)?;
                    if matches!(first, Value::Undefined)
                        && let Value::Symbol(id, _) = result
                    {
                        self.stdlib.no_description.insert(id);
                    }
                    Ok(Some(result))
                }
                "Map" | "Set" => {
                    if !construct {
                        return Err(self.property_type_error("Collection constructor requires new"));
                    }
                    let result = self.stdlib_object(
                        if name == "Map" { "Map" } else { "Set" },
                        if name == "Map" {
                            NativeObject::Map(Vec::new())
                        } else {
                            NativeObject::Set(Vec::new())
                        },
                    )?;
                    if !matches!(first, Value::Undefined | Value::Null) {
                        let adder = self.get(
                            result.clone(),
                            if name == "Map" { "set" } else { "add" },
                            host,
                        )?;
                        if !self.is_callable_value(&adder, host) {
                            return Err(
                                self.property_type_error("Collection adder must be callable")
                            );
                        }
                        let mut iterator = self.iterator(first, host)?;
                        let outcome = (|| {
                            let mut count = 0;
                            while let Some(value) = self.iterator_next(&mut iterator, host)? {
                                self.tick()?;
                                count += 1;
                                if count > MAX_ITEMS {
                                    return Err(JsError::resource(
                                        "Collection constructor iteration limit exceeded",
                                    ));
                                }
                                let arguments = if name == "Map" {
                                    if !matches!(
                                        value,
                                        Value::Object(_)
                                            | Value::Array(_)
                                            | Value::Function(_)
                                            | Value::Host(_)
                                            | Value::Native(_)
                                    ) {
                                        return Err(self.property_type_error(
                                            "Map iterator entry must be an object",
                                        ));
                                    }
                                    vec![
                                        self.get(value.clone(), "0", host)?,
                                        self.get(value, "1", host)?,
                                    ]
                                } else {
                                    vec![value]
                                };
                                self.call(adder.clone(), arguments, result.clone(), host)?;
                            }
                            Ok(())
                        })();
                        if let Err(error) = outcome {
                            if error.kind != crate::ErrorKind::Resource {
                                let _ = self.iterator_close(&mut iterator, host);
                            }
                            return Err(error);
                        }
                    }
                    Ok(Some(result))
                }
                "ArrayBuffer" => {
                    if !construct {
                        return Err(
                            self.property_type_error("ArrayBuffer constructor requires new")
                        );
                    }
                    if args.get(1).is_some_and(|v| !matches!(v, Value::Undefined)) {
                        return Err(JsError::runtime(
                            "Resizable ArrayBuffer options are not implemented",
                        ));
                    }
                    let length = self.view_index(args.first(), MAX_BUFFER, host)?;
                    self.stdlib_buffer(length).map(Some)
                }
                "DataView" => {
                    if !construct {
                        return Err(self.property_type_error("DataView constructor requires new"));
                    }
                    let buffer = object_id(&first)
                        .filter(|id| {
                            matches!(self.stdlib.objects.get(id), Some(NativeObject::Buffer(_)))
                        })
                        .ok_or_else(|| self.property_type_error("DataView requires ArrayBuffer"))?;
                    let size = self.buffer_length(buffer)?;
                    let offset = self.view_index(args.get(1), size, host)?;
                    let length = if args.get(2).is_none_or(|v| matches!(v, Value::Undefined)) {
                        size - offset
                    } else {
                        self.view_index(args.get(2), size - offset, host)?
                    };
                    self.stdlib_object(
                        "DataView",
                        NativeObject::View {
                            buffer,
                            offset,
                            length,
                            kind: None,
                        },
                    )
                    .map(Some)
                }
                "Date" => {
                    if !construct {
                        return Err(JsError::runtime(
                            "Calling Date as a function requires local-time formatting, not implemented",
                        ));
                    }
                    let ms = if args.is_empty() {
                        host.utc_now_millis()
                            .ok_or_else(|| JsError::runtime("Clock is unavailable in this host"))?
                            as f64
                    } else if args.len() == 1 {
                        match first {
                            Value::String(ref text) => parse_iso(text),
                            Value::Object(id)
                                if matches!(
                                    self.stdlib.objects.get(&id),
                                    Some(NativeObject::Date(_))
                                ) =>
                            {
                                if let NativeObject::Date(n) = self.stdlib.objects[&id] {
                                    n
                                } else {
                                    unreachable!()
                                }
                            }
                            _ => self.stdlib_number(&first)?,
                        }
                    } else {
                        return Err(JsError::runtime(
                            "Local-time Date constructor requires timezone integration; use Date.UTC",
                        ));
                    };
                    self.stdlib_object("Date", NativeObject::Date(time_clip(ms)))
                        .map(Some)
                }
                _ => Ok(None),
            };
        }
        if construct {
            return Ok(None);
        }
        let value = match name {
            "Map.size" | "Set.size" => {
                let id = object_id(&this)
                    .ok_or_else(|| self.property_type_error("Collection receiver required"))?;
                self.charge_stdlib_inspection(id)?;
                let size = match self.stdlib.objects.get(&id) {
                    Some(NativeObject::Map(entries)) if name == "Map.size" => {
                        entries.iter().filter(|v| v.is_some()).count()
                    }
                    Some(NativeObject::Set(entries)) if name == "Set.size" => {
                        entries.iter().filter(|v| v.is_some()).count()
                    }
                    _ => return Err(self.property_type_error("Incompatible collection receiver")),
                };
                Value::Number(size as f64)
            }
            "ArrayBuffer.byteLength" | "ArrayBuffer.maxByteLength" | "ArrayBuffer.resizable" => {
                let id = object_id(&this)
                    .ok_or_else(|| self.property_type_error("ArrayBuffer receiver required"))?;
                let size = self.buffer_length(id)?;
                if name == "ArrayBuffer.resizable" {
                    Value::Bool(false)
                } else {
                    Value::Number(size as f64)
                }
            }
            "DataView.buffer"
            | "DataView.byteOffset"
            | "DataView.byteLength"
            | "TypedArray.buffer"
            | "TypedArray.byteOffset"
            | "TypedArray.byteLength"
            | "TypedArray.length" => {
                let id = object_id(&this)
                    .ok_or_else(|| self.property_type_error("Buffer view receiver required"))?;
                let Some(NativeObject::View {
                    buffer,
                    offset,
                    length,
                    kind,
                }) = self.stdlib.objects.get(&id)
                else {
                    return Err(self.property_type_error("Buffer view receiver required"));
                };
                if name.starts_with("DataView.") != kind.is_none() {
                    return Err(self.property_type_error("Incompatible buffer view receiver"));
                }
                match name.rsplit('.').next().unwrap() {
                    "buffer" => Value::Object(*buffer),
                    "byteOffset" => Value::Number(*offset as f64),
                    "length" => Value::Number(*length as f64),
                    _ => Value::Number((*length * kind.map_or(1, ElementKind::size)) as f64),
                }
            }
            "Symbol.for" => {
                let key = self.text(&first, 0)?;
                if let Some(id) = self.stdlib.registry.get(&key) {
                    Value::Symbol(*id, self.stdlib.symbols[id].clone())
                } else {
                    let symbol = self.new_symbol(key.clone())?;
                    if let Value::Symbol(id, _) = symbol {
                        self.stdlib.registry.insert(key, id);
                    }
                    symbol
                }
            }
            "Symbol.keyFor" => {
                let Value::Symbol(id, _) = first else {
                    return Err(self.property_type_error("Symbol.keyFor requires Symbol"));
                };
                if let Some(key) = self
                    .stdlib
                    .registry
                    .iter()
                    .find_map(|(key, value)| (*value == id).then_some(key.clone()))
                {
                    self.string(key)?
                } else {
                    Value::Undefined
                }
            }
            "Symbol.toString" => {
                let Value::Symbol(_, description) = this else {
                    return Err(self.property_type_error("Symbol receiver required"));
                };
                self.string(format!("Symbol({description})"))?
            }
            "Symbol.description" => {
                let Value::Symbol(id, description) = this else {
                    return Err(self.property_type_error("Symbol receiver required"));
                };
                if self.stdlib.no_description.contains(&id) {
                    Value::Undefined
                } else {
                    self.string(description.to_string())?
                }
            }
            "Symbol.valueOf" => {
                if !matches!(this, Value::Symbol(..)) {
                    return Err(self.property_type_error("Symbol receiver required"));
                }
                this
            }
            "ArrayBuffer.isView" => Value::Bool(object_id(&first).is_some_and(|id| {
                matches!(
                    self.stdlib.objects.get(&id),
                    Some(NativeObject::View { .. })
                )
            })),
            "ArrayBuffer.slice" => self.buffer_slice(&this, args, host)?,
            "Date.now" => Value::Number(
                host.utc_now_millis()
                    .ok_or_else(|| JsError::runtime("Clock is unavailable in this host"))?
                    as f64,
            ),
            "Date.parse" => Value::Number(parse_iso(&self.text(&first, 0)?)),
            "Date.UTC" => Value::Number(self.date_utc(args)?),
            name if name.starts_with("Date.") => self.date_method(name, &this, args)?,
            name if name.starts_with("Map.") || name.starts_with("Set.") => {
                self.collection_method(name, &this, args, host)?
            }
            "NativeIterator.next" => self.native_iterator_next(&this)?,
            "NativeIterator.identity" => {
                if !object_id(&this).is_some_and(|id| {
                    matches!(
                        self.stdlib.objects.get(&id),
                        Some(NativeObject::Iterator { .. })
                    )
                }) {
                    return Err(self.property_type_error("Native iterator receiver required"));
                }
                this
            }
            "TypedArray.from" | "TypedArray.of" => {
                let Value::Native(constructor) = this else {
                    return Err(self.property_type_error(
                        "TypedArray static method requires a native constructor",
                    ));
                };
                let kind = element_kind(constructor).ok_or_else(|| {
                    self.property_type_error(
                        "TypedArray static method requires a typed-array constructor",
                    )
                })?;
                let callback = if name == "TypedArray.from" {
                    args.get(1)
                        .filter(|value| !matches!(value, Value::Undefined))
                        .cloned()
                } else {
                    None
                };
                if callback
                    .as_ref()
                    .is_some_and(|value| !self.is_callable_value(value, host))
                {
                    return Err(self
                        .property_type_error("TypedArray.from mapping callback is not callable"));
                }
                let source = args.first().cloned().unwrap_or(Value::Undefined);
                let values = if name == "TypedArray.of" {
                    Some(args.to_vec())
                } else {
                    self.typed_iterable_values(&source, host)?
                };
                let length = if let Some(values) = &values {
                    values.len()
                } else {
                    self.stdlib_arraylike_length(&source, host)?
                };
                let destination =
                    self.new_typed_array(kind, &[Value::Number(length as f64)], host)?;
                let receiver = args.get(2).cloned().unwrap_or(Value::Undefined);
                for index in 0..length {
                    let mut value = if let Some(values) = &values {
                        values[index].clone()
                    } else {
                        self.get(source.clone(), &index.to_string(), host)?
                    };
                    if let Some(callback) = &callback {
                        value = self.call(
                            callback.clone(),
                            vec![value, Value::Number(index as f64)],
                            receiver.clone(),
                            host,
                        )?;
                    }
                    self.stdlib_set(&destination, &index.to_string(), &value, host)?;
                }
                destination
            }
            name if name.starts_with("TypedArray.") => {
                self.typed_method(name, &this, args, host)?
            }
            name if name.starts_with("DataView.") => {
                self.data_view_method(name, &this, args, host)?
            }
            _ => return Ok(None),
        };
        Ok(Some(value))
    }

    fn collection_method(
        &mut self,
        name: &str,
        this: &Value,
        args: &[Value],
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let id = object_id(this)
            .ok_or_else(|| self.property_type_error("Collection receiver required"))?;
        let is_map = name.starts_with("Map.");
        if !matches!(self.stdlib.objects.get(&id),Some(NativeObject::Map(_)) if is_map)
            && !matches!(self.stdlib.objects.get(&id),Some(NativeObject::Set(_)) if !is_map)
        {
            return Err(self.property_type_error("Incompatible collection receiver"));
        }
        let key = canonical(args.first().cloned().unwrap_or(Value::Undefined));
        let method = name.split_once('.').unwrap().1;
        if matches!(method, "keys" | "values" | "entries") {
            return self.stdlib_object(
                "Iterator",
                NativeObject::Iterator {
                    target: this.clone(),
                    index: 0,
                    mode: match method {
                        "keys" => 0,
                        "values" => 1,
                        _ => 2,
                    },
                    done: false,
                },
            );
        }
        if method == "forEach" {
            let callback = args.first().cloned().unwrap_or(Value::Undefined);
            if !self.is_callable_value(&callback, host) {
                return Err(self.property_type_error("Collection callback is not callable"));
            }
            let receiver = args.get(1).cloned().unwrap_or(Value::Undefined);
            let mut index = 0;
            loop {
                self.tick()?;
                let entry = match self.stdlib.objects.get(&id) {
                    Some(NativeObject::Map(v)) => v
                        .get(index)
                        .map(|v| v.as_ref().map(|(k, v)| (k.clone(), v.clone()))),
                    Some(NativeObject::Set(v)) => v
                        .get(index)
                        .map(|v| v.as_ref().map(|v| (v.clone(), v.clone()))),
                    _ => unreachable!(),
                };
                let Some(entry) = entry else {
                    break;
                };
                index += 1;
                if let Some((key, value)) = entry {
                    self.call(
                        callback.clone(),
                        vec![value, key, this.clone()],
                        receiver.clone(),
                        host,
                    )?;
                }
            }
            return Ok(Value::Undefined);
        }
        let mut found = None;
        let mut length = 0;
        match &self.stdlib.objects[&id] {
            NativeObject::Map(entries) => {
                for (index, entry) in entries.iter().enumerate() {
                    if entry.as_ref().is_some_and(|(k, _)| same(k, &key)) {
                        found = Some(index);
                        break;
                    }
                }
                length = entries.len();
            }
            NativeObject::Set(entries) => {
                for (index, entry) in entries.iter().enumerate() {
                    if entry.as_ref().is_some_and(|v| same(v, &key)) {
                        found = Some(index);
                        break;
                    }
                }
                length = entries.len();
            }
            _ => {}
        }
        self.remaining = self
            .remaining
            .checked_sub(length as u64)
            .ok_or_else(|| JsError::resource("Collection search work exceeded"))?;
        match method {
            "has" => Ok(Value::Bool(found.is_some())),
            "get" => Ok(found
                .and_then(|index| {
                    if let NativeObject::Map(entries) = &self.stdlib.objects[&id] {
                        entries[index].as_ref().map(|(_, v)| v.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or(Value::Undefined)),
            "set" | "add" => {
                if found.is_none() {
                    if length >= MAX_ITEMS {
                        return Err(JsError::resource("Collection storage limit exceeded"));
                    }
                    self.charge_array_slots(if is_map { 2 } else { 1 })?;
                }
                match self.stdlib.objects.get_mut(&id).unwrap() {
                    NativeObject::Map(v) => {
                        let pair = (key, args.get(1).cloned().unwrap_or(Value::Undefined));
                        if let Some(index) = found {
                            v[index] = Some(pair);
                        } else {
                            v.push(Some(pair));
                        }
                    }
                    NativeObject::Set(v) => {
                        if found.is_none() {
                            v.push(Some(key));
                        }
                    }
                    _ => unreachable!(),
                }
                Ok(this.clone())
            }
            "delete" => {
                if let Some(index) = found {
                    match self.stdlib.objects.get_mut(&id).unwrap() {
                        NativeObject::Map(v) => v[index] = None,
                        NativeObject::Set(v) => v[index] = None,
                        _ => unreachable!(),
                    }
                }
                Ok(Value::Bool(found.is_some()))
            }
            "clear" => {
                match self.stdlib.objects.get_mut(&id).unwrap() {
                    NativeObject::Map(v) => v.iter_mut().for_each(|v| *v = None),
                    NativeObject::Set(v) => v.iter_mut().for_each(|v| *v = None),
                    _ => unreachable!(),
                }
                Ok(Value::Undefined)
            }
            _ => Err(JsError::runtime("Unknown collection method")),
        }
    }
    fn native_iterator_next(&mut self, this: &Value) -> Result<Value, JsError> {
        let id = object_id(this)
            .ok_or_else(|| self.property_type_error("Iterator receiver required"))?;
        let Some(NativeObject::Iterator {
            target,
            mut index,
            mode,
            done,
        }) = self.stdlib.objects.get(&id).cloned()
        else {
            return Err(self.property_type_error("Iterator receiver required"));
        };
        let mut value = None;
        if !done && let Some(target_id) = object_id(&target) {
            loop {
                self.tick()?;
                let view = match self.stdlib.objects.get(&target_id) {
                    Some(NativeObject::View {
                        buffer,
                        offset,
                        length,
                        kind: Some(kind),
                    }) => Some((*buffer, *offset, *length, *kind)),
                    _ => None,
                };
                let entry = if let Some((buffer, offset, length, kind)) = view {
                    if index < length {
                        let key = Value::Number(index as f64);
                        let element = if mode == 0 {
                            Value::Undefined
                        } else {
                            self.read_element(buffer, offset + index * kind.size(), kind, true)?
                        };
                        Some(Some((key, element)))
                    } else {
                        None
                    }
                } else {
                    match self.stdlib.objects.get(&target_id) {
                        Some(NativeObject::Map(entries)) => entries
                            .get(index)
                            .map(|entry| entry.as_ref().map(|(k, v)| (k.clone(), v.clone()))),
                        Some(NativeObject::Set(entries)) => entries
                            .get(index)
                            .map(|entry| entry.as_ref().map(|v| (v.clone(), v.clone()))),
                        _ => {
                            return Err(self.property_type_error("Invalid native iterator target"));
                        }
                    }
                };
                let Some(entry) = entry else {
                    break;
                };
                index += 1;
                if let Some((key, entry)) = entry {
                    value = Some(match mode {
                        0 => key,
                        1 => entry,
                        _ => Value::Array(self.allocate(Heap::Array(vec![key, entry]))?),
                    });
                    break;
                }
            }
        }
        let finished = value.is_none();
        if let Some(NativeObject::Iterator {
            index: current,
            done,
            ..
        }) = self.stdlib.objects.get_mut(&id)
        {
            *current = index;
            *done = finished;
        }
        self.object(
            BTreeMap::from([
                ("value".into(), value.unwrap_or(Value::Undefined)),
                ("done".into(), Value::Bool(finished)),
            ]),
            None,
        )
    }

    pub(super) fn stdlib_values(&mut self, value: &Value) -> Result<Option<Vec<Value>>, JsError> {
        let Some(id) = object_id(value) else {
            return Ok(None);
        };
        let Some(brand) = self.stdlib.objects.get(&id).cloned() else {
            return Ok(None);
        };
        let result = match brand {
            NativeObject::Set(entries) => entries.into_iter().flatten().collect(),
            NativeObject::Map(entries) => {
                let mut values = Vec::new();
                for (key, value) in entries.into_iter().flatten() {
                    self.tick()?;
                    values.push(Value::Array(self.allocate(Heap::Array(vec![key, value]))?));
                }
                values
            }
            NativeObject::View {
                buffer,
                offset,
                length,
                kind: Some(kind),
            } => {
                let mut values = Vec::with_capacity(length);
                for index in 0..length {
                    self.tick()?;
                    values.push(self.read_element(
                        buffer,
                        offset + index * kind.size(),
                        kind,
                        true,
                    )?);
                }
                values
            }
            _ => return Ok(None),
        };
        Ok(Some(result))
    }

    fn buffer_length(&self, id: usize) -> Result<usize, JsError> {
        if let Some(NativeObject::Buffer(bytes)) = self.stdlib.objects.get(&id) {
            Ok(bytes.len())
        } else {
            Err(JsError::runtime("Missing ArrayBuffer backing storage"))
        }
    }
    fn read_element(
        &mut self,
        buffer: usize,
        offset: usize,
        kind: ElementKind,
        little: bool,
    ) -> Result<Value, JsError> {
        let Some(NativeObject::Buffer(bytes)) = self.stdlib.objects.get(&buffer) else {
            return Err(JsError::runtime("Invalid ArrayBuffer backing storage"));
        };
        let data = bytes
            .get(offset..offset.saturating_add(kind.size()))
            .ok_or_else(|| JsError::runtime("Buffer read is outside its range"))?;
        let mut raw = [0u8; 8];
        raw[..data.len()].copy_from_slice(data);
        if !little {
            raw[..data.len()].reverse();
        }
        if kind.is_bigint() {
            self.tick()?;
            self.charge_string(8)?;
            return Ok(Value::BigInt(Arc::new(crate::BigInt::from_64_le(
                raw,
                matches!(kind, ElementKind::BigI64),
            ))));
        }
        Ok(Value::Number(match kind {
            ElementKind::I8 => i8::from_le_bytes([raw[0]]) as f64,
            ElementKind::U8 | ElementKind::U8Clamped => raw[0] as f64,
            ElementKind::I16 => i16::from_le_bytes(raw[..2].try_into().unwrap()) as f64,
            ElementKind::U16 => u16::from_le_bytes(raw[..2].try_into().unwrap()) as f64,
            ElementKind::I32 => i32::from_le_bytes(raw[..4].try_into().unwrap()) as f64,
            ElementKind::U32 => u32::from_le_bytes(raw[..4].try_into().unwrap()) as f64,
            ElementKind::F32 => f32::from_le_bytes(raw[..4].try_into().unwrap()) as f64,
            ElementKind::F64 => f64::from_le_bytes(raw),
            ElementKind::BigI64 | ElementKind::BigU64 => unreachable!(),
        }))
    }
    fn element_value(
        &mut self,
        kind: ElementKind,
        value: &Value,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        if kind.is_bigint() {
            let primitive =
                self.operator_primitive(value.clone(), crate::coercion::Hint::Number, host)?;
            // ToBigInt rejects Number; storage truncates modulo 2^64 only after
            // the full primitive conversion and validation have succeeded.
            self.bigint_as_uint_n(Value::Number(64.), primitive)
        } else {
            self.operator_number(value.clone(), host)
        }
    }
    fn write_element(
        &mut self,
        buffer: usize,
        offset: usize,
        kind: ElementKind,
        value: &Value,
        little: bool,
    ) -> Result<(), JsError> {
        let mut raw = if kind.is_bigint() {
            let Value::BigInt(number) = value else {
                return Err(self.property_type_error("BigInt typed array requires BigInt values"));
            };
            number.low_64_le().to_vec()
        } else {
            let Value::Number(number) = value else {
                return Err(
                    self.property_type_error("Numeric typed array cannot store BigInt values")
                );
            };
            let integer = if !number.is_finite() || *number == 0. {
                0.
            } else {
                number.trunc().rem_euclid(4_294_967_296.)
            } as u32;
            match kind {
                ElementKind::I8 | ElementKind::U8 => vec![integer as u8],
                ElementKind::U8Clamped => vec![if number.is_nan() {
                    0
                } else {
                    number.clamp(0., 255.).round_ties_even() as u8
                }],
                ElementKind::I16 | ElementKind::U16 => (integer as u16).to_le_bytes().to_vec(),
                ElementKind::I32 | ElementKind::U32 => integer.to_le_bytes().to_vec(),
                ElementKind::F32 => (*number as f32).to_le_bytes().to_vec(),
                ElementKind::F64 => number.to_le_bytes().to_vec(),
                ElementKind::BigI64 | ElementKind::BigU64 => unreachable!(),
            }
        };
        if !little {
            raw.reverse();
        }
        let Some(NativeObject::Buffer(bytes)) = self.stdlib.objects.get_mut(&buffer) else {
            return Err(JsError::runtime("Invalid ArrayBuffer backing storage"));
        };
        let destination = bytes
            .get_mut(offset..offset.saturating_add(raw.len()))
            .ok_or_else(|| JsError::runtime("Buffer write is outside its range"))?;
        destination.copy_from_slice(&raw);
        Ok(())
    }
    fn require_typed_content_type(
        &mut self,
        target: ElementKind,
        source: &Value,
    ) -> Result<(), JsError> {
        if let Some(NativeObject::View {
            kind: Some(source_kind),
            ..
        }) = object_id(source).and_then(|id| self.stdlib.objects.get(&id))
            && target.is_bigint() != source_kind.is_bigint()
        {
            return Err(self.property_type_error("Cannot mix numeric and BigInt typed arrays"));
        }
        Ok(())
    }
    fn view_index(
        &mut self,
        value: Option<&Value>,
        limit: usize,
        host: &mut impl Host,
    ) -> Result<usize, JsError> {
        let Value::Number(number) =
            self.operator_number(value.cloned().unwrap_or(Value::Undefined), host)?
        else {
            unreachable!()
        };
        let index = if number.is_nan() { 0. } else { number.trunc() };
        if !index.is_finite() || index < 0. || index > (limit as f64).min(9_007_199_254_740_991.) {
            return Err(self.bigint_error("RangeError", "Buffer/view index is outside its range"));
        }
        Ok(index as usize)
    }
    fn new_typed_array(
        &mut self,
        kind: ElementKind,
        args: &[Value],
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let first = args.first().cloned().unwrap_or(Value::Undefined);
        if let Some(buffer) = object_id(&first)
            .filter(|id| matches!(self.stdlib.objects.get(id), Some(NativeObject::Buffer(_))))
        {
            let size = self.buffer_length(buffer)?;
            let offset = self.view_index(args.get(1), usize::MAX, host)?;
            if offset % kind.size() != 0 {
                return Err(
                    self.bigint_error("RangeError", "Typed array byteOffset is not aligned")
                );
            }
            let length =
                if args.get(2).is_none_or(|v| matches!(v, Value::Undefined)) {
                    if offset > size {
                        return Err(self
                            .bigint_error("RangeError", "Typed array byteOffset exceeds buffer"));
                    }
                    let available = size - offset;
                    if available % kind.size() != 0 {
                        return Err(self
                            .bigint_error("RangeError", "Typed array byteLength is not aligned"));
                    }
                    available / kind.size()
                } else {
                    // Convert explicit length before checking capacity, even when offset is past the buffer.
                    let length = self.view_index(args.get(2), usize::MAX, host)?;
                    if offset > size || length > (size - offset) / kind.size() {
                        return Err(
                            self.bigint_error("RangeError", "Typed array length exceeds buffer")
                        );
                    }
                    length
                };
            if length > MAX_ITEMS {
                return Err(JsError::resource("Typed array element count exceeded"));
            }
            return self.stdlib_object(
                kind.name(),
                NativeObject::View {
                    buffer,
                    offset,
                    length,
                    kind: Some(kind),
                },
            );
        }
        self.require_typed_content_type(kind, &first)?;
        let object_source = matches!(
            first,
            Value::Array(_)
                | Value::Object(_)
                | Value::Function(_)
                | Value::Host(_)
                | Value::Native(_)
        );
        let typed_source = object_id(&first).is_some_and(|id| {
            matches!(
                self.stdlib.objects.get(&id),
                Some(NativeObject::View { kind: Some(_), .. })
            )
        });
        let values = if typed_source {
            self.stdlib_values(&first)?
        } else if object_source {
            self.typed_iterable_values(&first, host)?
        } else {
            None
        };
        let length = if let Some(values) = &values {
            values.len()
        } else if object_source {
            self.stdlib_arraylike_length(&first, host)?
        } else {
            self.view_index(Some(&first), MAX_ITEMS, host)?
        };
        let buffer = self.stdlib_buffer(length * kind.size())?;
        let Value::Object(buffer) = buffer else {
            unreachable!()
        };
        if object_source {
            for index in 0..length {
                self.tick()?;
                let value = if let Some(values) = &values {
                    values[index].clone()
                } else {
                    self.get(first.clone(), &index.to_string(), host)?
                };
                let number = self.element_value(kind, &value, host)?;
                self.write_element(buffer, index * kind.size(), kind, &number, true)?;
            }
        }
        self.stdlib_object(
            kind.name(),
            NativeObject::View {
                buffer,
                offset: 0,
                length,
                kind: Some(kind),
            },
        )
    }
    fn buffer_slice(
        &mut self,
        this: &Value,
        args: &[Value],
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let id = object_id(this)
            .ok_or_else(|| self.property_type_error("ArrayBuffer receiver required"))?;
        let length = self.buffer_length(id)?;
        let start = relative_index(args.first(), length, self, host)?;
        let end = if args.get(1).is_none_or(|v| matches!(v, Value::Undefined)) {
            length
        } else {
            relative_index(args.get(1), length, self, host)?
        };
        let size = end.saturating_sub(start);
        let result = self.stdlib_buffer(size)?;
        let Value::Object(next) = result else {
            unreachable!()
        };
        let Some(NativeObject::Buffer(source)) = self.stdlib.objects.get(&id) else {
            unreachable!()
        };
        let copy = source[start..start + size].to_vec();
        if let Some(NativeObject::Buffer(dest)) = self.stdlib.objects.get_mut(&next) {
            dest.copy_from_slice(&copy);
        }
        Ok(Value::Object(next))
    }
    fn typed_method(
        &mut self,
        name: &str,
        this: &Value,
        args: &[Value],
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let id = object_id(this)
            .ok_or_else(|| self.property_type_error("Typed array receiver required"))?;
        let Some(NativeObject::View {
            buffer,
            offset,
            length,
            kind: Some(kind),
        }) = self.stdlib.objects.get(&id).cloned()
        else {
            return Err(self.property_type_error("Typed array receiver required"));
        };
        let method = name.split_once('.').unwrap().1;
        match method {
            "values" | "keys" | "entries" => self.stdlib_object(
                "Iterator",
                NativeObject::Iterator {
                    target: this.clone(),
                    index: 0,
                    mode: match method {
                        "keys" => 0,
                        "values" => 1,
                        _ => 2,
                    },
                    done: false,
                },
            ),
            "at" => {
                let Value::Number(n) =
                    self.operator_number(args.first().cloned().unwrap_or(Value::Undefined), host)?
                else {
                    unreachable!()
                };
                let n = if n.is_nan() { 0. } else { n.trunc() };
                let index = if n < 0. { length as f64 + n } else { n };
                Ok(
                    if index.is_finite() && index >= 0. && index < length as f64 {
                        self.read_element(
                            buffer,
                            offset + index as usize * kind.size(),
                            kind,
                            true,
                        )?
                    } else {
                        Value::Undefined
                    },
                )
            }
            "subarray" | "slice" => {
                let start = relative_index(args.first(), length, self, host)?;
                let end = if args.get(1).is_none_or(|v| matches!(v, Value::Undefined)) {
                    length
                } else {
                    relative_index(args.get(1), length, self, host)?
                };
                let count = end.saturating_sub(start);
                if method == "subarray" {
                    self.stdlib_object(
                        kind.name(),
                        NativeObject::View {
                            buffer,
                            offset: offset + start * kind.size(),
                            length: count,
                            kind: Some(kind),
                        },
                    )
                } else {
                    // Copy storage directly: no temporary JS Array or observable iterator.
                    // This also preserves floating-point NaN payload bits.
                    let Value::Object(next) = self.stdlib_buffer(count * kind.size())? else {
                        unreachable!()
                    };
                    for index in 0..count {
                        self.tick()?;
                        let begin = offset + (start + index) * kind.size();
                        let mut raw = [0u8; 8];
                        let Some(NativeObject::Buffer(source)) = self.stdlib.objects.get(&buffer)
                        else {
                            unreachable!()
                        };
                        raw[..kind.size()].copy_from_slice(&source[begin..begin + kind.size()]);
                        let Some(NativeObject::Buffer(dest)) = self.stdlib.objects.get_mut(&next)
                        else {
                            unreachable!()
                        };
                        let begin = index * kind.size();
                        dest[begin..begin + kind.size()].copy_from_slice(&raw[..kind.size()]);
                    }
                    self.stdlib_object(
                        kind.name(),
                        NativeObject::View {
                            buffer: next,
                            offset: 0,
                            length: count,
                            kind: Some(kind),
                        },
                    )
                }
            }
            "set" => {
                // ToIntegerOrInfinity rejects negative offsets before source access;
                // positive overflow (including Infinity) is checked after source.length.
                let Value::Number(number) =
                    self.operator_number(args.get(1).cloned().unwrap_or(Value::Undefined), host)?
                else {
                    unreachable!()
                };
                let start = if number.is_nan() { 0. } else { number.trunc() };
                if start < 0. {
                    return Err(
                        self.bigint_error("RangeError", "TypedArray.set offset is negative")
                    );
                }
                let input = args.first().unwrap_or(&Value::Undefined);
                self.require_typed_content_type(kind, input)?;
                let typed_source = object_id(input).is_some_and(|id| {
                    matches!(
                        self.stdlib.objects.get(&id),
                        Some(NativeObject::View { kind: Some(_), .. })
                    )
                });
                let values = if typed_source {
                    self.stdlib_values(input)?
                } else {
                    None
                };
                let count = if let Some(values) = &values {
                    values.len()
                } else {
                    self.stdlib_arraylike_length(input, host)?
                };
                if start > length as f64 || count > length - start as usize {
                    return Err(
                        self.bigint_error("RangeError", "TypedArray.set input exceeds destination")
                    );
                }
                let start = start as usize;
                for index in 0..count {
                    self.tick()?;
                    let value = if let Some(values) = &values {
                        values[index].clone()
                    } else {
                        self.get(input.clone(), &index.to_string(), host)?
                    };
                    let number = self.element_value(kind, &value, host)?;
                    self.write_element(
                        buffer,
                        offset + (start + index) * kind.size(),
                        kind,
                        &number,
                        true,
                    )?;
                }
                Ok(Value::Undefined)
            }
            "fill" => {
                let number =
                    self.element_value(kind, args.first().unwrap_or(&Value::Undefined), host)?;
                let start = relative_index(args.get(1), length, self, host)?;
                let end = if args.get(2).is_none_or(|v| matches!(v, Value::Undefined)) {
                    length
                } else {
                    relative_index(args.get(2), length, self, host)?
                };
                for index in start..end {
                    self.tick()?;
                    self.write_element(buffer, offset + index * kind.size(), kind, &number, true)?;
                }
                Ok(this.clone())
            }
            "reverse" => {
                for index in 0..length / 2 {
                    self.tick()?;
                    let left =
                        self.read_element(buffer, offset + index * kind.size(), kind, true)?;
                    let right = self.read_element(
                        buffer,
                        offset + (length - 1 - index) * kind.size(),
                        kind,
                        true,
                    )?;
                    self.write_element(buffer, offset + index * kind.size(), kind, &right, true)?;
                    self.write_element(
                        buffer,
                        offset + (length - 1 - index) * kind.size(),
                        kind,
                        &left,
                        true,
                    )?;
                }
                Ok(this.clone())
            }
            "includes" | "indexOf" => {
                let sought = args.first().unwrap_or(&Value::Undefined);
                if length == 0 {
                    return Ok(if method == "includes" {
                        Value::Bool(false)
                    } else {
                        Value::Number(-1.)
                    });
                }
                let start = relative_index(args.get(1), length, self, host)?;
                for index in start..length {
                    self.tick()?;
                    let value =
                        self.read_element(buffer, offset + index * kind.size(), kind, true)?;
                    let equal = if method == "includes" {
                        same(&value, sought)
                    } else {
                        value == *sought
                    };
                    if equal {
                        return Ok(if method == "includes" {
                            Value::Bool(true)
                        } else {
                            Value::Number(index as f64)
                        });
                    }
                }
                Ok(if method == "includes" {
                    Value::Bool(false)
                } else {
                    Value::Number(-1.)
                })
            }
            "join" => {
                let separator = if args.first().is_none_or(|v| matches!(v, Value::Undefined)) {
                    vec![0x2c]
                } else {
                    let primitive = self.operator_primitive(
                        args[0].clone(),
                        crate::coercion::Hint::String,
                        host,
                    )?;
                    self.string_units_coerce(&primitive)?
                };
                let mut text = Vec::new();
                for index in 0..length {
                    self.tick()?;
                    if index > 0 {
                        if text.len() + separator.len() > 65536 {
                            return Err(JsError::resource("Typed array join exceeds limit"));
                        }
                        text.extend_from_slice(&separator);
                    }
                    let value =
                        self.read_element(buffer, offset + index * kind.size(), kind, true)?;
                    let units = self.string_units_coerce(&value)?;
                    if text.len() + units.len() > 65536 {
                        return Err(JsError::resource("Typed array join exceeds limit"));
                    }
                    text.extend_from_slice(&units);
                }
                self.utf16_string(text)
            }
            "forEach" | "map" => {
                let callback = args.first().cloned().unwrap_or(Value::Undefined);
                if !self.is_callable_value(&callback, host) {
                    return Err(self.property_type_error("Typed array callback is not callable"));
                }
                let receiver = args.get(1).cloned().unwrap_or(Value::Undefined);
                let destination = if method == "map" {
                    Some(self.new_typed_array(kind, &[Value::Number(length as f64)], host)?)
                } else {
                    None
                };
                for index in 0..length {
                    self.tick()?;
                    let value =
                        self.read_element(buffer, offset + index * kind.size(), kind, true)?;
                    let result = self.call(
                        callback.clone(),
                        vec![value, Value::Number(index as f64), this.clone()],
                        receiver.clone(),
                        host,
                    )?;
                    if let Some(destination) = &destination {
                        self.stdlib_set(destination, &index.to_string(), &result, host)?;
                    }
                }
                Ok(destination.unwrap_or(Value::Undefined))
            }
            _ => Err(JsError::runtime("Unknown typed array method")),
        }
    }
    fn data_view_method(
        &mut self,
        name: &str,
        this: &Value,
        args: &[Value],
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let id = object_id(this)
            .ok_or_else(|| self.property_type_error("DataView receiver required"))?;
        let Some(NativeObject::View {
            buffer,
            offset,
            length,
            kind: None,
        }) = self.stdlib.objects.get(&id).cloned()
        else {
            return Err(self.property_type_error("DataView receiver required"));
        };
        let method = name.split_once('.').unwrap().1;
        let write = method.starts_with("set");
        let kind = match &method[3..] {
            "Int8" => ElementKind::I8,
            "Uint8" => ElementKind::U8,
            "Int16" => ElementKind::I16,
            "Uint16" => ElementKind::U16,
            "Int32" => ElementKind::I32,
            "Uint32" => ElementKind::U32,
            "Float32" => ElementKind::F32,
            "Float64" => ElementKind::F64,
            "BigInt64" => ElementKind::BigI64,
            "BigUint64" => ElementKind::BigU64,
            _ => return Err(JsError::runtime("Unknown DataView method")),
        };
        let index = self.view_index(args.first(), usize::MAX, host)?;
        let converted = if write {
            Some(self.element_value(kind, args.get(1).unwrap_or(&Value::Undefined), host)?)
        } else {
            None
        };
        if index.saturating_add(kind.size()) > length {
            return Err(self.bigint_error("RangeError", "DataView access is outside its range"));
        }
        let little = args
            .get(if write { 2 } else { 1 })
            .is_some_and(Value::truthy);
        if write {
            let number = converted.unwrap();
            self.write_element(buffer, offset + index, kind, &number, little)?;
            Ok(Value::Undefined)
        } else {
            Ok(self.read_element(buffer, offset + index, kind, little)?)
        }
    }

    fn date_utc(&mut self, args: &[Value]) -> Result<f64, JsError> {
        if args.is_empty() {
            return Ok(f64::NAN);
        }
        let mut values = [0., 0., 1., 0., 0., 0., 0.];
        for (index, value) in args.iter().take(7).enumerate() {
            values[index] = self.stdlib_number(value)?;
        }
        if values.iter().any(|value| !value.is_finite()) {
            return Ok(f64::NAN);
        }
        if values[0].trunc() >= 0. && values[0].trunc() <= 99. {
            values[0] = values[0].trunc() + 1900.;
        }
        Ok(make_date(values))
    }
    fn date_method(&mut self, name: &str, this: &Value, args: &[Value]) -> Result<Value, JsError> {
        let id =
            object_id(this).ok_or_else(|| self.property_type_error("Date receiver required"))?;
        let Some(NativeObject::Date(ms)) = self.stdlib.objects.get(&id).cloned() else {
            return Err(self.property_type_error("Date receiver required"));
        };
        let method = &name[5..];
        if method == "getTime" {
            return Ok(Value::Number(ms));
        }
        if method == "setTime" {
            let next = time_clip(self.stdlib_number(args.first().unwrap_or(&Value::Undefined))?);
            self.stdlib.objects.insert(id, NativeObject::Date(next));
            return Ok(Value::Number(next));
        }
        if method == "toJSON" && !ms.is_finite() {
            return Ok(Value::Null);
        }
        if matches!(method, "toISOString" | "toJSON") {
            if !ms.is_finite() {
                return Err(JsError::runtime("Invalid Date cannot be formatted as ISO"));
            }
            return self.string(iso_string(ms));
        }
        if method == "toUTCString" {
            if !ms.is_finite() {
                return self.string("Invalid Date".into());
            }
            let c = date_parts(ms);
            return self.string(format!(
                "{}, {:02} {} {} {:02}:{:02}:{:02} GMT",
                ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][weekday(ms)],
                c[2] as i64,
                [
                    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov",
                    "Dec"
                ][c[1] as usize],
                format_year(c[0] as i64),
                c[3] as i64,
                c[4] as i64,
                c[5] as i64
            ));
        }
        if method.starts_with("getUTC") {
            if !ms.is_finite() {
                return Ok(Value::Number(f64::NAN));
            }
            let c = date_parts(ms);
            return Ok(Value::Number(match method {
                "getUTCFullYear" => c[0],
                "getUTCMonth" => c[1],
                "getUTCDate" => c[2],
                "getUTCDay" => weekday(ms) as f64,
                "getUTCHours" => c[3],
                "getUTCMinutes" => c[4],
                "getUTCSeconds" => c[5],
                "getUTCMilliseconds" => c[6],
                _ => return Err(JsError::runtime("Unknown Date getter")),
            }));
        }
        if method.starts_with("setUTC") {
            let mut c = if ms.is_finite() {
                date_parts(ms)
            } else if method == "setUTCFullYear" {
                date_parts(0.)
            } else {
                return Ok(Value::Number(f64::NAN));
            };
            let start = match method {
                "setUTCFullYear" => 0,
                "setUTCMonth" => 1,
                "setUTCDate" => 2,
                "setUTCHours" => 3,
                "setUTCMinutes" => 4,
                "setUTCSeconds" => 5,
                "setUTCMilliseconds" => 6,
                _ => return Err(JsError::runtime("Unknown Date setter")),
            };
            let count = match start {
                0 => 3,
                1 => 2,
                2 => 1,
                3 => 4,
                4 => 3,
                5 => 2,
                _ => 1,
            };
            c[start] = self.stdlib_number(args.first().unwrap_or(&Value::Undefined))?;
            for (index, value) in args.iter().take(count).enumerate().skip(1) {
                c[start + index] = self.stdlib_number(value)?;
            }
            let next = make_date(c);
            self.stdlib.objects.insert(id, NativeObject::Date(next));
            return Ok(Value::Number(next));
        }
        Err(JsError::runtime(
            "Date local-time operation is not implemented",
        ))
    }

    pub(super) fn stdlib_cached_roots(&self) -> Vec<Value> {
        self.stdlib.prototypes.values().cloned().collect()
    }
    fn stdlib_arraylike_length(
        &mut self,
        value: &Value,
        host: &mut impl Host,
    ) -> Result<usize, JsError> {
        if matches!(value, Value::Null | Value::Undefined) {
            return Err(self.property_type_error("Array-like value cannot be null or undefined"));
        }
        if matches!(
            value,
            Value::Bool(_) | Value::Number(_) | Value::BigInt(_) | Value::Symbol(..)
        ) {
            return Ok(0);
        }
        if matches!(value, Value::Host(_) | Value::Native(_)) {
            return Err(
                self.property_type_error("Host/native array-like coercion is not implemented")
            );
        }
        let length = self.get(value.clone(), "length", host)?;
        let Value::Number(number) = self.operator_number(length, host)? else {
            unreachable!()
        };
        if number.is_nan() || number <= 0. {
            return Ok(0);
        }
        if !number.is_finite() || number > MAX_ITEMS as f64 {
            return Err(JsError::resource("Native array-like length exceeded"));
        }
        Ok(number.trunc() as usize)
    }
    fn typed_iterable_values(
        &mut self,
        source: &Value,
        host: &mut impl Host,
    ) -> Result<Option<Vec<Value>>, JsError> {
        use crate::language::IteratorRecord;
        let method = if matches!(
            source,
            Value::Object(_) | Value::Array(_) | Value::Function(_)
        ) {
            let Value::Symbol(key, _) = self.well_known_symbol("iterator")? else {
                unreachable!()
            };
            self.get_symbol(source.clone(), key, host)?
        } else {
            Value::Undefined
        };
        let mut iterator = if !matches!(method, Value::Undefined | Value::Null) {
            if !self.is_callable_value(&method, host) {
                return Err(self.property_type_error("Iterator method must be callable"));
            }
            let object = self.call(method, vec![], source.clone(), host)?;
            if !matches!(
                object,
                Value::Object(_) | Value::Array(_) | Value::Function(_) | Value::Host(_)
            ) {
                return Err(self.property_type_error("Iterator must be an object"));
            }
            let next = self.get(object.clone(), "next", host)?;
            if !self.is_callable_value(&next, host) {
                return Err(self.property_type_error("Iterator.next must be callable"));
            }
            IteratorRecord::Custom {
                object,
                next,
                done: false,
            }
        } else {
            match source {
                // Mirrors the existing VM's built-in array/string iterators.
                Value::Array(id) => IteratorRecord::Array(*id, 0),
                Value::String(_) | Value::Utf16(_) => {
                    IteratorRecord::String(crate::strings::units(source).unwrap(), 0)
                }
                _ => return Ok(None),
            }
        };
        let mut values = Vec::new();
        while let Some(value) = self.iterator_next(&mut iterator, host)? {
            if values.len() >= MAX_ITEMS {
                return Err(JsError::resource(
                    "Typed array iterable item limit exceeded",
                ));
            }
            values.push(value);
        }
        Ok(Some(values))
    }
    pub(super) fn stdlib_own_keys(&self, value: &Value) -> Option<Vec<String>> {
        let id = object_id(value)?;
        match self.stdlib.objects.get(&id)? {
            NativeObject::View {
                length,
                kind: Some(_),
                ..
            } => Some((0..*length).map(|index| index.to_string()).collect()),
            _ => None,
        }
    }
    pub(super) fn stdlib_own_index(&self, value: &Value, property: &str) -> Option<bool> {
        let id = object_id(value)?;
        if matches!(
            self.stdlib.objects.get(&id),
            Some(NativeObject::View { kind: Some(_), .. })
        ) && invalid_numeric_property(property)
        {
            return Some(false);
        }
        match self.stdlib.objects.get(&id)? {
            NativeObject::View {
                length,
                kind: Some(_),
                ..
            } => canonical_index(property).map(|index| index < *length),
            _ => None,
        }
    }
    pub(super) fn stdlib_define_index(
        &mut self,
        value: &Value,
        property: &str,
        definition: &Definition,
        host: &mut impl Host,
    ) -> Result<Option<bool>, JsError> {
        let Some(present) = self.stdlib_own_index(value, property) else {
            return Ok(None);
        };
        if !present
            || definition.getter.is_some()
            || definition.setter.is_some()
            || definition.writable == Some(false)
            || definition.enumerable == Some(false)
            || definition.configurable == Some(false)
        {
            return Ok(Some(false));
        }
        if let Some(new) = definition.value.as_ref() {
            self.stdlib_set(value, property, new, host)?;
        }
        Ok(Some(true))
    }
    pub(super) fn stdlib_trace(&self, id: usize) -> Vec<Value> {
        match self.stdlib.objects.get(&id) {
            Some(NativeObject::Map(values)) => values
                .iter()
                .flatten()
                .flat_map(|(k, v)| [k.clone(), v.clone()])
                .collect(),
            Some(NativeObject::Set(values)) => values.iter().flatten().cloned().collect(),
            Some(NativeObject::View { buffer, .. }) => vec![Value::Object(*buffer)],
            Some(NativeObject::Iterator { target, .. }) => vec![target.clone()],
            _ => Vec::new(),
        }
    }
    pub(super) fn stdlib_sweep(&mut self, live: &BTreeSet<usize>) {
        self.stdlib.objects.retain(|id, _| live.contains(id));
        self.stdlib.buffer_bytes = self
            .stdlib
            .objects
            .values()
            .filter_map(|object| {
                if let NativeObject::Buffer(bytes) = object {
                    Some(bytes.len())
                } else {
                    None
                }
            })
            .sum();
    }
}

fn canonical_index(property: &str) -> Option<usize> {
    let index = property.parse::<usize>().ok()?;
    (index.to_string() == property).then_some(index)
}
fn invalid_numeric_property(property: &str) -> bool {
    if canonical_index(property).is_some() {
        return false;
    }
    if matches!(property, "-0" | "NaN" | "Infinity" | "-Infinity") {
        return true;
    }
    property.parse::<f64>().ok().is_some_and(|number| {
        Value::Number(number)
            .primitive_text()
            .is_ok_and(|text| text == property)
    })
}
fn relative_index(
    value: Option<&Value>,
    length: usize,
    runtime: &mut Runtime,
    host: &mut impl Host,
) -> Result<usize, JsError> {
    let Value::Number(value) =
        runtime.operator_number(value.cloned().unwrap_or(Value::Undefined), host)?
    else {
        unreachable!()
    };
    Ok(if value.is_nan() {
        0
    } else if value < 0. {
        (length as f64 + value.trunc()).clamp(0., length as f64) as usize
    } else {
        value.trunc().min(length as f64) as usize
    })
}

// Gregorian calendar conversion, using integer eras and Euclidean division.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let mp = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468
}
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    (y + i64::from(m <= 2), m, d)
}
fn make_date(c: [f64; 7]) -> f64 {
    if c.iter().any(|v| !v.is_finite() || v.abs() > 1e10) {
        return f64::NAN;
    }
    let year = c[0].trunc() as i64;
    let month = c[1].trunc() as i64;
    let year = year + month.div_euclid(12);
    let month = month.rem_euclid(12) + 1;
    time_clip(
        days_from_civil(year, month, 1) as f64 * 86_400_000.
            + (c[2].trunc() - 1.) * 86_400_000.
            + c[3].trunc() * 3_600_000.
            + c[4].trunc() * 60_000.
            + c[5].trunc() * 1000.
            + c[6].trunc(),
    )
}
fn date_parts(ms: f64) -> [f64; 7] {
    let ms = ms as i64;
    let days = ms.div_euclid(86_400_000);
    let time = ms.rem_euclid(86_400_000);
    let (y, m, d) = civil_from_days(days);
    [
        y as f64,
        (m - 1) as f64,
        d as f64,
        (time / 3_600_000) as f64,
        (time / 60_000 % 60) as f64,
        (time / 1000 % 60) as f64,
        (time % 1000) as f64,
    ]
}
fn weekday(ms: f64) -> usize {
    ((ms as i64).div_euclid(86_400_000) + 4).rem_euclid(7) as usize
}
fn format_year(y: i64) -> String {
    if (0..=9999).contains(&y) {
        format!("{y:04}")
    } else {
        format!(
            "{}{number:06}",
            if y < 0 { '-' } else { '+' },
            number = y.abs()
        )
    }
}
fn iso_string(ms: f64) -> String {
    let c = date_parts(ms);
    format!(
        "{}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        format_year(c[0] as i64),
        c[1] as i64 + 1,
        c[2] as i64,
        c[3] as i64,
        c[4] as i64,
        c[5] as i64,
        c[6] as i64
    )
}
fn parse_iso(source: &str) -> f64 {
    // Date-only ISO forms use UTC. Time forms require an explicit Z or offset;
    // local timezone interpretation is intentionally not guessed.
    if source.len() > 64 || !source.is_ascii() {
        return f64::NAN;
    }
    let bytes = source.as_bytes();
    let signed = matches!(bytes.first(), Some(b'+' | b'-'));
    let year_len = if signed { 7 } else { 4 };
    if source.len() < year_len {
        return f64::NAN;
    }
    let Ok(year) = source[..year_len].parse::<i64>() else {
        return f64::NAN;
    };
    if source.starts_with("-000000") {
        return f64::NAN;
    }
    let mut c = [year as f64, 0., 1., 0., 0., 0., 0.];
    let mut pos = year_len;
    fn pair(source: &str, pos: &mut usize, separator: u8) -> Option<i64> {
        let b = source.as_bytes();
        if b.get(*pos) != Some(&separator) {
            return None;
        }
        *pos += 1;
        let end = (*pos).checked_add(2)?;
        let text = source.get(*pos..end)?;
        if !text.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        *pos = end;
        text.parse().ok()
    }
    if pos < source.len() {
        let Some(month) = pair(source, &mut pos, b'-') else {
            return f64::NAN;
        };
        if !(1..=12).contains(&month) {
            return f64::NAN;
        }
        c[1] = (month - 1) as f64;
    }
    if pos < source.len() {
        let Some(day) = pair(source, &mut pos, b'-') else {
            return f64::NAN;
        };
        if !(1..=31).contains(&day) {
            return f64::NAN;
        }
        c[2] = day as f64;
    }
    if pos == source.len() {
        if !valid_iso_day(c) {
            return f64::NAN;
        }
        return make_date(c);
    }
    let Some(hour) = pair(source, &mut pos, b'T') else {
        return f64::NAN;
    };
    let Some(minute) = pair(source, &mut pos, b':') else {
        return f64::NAN;
    };
    c[3] = hour as f64;
    c[4] = minute as f64;
    if bytes.get(pos) == Some(&b':') {
        let Some(second) = pair(source, &mut pos, b':') else {
            return f64::NAN;
        };
        c[5] = second as f64;
    }
    if bytes.get(pos) == Some(&b'.') {
        pos += 1;
        let start = pos;
        while bytes.get(pos).is_some_and(u8::is_ascii_digit) {
            pos += 1;
        }
        if pos - start != 3 {
            return f64::NAN;
        }
        c[6] = source[start..pos].parse::<f64>().unwrap_or(f64::NAN);
    }
    if c[3] > 24.
        || c[4] > 59.
        || c[5] > 59.
        || (c[3] == 24. && (c[4] != 0. || c[5] != 0. || c[6] != 0.))
    {
        return f64::NAN;
    }
    if !valid_iso_day(c) {
        return f64::NAN;
    }
    let offset = if bytes.get(pos) == Some(&b'Z') {
        pos += 1;
        0.
    } else {
        let sign = match bytes.get(pos) {
            Some(b'+') => 1.,
            Some(b'-') => -1.,
            _ => return f64::NAN,
        };
        let separator = bytes[pos];
        let Some(hours) = pair(source, &mut pos, separator) else {
            return f64::NAN;
        };
        let Some(minutes) = pair(source, &mut pos, b':') else {
            return f64::NAN;
        };
        if hours > 23 || minutes > 59 {
            return f64::NAN;
        }
        sign * (hours as f64 * 60. + minutes as f64) * 60_000.
    };
    if pos != source.len() {
        return f64::NAN;
    }
    time_clip(make_date(c) - offset)
}
fn valid_iso_day(c: [f64; 7]) -> bool {
    let year = c[0] as i64;
    let month = c[1] as i64 + 1;
    let next_month = if month == 12 { 1 } else { month + 1 };
    let next_year = year + i64::from(month == 12);
    let days = days_from_civil(next_year, next_month, 1) - days_from_civil(year, month, 1);
    c[2] >= 1. && c[2] <= days as f64
}

#[cfg(test)]
mod bigint_storage_tests {
    use crate::{ErrorKind, Host, JsError, Runtime, Value};
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
    fn run(source: &str) -> Result<Value, JsError> {
        Runtime::default().execute(source, &mut Empty)
    }
    fn yes(source: &str) {
        assert_eq!(run(source).unwrap(), Value::Bool(true), "{source}");
    }
    #[test]
    fn async_iterator_symbol_has_a_stable_distinct_identity() {
        yes(
            "typeof Symbol.asyncIterator==='symbol' && Symbol.asyncIterator===Symbol.asyncIterator && Symbol.asyncIterator!==Symbol.iterator && ('asyncIterator' in Symbol);",
        );
        yes(
            "let o={};o[Symbol.asyncIterator]=7;o[Symbol.asyncIterator]===7 && Symbol.keyFor(Symbol.asyncIterator)===undefined;",
        );
    }
    #[test]
    fn exact_signed_unsigned_elements_and_modulo() {
        yes(
            "const a=new BigInt64Array([0n,9223372036854775807n,9223372036854775808n,-9223372036854775808n,18446744073709551615n,18446744073709551617n]);a.join('|')==='0|9223372036854775807|-9223372036854775808|-9223372036854775808|-1|1' && a.BYTES_PER_ELEMENT===8 && BigInt64Array.BYTES_PER_ELEMENT===8;",
        );
        yes(
            "const a=new BigUint64Array([-1n,-18446744073709551617n,18446744073709551616n,9007199254740993n]);a[0]===18446744073709551615n && a[1]===18446744073709551615n && a[2]===0n && a[3]===9007199254740993n && a.byteLength===32 && a.length===4 && ArrayBuffer.isView(a);",
        );
        yes(
            "let a=new BigInt64Array(2);a[0]='18446744073709551615';a[1]=true;a[0]===-1n && a[1]===1n && typeof a[0]==='bigint' && a instanceof BigInt64Array;",
        );
        yes(
            "const b=new ArrayBuffer(16);const a=new BigInt64Array(b,8,1);a[0]=-1n;const u=new BigUint64Array(b);u[1]===18446744073709551615n && a.buffer===b && a.byteOffset===8;",
        );
    }
    #[test]
    fn dataview_endianness_unaligned_offsets_and_boundaries() {
        yes(
            "const b=new ArrayBuffer(18);const d=new DataView(b,1,16);d.setBigUint64(0,0x0102030405060708n);d.setBigInt64(8,-9223372036854775808n,true);const bytes=new Uint8Array(b);bytes[1]===1&&bytes[8]===8&&bytes[16]===128&&d.getBigUint64(0)===0x0102030405060708n&&d.getBigUint64(0,true)===0x0807060504030201n&&d.getBigInt64(8,true)===-9223372036854775808n&&d.getBigUint64(8,true)===9223372036854775808n;",
        );
        yes(
            "const d=new DataView(new ArrayBuffer(8));d.setBigInt64(0,18446744073709551617n);d.getBigInt64(0)===1n&&d.getBigUint64.length===1&&d.setBigInt64.length===2;",
        );
        for source in [
            "new BigInt64Array(new ArrayBuffer(9))",
            "new BigUint64Array(new ArrayBuffer(16),1)",
            "new BigInt64Array(new ArrayBuffer(8),0,2)",
            "new DataView(new ArrayBuffer(8)).getBigInt64(1)",
            "new DataView(new ArrayBuffer(8)).setBigUint64(1,1n)",
            "new DataView(new ArrayBuffer(8)).getBigUint64(-1)",
        ] {
            yes(&format!(
                "let ok=false;try{{{source};}}catch(e){{ok=e.name==='RangeError'}}ok;"
            ));
        }
    }
    #[test]
    fn typed_methods_keep_bigints_and_snapshot_overlap() {
        yes(
            "let a=BigInt64Array.of(1n,2n,3n,4n);a.set(a.subarray(0,3),1);a.join()==='1,1,2,3' && a.slice(1).join()==='1,2,3' && a.includes(2n) && !a.includes(2) && a.indexOf(3n)===3 && a.indexOf(3)===-1;",
        );
        yes(
            "const a=BigInt64Array.from([1,2],v=>BigInt(v));const b=a.map(v=>v+9007199254740992n);a.fill(-1n,1);let n=0;for(const v of b)n+=typeof v==='bigint'?1:0;b.reverse().join()==='9007199254740994,9007199254740993' && a.at(-1)===-1n && n===2 && [...a.keys()].join()==='0,1' && a.entries().next().value[1]===1n;",
        );
        yes(
            "const a=new BigUint64Array([1n,2n]);let sum=0n;a.forEach(v=>sum+=v);const b=a.slice();a.fill(7n);b.join()==='1,2' && sum===3n;",
        );
        yes(
            "Reflect.construct(BigInt64Array,[[1n]])[0]===1n && Object.getOwnPropertyDescriptor(new BigInt64Array([2n]),'0').value===2n;",
        );
    }
    #[test]
    fn typed_capacity_checks_preserve_conversion_order() {
        yes(
            "const source={};Object.defineProperty(source,'length',{get:function(){throw 77},enumerable:true,configurable:true});let ok=false;try{new BigInt64Array(0).set(source,1)}catch(e){ok=e===77}ok;",
        );
        yes(
            "const source={};Object.defineProperty(source,'length',{get:function(){throw 77},enumerable:true,configurable:true});let ok=false;try{new BigInt64Array(0).set(source,Infinity)}catch(e){ok=e===77}ok;",
        );
        yes(
            "let seen=false;const source={};Object.defineProperty(source,'length',{get:function(){seen=true;return 0},enumerable:true,configurable:true});let ok=false;try{new BigInt64Array(0).set(source,-1)}catch(e){ok=e.name==='RangeError'}ok&&!seen;",
        );
        yes(
            "let ok=false;try{new BigInt64Array(new ArrayBuffer(8),16,{valueOf:function(){throw 77}})}catch(e){ok=e===77}ok;",
        );
        yes(
            "let seen=false;let ok=false;try{new BigInt64Array(new ArrayBuffer(8),16,{valueOf:function(){seen=true;return 0}})}catch(e){ok=e.name==='RangeError'}ok&&seen;",
        );
        yes(
            "let seen=false;let ok=false;try{new BigInt64Array(new ArrayBuffer(8),1,{valueOf:function(){seen=true;return 0}})}catch(e){ok=e.name==='RangeError'}ok&&!seen;",
        );
    }
    #[test]
    fn typed_slice_does_not_observe_array_iterator() {
        yes(
            "let a=BigInt64Array.of(1n,2n);Array.prototype[Symbol.iterator]=function(){throw 77};let b=a.slice();a[0]=9n;b[0]===1n&&b[1]===2n;",
        );
        yes(
            "let a=Uint8Array.of(1,2);Array.prototype[Symbol.iterator]=function(){throw 77};a.slice(1)[0]===2;",
        );
    }
    #[test]
    fn conversion_hooks_are_observed_once_and_in_order() {
        yes(
            "let log='';let x={};x[Symbol.toPrimitive]=function(h){log+=h;return -1n};const a=new BigUint64Array([x]);a[0]=x;a.fill(x);a[0]===18446744073709551615n && log==='numbernumbernumber';",
        );
        yes("const x=Object(2n);x.valueOf=()=>7n;new BigInt64Array([x])[0]===7n;");
        // Reflect deliberately rejects native exotics in this subset; no index coercion follows.
        yes(
            "let count=0;let a=new BigInt64Array(0);let x={valueOf:function(){count++;return 1n}};a[2]=x;a['-0']=x;a[-1]=x;let rejected=false;try{Reflect.defineProperty(a,'0',{value:x})}catch(e){rejected=e.name==='TypeError'}count===3&&rejected;",
        );
        yes(
            "let log='';let offset={valueOf:function(){log+='i';return 1}};let value={valueOf:function(){log+='v';return 1n}};try{new DataView(new ArrayBuffer(8)).setBigInt64(offset,value)}catch(e){}log==='iv';",
        );
        yes("let n=0;try{BigInt64Array.from([1,2],v=>{n++;return v})}catch(e){}n===1;");
        yes(
            "let x={valueOf:function(){throw 77}};let ok=false;try{new BigInt64Array([x])}catch(e){ok=e===77}ok;",
        );
    }
    #[test]
    fn bigint_numeric_content_mixing_and_bad_values_throw() {
        for source in [
            "new BigInt64Array([1])",
            "BigUint64Array.of(1)",
            "new BigInt64Array([null])",
            "new BigInt64Array([undefined])",
            "new BigInt64Array([Symbol()])",
            "new Uint8Array([1n])",
            "new BigInt64Array(new Uint8Array(0))",
            "new Uint8Array(new BigInt64Array(0))",
            "new BigInt64Array(0).set(new Uint8Array(0))",
            "new Uint8Array(0).set(new BigInt64Array(0))",
            "new BigInt64Array([1n]).map(()=>1)",
            "new DataView(new ArrayBuffer(8)).setBigInt64(0,1)",
            "new DataView(new ArrayBuffer(8)).setFloat64(0,1n)",
            "new BigInt64Array(1n)",
            "new BigInt64Array(1).slice(0n)",
        ] {
            yes(&format!(
                "let ok=false;try{{{source};}}catch(e){{ok=e.name==='TypeError'}}ok;"
            ));
        }
        yes(
            "let ok=false;try{new BigInt64Array(['not-an-integer'])}catch(e){ok=e.name==='SyntaxError'}ok;",
        );
        yes(
            "let ok=false;try{JSON.stringify(new BigInt64Array([1n]))}catch(e){ok=e.name==='TypeError'}ok && JSON.stringify(new BigInt64Array(0))==='{}';",
        );
    }
    #[test]
    fn numeric_views_remain_numeric_and_gc_retains_backing_storage() {
        yes(
            "const b=new ArrayBuffer(16);const v=new DataView(b);v.setUint32(0,4294967295,true);v.setFloat64(8,1.5,true);new Uint32Array(b,0,1)[0]===4294967295 && v.getFloat64(8,true)===1.5 && typeof v.getUint32(0)==='number';",
        );
        let mut runtime = Runtime::default();
        let value = runtime
            .execute("new BigInt64Array([9007199254740993n]);", &mut Empty)
            .unwrap();
        runtime
            .collect_garbage(std::slice::from_ref(&value), &Empty)
            .unwrap();
        let element = runtime.get(value, "0", &mut Empty).unwrap();
        assert_eq!(element.primitive_text().unwrap(), "9007199254740993");
        let mut runtime = Runtime::default();
        runtime.limits.string_bytes = 0;
        let buffer = runtime.stdlib_buffer(8);
        // Native prototype allocation is bounded by the same VM budgets.
        assert!(buffer.is_err());
        assert_eq!(
            run("new ArrayBuffer(1048577);").unwrap_err().kind,
            ErrorKind::Runtime
        );
    }
    #[test]
    fn arraylike_access_and_conversion_interleave_but_iterables_are_collected() {
        yes(
            "let log='';let source={length:2};Object.defineProperty(source,'0',{get:function(){log+='g0';return {valueOf:function(){log+='c0';return 1n}}}});Object.defineProperty(source,'1',{get:function(){log+='g1';return {valueOf:function(){log+='c1';return 2n}}}});let a=new BigInt64Array(source);a.join()==='1,2'&&log==='g0c0g1c1';",
        );
        yes(
            "let log='';let source={length:2};Object.defineProperty(source,'0',{get:function(){log+='g0';return {valueOf:function(){log+='c0';return 1n}}}});Object.defineProperty(source,'1',{get:function(){log+='g1';return {valueOf:function(){log+='c1';return 2n}}}});BigInt64Array.from(source,(v,i)=>{log+='m'+i;return v});log==='g0m0c0g1m1c1';",
        );
        yes(
            "let a=new BigInt64Array(2);let source={length:2};Object.defineProperty(source,'0',{get:function(){return 3n}});Object.defineProperty(source,'1',{get:function(){return a[0]+1n}});a.set(source);a.join()==='3,4';",
        );
        yes(
            "let calls=0;let log='';let source={};Object.defineProperty(source,Symbol.iterator,{get:function(){calls++;let i=0;return function(){return {next:function(){if(i===2)return {done:true};let n=i++;log+='n'+n;return {done:false,value:{valueOf:function(){log+='c'+n;return BigInt(n)}}}}}}}});let a=new BigInt64Array(source);a.join()==='0,1'&&calls===1&&log==='n0n1c0c1';",
        );
        yes(
            "let calls=0;let separator={};separator[Symbol.toPrimitive]=function(h){calls++;if(h!=='string')throw 'hint';return String.fromCharCode(55296)};let text=new BigInt64Array([1n,2n]).join(separator);calls===1&&text.length===3&&text.charCodeAt(1)===55296;",
        );
    }
}
