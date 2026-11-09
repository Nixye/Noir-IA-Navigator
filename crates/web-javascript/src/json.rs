//! Bounded JSON data parsing, reviver traversal and serialization.
use crate::{ErrorKind, Heap, Host, JsError, Runtime, Value};
use std::collections::BTreeMap;

const MAX_INPUT: usize = 65_536;
const MAX_DEPTH: usize = 32;

fn json_utf16_source(units: &[u16]) -> Result<String, JsError> {
    let mut output = String::new();
    let mut quoted = false;
    let mut escaped = false;
    for scalar in char::decode_utf16(units.iter().copied()) {
        match scalar {
            Ok(ch) => {
                if quoted {
                    if escaped {
                        escaped = false;
                    } else if ch == '\\' {
                        escaped = true;
                    } else if ch == '"' {
                        quoted = false;
                    }
                } else if ch == '"' {
                    quoted = true;
                }
                if output.len().saturating_add(ch.len_utf8()) > MAX_INPUT {
                    return Err(JsError::resource("JSON input exceeds 64 KiB"));
                }
                output.push(ch);
            }
            Err(error) => {
                let escape = if escaped {
                    escaped = false;
                    "x".into()
                } else {
                    format!("\\u{:04x}", error.unpaired_surrogate())
                };
                if output.len().saturating_add(escape.len()) > MAX_INPUT {
                    return Err(JsError::resource("JSON input exceeds 64 KiB"));
                }
                output.push_str(&escape);
            }
        }
    }
    Ok(output)
}

impl Runtime {
    pub(crate) fn json_namespace(&mut self) -> Result<Value, JsError> {
        if let Some(id) = self.json_namespace {
            return Ok(Value::Object(id));
        }
        let value = self.object(
            BTreeMap::from([
                ("parse".into(), Value::Native("JSON.parse")),
                ("stringify".into(), Value::Native("JSON.stringify")),
            ]),
            None,
        )?;
        self.hide_properties(&value, &["parse", "stringify"]);
        if let Value::Object(id) = value {
            self.json_namespace = Some(id);
        }
        Ok(value)
    }

    pub(crate) fn json_parse(&mut self, args: &[Value]) -> Result<Value, JsError> {
        if matches!(args.first(), Some(Value::Symbol(..))) {
            return Err(self.property_type_error("Cannot convert a Symbol value to a string"));
        }
        let source = match args.first() {
            Some(Value::Utf16(units)) => json_utf16_source(units)?,
            _ => self.text(args.first().unwrap_or(&Value::Undefined), 0)?,
        };
        if source.len() > MAX_INPUT {
            return Err(JsError::resource("JSON input exceeds 64 KiB"));
        }
        // Charge scanning before walking the data, including whitespace and
        // escapes. Individual nodes and allocations consume the usual budgets.
        self.remaining = self
            .remaining
            .checked_sub(source.len() as u64)
            .ok_or_else(|| JsError::resource("JSON parsing instruction budget exceeded"))?;
        let result = (|| {
            let mut parser = JsonParser {
                runtime: self,
                source: &source,
                index: 0,
            };
            let value = parser.value(0)?;
            parser.whitespace();
            if parser.index != source.len() {
                return Err(parser.error("Unexpected trailing JSON data"));
            }
            Ok(value)
        })();
        result.map_err(|mut error: JsError| {
            if error.kind == ErrorKind::Syntax {
                // Invalid data is a catchable runtime SyntaxError; a malformed
                // JavaScript program is still rejected before any execution.
                let exception = (|| {
                    let name = self.string("SyntaxError".into())?;
                    let message = self.string(error.message.clone())?;
                    self.object(
                        BTreeMap::from([("name".into(), name), ("message".into(), message)]),
                        None,
                    )
                })();
                match exception {
                    Ok(value) => {
                        error.kind = ErrorKind::Runtime;
                        error.thrown = Some(value);
                    }
                    Err(resource) => return resource,
                }
            }
            error
        })
    }

    pub(super) fn json_parse_with_host(
        &mut self,
        args: &[Value],
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let value = self.json_parse(args)?;
        let Some(reviver) = args
            .get(1)
            .filter(|value| self.is_callable_value(value, host))
            .cloned()
        else {
            return Ok(value);
        };
        let holder = self.object(BTreeMap::from([("".into(), value)]), None)?;
        self.json_revive(holder, "", &reviver, host, 0)
    }

    fn json_revive(
        &mut self,
        holder: Value,
        key: &str,
        reviver: &Value,
        host: &mut impl Host,
        depth: usize,
    ) -> Result<Value, JsError> {
        self.tick()?;
        if depth > MAX_DEPTH {
            return Err(JsError::resource("JSON reviver nesting exceeds 32 levels"));
        }
        let value = self.get(holder.clone(), key, host)?;
        let keys = match value {
            Value::Array(id) => {
                let Some(Heap::Array(values)) = self.heap.get(id) else {
                    return Err(JsError::runtime("Invalid JSON array"));
                };
                (0..values.len())
                    .map(|index| index.to_string())
                    .collect::<Vec<_>>()
            }
            Value::Object(_) => {
                let owner = self.heap_owner(&value)?;
                self.own_names(&value)?
                    .into_iter()
                    .filter(|key| self.own_enumerable(owner, key))
                    .collect()
            }
            _ => Vec::new(),
        };
        for key in keys {
            let replacement = self.json_revive(value.clone(), &key, reviver, host, depth + 1)?;
            if replacement == Value::Undefined {
                self.delete_property(value.clone(), &key, host)?;
            } else {
                // A reviver may change descriptors while walking siblings. A
                // failed ordinary definition is ignored by InternalizeJSONProperty.
                self.create_data_property(&value, &key, replacement, host)?;
            }
        }
        let key = self.string(key.into())?;
        self.call(reviver.clone(), vec![key, value], holder, host)
    }

    pub(super) fn json_stringify(
        &mut self,
        args: &[Value],
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let replacer = args
            .get(1)
            .filter(|value| self.is_callable_value(value, host))
            .cloned();
        let mut properties = None;
        if let Some(Value::Array(id)) = args.get(1) {
            let Some(Heap::Array(values)) = self.heap.get(*id) else {
                return Err(JsError::runtime("Invalid JSON replacer array"));
            };
            let length = values.len();
            let mut names = Vec::new();
            let mut bytes = 0usize;
            for index in 0..length {
                self.tick()?;
                let value = self.get(Value::Array(*id), &index.to_string(), host)?;
                let name = match value {
                    Value::String(text) => text.to_string(),
                    Value::Utf16(text) => String::from_utf16(&text).map_err(|_| {
                        JsError::runtime("Unpaired UTF-16 property keys are not implemented")
                    })?,
                    Value::Number(_) => self.text(&value, 0)?,
                    _ => continue,
                };
                if name.len() > 1024 {
                    return Err(JsError::resource(
                        "JSON replacer property name exceeds 1024 bytes",
                    ));
                }
                if !names.contains(&name) {
                    bytes = bytes.saturating_add(name.len());
                    if names.len() >= 4096 || bytes > MAX_INPUT {
                        return Err(JsError::resource("JSON replacer property budget exceeded"));
                    }
                    self.charge_string(name.len())?;
                    names.push(name);
                }
            }
            properties = Some(names);
        }
        let gap = match args.get(2) {
            Some(Value::String(text)) => text.encode_utf16().take(10).collect::<Vec<_>>(),
            Some(Value::Utf16(text)) => text.iter().copied().take(10).collect(),
            Some(Value::Number(number)) => vec![
                0x20;
                if number.is_nan() || *number <= 0. {
                    0
                } else {
                    number.floor().min(10.) as usize
                }
            ],
            _ => Vec::new(),
        };
        let holder = self.object(
            BTreeMap::from([("".into(), args.first().cloned().unwrap_or(Value::Undefined))]),
            None,
        )?;
        let mut serializer = JsonSerializer {
            runtime: self,
            host,
            replacer,
            properties,
            gap,
            stack: Vec::new(),
            output: Vec::new(),
        };
        if !serializer.property(holder, "", 0, false)? {
            return Ok(Value::Undefined);
        }
        let output = serializer.output;
        self.utf16_string(output)
    }
}

struct JsonSerializer<'runtime, 'host, H: Host> {
    runtime: &'runtime mut Runtime,
    host: &'host mut H,
    replacer: Option<Value>,
    properties: Option<Vec<String>>,
    gap: Vec<u16>,
    stack: Vec<usize>,
    output: Vec<u16>,
}

impl<H: Host> JsonSerializer<'_, '_, H> {
    fn append(&mut self, text: &str) -> Result<(), JsError> {
        self.append_units(&text.encode_utf16().collect::<Vec<_>>())
    }
    fn append_units(&mut self, text: &[u16]) -> Result<(), JsError> {
        if self.output.len().saturating_add(text.len()) > MAX_INPUT {
            return Err(JsError::resource("JSON output exceeds 64 KiB"));
        }
        self.runtime.remaining = self
            .runtime
            .remaining
            .checked_sub(text.len() as u64)
            .ok_or_else(|| JsError::resource("JSON serialization work budget exceeded"))?;
        self.output.extend_from_slice(text);
        Ok(())
    }
    fn quote(&mut self, text: &str) -> Result<(), JsError> {
        self.quote_units(&text.encode_utf16().collect::<Vec<_>>())
    }
    fn quote_units(&mut self, text: &[u16]) -> Result<(), JsError> {
        self.append("\"")?;
        for scalar in char::decode_utf16(text.iter().copied()) {
            let ch = match scalar {
                Ok(ch) => ch,
                Err(error) => {
                    self.append(&format!("\\u{:04x}", error.unpaired_surrogate()))?;
                    continue;
                }
            };
            match ch {
                '"' => self.append("\\\"")?,
                '\\' => self.append("\\\\")?,
                '\n' => self.append("\\n")?,
                '\r' => self.append("\\r")?,
                '\t' => self.append("\\t")?,
                '\u{8}' => self.append("\\b")?,
                '\u{c}' => self.append("\\f")?,
                ch if (ch as u32) < 0x20 => self.append(&format!("\\u{:04x}", ch as u32))?,
                ch => self.append(ch.encode_utf8(&mut [0; 4]))?,
            }
        }
        self.append("\"")
    }
    fn indent(&mut self, depth: usize) -> Result<(), JsError> {
        if self.gap.is_empty() {
            return Ok(());
        }
        self.append("\n")?;
        for _ in 0..depth {
            self.append_units(&self.gap.clone())?;
        }
        Ok(())
    }
    fn property(
        &mut self,
        holder: Value,
        key: &str,
        depth: usize,
        array: bool,
    ) -> Result<bool, JsError> {
        self.runtime.tick()?;
        if depth > MAX_DEPTH {
            return Err(JsError::resource(
                "JSON serialization nesting exceeds 32 levels",
            ));
        }
        let mut value = self.runtime.get(holder.clone(), key, self.host)?;
        if matches!(
            value,
            Value::Object(_) | Value::Array(_) | Value::Function(_) | Value::BigInt(_)
        ) {
            let to_json = self.runtime.get(value.clone(), "toJSON", self.host)?;
            if self.runtime.is_callable_value(&to_json, self.host) {
                let key_value = self.runtime.string(key.into())?;
                value = self
                    .runtime
                    .call(to_json, vec![key_value], value, self.host)?;
            }
        }
        if let Some(replacer) = self.replacer.clone() {
            let key_value = self.runtime.string(key.into())?;
            value = self
                .runtime
                .call(replacer, vec![key_value, value], holder, self.host)?;
        }
        if let Value::Object(id) = &value
            && let Some(primitive @ Value::BigInt(_)) = self.runtime.reflection.boxed.get(id)
        {
            value = primitive.clone();
        }
        match value {
            Value::BigInt(_) => {
                return Err(self
                    .runtime
                    .property_type_error("Cannot serialize BigInt to JSON"));
            }
            Value::Null => self.append("null")?,
            Value::Bool(value) => self.append(if value { "true" } else { "false" })?,
            Value::Number(number) => {
                if number.is_finite() {
                    self.append(&Value::Number(number).primitive_text()?)?;
                } else {
                    self.append("null")?;
                }
            }
            Value::String(text) => self.quote(&text)?,
            Value::Utf16(text) => self.quote_units(&text)?,
            Value::Array(id) | Value::Object(id) => {
                if self.stack.contains(&id) {
                    return Err(self
                        .runtime
                        .property_type_error("Converting circular structure to JSON"));
                }
                self.stack.push(id);
                let result = self.container(value, id, depth);
                self.stack.pop();
                result?;
            }
            Value::Host(_) => {
                return Err(JsError::runtime(
                    "JSON serialization of host objects is not implemented",
                ));
            }
            _ => {
                if array {
                    self.append("null")?;
                } else {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
    fn container(&mut self, value: Value, id: usize, depth: usize) -> Result<(), JsError> {
        if let Value::Array(_) = value {
            let Some(Heap::Array(values)) = self.runtime.heap.get(id) else {
                return Err(JsError::runtime("Invalid JSON array"));
            };
            let length = values.len();
            self.append("[")?;
            for index in 0..length {
                if index > 0 {
                    self.append(",")?;
                }
                self.indent(depth + 1)?;
                self.property(value.clone(), &index.to_string(), depth + 1, true)?;
            }
            if length > 0 {
                self.indent(depth)?;
            }
            return self.append("]");
        }
        let names = if let Some(properties) = &self.properties {
            properties.clone()
        } else {
            let owner = self.runtime.heap_owner(&value)?;
            self.runtime
                .own_names(&value)?
                .into_iter()
                .filter(|name| self.runtime.own_enumerable(owner, name))
                .collect()
        };
        self.append("{")?;
        let mut emitted = false;
        for key in names {
            let before = self.output.len();
            if emitted {
                self.append(",")?;
            }
            self.indent(depth + 1)?;
            self.quote(&key)?;
            self.append(if self.gap.is_empty() { ":" } else { ": " })?;
            if self.property(value.clone(), &key, depth + 1, false)? {
                emitted = true;
            } else {
                self.output.truncate(before);
            }
        }
        if emitted {
            self.indent(depth)?;
        }
        self.append("}")
    }
}

struct JsonParser<'runtime, 'source> {
    runtime: &'runtime mut Runtime,
    source: &'source str,
    index: usize,
}

impl JsonParser<'_, '_> {
    fn error(&self, message: &str) -> JsError {
        JsError::syntax(self.index, message)
    }
    fn whitespace(&mut self) {
        while self
            .source
            .as_bytes()
            .get(self.index)
            .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
        {
            self.index += 1;
        }
    }
    fn eat(&mut self, byte: u8) -> bool {
        if self.source.as_bytes().get(self.index) == Some(&byte) {
            self.index += 1;
            true
        } else {
            false
        }
    }
    fn value(&mut self, depth: usize) -> Result<Value, JsError> {
        self.runtime.tick()?;
        if depth > MAX_DEPTH {
            return Err(JsError::resource("JSON nesting exceeds 32 levels"));
        }
        self.whitespace();
        let next = self.source.as_bytes().get(self.index).copied();
        if depth == MAX_DEPTH && matches!(next, Some(b'{' | b'[')) {
            return Err(JsError::resource("JSON nesting exceeds 32 levels"));
        }
        match next {
            Some(b'"') => {
                let text = self.string()?;
                self.runtime.utf16_string(text)
            }
            Some(b'{') => {
                self.index += 1;
                let mut properties = BTreeMap::new();
                let mut order = Vec::new();
                self.whitespace();
                if !self.eat(b'}') {
                    loop {
                        self.whitespace();
                        if !self.eat(b'"') {
                            return Err(self.error("Expected quoted JSON property"));
                        }
                        self.index -= 1;
                        let key = String::from_utf16(&self.string()?).map_err(|_| {
                            JsError::runtime("Unpaired UTF-16 property keys are not implemented")
                        })?;
                        if key.len() > 1024 {
                            return Err(JsError::resource("JSON property name exceeds 1024 bytes"));
                        }
                        self.runtime.charge_string(key.len())?;
                        if properties.len() >= 128 && !properties.contains_key(&key) {
                            return Err(JsError::resource("JSON object property limit exceeded"));
                        }
                        self.whitespace();
                        if !self.eat(b':') {
                            return Err(self.error("Expected colon after JSON property"));
                        }
                        let value = self.value(depth + 1)?;
                        // Includes __proto__: data only, never a prototype setter.
                        if !properties.contains_key(&key) {
                            order.push(key.clone());
                        }
                        properties.insert(key, value);
                        self.whitespace();
                        if self.eat(b'}') {
                            break;
                        }
                        if !self.eat(b',') {
                            return Err(self.error("Expected comma or closing JSON object"));
                        }
                    }
                }
                let value = self.runtime.object(properties, None)?;
                if let Value::Object(id) = value {
                    self.runtime.remember_property_order(id, order);
                }
                Ok(value)
            }
            Some(b'[') => {
                self.index += 1;
                let mut values = Vec::new();
                self.whitespace();
                if !self.eat(b']') {
                    loop {
                        if values.len() >= 4096 {
                            return Err(JsError::resource("JSON array item limit exceeded"));
                        }
                        values.push(self.value(depth + 1)?);
                        self.whitespace();
                        if self.eat(b']') {
                            break;
                        }
                        if !self.eat(b',') {
                            return Err(self.error("Expected comma or closing JSON array"));
                        }
                    }
                }
                Ok(Value::Array(self.runtime.allocate(Heap::Array(values))?))
            }
            Some(b't') => self.literal("true", Value::Bool(true)),
            Some(b'f') => self.literal("false", Value::Bool(false)),
            Some(b'n') => self.literal("null", Value::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(self.error("Expected JSON value")),
        }
    }
    fn literal(&mut self, text: &str, value: Value) -> Result<Value, JsError> {
        if !self.source[self.index..].starts_with(text) {
            return Err(self.error("Invalid JSON literal"));
        }
        self.index += text.len();
        Ok(value)
    }
    fn digits(&mut self) -> bool {
        let start = self.index;
        while self
            .source
            .as_bytes()
            .get(self.index)
            .is_some_and(u8::is_ascii_digit)
        {
            self.index += 1;
        }
        self.index > start
    }
    fn number(&mut self) -> Result<Value, JsError> {
        let start = self.index;
        self.eat(b'-');
        if self.eat(b'0') {
            if self
                .source
                .as_bytes()
                .get(self.index)
                .is_some_and(u8::is_ascii_digit)
            {
                return Err(self.error("Leading zeros are not JSON numbers"));
            }
        } else if !self.digits() {
            return Err(self.error("Expected JSON number digits"));
        }
        if self.eat(b'.') && !self.digits() {
            return Err(self.error("Expected JSON fraction digits"));
        }
        if self.eat(b'e') || self.eat(b'E') {
            if !self.eat(b'+') {
                self.eat(b'-');
            }
            if !self.digits() {
                return Err(self.error("Expected JSON exponent digits"));
            }
        }
        let number = self.source[start..self.index]
            .parse::<f64>()
            .map_err(|_| self.error("Invalid JSON number"))?;
        Ok(Value::Number(number))
    }
    fn hex_unit(&mut self) -> Result<u16, JsError> {
        let mut unit = 0;
        for _ in 0..4 {
            let Some(&byte) = self.source.as_bytes().get(self.index) else {
                return Err(self.error("Incomplete JSON Unicode escape"));
            };
            let digit = match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                b'A'..=b'F' => byte - b'A' + 10,
                _ => return Err(self.error("Invalid JSON Unicode escape")),
            };
            unit = unit * 16 + u16::from(digit);
            self.index += 1;
        }
        Ok(unit)
    }
    fn string(&mut self) -> Result<Vec<u16>, JsError> {
        if !self.eat(b'"') {
            return Err(self.error("Expected JSON string"));
        }
        let mut output = Vec::new();
        loop {
            let Some(ch) = self.source[self.index..].chars().next() else {
                return Err(self.error("Unterminated JSON string"));
            };
            self.index += ch.len_utf8();
            match ch {
                '"' => return Ok(output),
                '\\' => {
                    let Some(&escaped) = self.source.as_bytes().get(self.index) else {
                        return Err(self.error("Incomplete JSON escape"));
                    };
                    self.index += 1;
                    output.push(match escaped {
                        b'"' => 0x22,
                        b'\\' => 0x5c,
                        b'/' => 0x2f,
                        b'b' => 0x8,
                        b'f' => 0xc,
                        b'n' => 0xa,
                        b'r' => 0xd,
                        b't' => 0x9,
                        b'u' => self.hex_unit()?,
                        _ => return Err(self.error("Invalid JSON escape")),
                    });
                }
                ch if (ch as u32) < 0x20 => {
                    return Err(self.error("Unescaped JSON control character"));
                }
                ch => output.extend_from_slice(ch.encode_utf16(&mut [0; 2])),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{ErrorKind, Host, JsError, Limits, Runtime, Value};
    struct Empty;
    impl Host for Empty {
        fn global(&mut self, _: &str) -> Option<Value> {
            None
        }
        fn get(&mut self, _: u64, _: &str) -> Result<Value, JsError> {
            panic!("JSON must not call host")
        }
        fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
            panic!("JSON must not call host")
        }
        fn call(&mut self, _: u64, _: &str, _: &[Value]) -> Result<Value, JsError> {
            panic!("JSON must not call host")
        }
    }
    fn run(source: &str) -> Result<Value, JsError> {
        Runtime::default().execute(source, &mut Empty)
    }
    #[test]
    fn parses_config_data_without_execution_or_prototype_mutation() {
        assert_eq!(run(r#"const c=JSON.parse('{"cfg":{"ok":true,"none":null},"a":[1,-2.5,3e2],"duplicate":1,"duplicate":2,"__proto__":{"polluted":true}}'); typeof JSON==='object' && typeof JSON.parse==='function' && c.cfg.ok && c.cfg.none===null && c.a[2]===300 && c.duplicate===2 && Object.hasOwn(c,'__proto__') && c.__proto__.polluted && Object.getPrototypeOf(c)===null && typeof JSON.stringify==='function';"#).unwrap(), Value::Bool(true));
        assert_eq!(run("JSON.parse(42);").unwrap(), Value::Number(42.));
        assert_eq!(run("JSON.parse(null);").unwrap(), Value::Null);
        assert_eq!(
            run("let JSON={parse:()=>7};JSON.parse();").unwrap(),
            Value::Number(7.)
        );
        assert_eq!(
            run("try{JSON.parse('invalid');}catch(e){e.name;}").unwrap(),
            Value::string("SyntaxError")
        );
        assert_eq!(
            run("const parse=JSON.parse;parse('[7]')[0];").unwrap(),
            Value::Number(7.)
        );
        struct Global;
        impl Host for Global {
            fn global(&mut self, name: &str) -> Option<Value> {
                (name == "window").then_some(Value::Host(1))
            }
            fn global_object(&self) -> Option<u64> {
                Some(1)
            }
            fn get(&mut self, _: u64, _: &str) -> Result<Value, JsError> {
                panic!("JSON alias must be resolved by the VM")
            }
            fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
                panic!("JSON alias must not call the host")
            }
            fn call(&mut self, _: u64, _: &str, _: &[Value]) -> Result<Value, JsError> {
                panic!("JSON alias must not call the host")
            }
        }
        assert_eq!(Runtime::default().execute("JSON===window.JSON && typeof window.JSON==='object' && window.JSON.parse('42')===42;",&mut Global).unwrap(),Value::Bool(true));
    }
    #[test]
    fn parses_escapes_unicode_and_rejects_malformed_json() {
        // JSON strings preserve UTF-16 code units, including unpaired surrogates.
        for (source, units) in [
            (r#""\uD800""#, vec![0xd800]),
            (r#""\uDC00""#, vec![0xdc00]),
            (r#""\uD800\u0041""#, vec![0xd800, 0x0041]),
        ] {
            assert_eq!(
                Runtime::default()
                    .json_parse(&[Value::string(source)])
                    .unwrap(),
                Value::Utf16(units.into()),
                "{source}"
            );
        }
        let mut runtime = Runtime::default();
        assert_eq!(
            runtime
                .json_parse(&[Value::string(r#""a\n\t\b\f\r\/\\\"\u00e1\uD83D\uDE00""#)])
                .unwrap(),
            Value::string("a\n\t\u{8}\u{c}\r/\\\"á😀")
        );
        for source in [
            "",
            "01",
            "-",
            "+1",
            "1.",
            "1e",
            "0x10",
            "NaN",
            "undefined",
            "[1,]",
            "{\"x\":1,}",
            "{'x':1}",
            "true false",
            "/*x*/null",
            "\u{a0}null",
            "\"\n\"",
            r#""\x41""#,
            r#""\uGGGG""#,
        ] {
            let error = Runtime::default()
                .json_parse(&[Value::string(source)])
                .unwrap_err();
            assert_eq!(error.kind, ErrorKind::Runtime, "{source}");
            assert!(error.thrown.is_some(), "{source}");
        }
        assert_eq!(
            run("JSON.parse('{\"x\":2}',(key,value)=>key==='x'?value*3:value).x;").unwrap(),
            Value::Number(6.)
        );
        assert_eq!(
            Runtime::default()
                .json_parse(&[Value::string("1e400")])
                .unwrap(),
            Value::Number(f64::INFINITY)
        );
        let negative_zero = Runtime::default()
            .json_parse(&[Value::string("-0")])
            .unwrap()
            .as_number()
            .unwrap();
        assert!(negative_zero.is_sign_negative());
    }
    #[test]
    fn enforces_nesting_counts_input_and_existing_heap_string_work_limits() {
        let parse = |source: String| {
            Runtime::default()
                .json_parse(&[Value::string(source)])
                .unwrap_err()
        };
        assert_eq!(
            parse(format!("{}0{}", "[".repeat(33), "]".repeat(33))).kind,
            ErrorKind::Resource
        );
        assert_eq!(
            parse(format!("{}{}", "[".repeat(33), "]".repeat(33))).kind,
            ErrorKind::Resource
        );
        assert!(
            Runtime::default()
                .json_parse(&[Value::string(format!(
                    "{}0{}",
                    "[".repeat(32),
                    "]".repeat(32)
                ))])
                .is_ok()
        );
        assert_eq!(
            parse(format!("[{}]", vec!["0"; 4097].join(","))).kind,
            ErrorKind::Resource
        );
        assert_eq!(
            run(&format!(
                "try{{JSON.parse('[{}]');}}catch(e){{'hidden';}}",
                vec!["0"; 4097].join(",")
            ))
            .unwrap_err()
            .kind,
            ErrorKind::Resource
        );
        assert_eq!(
            parse(format!(
                "{{{}}}",
                (0..129)
                    .map(|i| format!("\"k{i}\":0"))
                    .collect::<Vec<_>>()
                    .join(",")
            ))
            .kind,
            ErrorKind::Resource
        );
        assert_eq!(
            parse(format!("{{\"{}\":0}}", "k".repeat(1025))).kind,
            ErrorKind::Resource
        );
        assert_eq!(parse(" ".repeat(65_537)).kind, ErrorKind::Resource);
        for limits in [
            Limits {
                heap_objects: 0,
                ..Limits::default()
            },
            Limits {
                string_bytes: 0,
                ..Limits::default()
            },
            Limits {
                instructions: 1,
                ..Limits::default()
            },
        ] {
            assert_eq!(
                Runtime::new(limits)
                    .json_parse(&[Value::string("{\"x\":\"value\"}")])
                    .unwrap_err()
                    .kind,
                ErrorKind::Resource
            );
        }
    }
}
