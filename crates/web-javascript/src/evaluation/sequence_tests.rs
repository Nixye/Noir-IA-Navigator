use super::*;
use crate::{ErrorKind, Stmt};
struct Empty;
impl Host for Empty {
    fn global(&mut self, _: &str) -> Option<Value> {
        None
    }
    fn get(&mut self, _: u64, _: &str) -> Result<Value, JsError> {
        panic!("unexpected host")
    }
    fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
        panic!("unexpected host")
    }
    fn call(&mut self, _: u64, _: &str, _: &[Value]) -> Result<Value, JsError> {
        panic!("unexpected host")
    }
}
fn pair(source: &str) -> (Expr, Expr) {
    let Stmt::Expression(flat) = crate::parser::parse(source).unwrap().remove(0) else {
        panic!()
    };
    let Expr::Sequence(sequence) = &flat else {
        panic!()
    };
    let mut operands = sequence.items().iter().cloned();
    let head = operands.next().unwrap();
    let binary = operands.fold(head, |left, right| {
        Expr::Binary(",".into(), Box::new(left), Box::new(right))
    });
    (flat, binary)
}
#[test]
fn sequence_sync_ticks_errors_and_effect_prefix_match_manual_binary_ast() {
    for source in ["n=1,n=2,n=3,n=4", "n=1,missing,n=3", "n=1,n+=2,n*=3,n-4"] {
        let (flat, binary) = pair(source);
        for budget in 0..80 {
            let run = |expression: &Expr| {
                let mut runtime = Runtime::default();
                runtime.execute("let n=0;", &mut Empty).unwrap();
                runtime.remaining = budget;
                let result = runtime
                    .eval(expression, 0, &mut Empty)
                    .map_err(|error| (error.kind, error.message));
                (
                    result,
                    runtime.remaining,
                    runtime.scopes[0].bindings["n"].value.clone(),
                    runtime.eval_depth,
                )
            };
            assert_eq!(run(&flat), run(&binary), "{source} budget={budget}");
        }
    }
}
#[test]
fn sequence_preserves_reference_loss_getvalue_eval_and_left_to_right_effects() {
    for source in [
        "let order='';(order+='a',(order+='b',order+='c',order+='d')),order+='e';order==='abcde'",
        "('use strict',0,0);function f(eval){return eval;}f(3)===3",
        "let o={m:function(){'use strict';return this;}};(0,0,o.m)()===undefined",
        "let o={x:7,m(){return this.x;}};((o.m))()===7",
        "let n=0,o={};Object.defineProperty(o,'x',{get:function(){n++;return 1;},configurable:true});delete(0,0,o.x);n===1&&('x' in o)",
        "let caught=false;try{typeof(0,0,missing);}catch(e){caught=true;}caught",
        "var x=1;function f(){let x=2;(0,0,eval)('x=3');return x;}f()===2&&x===3",
        "let order='';let value=(order+='a',false&&(order+='bad'),order+='b');order==='ab'&&value==='ab'",
        "let order='';let value=(order+='a',true?order+='b':order+='bad',order+='c');order==='abc'&&value==='abc'",
        "let n=0;try{(n=1,missing,n=2);}catch(e){}n===1",
    ] {
        assert_eq!(
            Runtime::default().execute(source, &mut Empty).unwrap(),
            Value::Bool(true),
            "{source}"
        );
    }
    for source in ["(0,0,x)=1", "(0,0,x)++", "for((0,0,x) of []){}"] {
        assert_eq!(
            crate::parser::parse(source).unwrap_err().kind,
            ErrorKind::Syntax,
            "{source}"
        );
    }
}
#[test]
fn sequence_long_groups_execute_without_recursive_binary_depth() {
    std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(|| {
            let source = format!("{}1,1{}", "(".repeat(1022), "),1".repeat(1022));
            assert_eq!(
                Runtime::default().execute(&source, &mut Empty).unwrap(),
                Value::Number(1.)
            );
            let source = format!(
                "function f(){{return {};}}f();",
                std::iter::repeat_n("1", 64).collect::<Vec<_>>().join(",")
            );
            assert_eq!(
                Runtime::default().execute(&source, &mut Empty).unwrap(),
                Value::Number(1.)
            );
        })
        .unwrap()
        .join()
        .unwrap();
}
