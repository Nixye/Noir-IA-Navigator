use crate::{ErrorKind, Host, JsError, Limits, Runtime, Value};

struct FixedEntropy(u64);
impl Host for FixedEntropy {
    fn random_u64(&mut self) -> Result<u64, JsError> {
        Ok(self.0)
    }
    fn global(&mut self, _: &str) -> Option<Value> {
        None
    }
    fn get(&mut self, _: u64, _: &str) -> Result<Value, JsError> {
        Err(JsError::runtime("No host"))
    }
    fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
        Err(JsError::runtime("No host"))
    }
    fn call(&mut self, _: u64, _: &str, _: &[Value]) -> Result<Value, JsError> {
        Err(JsError::runtime("No host"))
    }
}

#[test]
fn array_constructor_diagnostic_preserves_limit_and_rejects_before_allocation() {
    let mut vm = Runtime::default();
    let result = vm.native("Array", &[Value::Number(4096.)]).unwrap();
    assert!(matches!(result, Value::Array(_)));
    assert_eq!(vm.array_slots, 4096);
    let objects = vm.heap.len();
    for length in [8193., 16_384., f64::MAX] {
        let error = vm.native("Array", &[Value::Number(length)]).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert_eq!(error.message, "Array item limit exceeded");
        assert_eq!(error.offset, None);
        assert_eq!(
            error.diagnostic_contexts(),
            &[format!(
                "array operation=construct requested-length={length:e} limit=8192"
            )]
        );
        assert!(error.diagnostic_contexts()[0].len() <= 128);
        assert_eq!(vm.array_slots, 4096);
        assert_eq!(vm.heap.len(), objects);
    }
}

#[test]
fn entropy_dependent_array_request_fits_within_the_new_bounded_storage() {
    let source = "Array(Math.random()*7824|0).length;";
    for (bits, length) in [
        (0, 0.),
        (1u64 << 63, 3912.),
        (3u64 << 62, 5868.),
        (u64::MAX, 7823.),
    ] {
        assert_eq!(
            Runtime::default()
                .execute(source, &mut FixedEntropy(bits))
                .unwrap(),
            Value::Number(length)
        );
    }
    for (bits, length) in [(3u64 << 62, 12288.), (u64::MAX, 16383.)] {
        let error = Runtime::default()
            .execute("Array(Math.random()*16384|0);", &mut FixedEntropy(bits))
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert!(
            error
                .to_string()
                .contains(&format!("requested-length={length:e}"))
        );
    }
}

#[test]
fn array_resource_error_stays_uncatchable_and_aggregate_budget_remains() {
    let mut vm = Runtime::default();
    let error = vm
        .execute(
            "var escaped=false;try{Array(8193);}catch(e){escaped=true;}escaped=true;",
            &mut FixedEntropy(0),
        )
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert_eq!(vm.global_value("escaped"), Some(Value::Bool(false)));
    // The call argument was reserved; the rejected Array itself allocated none.
    assert_eq!(vm.array_slots, 1);
    let mut vm = Runtime::new(Limits {
        instructions: 1_000_000,
        ..Limits::default()
    });
    for _ in 0..16 {
        vm.native("Array", &[Value::Number(4096.)]).unwrap();
    }
    assert_eq!(vm.array_slots, 65_536);
    let error = vm.native("Array", &[Value::Number(1.)]).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert_eq!(
        error.message,
        "JavaScript aggregate array slot budget exceeded"
    );
    assert!(!error.to_string().contains("operation=construct"));
    assert_eq!(vm.array_slots, 65_536);
}
