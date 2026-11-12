use super::*;
use crate::{ErrorKind, Stmt};

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

fn arguments(source: &str) -> Vec<Expr> {
    let Stmt::Expression(Expr::Call(_, arguments)) =
        crate::parser::parse(source).unwrap().remove(0)
    else {
        panic!("Expected a call")
    };
    arguments
}

fn evaluate_cursor(runtime: &mut Runtime, arguments: &[Expr]) -> Result<Vec<Value>, JsError> {
    let mut step = runtime.sync_start_arguments(arguments)?;
    loop {
        step = match step {
            ArgumentsStep::Evaluate { expression, cursor } => {
                let value = runtime.eval(expression, 0, &mut Empty)?;
                runtime.sync_apply_argument(cursor, value, &mut Empty)?
            }
            ArgumentsStep::Complete(values) => return Ok(values),
        };
    }
}

#[test]
fn argument_cursor_matches_legacy_ticks_storage_errors_and_effect_prefixes() {
    for source in [
        "f(n=1,n=2,n=3)",
        "f(n=1,...(n=2,[3,4]),n=5)",
        "f(...[],n=1,...[2],n=3)",
        "f(n=1,missing,n=3)",
        "f(n=1,...null,n=3)",
    ] {
        let arguments = arguments(source);
        for budget in 0..100 {
            let run = |legacy: bool| {
                let mut runtime = Runtime::default();
                runtime.execute("var n=0;", &mut Empty).unwrap();
                runtime.remaining = budget;
                let result = if legacy {
                    runtime.argument_values(&arguments, 0, &mut Empty)
                } else {
                    evaluate_cursor(&mut runtime, &arguments)
                }
                .map_err(|error| (error.kind, error.message, error.thrown));
                (
                    result,
                    runtime.remaining,
                    runtime.array_slots,
                    runtime.global_value("n"),
                )
            };
            assert_eq!(run(false), run(true), "{source}, budget={budget}");
        }
    }
}

#[test]
fn regular_argument_cursor_reserves_once_and_moves_its_buffer() {
    let arguments = vec![Expr::Literal(Value::Number(7.)); 64];
    let mut runtime = Runtime::default();
    let mut step = runtime.sync_start_arguments(&arguments).unwrap();
    let mut pointer = None;
    loop {
        step = match step {
            ArgumentsStep::Evaluate { expression, cursor } => {
                assert_eq!(runtime.array_slots, 64);
                assert!(cursor.values.capacity() >= 64);
                let initial = pointer.get_or_insert(cursor.values.as_ptr());
                assert_eq!(*initial, cursor.values.as_ptr());
                // Every scheduled child is borrowed from the input slice.
                assert!(std::ptr::eq(expression, &arguments[cursor.next - 1]));
                runtime
                    .sync_apply_argument(cursor, Value::Number(7.), &mut Empty)
                    .unwrap()
            }
            ArgumentsStep::Complete(values) => {
                assert_eq!(values.len(), 64);
                assert_eq!(Some(values.as_ptr()), pointer);
                assert_eq!(runtime.array_slots, 64);
                break;
            }
        };
    }
}

#[test]
fn parser_bypassing_regular_argument_sixty_five_never_runs() {
    let mut runtime = Runtime::default();
    runtime.execute("var effects=0;", &mut Empty).unwrap();
    let expression = Expr::Update(Box::new(Expr::Name("effects".into())), true, false);
    let arguments = vec![expression; 65];
    let error = evaluate_cursor(&mut runtime, &arguments).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert_eq!(runtime.global_value("effects"), Some(Value::Number(64.)));
    assert_eq!(runtime.array_slots, 64);
}

#[test]
fn known_width_storage_preflight_precedes_argument_effects() {
    let mut runtime = Runtime::default();
    runtime.execute("var n=0;", &mut Empty).unwrap();
    runtime.array_slots = 65_536;
    let error = evaluate_cursor(&mut runtime, &arguments("f(n++)")).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert_eq!(runtime.global_value("n"), Some(Value::Number(0.)));
}

#[test]
fn spread_cursor_preserves_final_next_and_never_closes_after_resource() {
    for length in [64, 65] {
        let mut runtime = Runtime::default();
        let setup = format!(
            "var nexts=0;var closes=0;var tails=0;var source={{}};source[Symbol.iterator]=function(){{return {{next:function(){{nexts++;return {{done:nexts>{length},value:nexts}}}},return:function(){{closes++;return {{done:true}}}}}}}};"
        );
        runtime.execute(&setup, &mut Empty).unwrap();
        let result = evaluate_cursor(&mut runtime, &arguments("f(...source)"));
        if length == 64 {
            assert_eq!(result.unwrap().len(), 64);
        } else {
            let error = result.unwrap_err();
            assert_eq!(error.kind, ErrorKind::Resource);
            assert!(error.thrown.is_none());
        }
        assert_eq!(runtime.global_value("nexts"), Some(Value::Number(65.)));
        assert_eq!(runtime.global_value("closes"), Some(Value::Number(0.)));
    }
}

#[test]
fn spread_cursor_preserves_ordinary_error_close_and_stops_later_children() {
    for next in [
        "function(){throw 77}",
        "function(){let r={};Object.defineProperty(r,'done',{get:function(){throw 77}});return r}",
        "function(){let r={done:false};Object.defineProperty(r,'value',{get:function(){throw 77}});return r}",
    ] {
        let mut runtime = Runtime::default();
        let setup = format!(
            "var closes=0;var n=0;var source={{}};source[Symbol.iterator]=function(){{return {{next:{next},return:function(){{closes++;return {{done:true}}}}}}}};"
        );
        runtime.execute(&setup, &mut Empty).unwrap();
        let error = evaluate_cursor(&mut runtime, &arguments("f(...source,n++)")).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Runtime);
        assert_eq!(error.thrown, Some(Value::Number(77.)));
        assert_eq!(runtime.global_value("closes"), Some(Value::Number(1.)));
        assert_eq!(runtime.global_value("n"), Some(Value::Number(0.)));
    }
}

#[test]
fn callees_and_receivers_are_captured_before_argument_cursor_effects() {
    for source in [
        "let log='';let object={tag:7};function chosen(a){log+='c';return this.tag+a}Object.defineProperty(object,'method',{get:function(){log+='g';return chosen}});function argument(){log+='a';chosen=function(){return 0};return 2}object.method(argument())===9&&log==='gac'",
        "let effects=0;let object={method:1};let caught=false;try{object.method(effects++)}catch(e){caught=true}caught&&effects===1",
        "let calls=0;function C(x){this.x=x;calls++}let Saved=C;function argument(){C=function(){calls+=100};return 7}let instance=new C(argument());instance instanceof Saved&&instance.x===7&&calls===1",
    ] {
        assert_eq!(
            Runtime::default().execute(source, &mut Empty).unwrap(),
            Value::Bool(true),
            "{source}"
        );
    }
}

#[test]
fn spread_slot_exhaustion_stops_after_one_next_without_close_or_later_argument() {
    let mut runtime = Runtime::default();
    runtime.execute("var nexts=0;var closes=0;var n=0;var source={};source[Symbol.iterator]=function(){return {next:function(){nexts++;return {done:false,value:7}},return:function(){closes++;return {done:true}}}};", &mut Empty).unwrap();
    runtime.array_slots = 65_536;
    let error = evaluate_cursor(&mut runtime, &arguments("f(...source,n++)")).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert!(error.thrown.is_none());
    assert_eq!(runtime.global_value("nexts"), Some(Value::Number(1.)));
    assert_eq!(runtime.global_value("closes"), Some(Value::Number(0.)));
    assert_eq!(runtime.global_value("n"), Some(Value::Number(0.)));
    assert_eq!(runtime.array_slots, 65_536);
}
