//! Internal async iteration records. Async-from-sync results use typed Promise jobs.
use crate::{ErrorKind, Host, JsError, Runtime, Value, language::IteratorRecord};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AsyncIteratorOperation {
    Next,
    Throw,
    Return,
}

#[derive(Clone, Debug)]
pub(super) enum AsyncIteratorRecord {
    Direct { object: Value, next: Value },
    FromSync(IteratorRecord),
}
impl AsyncIteratorRecord {
    pub(super) fn trace_values(&self, values: &mut impl Extend<Value>) {
        match self {
            Self::Direct { object, next } => values.extend([object.clone(), next.clone()]),
            Self::FromSync(iterator) => iterator.trace_values(values),
        }
    }
}
fn object(value: &Value) -> bool {
    matches!(
        value,
        Value::Object(_) | Value::Array(_) | Value::Function(_) | Value::Native(_) | Value::Host(_)
    )
}
impl Runtime {
    pub(super) fn async_iterator(
        &mut self,
        source: Value,
        host: &mut impl Host,
    ) -> Result<AsyncIteratorRecord, JsError> {
        let Value::Symbol(key, _) = self.well_known_symbol("asyncIterator")? else {
            unreachable!()
        };
        let method = if matches!(
            source,
            Value::Object(_) | Value::Array(_) | Value::Function(_)
        ) {
            self.get_symbol(source.clone(), key, host)?
        } else {
            Value::Undefined
        };
        if matches!(method, Value::Undefined | Value::Null) {
            return self
                .sync_iterator_for_async(source, host)
                .map(AsyncIteratorRecord::FromSync);
        }
        if !self.is_callable_value(&method, host) {
            return Err(self.property_type_error("Symbol.asyncIterator method is not callable"));
        }
        let iterator = self.call(method, Vec::new(), source, host)?;
        if !object(&iterator) {
            return Err(self.property_type_error("Async iterator must be an object"));
        }
        let next = self.get(iterator.clone(), "next", host)?;
        Ok(AsyncIteratorRecord::Direct {
            object: iterator,
            next,
        })
    }
    fn sync_iterator_for_async(
        &mut self,
        source: Value,
        host: &mut impl Host,
    ) -> Result<IteratorRecord, JsError> {
        if matches!(
            source,
            Value::Object(_) | Value::Array(_) | Value::Function(_)
        ) {
            let Value::Symbol(key, _) = self.well_known_symbol("iterator")? else {
                unreachable!()
            };
            let method = self.get_symbol(source.clone(), key, host)?;
            if !matches!(method, Value::Undefined | Value::Null) {
                if !self.is_callable_value(&method, host) {
                    return Err(self.property_type_error("Symbol.iterator method is not callable"));
                }
                let receiver = self.call(method, Vec::new(), source, host)?;
                if !object(&receiver) {
                    return Err(self.property_type_error("Iterator must be an object"));
                }
                let next = self.get(receiver.clone(), "next", host)?;
                return Ok(IteratorRecord::Custom {
                    object: receiver,
                    next,
                    done: false,
                });
            }
        }
        match source {
            Value::Array(id) => Ok(IteratorRecord::Array(id, 0)),
            value @ (Value::String(_) | Value::Utf16(_)) => Ok(IteratorRecord::String(
                crate::strings::units(&value).unwrap(),
                0,
            )),
            other => match self.stdlib_values(&other)? {
                Some(values) => Ok(IteratorRecord::Values(values, 0)),
                None => Err(self.property_type_error("Value is not iterable")),
            },
        }
    }
    pub(super) fn async_iterator_next(
        &mut self,
        record: &mut AsyncIteratorRecord,
        owner: usize,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        self.async_iterator_operation(record, owner, AsyncIteratorOperation::Next, None, host)?
            .ok_or_else(|| JsError::runtime("Missing async iterator next result"))
    }
    pub(super) fn async_iterator_return(
        &mut self,
        record: &mut AsyncIteratorRecord,
        owner: usize,
        host: &mut impl Host,
    ) -> Result<Option<Value>, JsError> {
        self.async_iterator_operation(record, owner, AsyncIteratorOperation::Return, None, host)
    }
    /// None as an argument means zero arguments, not one undefined argument.
    /// Only direct iterators can report an absent optional method.
    pub(super) fn async_iterator_operation(
        &mut self,
        record: &mut AsyncIteratorRecord,
        owner: usize,
        operation: AsyncIteratorOperation,
        argument: Option<Value>,
        host: &mut impl Host,
    ) -> Result<Option<Value>, JsError> {
        self.tick()?;
        let mut values = Vec::new();
        record.trace_values(&mut values);
        values.extend(argument.iter().cloned());
        let mut roots = Vec::new();
        for value in values {
            match self.retain_value(value) {
                Ok(root) => roots.push(root),
                Err(error) => {
                    for root in roots {
                        self.release_value(root);
                    }
                    return Err(error);
                }
            }
        }
        let result = (|| match record {
            AsyncIteratorRecord::Direct { object, next } => {
                let method = match operation {
                    AsyncIteratorOperation::Next => next.clone(),
                    AsyncIteratorOperation::Throw => self.get(object.clone(), "throw", host)?,
                    AsyncIteratorOperation::Return => self.get(object.clone(), "return", host)?,
                };
                if operation != AsyncIteratorOperation::Next
                    && matches!(method, Value::Undefined | Value::Null)
                {
                    return Ok(None);
                }
                if !self.is_callable_value(&method, host) {
                    return Err(self.property_type_error("Async iterator method is not callable"));
                }
                self.call(method, argument.into_iter().collect(), object.clone(), host)
                    .map(Some)
            }
            AsyncIteratorRecord::FromSync(iterator) => self
                .async_from_sync_operation(iterator, owner, operation, argument, host)
                .map(Some),
        })();
        for root in roots {
            self.release_value(root);
        }
        result
    }
    fn async_from_sync_operation(
        &mut self,
        iterator: &mut IteratorRecord,
        owner: usize,
        operation: AsyncIteratorOperation,
        argument: Option<Value>,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let promise = self.new_async_promise()?;
        let Value::Object(child) = promise else {
            unreachable!()
        };
        let root = self.retain_value(promise.clone())?;
        let result = self.sync_result_for_async(iterator, operation, argument.clone(), host);
        let adapted = match result {
            Ok(None) => self.resolve_empty_sync_return(
                child,
                owner,
                argument.unwrap_or(Value::Undefined),
                host,
            ),
            result => self.adapt_sync_iteration(
                child,
                owner,
                iterator.clone(),
                result.map(|value| value.unwrap()),
                operation != AsyncIteratorOperation::Return,
                host,
            ),
        };
        self.release_value(root);
        adapted.map(|()| promise)
    }
    fn sync_result_for_async(
        &mut self,
        iterator: &mut IteratorRecord,
        operation: AsyncIteratorOperation,
        argument: Option<Value>,
        host: &mut impl Host,
    ) -> Result<Option<(Value, bool)>, JsError> {
        if let IteratorRecord::Custom {
            object: receiver,
            next,
            ..
        } = iterator
        {
            let method = match operation {
                AsyncIteratorOperation::Next => next.clone(),
                AsyncIteratorOperation::Throw => self.get(receiver.clone(), "throw", host)?,
                AsyncIteratorOperation::Return => self.get(receiver.clone(), "return", host)?,
            };
            if operation != AsyncIteratorOperation::Next
                && matches!(method, Value::Undefined | Value::Null)
            {
                if operation == AsyncIteratorOperation::Return {
                    return Ok(None);
                }
                // A normal close: its errors win over the missing-throw error.
                // The synchronous result is not awaited or read further.
                self.close_missing_sync_throw(iterator, host)?;
                return Err(self.property_type_error("Synchronous iterator has no throw method"));
            }
            if !self.is_callable_value(&method, host) {
                return Err(self.property_type_error("Synchronous iterator method is not callable"));
            }
            let result = self.call(
                method,
                argument.into_iter().collect(),
                receiver.clone(),
                host,
            )?;
            if !object(&result) {
                return Err(
                    self.property_type_error("Synchronous iterator result must be an object")
                );
            }
            // The adapter reads value even when done is true, unlike for-of.
            let root = self.retain_value(result.clone())?;
            let read = (|| {
                let done = self.get(result.clone(), "done", host)?.truthy();
                let value = self.get(result, "value", host)?;
                Ok(Some((value, done)))
            })();
            self.release_value(root);
            read
        } else if operation == AsyncIteratorOperation::Return {
            Ok(None)
        } else if operation == AsyncIteratorOperation::Throw {
            self.close_missing_sync_throw(iterator, host)?;
            Err(self.property_type_error("Synchronous iterator has no throw method"))
        } else {
            self.iterator_next(iterator, host).map(|value| {
                Some(match value {
                    Some(value) => (value, false),
                    None => (Value::Undefined, true),
                })
            })
        }
    }
    /// IteratorClose with a normal completion for Async-from-Sync throw.
    /// Check callability here because the generic call path reports RuntimeError.
    fn close_missing_sync_throw(
        &mut self,
        iterator: &mut IteratorRecord,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        if let IteratorRecord::Custom {
            object: receiver, ..
        } = iterator
        {
            let method = self.get(receiver.clone(), "return", host)?;
            if matches!(method, Value::Undefined | Value::Null) {
                return Ok(());
            }
            if !self.is_callable_value(&method, host) {
                return Err(self.property_type_error("Synchronous iterator return is not callable"));
            }
            let result = self.call(method, Vec::new(), receiver.clone(), host)?;
            if !object(&result) {
                return Err(self.property_type_error("Iterator.return must return an object"));
            }
        }
        Ok(())
    }
    pub(super) fn close_rejected_sync_iteration(
        &mut self,
        iterator: &mut IteratorRecord,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        match self.iterator_close(iterator, host) {
            Err(error) if error.kind == ErrorKind::Resource => Err(error),
            _ => Ok(()), // The original throw wins over ordinary close failures.
        }
    }
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

    #[test]
    fn direct_operations_preserve_argument_count_and_cached_next() {
        let mut vm = Runtime::default();
        let source = vm.execute("let log='';let source={next:function(v){log+='n'+arguments.length+':'+v+';';return {};},throw:function(v){log+='t'+arguments.length+':'+v+';';return {};},return:function(v){log+='r'+arguments.length+':'+v+';';return {};}};source[Symbol.asyncIterator]=function(){return this;};source", &mut Empty).unwrap();
        let mut record = vm.async_iterator(source, &mut Empty).unwrap();
        vm.execute("source.next=null", &mut Empty).unwrap();
        vm.async_iterator_next(&mut record, 0, &mut Empty).unwrap();
        vm.async_iterator_operation(
            &mut record,
            0,
            AsyncIteratorOperation::Next,
            Some(Value::Undefined),
            &mut Empty,
        )
        .unwrap();
        vm.async_iterator_operation(
            &mut record,
            0,
            AsyncIteratorOperation::Throw,
            Some(Value::Number(7.)),
            &mut Empty,
        )
        .unwrap();
        vm.async_iterator_return(&mut record, 0, &mut Empty)
            .unwrap();
        assert_eq!(
            vm.execute("log", &mut Empty).unwrap(),
            Value::string("n0:undefined;n1:undefined;t1:7;r0:undefined;")
        );
    }

    #[test]
    fn sync_operations_preserve_arguments_and_read_done_before_value() {
        let mut vm = Runtime::default();
        let source = vm.execute("let log='';let result={};Object.defineProperty(result,'done',{get:function(){log+='d';return false;}});Object.defineProperty(result,'value',{get:function(){log+='v';return 3;}});let source={next:function(v){log+='n'+arguments.length;return result;},throw:function(v){log+='t'+arguments.length+v;return result;},return:function(v){log+='r'+arguments.length+v;return result;}};source[Symbol.iterator]=function(){return this;};source", &mut Empty).unwrap();
        let mut record = vm.async_iterator(source, &mut Empty).unwrap();
        vm.async_iterator_next(&mut record, 0, &mut Empty).unwrap();
        vm.async_iterator_operation(
            &mut record,
            0,
            AsyncIteratorOperation::Next,
            Some(Value::Undefined),
            &mut Empty,
        )
        .unwrap();
        vm.async_iterator_operation(
            &mut record,
            0,
            AsyncIteratorOperation::Throw,
            Some(Value::Number(7.)),
            &mut Empty,
        )
        .unwrap();
        vm.async_iterator_operation(
            &mut record,
            0,
            AsyncIteratorOperation::Return,
            Some(Value::Number(8.)),
            &mut Empty,
        )
        .unwrap();
        assert_eq!(
            vm.execute("log", &mut Empty).unwrap(),
            Value::string("n0dvn1dvt17dvr18dv")
        );
    }

    #[test]
    fn absent_sync_return_does_not_assimilate_argument() {
        let mut vm = Runtime::default();
        let source = vm.execute("let reads=0;let arg={};Object.defineProperty(arg,'then',{get:function(){reads++;throw 9;}});let source={next:function(){return {done:false,value:1};}};source[Symbol.iterator]=function(){return this;};source", &mut Empty).unwrap();
        let mut record = vm.async_iterator(source, &mut Empty).unwrap();
        let argument = vm.global_value("arg").unwrap();
        assert!(
            vm.async_iterator_operation(
                &mut record,
                0,
                AsyncIteratorOperation::Return,
                Some(argument),
                &mut Empty
            )
            .unwrap()
            .is_some()
        );
        assert_eq!(vm.execute("reads", &mut Empty).unwrap(), Value::Number(0.));
    }

    #[test]
    fn absent_sync_throw_closes_without_reading_result_or_awaiting_it() {
        let mut vm = Runtime::default();
        let source = vm.execute("let log='';let result={};Object.defineProperty(result,'done',{get:function(){throw 1;}});Object.defineProperty(result,'value',{get:function(){throw 2;}});Object.defineProperty(result,'then',{get:function(){throw 3;}});let source={next:function(){return {};},return:function(){log+='r'+arguments.length;return result;}};source[Symbol.iterator]=function(){return this;};source", &mut Empty).unwrap();
        let mut record = vm.async_iterator(source, &mut Empty).unwrap();
        assert!(
            vm.async_iterator_operation(
                &mut record,
                0,
                AsyncIteratorOperation::Throw,
                Some(Value::Number(7.)),
                &mut Empty
            )
            .unwrap()
            .is_some()
        );
        assert_eq!(vm.execute("log", &mut Empty).unwrap(), Value::string("r0"));
    }
}
