//! Original binding patterns, iterators and class environments.
use crate::{
    Declaration, Expr, Heap, Host, JsError, Runtime, Value,
    parser::{ClassMember, Pattern},
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default)]
pub(super) struct LanguageState {
    pub(super) classes: BTreeMap<usize, ClassInfo>,
    pub(super) methods: BTreeMap<usize, MethodInfo>,
    pub(super) private: BTreeMap<usize, BTreeMap<(usize, String), PrivateSlot>>,
    pub(super) construction: Vec<Construction>,
}
#[derive(Clone, Debug)]
pub(super) struct ClassInfo {
    pub base: Option<Value>,
    pub prototype: Value,
    pub constructor: Option<usize>,
    pub fields: Vec<ClassMember>,
    pub private_methods: Vec<(ClassMember, Value)>,
    pub scope: usize,
}
#[derive(Clone, Debug)]
pub(super) struct MethodInfo {
    pub class: usize,
    pub home: Value,
}
#[derive(Clone, Debug)]
pub(super) enum PrivateSlot {
    Data(Value, bool),
    Accessor(Option<Value>, Option<Value>),
}
#[derive(Clone, Debug)]
pub(super) struct Construction {
    pub class: usize,
    pub new_target: usize,
    pub instance: Option<Value>,
    pub this_scope: Option<usize>,
}
#[derive(Clone, Debug)]
pub(super) enum IteratorRecord {
    Array(usize, usize),
    String(Vec<u16>, usize),
    Values(Vec<Value>, usize),
    Custom {
        object: Value,
        next: Value,
        done: bool,
    },
}

impl IteratorRecord {
    /// Heap edges retained by a suspended synchronous iterator.
    pub(super) fn trace_values(&self, values: &mut Vec<Value>) {
        match self {
            Self::Array(id, _) => values.push(Value::Array(*id)),
            Self::String(..) => {}
            Self::Values(items, _) => values.extend(items.iter().cloned()),
            Self::Custom { object, next, .. } => {
                values.push(object.clone());
                values.push(next.clone());
            }
        }
    }
}

impl LanguageState {
    pub(super) fn trace_owner(&self, id: usize) -> (Vec<Value>, Vec<usize>) {
        let mut values = Vec::new();
        let mut scopes = Vec::new();
        if let Some(class) = self.classes.get(&id) {
            values.extend(class.base.clone());
            values.push(class.prototype.clone());
            values.extend(class.constructor.map(Value::Function));
            values.extend(class.private_methods.iter().map(|(_, value)| value.clone()));
            scopes.push(class.scope);
        }
        if let Some(method) = self.methods.get(&id) {
            values.push(Value::Function(method.class));
            values.push(method.home.clone());
        }
        if let Some(slots) = self.private.get(&id) {
            for ((class, _), slot) in slots {
                values.push(Value::Function(*class));
                match slot {
                    PrivateSlot::Data(value, _) => values.push(value.clone()),
                    PrivateSlot::Accessor(get, set) => {
                        values.extend(get.clone());
                        values.extend(set.clone());
                    }
                }
            }
        }
        (values, scopes)
    }
    pub(super) fn sweep(&mut self, live: &BTreeSet<usize>) {
        self.classes.retain(|id, _| live.contains(id));
        self.methods.retain(|id, _| live.contains(id));
        self.private.retain(|id, _| live.contains(id));
    }
}
impl Runtime {
    pub(super) fn iteration_scope(
        &mut self,
        binding: &crate::ForBinding,
        value: Value,
        scope: usize,
        host: &mut impl Host,
    ) -> Result<usize, JsError> {
        match binding {
            crate::ForBinding::Declaration(kind, name) => {
                let target = if *kind == Declaration::Var {
                    self.function_scope(scope)
                } else {
                    self.scope(scope, false)?
                };
                if !self.scopes[target].bindings.contains_key(name) {
                    self.declare(target, name, *kind != Declaration::Const, true)?;
                }
                let binding = self.scopes[target].bindings.get_mut(name).unwrap();
                binding.value = value;
                binding.initialized = true;
                Ok(if *kind == Declaration::Var {
                    scope
                } else {
                    target
                })
            }
            crate::ForBinding::Pattern(kind, pattern) => {
                let target = if *kind == Declaration::Var {
                    scope
                } else {
                    self.scope(scope, false)?
                };
                self.declare_pattern(pattern, *kind, target)?;
                self.bind_declared_pattern(pattern, value, *kind, target, host)?;
                Ok(target)
            }
            crate::ForBinding::Reference(expr) => {
                let place = self.place(expr, scope, host)?;
                self.write_place(place, value, host)?;
                Ok(scope)
            }
        }
    }
    pub(super) fn iterator(
        &mut self,
        value: Value,
        host: &mut impl Host,
    ) -> Result<IteratorRecord, JsError> {
        // A user-provided iterator always takes precedence over built-in paths.
        if matches!(
            value,
            Value::Object(_) | Value::Array(_) | Value::Function(_)
        ) {
            let Value::Symbol(key, _) = self.well_known_symbol("iterator")? else {
                unreachable!()
            };
            let iterator = self.get_symbol(value.clone(), key, host)?;
            if !matches!(iterator, Value::Undefined | Value::Null) {
                let object = self.call(iterator, Vec::new(), value, host)?;
                if !matches!(
                    object,
                    Value::Object(_)
                        | Value::Array(_)
                        | Value::Function(_)
                        | Value::Native(_)
                        | Value::Host(_)
                ) {
                    return Err(self.property_type_error("Iterator must be an object"));
                }
                let next = self.get(object.clone(), "next", host)?;
                if !self.is_callable_value(&next, host) {
                    return Err(self.property_type_error("Iterator.next must be callable"));
                }
                return Ok(IteratorRecord::Custom {
                    object,
                    next,
                    done: false,
                });
            }
        }
        match value {
            Value::Array(id) => Ok(IteratorRecord::Array(id, 0)),
            value @ (Value::String(_) | Value::Utf16(_)) => Ok(IteratorRecord::String(
                crate::strings::units(&value).unwrap(),
                0,
            )),
            other => {
                if let Some(values) = self.stdlib_values(&other)? {
                    Ok(IteratorRecord::Values(values, 0))
                } else {
                    Err(self.property_type_error("Value is not iterable"))
                }
            }
        }
    }
    pub(super) fn iterator_next(
        &mut self,
        iterator: &mut IteratorRecord,
        host: &mut impl Host,
    ) -> Result<Option<Value>, JsError> {
        self.tick()?;
        match iterator {
            IteratorRecord::Array(id, index) => {
                let length = match self.heap.get(*id) {
                    Some(Heap::Array(values)) => values.len(),
                    _ => return Err(JsError::runtime("Invalid array iterator")),
                };
                if *index >= length {
                    return Ok(None);
                }
                let value = self.get(Value::Array(*id), &index.to_string(), host)?;
                *index += 1;
                Ok(Some(value))
            }
            IteratorRecord::String(values, index) => {
                let Some(first) = values.get(*index).copied() else {
                    return Ok(None);
                };
                let count = if (0xd800..=0xdbff).contains(&first)
                    && values
                        .get(*index + 1)
                        .is_some_and(|unit| (0xdc00..=0xdfff).contains(unit))
                {
                    2
                } else {
                    1
                };
                let units = values[*index..*index + count].to_vec();
                *index += count;
                self.utf16_string(units).map(Some)
            }
            IteratorRecord::Values(values, index) => {
                let value = values.get(*index).cloned();
                *index += usize::from(value.is_some());
                Ok(value)
            }
            IteratorRecord::Custom { object, next, done } => {
                if *done {
                    return Ok(None);
                }
                let result = self.call(next.clone(), Vec::new(), object.clone(), host)?;
                if !matches!(
                    result,
                    Value::Object(_)
                        | Value::Array(_)
                        | Value::Function(_)
                        | Value::Native(_)
                        | Value::Host(_)
                ) {
                    return Err(self.property_type_error("Iterator result must be an object"));
                }
                if self.get(result.clone(), "done", host)?.truthy() {
                    *done = true;
                    Ok(None)
                } else {
                    self.get(result, "value", host).map(Some)
                }
            }
        }
    }
    pub(super) fn iterator_close(
        &mut self,
        iterator: &mut IteratorRecord,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        if let IteratorRecord::Custom { object, done, .. } = iterator {
            if *done {
                return Ok(());
            }
            *done = true;
            let close = self.get(object.clone(), "return", host)?;
            if !matches!(close, Value::Undefined | Value::Null) {
                let result = self.call(close, Vec::new(), object.clone(), host)?;
                if !matches!(
                    result,
                    Value::Object(_)
                        | Value::Array(_)
                        | Value::Function(_)
                        | Value::Native(_)
                        | Value::Host(_)
                ) {
                    return Err(self.property_type_error("Iterator.return must return an object"));
                }
            }
        }
        Ok(())
    }
    pub(super) fn iterable_values(
        &mut self,
        value: Value,
        host: &mut impl Host,
    ) -> Result<Vec<Value>, JsError> {
        let mut iterator = self.iterator(value, host)?;
        let mut values = Vec::new();
        loop {
            match self.iterator_next(&mut iterator, host) {
                Ok(Some(value)) => {
                    if values.len() >= 4096 {
                        return Err(JsError::resource("Iterable item limit exceeded"));
                    }
                    values.push(value);
                }
                Ok(None) => break,
                Err(error) => {
                    if error.kind != crate::ErrorKind::Resource {
                        let _ = self.iterator_close(&mut iterator, host);
                    }
                    return Err(error);
                }
            }
        }
        Ok(values)
    }
    pub(super) fn argument_values(
        &mut self,
        args: &[Expr],
        scope: usize,
        host: &mut impl Host,
    ) -> Result<Vec<Value>, JsError> {
        let mut values = Vec::new();
        for expr in args {
            if let Expr::Spread(expr) = expr {
                let source = self.eval(expr, scope, host)?;
                let expanded = self.iterable_values(source, host)?;
                if values.len() + expanded.len() > 32 {
                    return Err(JsError::resource("Expanded argument limit exceeded"));
                }
                values.extend(expanded);
            } else {
                if values.len() >= 32 {
                    return Err(JsError::resource("Call argument limit exceeded"));
                }
                values.push(self.eval(expr, scope, host)?);
            }
        }
        Ok(values)
    }
    fn destructuring_place(
        &mut self,
        target: &Expr,
        scope: usize,
        host: &mut impl Host,
    ) -> Result<Option<crate::Place>, JsError> {
        let target = match target {
            Expr::Assign(op, target, _) if op == "=" => target.as_ref(),
            _ => target,
        };
        if matches!(
            target,
            Expr::Name(_) | Expr::Member(..) | Expr::PrivateMember(..)
        ) {
            self.place(target, scope, host).map(Some)
        } else {
            Ok(None)
        }
    }
    fn assign_prepared(
        &mut self,
        target: &Expr,
        place: Option<crate::Place>,
        value: Value,
        scope: usize,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        if let Some(place) = place {
            let value = if let Expr::Assign(op, _, default) = target
                && op == "="
                && value == Value::Undefined
            {
                self.eval(default, scope, host)?
            } else {
                value
            };
            self.write_place(place, value, host)
        } else {
            self.assign_destructure(target, value, scope, host)
        }
    }
    /// Assignment patterns write existing references; binding patterns initialize declarations.
    pub(super) fn assign_destructure(
        &mut self,
        target: &Expr,
        value: Value,
        scope: usize,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        self.tick()?;
        match target {
            Expr::Assign(op, target, default) if op == "=" => {
                // Resolve simple references before evaluating a default initializer.
                if matches!(
                    target.as_ref(),
                    Expr::Name(_) | Expr::Member(..) | Expr::PrivateMember(..)
                ) {
                    let place = self.place(target, scope, host)?;
                    let value = if value == Value::Undefined {
                        self.eval(default, scope, host)?
                    } else {
                        value
                    };
                    self.write_place(place, value, host)
                } else {
                    let value = if value == Value::Undefined {
                        self.eval(default, scope, host)?
                    } else {
                        value
                    };
                    self.assign_destructure(target, value, scope, host)
                }
            }
            Expr::Array(items) => {
                let mut iterator = self.iterator(value, host)?;
                let result = (|| {
                    for item in items {
                        if let Expr::Spread(target) = item {
                            let place = self.destructuring_place(target, scope, host)?;
                            let mut tail = Vec::new();
                            while let Some(value) = self.iterator_next(&mut iterator, host)? {
                                if tail.len() >= 4096 {
                                    return Err(JsError::resource(
                                        "Rest assignment exceeds array limit",
                                    ));
                                }
                                tail.push(value);
                            }
                            let tail = Value::Array(self.allocate(Heap::Array(tail))?);
                            self.assign_prepared(target, place, tail, scope, host)?;
                        } else {
                            let place = self.destructuring_place(item, scope, host)?;
                            let value = self
                                .iterator_next(&mut iterator, host)?
                                .unwrap_or(Value::Undefined);
                            if !matches!(item, Expr::Hole) {
                                self.assign_prepared(item, place, value, scope, host)?;
                            }
                        }
                    }
                    Ok(())
                })();
                if !result
                    .as_ref()
                    .is_err_and(|error: &JsError| error.kind == crate::ErrorKind::Resource)
                {
                    let closed = self.iterator_close(&mut iterator, host);
                    if result.is_ok() {
                        closed?;
                    }
                }
                result
            }
            Expr::Object(items) => {
                if matches!(value, Value::Null | Value::Undefined) {
                    return Err(self.property_type_error("Cannot destructure null or undefined"));
                }
                let mut excluded = BTreeSet::new();
                for (key, target) in items {
                    if let Expr::Spread(target) = target {
                        let place = self.destructuring_place(target, scope, host)?;
                        let copy = self.object(BTreeMap::new(), None)?;
                        self.copy_properties(copy.clone(), value.clone(), &excluded, host)?;
                        self.assign_prepared(target, place, copy, scope, host)?;
                    } else {
                        excluded.insert(key.clone());
                        let place = self.destructuring_place(target, scope, host)?;
                        let item = self.get(value.clone(), key, host)?;
                        self.assign_prepared(target, place, item, scope, host)?;
                    }
                }
                Ok(())
            }
            Expr::Name(_) | Expr::Member(..) | Expr::PrivateMember(..) => {
                let place = self.place(target, scope, host)?;
                self.write_place(place, value, host)
            }
            _ => Err(JsError::syntax(
                0,
                "Invalid destructuring assignment target",
            )),
        }
    }
    pub(super) fn declare_pattern(
        &mut self,
        pattern: &Pattern,
        kind: Declaration,
        scope: usize,
    ) -> Result<(), JsError> {
        let mut names = Vec::new();
        pattern.names(&mut names);
        if names.len() > 128 {
            return Err(JsError::resource("Pattern binding limit exceeded"));
        }
        let target = if kind == Declaration::Var {
            self.function_scope(scope)
        } else {
            scope
        };
        for name in names {
            if kind == Declaration::Var && self.scopes[target].bindings.contains_key(&name) {
                continue;
            }
            self.declare(
                target,
                &name,
                kind != Declaration::Const,
                kind == Declaration::Var,
            )?;
        }
        Ok(())
    }
    pub(super) fn bind_declared_pattern(
        &mut self,
        pattern: &Pattern,
        value: Value,
        kind: Declaration,
        scope: usize,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        let target = if kind == Declaration::Var {
            Some(self.function_scope(scope))
        } else {
            None
        };
        self.bind_pattern_to(pattern, value, scope, target, host)
    }
    pub(super) fn bind_pattern(
        &mut self,
        pattern: &Pattern,
        value: Value,
        scope: usize,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        self.bind_pattern_to(pattern, value, scope, None, host)
    }
    fn bind_pattern_to(
        &mut self,
        pattern: &Pattern,
        value: Value,
        scope: usize,
        target: Option<usize>,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        self.tick()?;
        match pattern {
            Pattern::Name(name) => {
                let target = target
                    .or_else(|| self.lookup_binding(scope, name))
                    .ok_or_else(|| JsError::runtime("Missing pattern binding"))?;
                let binding = self.scopes[target].bindings.get_mut(name).unwrap();
                binding.value = value;
                binding.initialized = true;
                Ok(())
            }
            Pattern::Default(pattern, expr) => {
                let value = if value == Value::Undefined {
                    self.eval(expr, scope, host)?
                } else {
                    value
                };
                self.bind_pattern_to(pattern, value, scope, target, host)
            }
            Pattern::Array(patterns, rest) => {
                let mut iterator = self.iterator(value, host)?;
                let result = (|| {
                    for pattern in patterns {
                        let value = self
                            .iterator_next(&mut iterator, host)?
                            .unwrap_or(Value::Undefined);
                        if let Some(pattern) = pattern {
                            self.bind_pattern_to(pattern, value, scope, target, host)?;
                        }
                    }
                    if let Some(rest) = rest {
                        let mut tail = Vec::new();
                        while let Some(value) = self.iterator_next(&mut iterator, host)? {
                            if tail.len() >= 4096 {
                                return Err(JsError::resource("Rest binding exceeds array limit"));
                            }
                            tail.push(value);
                        }
                        let tail = Value::Array(self.allocate(Heap::Array(tail))?);
                        self.bind_pattern_to(rest, tail, scope, target, host)?;
                    }
                    Ok(())
                })();
                if !result
                    .as_ref()
                    .is_err_and(|error: &JsError| error.kind == crate::ErrorKind::Resource)
                {
                    let closed = self.iterator_close(&mut iterator, host);
                    if result.is_ok() {
                        closed?;
                    }
                }
                result
            }
            Pattern::Object(patterns, rest) => {
                if matches!(value, Value::Null | Value::Undefined) {
                    return Err(self.property_type_error("Cannot destructure null or undefined"));
                }
                let mut excluded = BTreeSet::new();
                for (key, pattern) in patterns {
                    excluded.insert(key.clone());
                    let item = self.get(value.clone(), key, host)?;
                    self.bind_pattern_to(pattern, item, scope, target, host)?;
                }
                if let Some(rest) = rest {
                    let copy = self.object(BTreeMap::new(), None)?;
                    self.copy_properties(copy.clone(), value, &excluded, host)?;
                    self.bind_pattern_to(rest, copy, scope, target, host)?;
                }
                Ok(())
            }
        }
    }
    pub(super) fn copy_properties(
        &mut self,
        target: Value,
        source: Value,
        excluded: &BTreeSet<String>,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        if matches!(
            source,
            Value::Null | Value::Undefined | Value::Bool(_) | Value::Number(_) | Value::Symbol(..)
        ) {
            return Ok(());
        }
        if let Some(units) = crate::strings::units(&source) {
            for (index, unit) in units.into_iter().enumerate() {
                self.tick()?;
                let key = index.to_string();
                if !excluded.contains(&key) {
                    let value = self.utf16_string(vec![unit])?;
                    self.define_field(target.clone(), &key, value)?;
                }
            }
            return Ok(());
        }
        let keys = self.own_names(&source)?;
        for key in keys {
            self.tick()?;
            if excluded.contains(&key) {
                continue;
            }
            let id = self.heap_owner(&source)?;
            if !self.own_enumerable(id, &key) {
                continue;
            }
            let value = self.get(source.clone(), &key, host)?;
            self.define_field(target.clone(), &key, value)?;
        }
        let owner = self.heap_owner(&source)?;
        let symbols = self
            .symbol_properties
            .get(&owner)
            .map(|map| map.keys().copied().collect::<Vec<_>>())
            .unwrap_or_default();
        for key in symbols {
            if self
                .symbol_metadata
                .get(&owner)
                .and_then(|map| map.get(&key))
                .is_some_and(|meta| !meta.enumerable)
            {
                continue;
            }
            let value = self.get_symbol(source.clone(), key, host)?;
            let target_id = self.heap_owner(&target)?;
            let properties = self.symbol_properties.entry(target_id).or_default();
            if properties.len() >= 128 && !properties.contains_key(&key) {
                return Err(JsError::resource("Symbol property limit exceeded"));
            }
            properties.insert(key, value);
            self.symbol_metadata.entry(target_id).or_default().insert(
                key,
                crate::descriptors::PropertyMetadata {
                    writable: true,
                    enumerable: true,
                    configurable: true,
                    accessor: None,
                },
            );
        }
        Ok(())
    }
}
