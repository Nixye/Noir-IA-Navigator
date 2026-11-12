//! Bounded Array.from. Alternative constructors are deliberately unsupported.
use crate::{
    ErrorKind, Heap, Host, JsError, MAX_ARRAY_LENGTH, Runtime, Value, language::IteratorRecord,
};

enum FromMethod {
    Custom(Value),
    Array(usize),
    String(Value),
}

impl Runtime {
    pub(super) fn array_from(
        &mut self,
        args: &[Value],
        receiver: Value,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let source = args.first().cloned().unwrap_or(Value::Undefined);
        let mapper = args.get(1).cloned().unwrap_or(Value::Undefined);
        let this_arg = args.get(2).cloned().unwrap_or(Value::Undefined);
        let mut roots = Vec::new();
        let result = (|| {
            for value in [&source, &mapper, &this_arg, &receiver] {
                roots.push(self.retain_value(value.clone())?);
            }
            // Validate the mapper before touching even @@iterator or length.
            if mapper != Value::Undefined && !self.is_callable_value(&mapper, host) {
                return Err(self.property_type_error("Array.from mapper must be callable"));
            }
            if matches!(source, Value::Null | Value::Undefined) {
                return Err(self.property_type_error("Array.from requires a non-null source"));
            }
            let method = self.array_from_method(&source, host)?;
            if let Some(method) = method {
                if let FromMethod::Custom(value) = &method {
                    roots.push(self.retain_value(value.clone())?);
                }
                // Construction precedes invoking the iterator method, but follows GetMethod.
                self.array_from_receiver(&receiver, host)?;
                let destination = Value::Array(self.allocate(Heap::Array(Vec::new()))?);
                roots.push(self.retain_value(destination.clone())?);
                let mut iterator = match method {
                    FromMethod::Array(id) => IteratorRecord::Array(id, 0),
                    FromMethod::String(value) => {
                        IteratorRecord::String(crate::strings::units(&value).unwrap(), 0)
                    }
                    FromMethod::Custom(method) => {
                        let object = self.call(method, Vec::new(), source.clone(), host)?;
                        if !from_object(&object) {
                            return Err(self.property_type_error("Iterator must be an object"));
                        }
                        roots.push(self.retain_value(object.clone())?);
                        let next = self.get(object.clone(), "next", host)?;
                        // GetIterator records next without calling or validating it yet.
                        roots.push(self.retain_value(next.clone())?);
                        IteratorRecord::Custom {
                            object,
                            next,
                            done: false,
                        }
                    }
                };
                let mut index = 0usize;
                loop {
                    // Step failures (next, done, value) do not trigger IteratorClose.
                    let Some(value) = self.array_from_next(&mut iterator, host)? else {
                        return Ok(destination);
                    };
                    if index >= MAX_ARRAY_LENGTH {
                        return Err(JsError::resource("Array.from item limit exceeded"));
                    }
                    // Reserve the slot before mapper execution. Alternative
                    // constructors are refused, so this destination cannot be
                    // observed until return. The final write grows by zero and
                    // must not charge the slot again.
                    self.charge_array_slots(1)?;
                    let Value::Array(destination_id) = destination else {
                        unreachable!()
                    };
                    let Heap::Array(values) = &mut self.heap[destination_id] else {
                        unreachable!()
                    };
                    values.push(Value::Undefined);
                    let written =
                        self.array_from_write(&destination, index, value, &mapper, &this_arg, host);
                    if let Err(error) = written {
                        return Err(self.array_from_close_error(&mut iterator, error, host));
                    }
                    index += 1;
                }
            }
            // Host objects (including NodeList) expose string-keyed length/indices.
            // Host symbol properties are not part of the current Host interface.
            let object = self.box_value(source.clone())?;
            roots.push(self.retain_value(object.clone())?);
            let length = self.get(object.clone(), "length", host)?;
            let length_root = self.retain_value(length.clone())?;
            let converted = self.coerce_to_number(length, host);
            self.release_value(length_root);
            let length = converted?;
            let length = if length.is_nan() || length <= 0. {
                0.
            } else {
                length.trunc()
            };
            if length > MAX_ARRAY_LENGTH as f64 {
                return Err(JsError::resource("Array.from item limit exceeded"));
            }
            let length = length as usize;
            self.check_array_slots(length)?;
            self.array_from_receiver(&receiver, host)?;
            let destination =
                Value::Array(self.allocate(Heap::Array(vec![Value::Undefined; length]))?);
            roots.push(self.retain_value(destination.clone())?);
            for index in 0..length {
                self.tick()?;
                // Get, not HasProperty: missing indices become own undefined entries.
                let value = self.get(object.clone(), &index.to_string(), host)?;
                self.array_from_write(&destination, index, value, &mapper, &this_arg, host)?;
            }
            Ok(destination)
        })();
        for root in roots {
            self.release_value(root);
        }
        result
    }

    fn array_from_method(
        &mut self,
        source: &Value,
        host: &mut impl Host,
    ) -> Result<Option<FromMethod>, JsError> {
        if matches!(
            source,
            Value::Object(_) | Value::Array(_) | Value::Function(_)
        ) {
            let Value::Symbol(key, _) = self.well_known_symbol("iterator")? else {
                unreachable!()
            };
            // Array's built-in iterator is an internal fallback in this VM. An
            // explicit null/undefined override must disable that fallback.
            let present = self.has_symbol(source, key, host)?;
            let method = self.get_symbol(source.clone(), key, host)?;
            if !matches!(method, Value::Undefined | Value::Null) {
                if !self.is_callable_value(&method, host) {
                    return Err(self.property_type_error("Array.from iterator must be callable"));
                }
                return Ok(Some(FromMethod::Custom(method)));
            }
            if !present && let Value::Array(id) = source {
                return Ok(Some(FromMethod::Array(*id)));
            }
            return Ok(None);
        }
        if matches!(source, Value::String(_) | Value::Utf16(_)) {
            return Ok(Some(FromMethod::String(source.clone())));
        }
        Ok(None)
    }

    fn array_from_receiver(&mut self, receiver: &Value, host: &impl Host) -> Result<(), JsError> {
        if *receiver == Value::Native("Array") {
            return Ok(());
        }
        let mut current = receiver.clone();
        for _ in 0..64 {
            self.tick()?;
            match current {
                // Native constructor classification is intentionally not guessed.
                Value::Native(_) => break,
                Value::Host(id) if host.is_constructor(id) => break,
                Value::Function(id) => {
                    if let Some(bound) = self.bound_functions.get(&id) {
                        current = bound.target.clone();
                        continue;
                    }
                    if matches!(self.heap.get(id), Some(Heap::Function(function)) if function.constructible)
                    {
                        break;
                    }
                    return Ok(());
                }
                _ => return Ok(()),
            }
        }
        Err(self.property_type_error(
            "Array.from supports Array or non-constructor receivers; alternative/native receivers are unsupported",
        ))
    }

    fn array_from_next(
        &mut self,
        iterator: &mut IteratorRecord,
        host: &mut impl Host,
    ) -> Result<Option<Value>, JsError> {
        let IteratorRecord::Custom { object, next, done } = iterator else {
            return self.iterator_next(iterator, host);
        };
        self.tick()?;
        let result = self.call(next.clone(), Vec::new(), object.clone(), host)?;
        if !from_object(&result) {
            return Err(self.property_type_error("Iterator result must be an object"));
        }
        let root = self.retain_value(result.clone())?;
        let value = (|| {
            if self.get(result.clone(), "done", host)?.truthy() {
                *done = true;
                Ok(None)
            } else {
                self.get(result, "value", host).map(Some)
            }
        })();
        self.release_value(root);
        value
    }

    fn array_from_write(
        &mut self,
        destination: &Value,
        index: usize,
        value: Value,
        mapper: &Value,
        this_arg: &Value,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        let root = self.retain_value(value.clone())?;
        let result = (|| {
            let mapped = if *mapper == Value::Undefined {
                value
            } else {
                self.call(
                    mapper.clone(),
                    vec![value, Value::Number(index as f64)],
                    this_arg.clone(),
                    host,
                )?
            };
            let mapped_root = self.retain_value(mapped.clone())?;
            let created = self.create_data_property(destination, &index.to_string(), mapped, host);
            self.release_value(mapped_root);
            if !created? {
                return Err(self.property_type_error("Array.from cannot create an index"));
            }
            Ok(())
        })();
        self.release_value(root);
        result
    }

    fn array_from_close_error(
        &mut self,
        iterator: &mut IteratorRecord,
        error: JsError,
        host: &mut impl Host,
    ) -> JsError {
        if error.kind == ErrorKind::Resource {
            return error;
        }
        let root = match self.retain_value(error.thrown.clone().unwrap_or(Value::Undefined)) {
            Ok(root) => root,
            Err(fatal) => return fatal,
        };
        let closed = self.iterator_close(iterator, host);
        self.release_value(root);
        // An ordinary close failure cannot replace the original thrown value.
        // Resource exhaustion is fatal even when it happens during closing.
        match closed {
            Err(fatal) if fatal.kind == ErrorKind::Resource => fatal,
            _ => error,
        }
    }
}

fn from_object(value: &Value) -> bool {
    matches!(
        value,
        Value::Object(_) | Value::Array(_) | Value::Function(_) | Value::Native(_) | Value::Host(_)
    )
}

#[cfg(test)]
mod tests {
    use crate::{ErrorKind, Heap, Host, JsError, Runtime, Value};

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
        let mut vm = Runtime::default();
        let result = vm.execute(source, &mut Empty).unwrap();
        assert!(vm.gc_roots.is_empty());
        result
    }

    #[test]
    fn iterator_mapping_is_interleaved_and_precedes_length() {
        assert_eq!(
            run(
                "let log='';let src={};Object.defineProperty(src,'length',{get:function(){throw 99}});Object.defineProperty(src,Symbol.iterator,{get:function(){log+='g';return function(){log+='i';let n=0;return {next:function(){log+='n';return {done:n===2,value:n++}}}}}});let a=Array.from(src,function(v,k){log+='m';return this.base+v+k},{base:10});Array.isArray(a)&&a.join() === '10,12'&&log==='ginmnmn'&&Array.from.length===1;"
            ),
            Value::Bool(true)
        );
    }

    #[test]
    fn array_like_reads_length_once_and_materializes_holes() {
        for source in [
            "let log='';let src={};Object.defineProperty(src,'length',{get:function(){log+='l';return {valueOf:function(){log+='c';return 2.9}}}});Object.defineProperty(src,'0',{get:function(){log+='g';return 4}});let a=Array.from(src,function(v,k){log+='m';return v});a.length===2&&a[0]===4&&a[1]===undefined&&Object.hasOwn(a,'1')&&log==='lcgmm';",
            "let a=[,3];a[Symbol.iterator]=null;let b=Array.from(a);b.length===2&&Object.hasOwn(b,'0')&&b[0]===undefined&&b[1]===3;",
            "let a=Array.from([,3]);a.length===2&&Object.hasOwn(a,'0')&&a[0]===undefined;",
            "Array.from({length:-1}).length===0&&Array.from({length:NaN}).length===0;",
        ] {
            assert_eq!(run(source), Value::Bool(true), "{source}");
        }
    }

    #[test]
    fn mapper_validation_and_bad_iterators_never_fall_back() {
        for source in [
            "let n=0;let src={};Object.defineProperty(src,Symbol.iterator,{get:function(){n++;throw 7}});let ok=false;try{Array.from(src,1)}catch(e){ok=e instanceof TypeError}ok&&n===0;",
            "let src={length:1,0:8};src[Symbol.iterator]=3;let ok=false;try{Array.from(src)}catch(e){ok=e instanceof TypeError}ok;",
            "let token={};let src={length:1};Object.defineProperty(src,Symbol.iterator,{get:function(){throw token}});let ok=false;try{Array.from(src)}catch(e){ok=e===token}ok;",
            "let ok=false;try{Array.from(null)}catch(e){ok=e instanceof TypeError}ok;",
        ] {
            assert_eq!(run(source), Value::Bool(true), "{source}");
        }
    }

    #[test]
    fn mapper_throw_closes_once_preserving_identity() {
        for close in [
            "function(){closed++;return {}}",
            "function(){closed++;throw 88}",
        ] {
            let source = format!(
                "let token={{}};let closed=0;let src={{}};src[Symbol.iterator]=function(){{return {{next:function(){{return {{done:false,value:1}}}},return:{close}}}}};let ok=false;try{{Array.from(src,function(){{throw token}})}}catch(e){{ok=e===token}}ok&&closed===1;"
            );
            assert_eq!(run(&source), Value::Bool(true));
        }
    }

    #[test]
    fn iterator_step_failures_do_not_close() {
        for next in [
            "function(){throw 77}",
            "function(){let r={};Object.defineProperty(r,'done',{get:function(){throw 77}});return r}",
            "function(){let r={done:false};Object.defineProperty(r,'value',{get:function(){throw 77}});return r}",
        ] {
            let source = format!(
                "let closed=0;let src={{}};src[Symbol.iterator]=function(){{return {{next:{next},return:function(){{closed++;return {{}}}}}}}};let ok=false;try{{Array.from(src)}}catch(e){{ok=e===77}}ok&&closed===0;"
            );
            assert_eq!(run(&source), Value::Bool(true));
        }
    }

    #[test]
    fn strings_use_code_points_and_preserve_unpaired_surrogates() {
        assert_eq!(
            run(
                "let pair=String.fromCharCode(55357,56832);let lone=String.fromCharCode(55296);let a=Array.from('a'+pair+lone);a.length===3&&a[0]==='a'&&a[1]===pair&&a[2]===lone;"
            ),
            Value::Bool(true)
        );
    }

    #[test]
    fn indexed_output_does_not_invoke_inherited_setter() {
        assert_eq!(
            run(
                "let hits=0;Object.defineProperty(Array.prototype,'0',{set:function(v){hits++},configurable:true});let a=Array.from({length:1,0:7});let b=Array.from([8]);let ok=a[0]===7&&b[0]===8&&hits===0&&Object.hasOwn(a,'0');delete Array.prototype[0];ok;"
            ),
            Value::Bool(true)
        );
    }

    #[test]
    fn supported_receivers_and_explicit_constructor_rejection() {
        for source in [
            "let f=Array.from;let a=f({length:1,0:4});let b=f.call({},[5]);let c=f.call(()=>0,[6]);a[0]===4&&b[0]===5&&c[0]===6&&Array.isArray(b);",
            "let log='';function C(){log+='c'}let src={};Object.defineProperty(src,'length',{get:function(){log+='l';return 1}});let ok=false;try{Array.from.call(C,src)}catch(e){ok=e instanceof TypeError}ok&&log==='l';",
            "let log='';function C(){}let src={};Object.defineProperty(src,Symbol.iterator,{get:function(){log+='g';return function(){log+='i';return {}}}});let ok=false;try{Array.from.call(C,src)}catch(e){ok=e instanceof TypeError}ok&&log==='g';",
            "let ok=false;try{new Array.from([])}catch(e){ok=e instanceof TypeError}ok;",
            "let ok=false;try{Array.from.call(Array.bind(null,7),[])}catch(e){ok=e instanceof TypeError}ok;",
        ] {
            assert_eq!(run(source), Value::Bool(true), "{source}");
        }
    }

    #[test]
    fn exact_boundary_and_resource_errors_release_roots() {
        assert_eq!(
            run("let a=Array.from({length:4096});a.length===4096&&Object.hasOwn(a,'4095');"),
            Value::Bool(true)
        );
        let mut vm = Runtime::default();
        let error = vm
            .execute("Array.from({length:8193});", &mut Empty)
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert!(vm.gc_roots.is_empty());
        let mut vm = Runtime::default();
        let error = vm
            .execute("Array.from({length:Infinity});", &mut Empty)
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert!(vm.gc_roots.is_empty());
    }

    #[test]
    fn result_graph_survives_collection_and_root_pressure_cleans_up() {
        let mut vm = Runtime::default();
        let value = vm
            .execute(
                "Array.from([{v:1},{v:2}],function(v){return {item:v}});",
                &mut Empty,
            )
            .unwrap();
        vm.collect_garbage(std::slice::from_ref(&value), &Empty)
            .unwrap();
        let first = vm.get(value, "0", &mut Empty).unwrap();
        let item = vm.get(first, "item", &mut Empty).unwrap();
        assert_eq!(vm.get(item, "v", &mut Empty).unwrap(), Value::Number(1.));
        let mut roots = Vec::new();
        for _ in 0..4094 {
            roots.push(vm.retain_value(Value::Undefined).unwrap());
        }
        let source = Value::Array(vm.allocate(Heap::Array(Vec::new())).unwrap());
        let error = vm
            .array_from(&[source], Value::Native("Array"), &mut Empty)
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert_eq!(vm.gc_roots.len(), roots.len());
        for root in roots {
            vm.release_value(root);
        }
        assert!(vm.gc_roots.is_empty());
    }

    #[test]
    fn fatal_mapper_exhaustion_does_not_run_close_or_leak_roots() {
        struct FatalHost {
            closes: usize,
        }
        impl Host for FatalHost {
            fn global(&mut self, name: &str) -> Option<Value> {
                match name {
                    "fatal" => Some(Value::Host(1)),
                    "closed" => Some(Value::Host(2)),
                    _ => None,
                }
            }
            fn is_callable(&self, _: u64) -> bool {
                true
            }
            fn get(&mut self, _: u64, _: &str) -> Result<Value, JsError> {
                Ok(Value::Undefined)
            }
            fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
                Ok(())
            }
            fn call(&mut self, id: u64, _: &str, _: &[Value]) -> Result<Value, JsError> {
                if id == 1 {
                    return Err(JsError::resource("mapper exhausted"));
                }
                self.closes += 1;
                Ok(Value::Undefined)
            }
        }
        let mut vm = Runtime::default();
        let mut host = FatalHost { closes: 0 };
        let error = vm.execute(
            "let src={};src[Symbol.iterator]=function(){return {next:function(){return {done:false,value:1}},return:function(){closed();return {}}}};Array.from(src,fatal);",
            &mut host,
        ).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert_eq!(host.closes, 0);
        assert!(vm.gc_roots.is_empty());
    }
}
