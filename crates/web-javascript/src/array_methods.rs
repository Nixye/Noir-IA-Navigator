use crate::{ArrayHoles, MAX_ARRAY_LENGTH};
use crate::{Heap, Host, JsError, Runtime, Value, strict_equal};

impl Runtime {
    pub(super) fn array_method(
        &mut self,
        id: usize,
        property: &str,
        args: Vec<Value>,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let Some(Heap::Array(values)) = self.heap.get(id) else {
            return Err(JsError::runtime("Invalid array reference"));
        };
        let length = values.len();
        match property {
            "push" => {
                if length + args.len() > MAX_ARRAY_LENGTH {
                    return Err(JsError::resource("Array item limit exceeded"));
                }
                let new_length = length + args.len();
                self.check_array_slots(args.len())?;
                for (offset, value) in args.into_iter().enumerate() {
                    self.set_or_throw(
                        Value::Array(id),
                        &(length + offset).to_string(),
                        value,
                        host,
                    )?;
                }
                self.set_or_throw(
                    Value::Array(id),
                    "length",
                    Value::Number(new_length as f64),
                    host,
                )?;
                Ok(Value::Number(new_length as f64))
            }
            "pop" => {
                if length == 0 {
                    self.set_or_throw(Value::Array(id), "length", Value::Number(0.), host)?;
                    return Ok(Value::Undefined);
                }
                let value = self.get(Value::Array(id), &(length - 1).to_string(), host)?;
                if !self.delete_property(
                    Value::Array(id),
                    (length - 1).to_string().as_str(),
                    host,
                )? {
                    return Err(
                        self.property_type_error("Cannot pop a non-configurable array property")
                    );
                }
                self.set_or_throw(
                    Value::Array(id),
                    "length",
                    Value::Number((length - 1) as f64),
                    host,
                )?;
                Ok(value)
            }
            "join" => {
                let delimiter = match args.first() {
                    None | Some(Value::Undefined) => vec![0x2c],
                    Some(value) => {
                        let value = self.operator_primitive(
                            value.clone(),
                            crate::coercion::Hint::String,
                            host,
                        )?;
                        self.string_units_coerce(&value)?
                    }
                };
                let mut text = Vec::new();
                for index in 0..length {
                    self.tick()?;
                    if index > 0 {
                        if text.len() + delimiter.len() > 65536 {
                            return Err(JsError::resource("Joined string exceeds limit"));
                        }
                        text.extend_from_slice(&delimiter);
                    }
                    let value = self.get(Value::Array(id), &index.to_string(), host)?;
                    if !matches!(value, Value::Undefined | Value::Null) {
                        let value =
                            self.operator_primitive(value, crate::coercion::Hint::String, host)?;
                        let units = self.string_units_coerce(&value)?;
                        if text.len() + units.len() > 65536 {
                            return Err(JsError::resource("Joined string exceeds limit"));
                        }
                        text.extend_from_slice(&units);
                    }
                    if text.len() > 65_536 {
                        return Err(JsError::resource("Joined string exceeds limit"));
                    }
                }
                self.utf16_string(text)
            }
            "forEach" | "map" => {
                let callback = args.first().cloned().unwrap_or(Value::Undefined);
                if !matches!(callback, Value::Function(_) | Value::Native(_))
                    && !matches!(callback, Value::Host(id) if host.is_callable(id))
                {
                    return Err(JsError::runtime("Array callback must be callable"));
                }
                let this = args.get(1).cloned().unwrap_or(Value::Undefined);
                let is_map = property == "map";
                if is_map {
                    self.charge_array_slots(length)?;
                }
                let mut results = if is_map {
                    vec![Value::Undefined; length]
                } else {
                    Vec::new()
                };
                let mut holes = ArrayHoles::new();
                let mut output = results.iter_mut();
                for index in 0..length {
                    let target = output.next();
                    self.tick()?;
                    let key = index.to_string();
                    if !self.has_property(&Value::Array(id), &key, host)? {
                        if is_map {
                            holes.insert(index)?;
                        }
                        continue;
                    }
                    let value = self.get(Value::Array(id), &key, host)?;
                    let mapped = self.call(
                        callback.clone(),
                        vec![value, Value::Number(index as f64), Value::Array(id)],
                        this.clone(),
                        host,
                    )?;
                    if let Some(target) = target {
                        *target = mapped;
                    }
                }
                if is_map {
                    let result = self.allocate_reserved(Heap::Array(results))?;
                    if !holes.is_empty() {
                        self.array_holes.insert(result, holes);
                    }
                    Ok(Value::Array(result))
                } else {
                    Ok(Value::Undefined)
                }
            }
            "toString" => {
                let join = self.get(Value::Array(id), "join", host)?;
                if self.is_callable_value(&join, host) {
                    self.call(join, Vec::new(), Value::Array(id), host)
                } else {
                    self.string("[object Array]".into())
                }
            }
            "slice" => {
                let start = self.array_relative(args.first(), length, 0.)?;
                let end = self
                    .array_relative(args.get(1), length, length as f64)?
                    .max(start);
                self.charge_array_slots(end - start)?;
                let mut values = Vec::with_capacity(end - start);
                let mut holes = ArrayHoles::new();
                for index in start..end {
                    self.tick()?;
                    let key = index.to_string();
                    if self.has_property(&Value::Array(id), &key, host)? {
                        values.push(self.get(Value::Array(id), &key, host)?);
                    } else {
                        holes.insert(values.len())?;
                        values.push(Value::Undefined);
                    }
                }
                self.array_result(values, holes)
            }
            "concat" => {
                let mut values = Vec::new();
                let mut holes = ArrayHoles::new();
                for value in std::iter::once(Value::Array(id)).chain(args) {
                    if let Value::Array(source) = value {
                        let length = self.array_length(source)?;
                        if values.len().saturating_add(length) > MAX_ARRAY_LENGTH {
                            return Err(JsError::resource("Array item limit exceeded"));
                        }
                        self.charge_array_slots(length)?;
                        for index in 0..length {
                            self.tick()?;
                            let key = index.to_string();
                            if self.has_property(&Value::Array(source), &key, host)? {
                                values.push(self.get(Value::Array(source), &key, host)?);
                            } else {
                                holes.insert(values.len())?;
                                values.push(Value::Undefined);
                            }
                        }
                    } else {
                        if values.len() >= MAX_ARRAY_LENGTH {
                            return Err(JsError::resource("Array item limit exceeded"));
                        }
                        self.charge_array_slots(1)?;
                        values.push(value);
                    }
                }
                self.array_result(values, holes)
            }
            "filter" | "every" | "some" | "find" | "findIndex" | "findLast" | "findLastIndex" => {
                let callback = args.first().cloned().unwrap_or(Value::Undefined);
                if !self.is_callable_value(&callback, host) {
                    return Err(self.property_type_error("Array callback must be callable"));
                }
                let this = args.get(1).cloned().unwrap_or(Value::Undefined);
                let reverse = matches!(property, "findLast" | "findLastIndex");
                let visit_holes = matches!(
                    property,
                    "find" | "findIndex" | "findLast" | "findLastIndex"
                );
                let mut values = Vec::new();
                for offset in 0..length {
                    self.tick()?;
                    let index = if reverse { length - offset - 1 } else { offset };
                    let key = index.to_string();
                    if !visit_holes && !self.has_property(&Value::Array(id), &key, host)? {
                        continue;
                    }
                    let value = self.get(Value::Array(id), &key, host)?;
                    let result = self
                        .call(
                            callback.clone(),
                            vec![value.clone(), Value::Number(index as f64), Value::Array(id)],
                            this.clone(),
                            host,
                        )?
                        .truthy();
                    match property {
                        "filter" if result => {
                            self.charge_array_slots(1)?;
                            values.push(value);
                        }
                        "every" if !result => return Ok(Value::Bool(false)),
                        "some" if result => return Ok(Value::Bool(true)),
                        "find" | "findLast" if result => return Ok(value),
                        "findIndex" | "findLastIndex" if result => {
                            return Ok(Value::Number(index as f64));
                        }
                        _ => {}
                    }
                }
                match property {
                    "filter" => self.array_result(values, ArrayHoles::new()),
                    "every" => Ok(Value::Bool(true)),
                    "some" => Ok(Value::Bool(false)),
                    "findIndex" | "findLastIndex" => Ok(Value::Number(-1.)),
                    _ => Ok(Value::Undefined),
                }
            }
            "reduce" | "reduceRight" => {
                let callback = args.first().cloned().unwrap_or(Value::Undefined);
                if !self.is_callable_value(&callback, host) {
                    return Err(self.property_type_error("Array callback must be callable"));
                }
                let mut accumulator = args.get(1).cloned();
                for offset in 0..length {
                    self.tick()?;
                    let index = if property == "reduceRight" {
                        length - offset - 1
                    } else {
                        offset
                    };
                    let key = index.to_string();
                    if !self.has_property(&Value::Array(id), &key, host)? {
                        continue;
                    }
                    let value = self.get(Value::Array(id), &key, host)?;
                    accumulator = Some(if let Some(previous) = accumulator {
                        self.call(
                            callback.clone(),
                            vec![
                                previous,
                                value,
                                Value::Number(index as f64),
                                Value::Array(id),
                            ],
                            Value::Undefined,
                            host,
                        )?
                    } else {
                        value
                    });
                }
                accumulator.ok_or_else(|| {
                    self.property_type_error("Reduce of empty array with no initial value")
                })
            }
            "indexOf" | "lastIndexOf" | "includes" => {
                let needle = args.first().cloned().unwrap_or(Value::Undefined);
                // These searches return before coercing fromIndex on empty arrays.
                if length == 0 {
                    return Ok(if property == "includes" {
                        Value::Bool(false)
                    } else {
                        Value::Number(-1.)
                    });
                }
                let from = self.array_number(
                    args.get(1),
                    if property == "lastIndexOf" {
                        length as f64 - 1.
                    } else {
                        0.
                    },
                )?;
                let from = if from.is_nan() { 0. } else { from.trunc() };
                if property == "lastIndexOf" {
                    let start = if from < 0. {
                        length as f64 + from
                    } else {
                        from.min(length as f64 - 1.)
                    };
                    if start >= 0. {
                        for index in (0..=start as usize).rev() {
                            self.tick()?;
                            let key = index.to_string();
                            if self.has_property(&Value::Array(id), &key, host)?
                                && strict_equal(&self.get(Value::Array(id), &key, host)?, &needle)
                            {
                                return Ok(Value::Number(index as f64));
                            }
                        }
                    }
                } else {
                    let start = if from < 0. {
                        (length as f64 + from).max(0.)
                    } else {
                        from
                    };
                    if start < length as f64 {
                        for index in start as usize..length {
                            self.tick()?;
                            let key = index.to_string();
                            if property != "includes"
                                && !self.has_property(&Value::Array(id), &key, host)?
                            {
                                continue;
                            }
                            let value = self.get(Value::Array(id), &key, host)?;
                            let equal = strict_equal(&value, &needle)
                                || (property == "includes"
                                    && matches!((&value,&needle),(Value::Number(a),Value::Number(b)) if a.is_nan()&&b.is_nan()));
                            if equal {
                                return Ok(if property == "includes" {
                                    Value::Bool(true)
                                } else {
                                    Value::Number(index as f64)
                                });
                            }
                        }
                    }
                }
                Ok(if property == "includes" {
                    Value::Bool(false)
                } else {
                    Value::Number(-1.)
                })
            }
            "at" => {
                let number = self.array_number(args.first(), 0.)?;
                let number = if number.is_nan() { 0. } else { number.trunc() };
                let index = if number < 0. {
                    length as f64 + number
                } else {
                    number
                };
                if !(0. ..length as f64).contains(&index) {
                    Ok(Value::Undefined)
                } else {
                    self.get(Value::Array(id), &(index as usize).to_string(), host)
                }
            }
            "reverse" | "toReversed" => {
                if property == "toReversed" {
                    self.charge_array_slots(length)?;
                    let mut values = Vec::with_capacity(length);
                    for index in (0..length).rev() {
                        self.tick()?;
                        values.push(self.get(Value::Array(id), &index.to_string(), host)?);
                    }
                    return self.array_result(values, ArrayHoles::new());
                }
                for lower in 0..length / 2 {
                    self.tick()?;
                    let upper = length - lower - 1;
                    let lower_key = lower.to_string();
                    let upper_key = upper.to_string();
                    let lower_exists = self.has_property(&Value::Array(id), &lower_key, host)?;
                    let lower_value = if lower_exists {
                        self.get(Value::Array(id), &lower_key, host)?
                    } else {
                        Value::Undefined
                    };
                    let upper_exists = self.has_property(&Value::Array(id), &upper_key, host)?;
                    let upper_value = if upper_exists {
                        self.get(Value::Array(id), &upper_key, host)?
                    } else {
                        Value::Undefined
                    };
                    if upper_exists {
                        self.set_or_throw(Value::Array(id), &lower_key, upper_value, host)?;
                    } else {
                        self.array_delete_or_throw(id, &lower_key, host)?;
                    }
                    if lower_exists {
                        self.set_or_throw(Value::Array(id), &upper_key, lower_value, host)?;
                    } else {
                        self.array_delete_or_throw(id, &upper_key, host)?;
                    }
                }
                Ok(Value::Array(id))
            }
            "shift" => {
                if length == 0 {
                    self.set_or_throw(Value::Array(id), "length", Value::Number(0.), host)?;
                    return Ok(Value::Undefined);
                }
                let first = self.get(Value::Array(id), "0", host)?;
                for from in 1..length {
                    self.array_move(id, from, from - 1, host)?;
                }
                self.array_delete_or_throw(id, &(length - 1).to_string(), host)?;
                self.set_or_throw(
                    Value::Array(id),
                    "length",
                    Value::Number((length - 1) as f64),
                    host,
                )?;
                Ok(first)
            }
            "unshift" => {
                let count = args.len();
                if length.saturating_add(count) > MAX_ARRAY_LENGTH {
                    return Err(JsError::resource("Array item limit exceeded"));
                }
                self.check_array_slots(count)?;
                if count > 0 {
                    for from in (0..length).rev() {
                        self.array_move(id, from, from + count, host)?;
                    }
                }
                for (index, value) in args.into_iter().enumerate() {
                    self.set_or_throw(Value::Array(id), &index.to_string(), value, host)?;
                }
                self.set_or_throw(
                    Value::Array(id),
                    "length",
                    Value::Number((length + count) as f64),
                    host,
                )?;
                Ok(Value::Number((length + count) as f64))
            }
            "splice" | "toSpliced" => {
                let start = self.array_relative(args.first(), length, 0.)?;
                let deleted = if args.is_empty() {
                    0
                } else if args.len() == 1 {
                    length - start
                } else {
                    let n = self.array_number(args.get(1), 0.)?;
                    if n.is_nan() || n <= 0. {
                        0
                    } else {
                        n.trunc().min((length - start) as f64) as usize
                    }
                };
                let insert = args.len().saturating_sub(2);
                let new_length = length - deleted + insert;
                if new_length > MAX_ARRAY_LENGTH {
                    return Err(JsError::resource("Array item limit exceeded"));
                }
                if property == "toSpliced" {
                    self.charge_array_slots(new_length)?;
                    let mut values = Vec::with_capacity(new_length);
                    for index in 0..start {
                        self.tick()?;
                        values.push(self.get(Value::Array(id), &index.to_string(), host)?);
                    }
                    values.extend(args.into_iter().skip(2));
                    for index in start + deleted..length {
                        self.tick()?;
                        values.push(self.get(Value::Array(id), &index.to_string(), host)?);
                    }
                    return self.array_result(values, ArrayHoles::new());
                }
                self.charge_array_slots(deleted)?;
                self.check_array_slots(new_length.saturating_sub(length))?;
                let mut values = Vec::with_capacity(deleted);
                let mut holes = ArrayHoles::new();
                for index in start..start + deleted {
                    self.tick()?;
                    let key = index.to_string();
                    if self.has_property(&Value::Array(id), &key, host)? {
                        values.push(self.get(Value::Array(id), &key, host)?);
                    } else {
                        holes.insert(values.len())?;
                        values.push(Value::Undefined);
                    }
                }
                let removed = self.array_result(values, holes)?;
                if insert < deleted {
                    for from in start + deleted..length {
                        self.array_move(id, from, from - deleted + insert, host)?;
                    }
                    for index in (new_length..length).rev() {
                        self.array_delete_or_throw(id, &index.to_string(), host)?;
                    }
                } else if insert > deleted {
                    for from in (start + deleted..length).rev() {
                        self.array_move(id, from, from - deleted + insert, host)?;
                    }
                }
                for (offset, value) in args.into_iter().skip(2).enumerate() {
                    self.set_or_throw(
                        Value::Array(id),
                        &(start + offset).to_string(),
                        value,
                        host,
                    )?;
                }
                self.set_or_throw(
                    Value::Array(id),
                    "length",
                    Value::Number(new_length as f64),
                    host,
                )?;
                Ok(removed)
            }
            "fill" => {
                let value = args.first().cloned().unwrap_or(Value::Undefined);
                let start = self.array_relative(args.get(1), length, 0.)?;
                let end = self.array_relative(args.get(2), length, length as f64)?;
                for index in start..end {
                    self.tick()?;
                    self.set_or_throw(Value::Array(id), &index.to_string(), value.clone(), host)?;
                }
                Ok(Value::Array(id))
            }
            "copyWithin" => {
                let target = self.array_relative(args.first(), length, 0.)?;
                let start = self.array_relative(args.get(1), length, 0.)?;
                let end = self.array_relative(args.get(2), length, length as f64)?;
                let count = end.saturating_sub(start).min(length - target);
                if target > start && target < start + count {
                    for offset in (0..count).rev() {
                        self.array_move(id, start + offset, target + offset, host)?;
                    }
                } else {
                    for offset in 0..count {
                        self.array_move(id, start + offset, target + offset, host)?;
                    }
                }
                Ok(Value::Array(id))
            }
            "flat" | "flatMap" => {
                let callback = if property == "flatMap" {
                    let callback = args.first().cloned().unwrap_or(Value::Undefined);
                    if !self.is_callable_value(&callback, host) {
                        return Err(self.property_type_error("Array callback must be callable"));
                    }
                    Some(callback)
                } else {
                    None
                };
                let depth = if callback.is_some() {
                    1
                } else {
                    let n = self.array_number(
                        args.first().filter(|value| **value != Value::Undefined),
                        1.,
                    )?;
                    if n.is_nan() || n <= 0. {
                        0
                    } else {
                        n.trunc() as usize
                    }
                };
                let mut values = Vec::new();
                for index in 0..length {
                    self.tick()?;
                    let key = index.to_string();
                    if !self.has_property(&Value::Array(id), &key, host)? {
                        continue;
                    }
                    let mut value = self.get(Value::Array(id), &key, host)?;
                    if let Some(callback) = &callback {
                        value = self.call(
                            callback.clone(),
                            vec![value, Value::Number(index as f64), Value::Array(id)],
                            args.get(1).cloned().unwrap_or(Value::Undefined),
                            host,
                        )?;
                    }
                    self.array_flatten(value, depth, 0, &mut values, host)?;
                }
                self.array_result(values, ArrayHoles::new())
            }
            "sort" | "toSorted" => {
                let compare = args
                    .first()
                    .filter(|value| **value != Value::Undefined)
                    .cloned();
                if compare
                    .as_ref()
                    .is_some_and(|value| !self.is_callable_value(value, host))
                {
                    return Err(self.property_type_error("Array comparison must be callable"));
                }
                if property == "toSorted" {
                    self.charge_array_slots(length)?;
                }
                let mut values = Vec::new();
                for index in 0..length {
                    self.tick()?;
                    let key = index.to_string();
                    if property == "toSorted" || self.has_property(&Value::Array(id), &key, host)? {
                        values.push(self.get(Value::Array(id), &key, host)?);
                    }
                }
                // Stable insertion sorting is bounded by the shared instruction
                // budget, including comparator calls and default conversions.
                for index in 1..values.len() {
                    let mut position = index;
                    while position > 0 {
                        self.tick()?;
                        let left = &values[position - 1];
                        let right = &values[position];
                        let order = if *left == Value::Undefined {
                            *right != Value::Undefined
                        } else if *right == Value::Undefined {
                            false
                        } else if let Some(compare) = &compare {
                            let result = self.call(
                                compare.clone(),
                                vec![left.clone(), right.clone()],
                                Value::Undefined,
                                host,
                            )?;
                            self.array_number(Some(&result), 0.)? > 0.
                        } else {
                            self.string_units_coerce(left)?
                                .cmp(&self.string_units_coerce(right)?)
                                .is_gt()
                        };
                        if !order {
                            break;
                        }
                        values.swap(position - 1, position);
                        position -= 1;
                    }
                }
                if property == "toSorted" {
                    return self.array_result(values, ArrayHoles::new());
                }
                let count = values.len();
                for (index, value) in values.into_iter().enumerate() {
                    self.set_or_throw(Value::Array(id), &index.to_string(), value, host)?;
                }
                for index in count..length {
                    self.array_delete_or_throw(id, &index.to_string(), host)?;
                }
                Ok(Value::Array(id))
            }
            _ => Err(JsError::runtime("Unsupported array method")),
        }
    }

    fn array_length(&self, id: usize) -> Result<usize, JsError> {
        match self.heap.get(id) {
            Some(Heap::Array(values)) => Ok(values.len()),
            _ => Err(JsError::runtime("Invalid array reference")),
        }
    }
    fn array_number(&mut self, value: Option<&Value>, default: f64) -> Result<f64, JsError> {
        if matches!(value, Some(Value::BigInt(_) | Value::Symbol(..)))
            || value.is_some_and(|value| self.is_bigint_wrapper(value))
        {
            return Err(self.property_type_error("Cannot convert Symbol or BigInt to a number"));
        }
        Ok(value.map_or(default, |value| self.number(value)))
    }
    fn array_relative(
        &mut self,
        value: Option<&Value>,
        length: usize,
        default: f64,
    ) -> Result<usize, JsError> {
        let n = self.array_number(value.filter(|value| **value != Value::Undefined), default)?;
        let n = if n.is_nan() { 0. } else { n.trunc() };
        Ok(if n < 0. {
            (length as f64 + n).max(0.) as usize
        } else {
            n.min(length as f64) as usize
        })
    }
    fn array_result(&mut self, values: Vec<Value>, holes: ArrayHoles) -> Result<Value, JsError> {
        // Every producer reserves result slots before its buffer grows.
        let id = self.allocate_reserved(Heap::Array(values))?;
        if !holes.is_empty() {
            self.array_holes.insert(id, holes);
        }
        Ok(Value::Array(id))
    }
    fn array_delete_or_throw(
        &mut self,
        id: usize,
        key: &str,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        if !self.delete_property(Value::Array(id), key, host)? {
            return Err(self.property_type_error("Cannot delete a non-configurable array property"));
        }
        Ok(())
    }
    fn array_move(
        &mut self,
        id: usize,
        from: usize,
        to: usize,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        self.tick()?;
        let from = from.to_string();
        let to = to.to_string();
        if self.has_property(&Value::Array(id), &from, host)? {
            let value = self.get(Value::Array(id), &from, host)?;
            self.set_or_throw(Value::Array(id), &to, value, host)?;
        } else {
            self.array_delete_or_throw(id, &to, host)?;
        }
        Ok(())
    }
    fn array_flatten(
        &mut self,
        value: Value,
        depth: usize,
        level: usize,
        output: &mut Vec<Value>,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        self.tick()?;
        if let Value::Array(id) = value
            && depth > 0
        {
            if level >= 32 {
                return Err(JsError::resource("Array flatten nesting exceeds 32"));
            }
            let length = self.array_length(id)?;
            for index in 0..length {
                let key = index.to_string();
                if self.has_property(&Value::Array(id), &key, host)? {
                    let value = self.get(Value::Array(id), &key, host)?;
                    self.array_flatten(value, depth - 1, level + 1, output, host)?;
                }
            }
        } else {
            if output.len() >= MAX_ARRAY_LENGTH {
                return Err(JsError::resource("Array item limit exceeded"));
            }
            self.charge_array_slots(1)?;
            output.push(value);
        }
        Ok(())
    }
}
#[cfg(test)]
mod bigint_conversion_tests {
    use crate::{Host, JsError, Runtime, Value};
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
    #[test]
    fn numeric_positions_reject_bigint() {
        for source in [
            "'abc'.at(1n)",
            "'abc'.charAt(1n)",
            "'abc'.charCodeAt(1n)",
            "'abc'.codePointAt(1n)",
            "'abc'.slice(1n)",
            "'abc'.slice(0,1n)",
            "'abc'.substring(1n)",
            "'abc'.substr(0,1n)",
            "'abc'.repeat(1n)",
            "'abc'.padStart(5n)",
            "'abc'.padEnd(5n)",
            "'abc'.indexOf('a',1n)",
            "'abc'.lastIndexOf('a',1n)",
            "'abc'.includes('a',1n)",
            "'abc'.startsWith('a',1n)",
            "'abc'.endsWith('a',1n)",
            "'abc'.split('',1n)",
            "'abc'.split(/b/,1n)",
            "String.fromCharCode(1n)",
            "String.fromCodePoint(1n)",
            "[1].at(0n)",
            "[].at(0n)",
            "[1].indexOf(1,0n)",
            "[1].lastIndexOf(1,0n)",
            "[1].includes(1,0n)",
            "[1].slice(0n)",
            "[1].slice(0,1n)",
            "[1].splice(0n)",
            "[1].splice(0,1n)",
            "[1].toSpliced(0,1n)",
            "[1].fill(2,0n)",
            "[1].fill(2,0,1n)",
            "[1].copyWithin(0n,0)",
            "[1].copyWithin(0,0n)",
            "[1].copyWithin(0,0,1n)",
            "[1].flat(1n)",
            "[2,1].sort(()=>1n)",
            "[2,1].toSorted(()=>1n)",
        ] {
            let script =
                format!("let ok=false;try{{{source};}}catch(e){{ok=e.name==='TypeError'}}ok;");
            assert_eq!(run(&script).unwrap(), Value::Bool(true), "{source}");
        }
    }
    #[test]
    fn bigint_values_remain_legal_and_errors_do_not_mutate_arrays() {
        assert_eq!(run("let a=[2n,1n];a.sort();a[0]===1n && a.includes(2n) && a.join(3n)==='132' && '123'.includes(2n) && 'a'.padStart(3,2n)==='22a';").unwrap(), Value::Bool(true));
        assert_eq!(run("let a=[1,2];try{a.splice(0,1n,9)}catch(e){}try{a.fill(9,0,1n)}catch(e){}a.join() === '1,2' && [].indexOf(1,0n)===-1 && [].lastIndexOf(1,0n)===-1 && ![].includes(1,0n);").unwrap(), Value::Bool(true));
    }
}
