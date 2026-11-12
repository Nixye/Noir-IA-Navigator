use super::*;
use crate::{ErrorKind, Stmt};
#[derive(Default)]
struct Effects(Vec<String>);
impl Host for Effects {
    fn global(&mut self, name: &str) -> Option<Value> {
        (name == "h").then_some(Value::Host(1))
    }
    fn get(&mut self, _: u64, property: &str) -> Result<Value, JsError> {
        self.0.push(format!("get:{property}"));
        Ok(Value::Number(1.))
    }
    fn set(&mut self, _: u64, property: &str, value: Value) -> Result<(), JsError> {
        self.0.push(format!("set:{property}:{value:?}"));
        Ok(())
    }
    fn call(&mut self, _: u64, _: &str, _: &[Value]) -> Result<Value, JsError> {
        panic!("unexpected host call")
    }
}
fn pair(source: &str) -> (Expr, Expr) {
    let Stmt::Expression(flat) = crate::parser::parse(source).unwrap().remove(0) else {
        panic!()
    };
    let Expr::ConditionalChain(chain) = &flat else {
        panic!("Expected chain: {source}")
    };
    let mut legacy = chain.fallback().clone();
    for (condition, yes) in chain.clauses_rev() {
        legacy = Expr::Conditional(
            Box::new(condition.clone()),
            Box::new(yes.clone()),
            Box::new(legacy),
        );
    }
    (flat, legacy)
}
#[test]
fn conditional_chain_sync_matches_legacy_ticks_errors_and_host_effects_at_every_budget() {
    for source in [
        "(h.x=1,true)?(h.x=2,11):(h.x=3,false)?22:(h.x=4,true)?33:44",
        "(h.x=1,false)?11:(h.x=2,true)?(h.x=3,22):(h.x=4,true)?33:44",
        "(h.x=1,false)?11:(h.x=2,false)?22:(h.x=3,true)?(h.x=4,33):44",
        "(h.x=1,false)?11:(h.x=2,false)?22:(h.x=3,false)?33:(h.x=4,44)",
        "(h.x=1,false)?11:(h.x=2,missing)?22:(h.x=3,44)",
        "(h.x=1,false)?11:true?boom():44",
        "false?11:false?22:boom()",
        "(+bad,false)?11:true?22:44",
        "false?11:(+bad,false)?22:44",
        "false?11:true?+bad:44",
        "false?11:false?22:+bad",
    ] {
        let (flat, legacy) = pair(source);
        for budget in 0..140 {
            let run = |expr: &Expr| {
                let mut runtime = Runtime::default();
                let mut host = Effects::default();
                runtime.execute("function boom(){h.x=9;throw 7;}let bad={valueOf:function(){h.x=8;throw 6;}};", &mut host).unwrap();
                runtime.remaining = budget;
                let result = runtime
                    .eval(expr, 0, &mut host)
                    .map_err(|error| (error.kind, error.message, error.thrown));
                (result, runtime.remaining, host.0, runtime.eval_depth)
            };
            assert_eq!(run(&flat), run(&legacy), "{source}; budget={budget}");
        }
    }
}
#[test]
fn conditional_chain_sync_preserves_value_not_reference_and_lazy_choices() {
    for source in [
        "let o={m:function(){'use strict';return this;}};(false?null:true?o.m:null)()===undefined",
        "let o={m:function(){'use strict';return this;}};(true?o.m:false?null:null)()===undefined",
        "var x=1;function f(){let x=2;(false?null:true?eval:null)('x=3');return x;}f()===2&&x===3",
        "var x=1;function f(){let x=2;eval('x=3');return x;}f()===3&&x===1",
        "let n=0,o={};Object.defineProperty(o,'x',{get:function(){n++;return 1;},configurable:true});delete(false?0:true?o.x:0);n===1&&('x' in o)",
        "let caught=false;try{typeof(false?0:false?0:missing);}catch(e){caught=true;}caught",
        "typeof(false?missing:true?1:missing)==='number'",
        "let n=0;let value=false?(n=99):true?(n=1):(n=99);value===1&&n===1",
        "let n=0;try{false?0:true?(()=>{throw 7})():(n=9);}catch(e){n=e}n===7",
        "let n=0;false?missing:false?missing:(n=4);n===4",
        "(false?0:true?'use strict':0);function f(eval){return eval;}f(3)===3",
        "let o={valueOf:function(){throw 9;}};(false?0:o?1:2)===1",
    ] {
        assert_eq!(
            Runtime::default()
                .execute(source, &mut Effects::default())
                .unwrap(),
            Value::Bool(true),
            "{source}"
        );
    }
}
#[test]
fn conditional_chain_walkers_visit_unselected_branches_and_keep_early_errors() {
    for source in [
        "'use strict';true?1:false?(eval=1):0",
        "'use strict';true?1:false?0:delete missing",
        "'use strict';true?1:(arguments=1)?2:3",
        "true?1:false?function(){let x;var x;}:0",
        "true?1:false?0:class C{m(){arguments=1;}}",
        "true?1:(function(){let x;var x;})?2:3",
        "let f=Function('return true?1:false?function(){let x;var x;}:0');",
        "let f=Function('x=true?1:false?function(){let x;var x;}:0','return x');",
    ] {
        let mut host = Effects::default();
        let result = Runtime::default().execute(source, &mut host);
        assert_eq!(result.unwrap_err().kind, ErrorKind::Syntax, "{source}");
        assert!(host.0.is_empty());
    }
    let statements =
        crate::parser::parse("'use strict';true?(eval=1):false?delete missing:0").unwrap();
    let error = crate::strict_validation::validate(&statements, false).unwrap_err();
    assert_eq!(
        error.message,
        "Invalid binding, assignment or deletion in strict mode"
    );
}
#[test]
fn conditional_chain_sync_retains_active_eval_guard_and_composed_one_mib_execution() {
    let (flat, _) = pair("false?1:false?2:3");
    let mut runtime = Runtime {
        eval_depth: 64,
        ..Runtime::default()
    };
    let error = runtime.eval(&flat, 0, &mut Effects::default()).unwrap_err();
    assert_eq!(error.message, "Expression evaluation depth exceeded");
    assert_eq!(runtime.eval_depth, 64);
    std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(|| {
            let source = format!("({}7){}", "false?1:".repeat(20), "+1".repeat(45));
            assert_eq!(
                Runtime::default()
                    .execute(&source, &mut Effects::default())
                    .unwrap(),
                Value::Number(52.)
            );
            let source = format!("function f(){{return {source};}}f()");
            assert_eq!(
                Runtime::default()
                    .execute(&source, &mut Effects::default())
                    .unwrap(),
                Value::Number(52.)
            );
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn conditional_chain_mixed_yes_calls_and_functions_execute_on_one_mib() {
    std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(|| {
            let mut expression = "1".to_owned();
            for _ in 0..15 {
                expression = format!("true?(1+(function(){{return ({expression});}})()):false?0:0");
            }
            assert_eq!(
                Runtime::default()
                    .execute(&expression, &mut Effects::default())
                    .unwrap(),
                Value::Number(16.)
            );
        })
        .unwrap()
        .join()
        .unwrap();
}
