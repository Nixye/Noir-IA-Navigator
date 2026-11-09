//! Shared, bounded literal assembly for ordinary and suspended evaluation.
use crate::{Heap, Host, JsError, Runtime, Value};
use std::collections::BTreeSet;

impl Runtime {
    pub(super) fn literal_array_append(
        &mut self,
        values: &mut Vec<Value>,
        holes: &mut BTreeSet<usize>,
        value: Value,
        hole: bool,
    ) -> Result<(), JsError> {
        self.tick()?;
        if values.len() >= 4096 {
            return Err(JsError::resource("Array literal exceeds limit"));
        }
        self.charge_array_slots(1)?;
        if hole {
            holes.insert(values.len());
        }
        values.push(if hole { Value::Undefined } else { value });
        Ok(())
    }

    pub(super) fn literal_array_spread(
        &mut self,
        values: &mut Vec<Value>,
        source: Value,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        let mut roots = Vec::new();
        let mut retained = BTreeSet::new();
        let result = (|| {
            self.literal_retain(&source, &mut roots, &mut retained)?;
            let mut iterator = self.iterator(source, host)?;
            let mut iterator_values = Vec::new();
            iterator.trace_values(&mut iterator_values);
            for value in iterator_values {
                self.literal_retain(&value, &mut roots, &mut retained)?;
            }
            // ArrayAccumulation propagates IteratorStepValue failures without closing.
            // This intentionally differs from the legacy iterable_values consumers.
            while let Some(value) = self.iterator_next(&mut iterator, host)? {
                if values.len() >= 4096 {
                    // No JS cleanup or further source consumption after Resource.
                    return Err(JsError::resource("Spread array exceeds limit"));
                }
                self.charge_array_slots(1)?;
                self.literal_retain(&value, &mut roots, &mut retained)?;
                values.push(value);
            }
            Ok(())
        })();
        for root in roots {
            self.release_value(root);
        }
        result
    }

    pub(super) fn literal_array_finish(
        &mut self,
        values: Vec<Value>,
        holes: BTreeSet<usize>,
    ) -> Result<Value, JsError> {
        if values.len() > 4096 {
            return Err(JsError::resource("Array literal exceeds limit"));
        }
        let id = self.allocate_reserved(Heap::Array(values))?;
        if !holes.is_empty() {
            self.array_holes.insert(id, holes);
        }
        Ok(Value::Array(id))
    }

    pub(super) fn literal_template_append(
        &mut self,
        units: &mut Vec<u16>,
        value: Value,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        // Observable conversion completes before the next substitution begins.
        let text = self.language_string(value, host)?;
        let work = match &text {
            Value::String(text) => text.len(),
            Value::Utf16(text) => text.len(),
            _ => unreachable!("ToString must produce a string"),
        };
        self.literal_string_work(work)?;
        let additional = match &text {
            Value::String(text) => text.encode_utf16().count(),
            Value::Utf16(text) => text.len(),
            _ => unreachable!("ToString must produce a string"),
        };
        if units.len().saturating_add(additional) > 65_536 {
            return Err(JsError::resource("Template output exceeds unit limit"));
        }
        self.literal_string_work(additional)?;
        // Charge persistent temporary storage before growth, not only the final
        // string. Reservations are cumulative and are not refunded on abrupts.
        self.charge_string(additional.saturating_mul(2))?;
        match text {
            Value::String(text) => units.extend(text.encode_utf16()),
            Value::Utf16(text) => units.extend_from_slice(&text),
            _ => unreachable!("ToString must produce a string"),
        }
        Ok(())
    }

    pub(super) fn literal_template_finish(&mut self, units: Vec<u16>) -> Result<Value, JsError> {
        if units.len() > 65_536 {
            return Err(JsError::resource("Template output exceeds unit limit"));
        }
        self.literal_string_work(units.len())?;
        // Preflight the final representation before utf16_string allocates it.
        // A later isolated surrogate can change UTF-8 into UTF-16 storage, so
        // byte limits must not be applied to intermediate prefixes.
        let mut bytes = 0usize;
        for character in char::decode_utf16(units.iter().copied()) {
            match character {
                Ok(character) => bytes += character.len_utf8(),
                Err(_) => {
                    bytes = units.len().saturating_mul(2);
                    break;
                }
            }
        }
        if bytes > 65_536 {
            return Err(JsError::resource("Template output exceeds 64 KiB"));
        }
        let total = self
            .string_bytes
            .checked_add(bytes)
            .ok_or_else(|| JsError::resource("String budget overflow"))?;
        if total > self.limits.string_bytes {
            return Err(JsError::resource(
                "JavaScript string allocation budget exceeded",
            ));
        }
        // Only preflight here; utf16_string performs the single final charge.
        self.utf16_string(units)
    }

    fn literal_string_work(&mut self, work: usize) -> Result<(), JsError> {
        self.remaining = self
            .remaining
            .checked_sub(work as u64)
            .ok_or_else(|| JsError::resource("Template string work budget exceeded"))?;
        Ok(())
    }

    fn literal_retain(
        &mut self,
        value: &Value,
        roots: &mut Vec<u64>,
        retained: &mut BTreeSet<usize>,
    ) -> Result<(), JsError> {
        if let Value::Array(id) | Value::Object(id) | Value::Function(id) = value
            && retained.insert(*id)
        {
            roots.push(self.retain_value(value.clone())?);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{ErrorKind, Host, JsError, Runtime, Value};
    use std::collections::BTreeSet;

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

    fn run(source: &str) -> Value {
        Runtime::default().execute(source, &mut Empty).unwrap()
    }

    #[test]
    fn templates_convert_each_substitution_before_evaluating_the_next() {
        assert_eq!(
            run(
                "let log='';let a={};a[Symbol.toPrimitive]=function(h){log+='a'+h;return 'A'};function b(){log+='b';return 2n}let text=`x${a}${b()}z`;text==='xA2z' && log==='astringb';"
            ),
            Value::Bool(true)
        );
        assert_eq!(
            run(
                "let log='';let a={toString:function(){log+='s';return {}},valueOf:function(){log+='v';return 7n}};`${a}`==='7' && log==='sv';"
            ),
            Value::Bool(true)
        );
    }

    #[test]
    fn templates_reject_implicit_symbols_and_stop_before_later_effects() {
        for source in [
            "let n=0;let caught=false;try{`${Symbol('x')}${n++}`}catch(e){caught=e instanceof TypeError}caught && n===0 && String(Symbol('x'))==='Symbol(x)';",
            "let n=0;let a={};a[Symbol.toPrimitive]=function(){return Symbol('x')};let caught=false;try{`${a}${n++}`}catch(e){caught=e instanceof TypeError}caught && n===0;",
            "let n=0;let a={toString:function(){throw 77}};let caught=false;try{`${a}${n++}`}catch(e){caught=e===77}caught && n===0;",
        ] {
            assert_eq!(run(source), Value::Bool(true), "{source}");
        }
    }

    #[test]
    fn templates_preserve_bigint_and_utf16_units() {
        for source in [
            "`${123456789012345678901234567890n}`==='123456789012345678901234567890';",
            "let s=String.fromCharCode(55296);let t=`a${s}b`;t.length===3 && t.charCodeAt(1)===55296;",
            "let a=String.fromCharCode(55357);let b=String.fromCharCode(56832);let t=`${a}${b}`;t.length===2 && t.charCodeAt(0)===55357 && t.charCodeAt(1)===56832;",
        ] {
            assert_eq!(run(source), Value::Bool(true), "{source}");
        }
    }

    #[test]
    fn arrays_distinguish_holes_from_spread_undefined() {
        assert_eq!(
            run(
                "let a=[,undefined,...[,],,];a.length===4 && !(0 in a) && (1 in a) && (2 in a) && !(3 in a);"
            ),
            Value::Bool(true)
        );
        assert_eq!(
            run(
                "let log='';let source={};source[Symbol.iterator]=function(){log+='i';let n=0;return {next:function(){log+='n';return {value:++n,done:n>2}}}};function tail(){log+='t';return 3}let a=[0,...source,tail()];a.join(',')==='0,1,2,3' && log==='innnt';"
            ),
            Value::Bool(true)
        );
    }

    #[test]
    fn array_spread_step_failures_do_not_call_iterator_return() {
        for next in [
            "function(){throw 77}",
            "function(){let r={};Object.defineProperty(r,'done',{get:function(){throw 77}});return r}",
            "function(){let r={done:false};Object.defineProperty(r,'value',{get:function(){throw 77}});return r}",
        ] {
            let source = format!(
                "let closed=0;let source={{}};source[Symbol.iterator]=function(){{return {{next:{next},return:function(){{closed++;return {{done:true}}}}}}}};let caught=false;try{{[...source]}}catch(e){{caught=e===77}}caught && closed===0;"
            );
            assert_eq!(run(&source), Value::Bool(true), "{source}");
        }
    }

    #[test]
    fn template_buffers_preflight_real_final_bytes_and_work() {
        let mut runtime = Runtime::default();
        let error = runtime
            .literal_template_finish(vec![0x0800; 21_846])
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        // An isolated surrogate switches the final representation to UTF-16.
        let mut units = vec![0x0800; 21_846];
        units.push(0xd800);
        assert!(matches!(
            runtime.literal_template_finish(units).unwrap(),
            Value::Utf16(_)
        ));
        let error = runtime
            .literal_template_finish(vec![0xd800; 32_769])
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        runtime.remaining = 0;
        let error = runtime
            .literal_template_append(&mut Vec::new(), Value::string("x"), &mut Empty)
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
    }

    #[test]
    fn template_bigint_decimal_conversion_is_budgeted() {
        let mut runtime = Runtime::default();
        let value = runtime
            .execute("123456789012345678901234567890n;", &mut Empty)
            .unwrap();
        runtime.remaining = 3;
        let error = runtime
            .literal_template_append(&mut Vec::new(), value, &mut Empty)
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert!(error.message.contains("BigInt decimal conversion work"));
    }

    #[test]
    fn partial_literals_reserve_aggregate_budgets_before_growth() {
        let mut runtime = Runtime {
            array_slots: 65_535,
            ..Runtime::default()
        };
        let mut values = Vec::new();
        let mut holes = BTreeSet::new();
        runtime
            .literal_array_append(&mut values, &mut holes, Value::Number(1.), false)
            .unwrap();
        assert_eq!(runtime.array_slots, 65_536);
        let error = runtime
            .literal_array_append(&mut values, &mut holes, Value::Undefined, true)
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert_eq!(values.len(), 1);
        assert!(holes.is_empty());
        runtime.literal_array_finish(values, holes).unwrap();
        assert_eq!(runtime.array_slots, 65_536);

        let mut runtime = Runtime::default();
        runtime.limits.string_bytes = 3;
        let mut units = Vec::new();
        runtime
            .literal_template_append(&mut units, Value::string("a"), &mut Empty)
            .unwrap();
        assert_eq!(runtime.string_bytes, 2);
        let error = runtime
            .literal_template_append(&mut units, Value::string("b"), &mut Empty)
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert_eq!(units, vec![97]);
    }

    #[test]
    fn full_width_spread_reuses_roots_for_repeated_objects() {
        let mut runtime = Runtime::default();
        assert_eq!(
            runtime
                .execute(
                    "let obj={};let a=[...Array(4096).fill(obj)];a.length===4096 && a[0]===obj && a[4095]===obj;",
                    &mut Empty,
                )
                .unwrap(),
            Value::Bool(true)
        );
        assert!(runtime.gc_roots.is_empty());
    }

    #[test]
    fn template_final_representation_preflights_aggregate_budget_once() {
        for (units, bytes) in [(vec![0x0800], 3), (vec![0xd800], 2)] {
            let mut runtime = Runtime {
                string_bytes: 10,
                ..Runtime::default()
            };
            runtime.limits.string_bytes = 10 + bytes - 1;
            let error = runtime.literal_template_finish(units.clone()).unwrap_err();
            assert_eq!(error.kind, ErrorKind::Resource);
            // A failed preflight must not enter the final allocator/charge path.
            assert_eq!(runtime.string_bytes, 10);
            runtime.limits.string_bytes = 10 + bytes;
            runtime.literal_template_finish(units).unwrap();
            assert_eq!(runtime.string_bytes, 10 + bytes);
        }
        let mut runtime = Runtime {
            string_bytes: usize::MAX,
            ..Runtime::default()
        };
        let error = runtime.literal_template_finish(vec![97]).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert_eq!(error.message, "String budget overflow");
        assert_eq!(runtime.string_bytes, usize::MAX);
    }
}
