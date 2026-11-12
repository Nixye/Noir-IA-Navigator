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

fn expression(source: &str) -> Expr {
    let Stmt::Expression(expression) = crate::parser::parse(source).unwrap().remove(0) else {
        panic!("Expected an expression")
    };
    expression
}

#[test]
fn literal_cursor_matches_legacy_ticks_quotas_errors_and_effect_prefixes() {
    for source in [
        "[n=1,,...(n=2,[3,4]),n=5]",
        "[n=1,missing,n=3]",
        "({a:n=1,...(n=2,{b:3}),c:n=4})",
        "({m(){return 7},a:n=2,...{b:3}})",
        "`a${n=1}b${n=2}c`",
        "`${n=1}${Symbol('x')}${n=3}`",
    ] {
        let expression = expression(source);
        for budget in 0..100 {
            let run = |legacy: bool| {
                let mut runtime = Runtime::default();
                runtime.execute("var n=0;", &mut Empty).unwrap();
                runtime.remaining = budget;
                let result = if legacy {
                    runtime.tick().and_then(|()| match &expression {
                        Expr::Array(_) => runtime.eval_array_expression(&expression, 0, &mut Empty),
                        Expr::Object(_) => {
                            runtime.eval_object_expression(&expression, 0, &mut Empty)
                        }
                        Expr::Template(_) => {
                            runtime.eval_template_expression(&expression, 0, &mut Empty)
                        }
                        _ => unreachable!(),
                    })
                } else {
                    runtime.eval(&expression, 0, &mut Empty)
                }
                .map_err(|error| (error.kind, error.message, error.thrown));
                assert_eq!(runtime.eval_depth, 0);
                assert_eq!(runtime.sync_execution.active, 0);
                assert_eq!(runtime.sync_execution.live_frames, 0);
                assert_eq!(runtime.sync_execution.live_slots, 0);
                assert_eq!(runtime.sync_execution.live_bytes, 0);
                (
                    result,
                    runtime.remaining,
                    runtime.global_value("n"),
                    runtime.array_slots,
                    runtime.string_bytes,
                )
            };
            assert_eq!(run(false), run(true), "{source}, budget={budget}");
        }
    }
}

#[test]
fn literal_cursor_keeps_one_frame_and_moves_the_partial_array_buffer() {
    let expression = Expr::Array(vec![Expr::Literal(Value::Number(7.)); 4096]);
    let mut runtime = Runtime::default();
    let mut stack = SyncStack::new();
    let mut step = runtime
        .sync_start_literal(&expression, 0, &mut Empty, &mut stack)
        .unwrap();
    let capacity = runtime.sync_execution.live_slots;
    let mut output_pointer = None;
    let result = loop {
        match step {
            Step::Eval(Expr::Literal(value)) => {
                assert_eq!(runtime.sync_execution.live_frames, 1);
                assert_eq!(runtime.sync_execution.live_slots, capacity);
                let Some(SyncFrame::Literals(frame)) = stack.pop(&mut runtime).unwrap() else {
                    panic!("Expected exactly one literal continuation")
                };
                if let LiteralFrame::Array { values, .. } = &frame
                    && values.len() == 4095
                {
                    // At the final push the geometrically grown buffer has a
                    // spare slot. Finishing adopts this allocation unchanged.
                    assert!(values.capacity() >= 4096);
                    output_pointer = Some(values.as_ptr());
                }
                step = runtime
                    .sync_apply_literal(frame, value.clone(), 0, &mut Empty, &mut stack)
                    .unwrap();
            }
            Step::Value(value) => break value,
            _ => panic!("Expected literal child"),
        }
    };
    stack.finish(&mut runtime);
    let Value::Array(id) = result else {
        panic!("Expected array")
    };
    let crate::Heap::Array(values) = &runtime.heap[id] else {
        panic!("Expected array storage")
    };
    assert_eq!(values.len(), 4096);
    assert_eq!(Some(values.as_ptr()), output_pointer);
    assert_eq!(runtime.array_slots, 4096);
    assert_eq!(runtime.sync_execution.live_slots, 0);
    assert_eq!(runtime.sync_execution.live_frames, 0);
}

#[test]
fn literal_entry_preflight_stops_oversized_parser_bypassing_ast_effects() {
    let effect = Expr::Assign(
        "=".into(),
        Box::new(Expr::Name("n".into())),
        Box::new(Expr::Literal(Value::Number(1.))),
    );
    for expression in [
        Expr::Array(vec![effect.clone(); 4097]),
        Expr::Object(vec![("x".into(), effect); 4097]),
    ] {
        let mut runtime = Runtime::default();
        runtime.execute("var n=0;", &mut Empty).unwrap();
        let error = runtime.eval(&expression, 0, &mut Empty).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert_eq!(runtime.global_value("n"), Some(Value::Number(0.)));
        assert_eq!(runtime.sync_execution.live_slots, 0);
    }
}

#[test]
fn object_spread_and_method_names_preserve_getters_symbols_and_descriptors() {
    let source = "let log='';let key=Symbol('key');let source={};Object.defineProperty(source,'a',{enumerable:true,get:function(){log+='a';return 1}});Object.defineProperty(source,key,{enumerable:true,get:function(){log+='s';return 2}});function tail(){log+='t';return 3}let o={...source,m(){return this.a},z:tail()};let d=Object.getOwnPropertyDescriptor(o.m,'name');log==='ast'&&o[key]===2&&o.m()===1&&o.z===3&&o.m.name==='m'&&!d.writable&&!d.enumerable&&d.configurable;";
    assert_eq!(
        Runtime::default().execute(source, &mut Empty).unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn literal_child_abrupts_do_not_run_later_entries_or_resource_cleanup() {
    for source in [
        "var n=0;let source={};Object.defineProperty(source,'x',{enumerable:true,get:function(){throw 9}});let caught=false;try{let o={...source,z:n++}}catch(e){caught=e===9}caught&&n===0",
        "var n=0;let value={toString:function(){throw 9}};let caught=false;try{`${value}${n++}`}catch(e){caught=e===9}caught&&n===0",
    ] {
        assert_eq!(
            Runtime::default().execute(source, &mut Empty).unwrap(),
            Value::Bool(true)
        );
    }
    let mut runtime = Runtime::default();
    runtime.execute("var closes=0;var n=0;var source={};source[Symbol.iterator]=function(){return {next:function(){return {done:false,value:1}},return:function(){closes++;return {done:true}}}};", &mut Empty).unwrap();
    runtime.array_slots = 65_536;
    let expression = expression("[...source,n++];");
    let error = runtime.eval(&expression, 0, &mut Empty).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert!(error.thrown.is_none());
    assert_eq!(runtime.global_value("closes"), Some(Value::Number(0.)));
    assert_eq!(runtime.global_value("n"), Some(Value::Number(0.)));
    assert_eq!(runtime.sync_execution.active, 0);
    assert_eq!(runtime.sync_execution.live_slots, 0);
}

#[test]
fn nested_literals_and_calls_no_longer_accumulate_ast_eval_depth() {
    let nested = format!("{}f(n-1){}", "[".repeat(25), "][0]".repeat(25));
    let source = format!("function f(n){{if(n===0)return 7;return {nested}}}f(3)");
    assert_eq!(
        Runtime::default().execute(&source, &mut Empty).unwrap(),
        Value::Number(7.)
    );
}
