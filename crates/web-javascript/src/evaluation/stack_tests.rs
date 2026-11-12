use crate::{ErrorKind, Host, JsError, Runtime, Value};

struct Empty;
impl Host for Empty {
    fn global(&mut self, _: &str) -> Option<Value> {
        None
    }
    fn get(&mut self, _: u64, _: &str) -> Result<Value, JsError> {
        panic!("Unexpected host read")
    }
    fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
        panic!("Unexpected host write")
    }
    fn call(&mut self, _: u64, _: &str, _: &[Value]) -> Result<Value, JsError> {
        panic!("Unexpected host call")
    }
}

fn small_stack(test: impl FnOnce() + Send + 'static) {
    // Match the renderer's normal Windows PE stack reserve, not the larger
    // default Rust test-thread stack. Production stack sizes are unchanged.
    std::thread::Builder::new()
        .name("evaluation-1mib".into())
        .stack_size(1024 * 1024)
        .spawn(test)
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn bounded_expression_chains_fit_the_renderer_stack() {
    small_stack(|| {
        let mut vm = Runtime::default();
        let sum = std::iter::repeat_n("1", 60).collect::<Vec<_>>().join("+");
        assert_eq!(vm.execute(&sum, &mut Empty).unwrap(), Value::Number(60.));
        let unary = format!("{}true", "!".repeat(50));
        assert_eq!(vm.execute(&unary, &mut Empty).unwrap(), Value::Bool(true));
        assert_eq!(
            (vm.eval_depth, vm.call_depth, vm.statement_depth),
            (0, 0, 0)
        );
    });
}

#[test]
fn finite_calls_and_inherited_accessors_fit_the_renderer_stack() {
    small_stack(|| {
        for source in [
            "function f(n){if(n===1)return 7;return f(n-1)}f(16)",
            "function f(n){if(n===1)return 7;return g(n-1)}function g(n){if(n===1)return 7;return f(n-1)}f(16)",
            "let count=0;let base={};Object.defineProperty(base,'x',{get:function(){count++;if(count===16)return 7;return this.x}});function f(){};Object.setPrototypeOf(f,base);f.x",
        ] {
            let mut vm = Runtime::default();
            assert_eq!(vm.execute(source, &mut Empty).unwrap(), Value::Number(7.));
            assert_eq!(
                (vm.eval_depth, vm.call_depth, vm.statement_depth),
                (0, 0, 0)
            );
        }
    });
}

#[test]
fn call_budget_still_fails_fatally_and_recovers_on_a_small_stack() {
    small_stack(|| {
        let mut vm = Runtime::default();
        vm.execute("let caught=0;function f(){return f()}", &mut Empty)
            .unwrap();
        let before = vm.clone();
        let error = vm
            .execute("try{f()}catch(e){caught=1}", &mut Empty)
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert_eq!(error.message, "JavaScript call stack budget exceeded");
        assert!(error.thrown.is_none());
        assert_eq!(vm.global_value("caught"), Some(Value::Number(0.)));
        assert_eq!(
            (vm.eval_depth, vm.call_depth, vm.statement_depth),
            (0, 0, 0)
        );
        let spent = vm.remaining;
        vm.rollback_to(before);
        assert_eq!(vm.remaining, spent);
        assert_eq!(vm.execute("2+3", &mut Empty).unwrap(), Value::Number(5.));
    });
}

#[test]
fn mixed_call_and_expression_depth_uses_flat_frames_on_a_small_stack() {
    small_stack(|| {
        let sum = format!("f(n-1){}", "+1".repeat(40));
        let source = format!("function f(n){{if(n===0)return 1;return {sum}}}f(3)");
        let mut vm = Runtime::default();
        assert_eq!(
            vm.execute(&source, &mut Empty).unwrap(),
            Value::Number(121.)
        );
        assert_eq!(vm.sync_execution.live_slots, 0);
        assert_eq!(vm.sync_execution.live_frames, 0);
        assert_eq!(vm.sync_execution.active, 0);
        assert_eq!(
            (vm.eval_depth, vm.call_depth, vm.statement_depth),
            (0, 0, 0)
        );
        assert_eq!(vm.execute("7", &mut Empty).unwrap(), Value::Number(7.));
    });
}
