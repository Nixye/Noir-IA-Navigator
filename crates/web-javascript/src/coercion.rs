//! Fallible coercion for language operators, property keys and selected globals.
//! Other native API argument conversions remain separate.
use crate::{Host, JsError, Runtime, Value, strict_equal};

#[derive(Clone, Copy)]
pub(super) enum Hint {
    Default,
    Number,
    String,
}

fn object(value: &Value) -> bool {
    matches!(
        value,
        Value::Object(_) | Value::Array(_) | Value::Function(_) | Value::Native(_) | Value::Host(_)
    )
}

impl Runtime {
    /// Observable implicit ToNumber for host APIs. Unlike Number(), rejects BigInt.
    pub fn coerce_to_number(&mut self, value: Value, host: &mut impl Host) -> Result<f64, JsError> {
        self.tick()?;
        let primitive = self.operator_primitive(value, Hint::Number, host)?;
        let (work, bytes) = match &primitive {
            Value::String(text) => (text.len(), 0),
            Value::Utf16(units) => (units.len(), units.len().saturating_mul(3)),
            _ => (0, 0),
        };
        self.remaining = self
            .remaining
            .checked_sub(work as u64)
            .ok_or_else(|| JsError::resource("Host number conversion work budget exceeded"))?;
        self.charge_string(bytes)?;
        let Value::Number(number) = self.primitive_number(&primitive)? else {
            unreachable!()
        };
        Ok(number)
    }
    /// Observable JavaScript ToString at the UTF-8 host boundary. Symbols and
    /// unpaired UTF-16 surrogates are rejected; normal coercion budgets apply.
    pub fn coerce_to_string(
        &mut self,
        value: Value,
        host: &mut impl Host,
    ) -> Result<String, JsError> {
        self.tick()?;
        let value = self.language_string(value, host)?;
        let (units, bytes) = match &value {
            Value::String(text) => (text.len(), text.len()),
            Value::Utf16(units) => (units.len(), units.len().saturating_mul(3)),
            _ => unreachable!("ToString returns a string"),
        };
        if units > 65_536 {
            return Err(JsError::resource("Host string conversion exceeds limit"));
        }
        self.remaining = self
            .remaining
            .checked_sub(units as u64)
            .ok_or_else(|| JsError::resource("Host string conversion work budget exceeded"))?;
        self.charge_string(bytes)?;
        let text = value.primitive_text()?;
        if text.len() > 65_536 {
            return Err(JsError::resource("Host string exceeds 64 KiB"));
        }
        Ok(text)
    }
    pub(super) fn charge_bigint_decimal(&mut self, number: &crate::BigInt) -> Result<(), JsError> {
        let digits = number.bits().div_ceil(3).saturating_add(2);
        let work = number.bits().div_ceil(32).max(1).saturating_mul(digits);
        self.remaining = self
            .remaining
            .checked_sub(work as u64)
            .ok_or_else(|| JsError::resource("BigInt decimal conversion work budget exceeded"))?;
        self.charge_string(digits)
    }

    /// The same entrypoint serves direct calls, callable adapters and `new`.
    pub(super) fn conversion_builtin(
        &mut self,
        name: &str,
        args: &[Value],
        construct: bool,
        host: &mut impl Host,
    ) -> Result<Option<Value>, JsError> {
        if !matches!(
            name,
            "Number" | "String" | "BigInt" | "BigInt.asIntN" | "BigInt.asUintN"
        ) {
            return Ok(None);
        }
        if construct && !matches!(name, "Number" | "String") {
            return Err(self.property_type_error("BigInt and its methods are not constructors"));
        }
        let first = args.first().cloned().unwrap_or(Value::Undefined);
        let result = match name {
            "Number" if args.is_empty() => Value::Number(0.),
            "Number" => {
                let primitive = self.operator_primitive(first, Hint::Number, host)?;
                if let Value::BigInt(number) = primitive {
                    // Explicit Number conversion permits BigInt, unlike unary +.
                    self.charge_bigint_decimal(&number)?;
                    Value::Number(number.to_number())
                } else {
                    let (work, bytes) = match &primitive {
                        Value::String(text) => (text.len(), 0),
                        Value::Utf16(units) => (units.len(), units.len().saturating_mul(3)),
                        _ => (0, 0),
                    };
                    self.remaining = self.remaining.checked_sub(work as u64).ok_or_else(|| {
                        JsError::resource("Number conversion work budget exceeded")
                    })?;
                    self.charge_string(bytes)?;
                    self.primitive_number(&primitive)?
                }
            }
            "String" if args.is_empty() => self.string(String::new())?,
            "String" => {
                if let Value::Symbol(_, description) = &first
                    && !construct
                {
                    return self.string(format!("Symbol({description})")).map(Some);
                }
                self.language_string(first, host)?
            }
            "BigInt" => {
                let primitive = self.operator_primitive(first, Hint::Number, host)?;
                let (units, bytes) = match &primitive {
                    Value::String(text) => (text.len(), 0),
                    Value::Utf16(units) => (units.len(), units.len().saturating_mul(3)),
                    _ => (0, 0),
                };
                if units > 16_384 {
                    return Err(JsError::resource("BigInt conversion input exceeds limit"));
                }
                let work = (units as u64).saturating_mul(128).saturating_add(1);
                self.remaining = self
                    .remaining
                    .checked_sub(work)
                    .ok_or_else(|| JsError::resource("BigInt conversion work budget exceeded"))?;
                if !matches!(primitive, Value::BigInt(_)) {
                    self.charge_string(bytes.saturating_add(512))?;
                }
                self.bigint_convert(&primitive)?
            }
            // Width validation must complete before invoking the value hook.
            "BigInt.asIntN" | "BigInt.asUintN" => {
                let bits = self.operator_primitive(first, Hint::Number, host)?;
                let width = self.bigint_width(bits)?;
                let value = self.operator_primitive(
                    args.get(1).cloned().unwrap_or(Value::Undefined),
                    Hint::Number,
                    host,
                )?;
                self.bigint_truncate_width(width, value, name == "BigInt.asIntN")?
            }
            _ => unreachable!(),
        };
        if construct {
            self.box_value(result).map(Some)
        } else {
            Ok(Some(result))
        }
    }

    /// Implicit ToString; the explicit String(Symbol) exception stays in the builtin.
    pub(super) fn language_string(
        &mut self,
        value: Value,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let primitive = self.operator_primitive(value, Hint::String, host)?;
        match primitive {
            Value::String(_) | Value::Utf16(_) => Ok(primitive),
            Value::Symbol(..) => {
                Err(self.property_type_error("Cannot convert a Symbol to a string"))
            }
            Value::BigInt(number) => {
                self.charge_bigint_decimal(&number)?;
                self.string(number.text(10))
            }
            _ => self.string(primitive.primitive_text()?),
        }
    }

    /// Property storage is UTF-8 backed. Preserve Symbol identity and reject
    /// unpaired UTF-16 surrogates rather than replacing or aliasing key units.
    pub(super) fn coerce_property_key(
        &mut self,
        value: Value,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let primitive = self.operator_primitive(value, Hint::String, host)?;
        if matches!(primitive, Value::Symbol(..)) {
            return Ok(primitive);
        }
        if let Value::String(text) = &primitive {
            if text.len() > 1024 {
                return Err(JsError::resource("Property name exceeds 1024 bytes"));
            }
            return Ok(primitive);
        }
        let text = if let Value::Utf16(units) = &primitive {
            if units.len() > 1024 {
                return Err(JsError::resource("Property name exceeds 1024 bytes"));
            }
            String::from_utf16(units).map_err(|_| {
                self.property_type_error(
                    "Property keys with unpaired UTF-16 surrogates are not supported",
                )
            })?
        } else {
            primitive.primitive_text()?
        };
        if text.len() > 1024 {
            return Err(JsError::resource("Property name exceeds 1024 bytes"));
        }
        self.string(text)
    }

    pub(super) fn operator_primitive(
        &mut self,
        value: Value,
        hint: Hint,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        self.tick()?;
        if !object(&value) {
            return Ok(value);
        }
        // Hooks and their getters can reenter conversion through JavaScript.
        // Their native frames are larger than ordinary calls, so bound active
        // object conversions separately rather than rejecting repeated identity.
        if self.conversion_depth >= 8 {
            return Err(JsError::resource("Object conversion depth exceeds 8"));
        }
        self.conversion_depth += 1;
        let result = self.operator_object_primitive(value, hint, host);
        self.conversion_depth -= 1;
        result
    }

    fn operator_object_primitive(
        &mut self,
        value: Value,
        hint: Hint,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        if let Value::Host(object) = &value
            && let Some(text) = host.stringifier(*object)?
        {
            self.remaining = self
                .remaining
                .checked_sub(text.len() as u64)
                .ok_or_else(|| JsError::resource("Host stringifier work budget exceeded"))?;
            return self.string(text);
        }
        // Native/host exotics do not yet expose ordinary descriptor semantics.
        // In particular, Date must not silently use the ordinary default hint.
        if matches!(value, Value::Native(_) | Value::Host(_)) || self.stdlib_is_exotic(&value) {
            return Err(self.property_type_error(
                "Operator coercion of native/host exotic objects is not implemented",
            ));
        }
        let Value::Symbol(key, _) = self.well_known_symbol("toPrimitive")? else {
            unreachable!()
        };
        let exotic = self.get_symbol(value.clone(), key, host)?;
        if !matches!(exotic, Value::Undefined | Value::Null) {
            if !self.is_callable_value(&exotic, host) {
                return Err(self.property_type_error("Symbol.toPrimitive must be callable"));
            }
            let hint = self.string(
                match hint {
                    Hint::Default => "default",
                    Hint::Number => "number",
                    Hint::String => "string",
                }
                .into(),
            )?;
            let result = self.call(exotic, vec![hint], value, host)?;
            if object(&result) {
                return Err(self.property_type_error("Symbol.toPrimitive must return a primitive"));
            }
            return Ok(result);
        }
        let methods = if matches!(hint, Hint::String) {
            ["toString", "valueOf"]
        } else {
            ["valueOf", "toString"]
        };
        for name in methods {
            let method = self.get(value.clone(), name, host)?;
            if self.is_callable_value(&method, host) {
                let result = self.call(method, vec![], value.clone(), host)?;
                if !object(&result) {
                    return Ok(result);
                }
            }
        }
        Err(self.property_type_error("Cannot convert object to primitive value"))
    }

    fn primitive_number(&mut self, value: &Value) -> Result<Value, JsError> {
        if matches!(value, Value::Symbol(..) | Value::BigInt(_)) {
            return Err(
                self.property_type_error("Cannot implicitly convert Symbol or BigInt to Number")
            );
        }
        debug_assert!(!object(value));
        Ok(Value::Number(self.number(value)))
    }

    pub(super) fn operator_numeric(
        &mut self,
        value: Value,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let value = self.operator_primitive(value, Hint::Number, host)?;
        if matches!(value, Value::BigInt(_)) {
            Ok(value)
        } else {
            self.primitive_number(&value)
        }
    }

    pub(super) fn operator_number(
        &mut self,
        value: Value,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let value = self.operator_primitive(value, Hint::Number, host)?;
        self.primitive_number(&value)
    }

    fn operator_equal(
        &mut self,
        mut left: Value,
        mut right: Value,
        host: &mut impl Host,
    ) -> Result<bool, JsError> {
        // Each iteration removes an object or Boolean operand; no conversion is
        // replayed. The explicit bound is defensive against future extensions.
        for _ in 0..8 {
            self.tick()?;
            if std::mem::discriminant(&left) == std::mem::discriminant(&right)
                || (matches!(left, Value::String(_) | Value::Utf16(_))
                    && matches!(right, Value::String(_) | Value::Utf16(_)))
                || (object(&left) && object(&right))
            {
                return Ok(strict_equal(&left, &right));
            }
            if matches!(
                (&left, &right),
                (Value::Null, Value::Undefined) | (Value::Undefined, Value::Null)
            ) {
                return Ok(true);
            }
            if let Value::Bool(value) = left {
                left = Value::Number(if value { 1. } else { 0. });
                continue;
            }
            if let Value::Bool(value) = right {
                right = Value::Number(if value { 1. } else { 0. });
                continue;
            }
            let comparable = |value: &Value| {
                matches!(
                    value,
                    Value::Number(_)
                        | Value::BigInt(_)
                        | Value::String(_)
                        | Value::Utf16(_)
                        | Value::Symbol(..)
                )
            };
            if object(&left) && comparable(&right) {
                left = self.operator_primitive(left, Hint::Default, host)?;
                continue;
            }
            if comparable(&left) && object(&right) {
                right = self.operator_primitive(right, Hint::Default, host)?;
                continue;
            }
            if matches!(left, Value::BigInt(_)) || matches!(right, Value::BigInt(_)) {
                let Value::Bool(equal) = self.bigint_binary("==", &left, &right)? else {
                    unreachable!()
                };
                return Ok(equal);
            }
            if matches!(
                (&left, &right),
                (Value::Number(_), Value::String(_) | Value::Utf16(_))
                    | (Value::String(_) | Value::Utf16(_), Value::Number(_))
            ) {
                return Ok(self.number(&left) == self.number(&right));
            }
            return Ok(false);
        }
        Err(JsError::resource("Equality conversion limit exceeded"))
    }

    pub(super) fn binary(
        &mut self,
        op: &str,
        left: Value,
        right: Value,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        if matches!(op, "===" | "!==") {
            let equal = strict_equal(&left, &right);
            return Ok(Value::Bool(if op == "===" { equal } else { !equal }));
        }
        if matches!(op, "==" | "!=") {
            let equal = self.operator_equal(left, right, host)?;
            return Ok(Value::Bool(if op == "==" { equal } else { !equal }));
        }
        let (left, right) = if op == "+" || matches!(op, "<" | ">" | "<=" | ">=") {
            let hint = if op == "+" {
                Hint::Default
            } else {
                Hint::Number
            };
            // Even > and <= perform observable conversions in source order.
            let left = self.operator_primitive(left, hint, host)?;
            let right = self.operator_primitive(right, hint, host)?;
            (left, right)
        } else {
            let left = self.operator_numeric(left, host)?;
            let right = self.operator_numeric(right, host)?;
            (left, right)
        };
        self.primitive_binary(op, left, right)
    }
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn hooks_receive_hints_and_run_in_source_order_once() {
        assert_eq!(run("let log='';let a={};let b={};a[Symbol.toPrimitive]=function(h){log+='a'+h;return 2};b[Symbol.toPrimitive]=function(h){log+='b'+h;return 3};let sum=a+b;let more=a>b;sum===5 && !more && log==='adefaultbdefaultanumberbnumber';").unwrap(), Value::Bool(true));
        assert_eq!(run("let log='';function a(){log+='A';return {valueOf:function(){log+='a';return 2}}}function b(){log+='B';return {valueOf:function(){log+='b';return 3}}}a()*b()===6 && log==='ABab';").unwrap(), Value::Bool(true));
    }

    #[test]
    fn ordinary_fallback_and_wrapper_overrides_are_observed() {
        for source in [
            "let log='';let a={valueOf:function(){log+='v';return {}},toString:function(){log+='s';return '4'}};a+2==='42' && log==='vs';",
            "let a=Object(4n);a.valueOf=function(){return 7n};a+2n===9n && -a===-7n && ~a===-8n;",
            "let a=Object(4);a.valueOf=function(){return 9};a*2===18;",
            "let a=Object.create(null);a.toString=function(){return '8'};+a===8;",
            "let a={valueOf:17,toString:function(){return '8'}};+a===8;",
            "let a={};a[Symbol.toPrimitive]=null;a.valueOf=function(){return 6};a-1===5;",
        ] {
            assert_eq!(run(source).unwrap(), Value::Bool(true), "{source}");
        }
    }

    #[test]
    fn updates_preserve_places_and_return_converted_old_values() {
        assert_eq!(run("let reads=0;let writes=0;let calls=0;let keys=0;let object={};Object.defineProperty(object,'x',{get:function(){reads++;return {valueOf:function(){calls++;return 5n}}},set:function(v){writes++;if(v!==6n)throw 'bad'}});function key(){keys++;return 'x'}let previous=object[key()]++;previous===5n && reads===1 && writes===1 && calls===1 && keys===1;").unwrap(), Value::Bool(true));
        assert_eq!(
            run("let x={valueOf:function(){return '3'}};let old=x++;old===3 && x===4;").unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            run("let x={valueOf:function(){return 3n}};--x===2n && x===2n;").unwrap(),
            Value::Bool(true)
        );
    }

    #[test]
    fn identity_truthiness_typeof_and_null_equality_do_not_coerce() {
        assert_eq!(run("let n=0;let a={valueOf:function(){n++;throw 9}};let b={};a===a && a!==b && !(a==b) && !(a==null) && !(a==undefined) && !!a && typeof a==='object' && n===0;").unwrap(), Value::Bool(true));
        assert_eq!(run("let n=0;let a={valueOf:function(){n++;return 3n}};a==3 && 3==a && a=='3' && a!=4 && n===4;").unwrap(), Value::Bool(true));
        assert_eq!(run("let a={};let s=Symbol('x');a[Symbol.toPrimitive]=function(){return s};a==s && s==a;").unwrap(), Value::Bool(true));
    }

    #[test]
    fn invalid_hooks_and_numeric_types_throw_without_replaying_effects() {
        for source in [
            "let a={};a[Symbol.toPrimitive]=3;a+1;",
            "let a={};a[Symbol.toPrimitive]=function(){return {}};a+1;",
            "let a={valueOf:function(){return {}},toString:function(){return {}}};a+1;",
            "+Object(2n);",
            "Object(2n)+1;",
            "Object(2n)>>>1n;",
            "-Symbol('x');",
            "new Date(0)+1;",
            "new Date(0)-1;",
        ] {
            let error = run(source).unwrap_err();
            assert_eq!(error.kind, ErrorKind::Runtime, "{source}");
            assert!(error.thrown.is_some(), "{source}");
        }
        assert_eq!(run("let n=0;let a={valueOf:function(){n++;throw 77}};let b={valueOf:function(){n+=10;return 2}};let caught=false;try{a*b}catch(e){caught=e===77}caught && n===1;").unwrap(), Value::Bool(true));
        assert_eq!(run("let n=0;let a={};Object.defineProperty(a,Symbol.toPrimitive,{get:function(){n++;throw 77}});let caught=false;try{a+1}catch(e){caught=e===77}caught && n===1;").unwrap(), Value::Bool(true));
        // ToNumeric(left) fails before the right numeric hook, but + performs
        // both ToPrimitive calls before attempting string/numeric addition.
        assert_eq!(run("let n=0;let a={valueOf:function(){return Symbol('x')}};let b={valueOf:function(){n++;return 1}};try{a*b}catch(e){}let first=n;try{a+b}catch(e){}first===0 && n===1;").unwrap(), Value::Bool(true));
    }

    #[test]
    fn arrays_use_string_hints_for_nested_join_conversions() {
        assert_eq!(run("let log='';let a={};a[Symbol.toPrimitive]=function(h){log+=h;return '4'};[a]+2==='42' && log==='string';").unwrap(), Value::Bool(true));
        assert_eq!(run("let n=0;let a=[];Object.defineProperty(a,'0',{get:function(){n++;return {toString:function(){return '8'}}}});a.length=1;+a===8 && n===1;").unwrap(), Value::Bool(true));
        assert_eq!(run("let log='';let s={toString:function(){log+='s';return '-'}};let a={toString:function(){log+='a';return 'x'}};[a,a].join(s)==='x-x' && log==='saa';").unwrap(), Value::Bool(true));
    }
    #[test]
    fn computed_keys_use_string_hints_and_preserve_symbols() {
        for source in [
            "let hints='';const s=Symbol('key');const k={};k[Symbol.toPrimitive]=function(h){hints+=h;return s};const o={};o[k]=4;const got=o[k];const exists=k in o;const gone=delete o[k];got===4 && exists && gone && !(s in o) && hints==='stringstringstringstring';",
            "let log='';const k={toString:function(){log+='s';return {}},valueOf:function(){log+='v';return 12n}};const o={};o[k]=7;o['12']===7 && log==='sv';",
            "let n=0;const k={};Object.defineProperty(k,Symbol.toPrimitive,{get:function(){n++;return function(h){if(h!=='string')throw 'hint';return 'x'}}});const o={x:9};o[k]===9 && n===1;",
            "const o={};o[-0]=1;o[null]=2;o[undefined]=3;o[true]=4;o[2n]=5;o['0']===1 && o['null']===2 && o['undefined']===3 && o['true']===4 && o['2']===5;",
        ] {
            assert_eq!(run(source).unwrap(), Value::Bool(true), "{source}");
        }
    }

    #[test]
    fn computed_references_convert_before_rhs_or_arguments_once() {
        for source in [
            "let log='';const o={};const k={toString:function(){log+='k';return 'x'}};function base(){log+='b';return o}function key(){log+='e';return k}function rhs(){log+='r';return 3}base()[key()]=rhs();o.x===3 && log==='bekr';",
            "let log='';let stored=2;const o={};Object.defineProperty(o,'x',{get:function(){log+='g';return stored},set:function(v){log+='s';stored=v}});const k={toString:function(){log+='k';return 'x'}};function rhs(){log+='r';return 3}o[k]+=rhs();stored===5 && log==='kgrs';",
            "let log='';const o={tag:8};Object.defineProperty(o,'f',{get:function(){log+='g';return function(x){log+='c';return this.tag+x}}});const k={toString:function(){log+='k';return 'f'}};function arg(){log+='a';return 2}o[k](arg())===10 && log==='kgac';",
            "let n=0;const k={toString:function(){n++;return 'x'}};const o={x:2};let old=o[k]++;old===2 && o.x===3 && n===1;",
        ] {
            assert_eq!(run(source).unwrap(), Value::Bool(true), "{source}");
        }
    }

    #[test]
    fn object_and_reflect_apis_share_key_conversion_and_symbols() {
        for source in [
            "let n=0;const s=Symbol();const k={};k[Symbol.toPrimitive]=function(h){if(h!=='string')throw 'hint';n++;return s};const o={};Object.defineProperty(o,k,{value:4,writable:true,enumerable:true,configurable:true});Object.getOwnPropertyDescriptor(o,k).value===4 && Object.hasOwn(o,k) && o.hasOwnProperty(k) && o.propertyIsEnumerable(k) && n===5;",
            "let n=0;const s=Symbol();const k={};k[Symbol.toPrimitive]=function(h){if(h!=='string')throw 'hint';n++;return s};const o={};let ok=Reflect.defineProperty(o,k,{value:4,writable:true,enumerable:true,configurable:true});ok=ok && Reflect.get(o,k)===4 && Reflect.has(o,k) && Reflect.set(o,k,5) && Reflect.getOwnPropertyDescriptor(o,k).value===5 && Reflect.deleteProperty(o,k);ok && n===6 && !(s in o);",
            "let log='';const k={toString:function(){log+='k';return 'x'}};const d={};Object.defineProperty(d,'value',{get:function(){log+='d';return 7}});const o={};Object.defineProperty(o,k,d);Reflect.defineProperty({},k,d);o.x===7 && log==='kdkd';",
            "const k={toString:function(){return '0'}};Object.hasOwn('abc',k) && Object.getOwnPropertyDescriptor('abc',k).value==='a';",
        ] {
            assert_eq!(run(source).unwrap(), Value::Bool(true), "{source}");
        }
    }

    #[test]
    fn invalid_targets_and_prototype_methods_preserve_conversion_order() {
        assert_eq!(run("let n=0;const k={toString:function(){n++;return 'x'}};let failures=0;try{k in 3}catch(e){if(e.name==='TypeError')failures++}try{Reflect.get(3,k)}catch(e){if(e.name==='TypeError')failures++}try{Object.hasOwn(null,k)}catch(e){if(e.name==='TypeError')failures++}try{Object.getOwnPropertyDescriptor(null,k)}catch(e){if(e.name==='TypeError')failures++}try{Object.defineProperty(3,k,{})}catch(e){failures++}n===0 && failures===5;").unwrap(), Value::Bool(true));
        assert_eq!(run("let n=0;const k={toString:function(){n++;throw 17}};let failures=0;try{Object.prototype.hasOwnProperty.call(null,k)}catch(e){if(e===17)failures++}try{Object.prototype.propertyIsEnumerable.call(undefined,k)}catch(e){if(e===17)failures++}n===2 && failures===2;").unwrap(), Value::Bool(true));
        assert_eq!(run("let log='';function key(){log+='k';return {toString:function(){log+='c';return 'x'}}}function target(){log+='t';return {x:1}}(key() in target()) && log==='ktc';").unwrap(), Value::Bool(true));
    }

    #[test]
    fn key_errors_do_not_replay_or_continue_effects() {
        assert_eq!(run("let n=0;const k={};Object.defineProperty(k,Symbol.toPrimitive,{get:function(){n++;throw 17}});function rhs(){n+=10;return 1}let caught=false;try{({})[k]=rhs()}catch(e){caught=e===17}caught && n===1;").unwrap(), Value::Bool(true));
        for source in [
            "let k={};k[Symbol.toPrimitive]=7;({})[k];",
            "let k={};k[Symbol.toPrimitive]=function(){return {}};Reflect.has({},k);",
            "let k={toString:function(){return {}},valueOf:function(){return {}}};delete ({})[k];",
            "const k=String.fromCharCode(0xd800);({})[k];",
            "const k={toString:function(){return String.fromCharCode(0xdfff)}};Object.defineProperty({},k,{value:1});",
        ] {
            let error = run(source).unwrap_err();
            assert_eq!(error.kind, ErrorKind::Runtime, "{source}");
            assert!(error.thrown.is_some(), "{source}");
        }
        assert_eq!(
            run("let k='x'.repeat(1025);try{({})[k]}catch(e){42}")
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
        assert_eq!(
            run("let o={};let k={toString:function(){return o[k]}};try{o[k]}catch(e){42}")
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
    }

    #[test]
    fn super_keys_are_converted_but_symbol_super_is_explicitly_refused() {
        assert_eq!(run("let n=0;const k={toString:function(){n++;return 'x'}};class A{get x(){return this.value}}class B extends A{read(){return super[k]}}let b=new B();b.value=8;b.read()===8 && n===1;").unwrap(), Value::Bool(true));
        assert_eq!(run("const s=Symbol();const k={};k[Symbol.toPrimitive]=function(){return s};class A{}class B extends A{read(){return super[k]}}let ok=false;try{new B().read()}catch(e){ok=e.name==='TypeError'}ok;").unwrap(), Value::Bool(true));
    }

    #[test]
    fn global_conversions_use_hints_and_wrapper_overrides() {
        for source in [
            "let log='';const o={};o[Symbol.toPrimitive]=function(h){log+=h;return h==='string'?'seven':7n};Number(o)===7 && String(o)==='seven' && BigInt(o)===7n && log==='numberstringnumber';",
            "const o=Object(4n);o.valueOf=function(){return 9n};o.toString=function(){return 'changed'};Number(o)===9 && BigInt(o)===9n && String(o)==='changed';",
            "Number()===0 && Number(undefined)!==Number(undefined) && String()==='' && String(undefined)==='undefined' && new Number().valueOf()===0 && new String().valueOf()==='';",
            "new Number(Object(12n)).valueOf()===12 && new String(Object(12n)).valueOf()==='12' && BigInt(Object(12))===12n;",
            "const o={valueOf:function(){return {}},toString:function(){return '23'}};Number(o)===23 && BigInt(o)===23n;",
        ] {
            assert_eq!(run(source).unwrap(), Value::Bool(true), "{source}");
        }
    }

    #[test]
    fn string_conversion_preserves_utf16_and_symbol_exception_scope() {
        // Construct UTF-16 units at runtime; surrogate literal escapes remain unsupported.
        for source in [
            "const o={toString:function(){return String.fromCharCode(0xd800)}};String(o).length===1 && String(o).charCodeAt(0)===55296 && new String(o).valueOf()===String.fromCharCode(0xd800);",
            "String(Symbol('tag'))==='Symbol(tag)' && String(Symbol())==='Symbol()';",
            "let n=0;const o={};o[Symbol.toPrimitive]=function(h){n++;return Symbol('tag')};let ok=false;try{String(o)}catch(e){ok=e.name==='TypeError'}ok && n===1;",
        ] {
            assert_eq!(run(source).unwrap(), Value::Bool(true), "{source}");
        }
        for source in [
            "new String(Symbol('x'));",
            "Number(Symbol());",
            "new Number(Symbol());",
            "BigInt(Symbol());",
            "BigInt();",
        ] {
            let script =
                format!("let ok=false;try{{{source}}}catch(e){{ok=e.name==='TypeError'}}ok;");
            assert_eq!(run(&script).unwrap(), Value::Bool(true), "{source}");
        }
        assert_eq!(run("let ok=false;try{BigInt({valueOf:function(){return 1.5}})}catch(e){ok=e.name==='RangeError'}ok;").unwrap(), Value::Bool(true));
        assert_eq!(run("let ok=false;try{BigInt({valueOf:function(){return String.fromCharCode(0xd800)}})}catch(e){ok=e.name==='SyntaxError'}ok;").unwrap(), Value::Bool(true));
    }

    #[test]
    fn conversion_adapters_and_bound_constructors_use_same_path() {
        for source in [
            "let n=0;const o={valueOf:function(){n++;return 5n}};const bound=Number.bind(null,o);Number.call(null,o)===5 && Number.apply(null,[o])===5 && bound()===5 && new bound().valueOf()===5 && n===4;",
            "let n=0;const o={toString:function(){n++;return 'ok'}};const bound=String.bind(null,o);String.call(null,o)==='ok' && String.apply(null,[o])==='ok' && new bound().valueOf()==='ok' && n===3;",
            "const o={valueOf:function(){return 5}};BigInt.call(null,o)===5n && BigInt.apply(null,[o])===5n && BigInt.bind(null,o)()===5n;",
            "let n=0;const o={valueOf:function(){n++;return 1}};const bound=BigInt.bind(null,o);let ok=false;try{new bound()}catch(e){ok=e.name==='TypeError'}ok && n===0;",
        ] {
            assert_eq!(run(source).unwrap(), Value::Bool(true), "{source}");
        }
    }

    #[test]
    fn bigint_width_finishes_before_value_conversion() {
        for source in [
            "let log='';const bits={};bits[Symbol.toPrimitive]=function(h){log+='b'+h;return 8};const value={};value[Symbol.toPrimitive]=function(h){log+='v'+h;return 255n};BigInt.asIntN(bits,value)===-1n && log==='bnumbervnumber';",
            "let n=0;const value={valueOf:function(){n++;return 1n}};let ok=false;try{BigInt.asUintN({valueOf:function(){return -1}},value)}catch(e){ok=e.name==='RangeError'}ok && n===0;",
            "let n=0;const value={valueOf:function(){n++;return 1n}};let ok=false;try{BigInt.asUintN({valueOf:function(){return 1n}},value)}catch(e){ok=e.name==='TypeError'}ok && n===0;",
            "let n=0;const value={valueOf:function(){n++;return 1}};let ok=false;try{BigInt.asIntN(0,value)}catch(e){ok=e.name==='TypeError'}ok && n===1;",
            "let n=0;const value={valueOf:function(){n++;return '12'}};BigInt.asUintN(NaN,value)===0n && n===1;",
            "BigInt.asUintN.call(null,{valueOf:function(){return 8}},Object(-1n))===255n && BigInt.asIntN.apply(null,[8,Object(255n)])===-1n;",
        ] {
            assert_eq!(run(source).unwrap(), Value::Bool(true), "{source}");
        }
        assert_eq!(run("let n=0;const o={valueOf:function(){n++;return 1n}};let ok=false;try{BigInt.asIntN(9007199254740992,o)}catch(e){ok=e.name==='RangeError'}ok && n===0;").unwrap(), Value::Bool(true));
    }

    #[test]
    fn conversion_throws_and_resource_errors_escape_without_replay() {
        for name in ["Number", "String", "BigInt"] {
            let script = format!(
                "let n=0;const o={{}};Object.defineProperty(o,Symbol.toPrimitive,{{get:function(){{n++;throw 17}}}});let ok=false;try{{{name}(o)}}catch(e){{ok=e===17}}ok && n===1;"
            );
            assert_eq!(run(&script).unwrap(), Value::Bool(true), "{name}");
        }
        assert_eq!(run("let n=0;const o={valueOf:function(){n++;return 1}};let ok=false;try{new BigInt(o)}catch(e){ok=e.name==='TypeError'}ok && n===0;").unwrap(), Value::Bool(true));
        let error =
            run("const o={valueOf:function(){return Number(o)}};try{Number(o)}catch(e){42}")
                .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert!(error.thrown.is_none());
        let error = run(
            "const value={valueOf:function(){throw 17}};try{BigInt.asIntN(4097,value)}catch(e){42}",
        )
        .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert!(error.thrown.is_none());
    }

    #[test]
    fn conversion_recursion_is_a_resource_error_not_a_catchable_throw() {
        let error = run("let a={valueOf:function(){return +a}};try{+a}catch(e){42}").unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert!(error.thrown.is_none());
    }
    #[test]
    fn conversion_depth_allows_finite_reentry_and_releases_on_errors() {
        for source in [
            "let n=0;const o={valueOf:function(){n++;return n<8?+o:7}};+o===7 && n===8;",
            "let n=0;const o={valueOf:function(){n++;return n<8?Number(o):7}};Number(o)===7 && n===8;",
            "let n=0;const o={toString:function(){n++;return n<8?String(o):'ok'}};String(o)==='ok' && n===8;",
            "let n=0;const o={};Object.defineProperty(o,Symbol.toPrimitive,{get:function(){n++;if(n<8){+o}return function(){return 7}}});+o===7 && n===8;",
            "const o={valueOf:function(){throw 17}};let n=0;for(let i=0;i<20;i++){try{+o}catch(e){if(e===17)n++}}n===20;",
            "const o={};o[Symbol.toPrimitive]=7;let n=0;for(let i=0;i<20;i++){try{+o}catch(e){n++}}n===20;",
        ] {
            let mut vm = Runtime::default();
            assert_eq!(
                vm.execute(source, &mut Empty).unwrap(),
                Value::Bool(true),
                "{source}"
            );
            assert_eq!(vm.conversion_depth, 0);
            assert_eq!(vm.call_depth, 0);
            assert_eq!(vm.eval_depth, 0);
        }
    }

    #[test]
    fn conversion_depth_abort_skips_catch_finally_and_preserves_work() {
        for setup in [
            "let o={valueOf:function(){entered++;return +o}};",
            "let b={valueOf:function(){entered++;return +o}};let o={valueOf:function(){entered++;return +b}};",
            "let o={valueOf:function(){entered++;return Number(o)}};",
            "let o={toString:function(){entered++;return String(o)}};",
            "let o={};o[Symbol.toPrimitive]=function(){entered++;return +o};",
            "let o={};Object.defineProperty(o,Symbol.toPrimitive,{get:function(){entered++;return +o}});",
            "let a={};let o={toString:function(){entered++;return a[o]}};",
        ] {
            let mut vm = Runtime::default();
            vm.execute(
                "let entered=0;let caught=0;let finalized=0;let jobs=0;",
                &mut Empty,
            )
            .unwrap();
            let previous = vm.clone();
            let before = vm.remaining_instructions();
            let source = format!(
                "{setup}Promise.resolve().then(function(){{jobs++}});try{{String(o);+o}}catch(e){{caught++}}finally{{finalized++}}"
            );
            let error = vm.execute(&source, &mut Empty).unwrap_err();
            assert_eq!(error.kind, ErrorKind::Resource, "{setup}");
            assert!(error.thrown.is_none());
            assert_eq!(
                error.message,
                if setup.contains("return String(o)") {
                    // String's native adapter also occupies a call frame; at
                    // eight hooks its next call reaches the absolute ceiling.
                    "JavaScript call stack budget exceeded"
                } else {
                    "Object conversion depth exceeds 8"
                },
                "{setup}"
            );
            assert_eq!(
                vm.global_value("entered"),
                Some(Value::Number(8.)),
                "{setup}"
            );
            for name in ["caught", "finalized", "jobs"] {
                assert_eq!(
                    vm.global_value(name),
                    Some(Value::Number(0.)),
                    "{setup}: {name}"
                );
            }
            assert_eq!(vm.conversion_depth, 0);
            assert_eq!(vm.call_depth, 0);
            assert_eq!(vm.eval_depth, 0);
            let spent = vm.remaining_instructions();
            assert!(spent < before);
            vm.rollback_to(previous);
            assert_eq!(vm.remaining_instructions(), spent);
            assert_eq!(vm.conversion_depth, 0);
            assert_eq!(
                vm.execute("+{valueOf:function(){return 7}}", &mut Empty)
                    .unwrap(),
                Value::Number(7.)
            );
        }
    }
    #[test]
    fn conversion_depth_snapshot_keeps_active_outer_guard() {
        // A host transaction may snapshot the VM while a conversion is active.
        // Restoring that snapshot must not reset or double-charge its guard.
        let mut vm = Runtime::default();
        let value = vm
            .execute("({valueOf:function(){return 7}})", &mut Empty)
            .unwrap();
        vm.conversion_depth = 3;
        let previous = vm.clone();
        let before = vm.remaining_instructions();
        assert_eq!(vm.coerce_to_number(value, &mut Empty).unwrap(), 7.);
        assert_eq!(vm.conversion_depth, 3);
        let spent = vm.remaining_instructions();
        assert!(spent < before);
        vm.rollback_to(previous);
        assert_eq!(vm.conversion_depth, 3);
        assert_eq!(vm.remaining_instructions(), spent);
        vm.conversion_depth = 0;
    }
    #[test]
    fn conversion_instruction_exhaustion_always_releases_guard() {
        for budget in 1..80 {
            let mut vm = Runtime::default();
            let value = vm
                .execute(
                    "({valueOf:function(){return +{valueOf:function(){return 7}}}})",
                    &mut Empty,
                )
                .unwrap();
            vm.remaining = budget;
            let result = vm.coerce_to_number(value, &mut Empty);
            if let Err(error) = result {
                assert_eq!(error.kind, ErrorKind::Resource);
                assert!(error.thrown.is_none());
            } else {
                assert_eq!(result.unwrap(), 7.);
            }
            assert_eq!(vm.conversion_depth, 0, "budget {budget}");
            assert_eq!(vm.call_depth, 0, "budget {budget}");
            assert_eq!(vm.eval_depth, 0, "budget {budget}");
            assert!(vm.remaining < budget);
        }
    }
}

#[cfg(test)]
mod host_stringifier_tests {
    use crate::{ErrorKind, Host, JsError, Runtime, Value};
    struct DataHost {
        text: String,
        calls: usize,
    }
    impl Host for DataHost {
        fn stringifier(&mut self, object: u64) -> Result<Option<String>, JsError> {
            if object != 77 {
                return Ok(None);
            }
            self.calls += 1;
            Ok(Some(self.text.clone()))
        }
        fn global(&mut self, name: &str) -> Option<Value> {
            match name {
                "h" => Some(Value::Host(77)),
                "other" => Some(Value::Host(78)),
                _ => None,
            }
        }
        fn get(&mut self, _: u64, _: &str) -> Result<Value, JsError> {
            panic!("stringifier must not read arbitrary host properties")
        }
        fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
            panic!("stringifier must not write host properties")
        }
        fn call(&mut self, _: u64, _: &str, _: &[Value]) -> Result<Value, JsError> {
            panic!("stringifier must not invoke host methods")
        }
    }
    fn host(text: &str) -> DataHost {
        DataHost {
            text: text.into(),
            calls: 0,
        }
    }
    #[test]
    fn opt_in_uses_data_once_per_hint_without_property_dispatch() {
        let mut vm = Runtime::default();
        let mut h = host("17");
        assert_eq!(
            vm.execute("String(h)==='17' && Number(h)===17 && h+2==='172';", &mut h)
                .unwrap(),
            Value::Bool(true)
        );
        assert_eq!(h.calls, 3);
    }
    #[test]
    fn non_opted_host_and_native_exotic_remain_rejected() {
        let mut vm = Runtime::default();
        let mut h = host("17");
        assert_eq!(
            vm.execute(
                "let failed=false;try{String(other);}catch(e){failed=true;}failed;",
                &mut h
            )
            .unwrap(),
            Value::Bool(true)
        );
        assert_eq!(h.calls, 0);
        // Native functions retain the pre-existing exotic-coercion refusal.
        assert_eq!(vm.execute("let nativeFailed=false;try{String(parseInt);}catch(e){nativeFailed=true;}nativeFailed;",&mut h).unwrap(),Value::Bool(true));
    }
    #[test]
    fn stringifier_return_respects_vm_string_size_and_allocation_budget() {
        let mut vm = Runtime::default();
        let mut h = host(&"x".repeat(65_537));
        assert_eq!(
            vm.coerce_to_string(Value::Host(77), &mut h)
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
        let mut vm = Runtime::default();
        let mut h = host("bytes");
        vm.string_bytes = vm.limits.string_bytes;
        assert_eq!(
            vm.coerce_to_string(Value::Host(77), &mut h)
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
        assert_eq!(vm.conversion_depth, 0);
    }
    #[test]
    fn stringifier_work_exhaustion_is_resource_and_releases_conversion_depth() {
        let mut vm = Runtime::default();
        let mut h = host("ten bytes!");
        vm.remaining = 3;
        assert_eq!(
            vm.coerce_to_string(Value::Host(77), &mut h)
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
        assert_eq!(h.calls, 1);
        assert_eq!(vm.conversion_depth, 0);
    }
}
