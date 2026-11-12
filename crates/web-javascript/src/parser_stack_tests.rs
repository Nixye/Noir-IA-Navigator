use super::*;
use crate::ErrorKind;

fn small_stack(test: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .name("javascript-parser-1mib".into())
        .stack_size(1024 * 1024)
        .spawn(test)
        .unwrap()
        .join()
        .unwrap();
}

fn nested(open: &str, leaf: &str, close: &str, count: usize) -> String {
    format!("{}{leaf}{}", open.repeat(count), close.repeat(count))
}

#[test]
fn statement_nesting_parses_and_drops_on_one_mib_stack() {
    small_stack(|| {
        for (open, close) in [
            ("{", "}"),
            ("if (true)", ""),
            ("while (false)", ""),
            ("do ", " while (false);"),
            ("for (;;) ", ""),
            ("for (let item of []) ", ""),
            ("switch (0) { default:", "}"),
            ("try {", "} finally {}"),
            ("function f() {", "}"),
        ] {
            let source = nested(open, ";", close, 63);
            assert!(parse(&source).is_ok(), "{open}");
            assert!(parse_module(&source).is_ok(), "{open}");
        }
        let labels = (0..63)
            .map(|index| format!("label{index}:"))
            .collect::<String>();
        assert!(parse(&(labels + ";")).is_ok());
        let source = nested("{", "1;", "}", 62);
        assert!(crate::module_requests(&source).unwrap().is_empty());
    });
}

#[test]
fn nesting_resource_and_syntax_errors_unwind_on_one_mib_stack() {
    small_stack(|| {
        for (source, expected) in [
            (nested("{", ";", "}", 64), "Syntax nesting budget exceeded"),
            (
                nested("!", "1", "", 64),
                "Expression nesting budget exceeded",
            ),
        ] {
            let error = parse(&source).unwrap_err();
            assert_eq!(error.kind, ErrorKind::Resource);
            assert_eq!(error.message, expected);
        }
        let error = parse(&nested("{", ";", "", 62)).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Syntax);
        assert_eq!(error.message, "Unclosed block");
    });
}

#[test]
fn expression_nesting_parses_and_drops_on_one_mib_stack() {
    small_stack(|| {
        for (open, close) in [("(", ")"), ("[", "]"), ("!", ""), ("f(", ")"), ("a[", "]")] {
            assert!(parse(&nested(open, "1", close, 62)).is_ok(), "{open}");
        }
    });
}

#[test]
fn recursive_literal_and_function_grammars_fit_one_mib_stack() {
    small_stack(|| {
        for (name, source) in [
            ("object", format!("let x={};", nested("{a:", "1", "}", 60))),
            ("constructor", nested("new ", "F", "", 60)),
            (
                "class",
                format!("let x={};", nested("class { field = ", "1", ";}", 60)),
            ),
            (
                "function",
                format!("let x={};", nested("function() {return ", "1", ";}", 30)),
            ),
            ("pattern", format!("let {}=[];", nested("[", "x", "]", 60))),
            (
                "await",
                format!(
                    "async function f(){{return {};}}",
                    nested("await ", "1", "", 60)
                ),
            ),
            (
                "yield",
                format!("function* f(){{return {};}}", nested("yield ", "1", "", 60)),
            ),
        ] {
            assert!(parse(&source).is_ok(), "{name}");
        }
    });
}

#[test]
fn expression_structure_budget_prevents_composed_chains_on_one_mib_stack() {
    small_stack(|| {
        // Every individual chain/group is under its old local limit, but the
        // resulting tree would otherwise grow hundreds of native frames deep.
        let at_limit = format!("1{}", "+1".repeat(63));
        assert!(parse(&at_limit).is_ok());
        let mut binary = "1".to_string();
        for _ in 0..4 {
            binary = format!("({binary}){}", "+1".repeat(60));
        }
        let mut member = "a".to_string();
        for _ in 0..4 {
            member = format!("({member}){}", ".x".repeat(60));
        }
        let mut constructor = "F".to_string();
        for _ in 0..4 {
            constructor = format!("new {constructor}(){}", ".x".repeat(60));
        }
        for expression in [binary, member, constructor] {
            for source in [
                expression.clone(),
                format!("if(false){{{expression};}}"),
                format!("function f(){{return {expression};}}"),
            ] {
                let error = parse(&source).unwrap_err();
                assert_eq!(error.kind, ErrorKind::Resource);
                assert_eq!(error.message, "Expression tree nesting budget exceeded");
                assert_eq!(parse_module(&source).unwrap_err().kind, ErrorKind::Resource);
            }
        }
    });
}

#[test]
fn structural_walk_reaches_functions_parameters_methods_and_class_fields() {
    small_stack(|| {
        let expression = format!("1{}", "+1".repeat(63));
        for source in [
            format!("let f=function(){{return {expression};}};"),
            format!("function f(){{return {expression};}}"),
            format!("class C {{field={expression};}}"),
            format!("let f=function(p={expression}){{}};"),
            format!("let x=class {{field={expression};}};"),
            format!("let x=class {{m(){{return {expression};}}}};"),
            format!("let x={{m(){{return {expression};}}}};"),
        ] {
            let error = parse(&source).unwrap_err();
            assert_eq!(error.kind, ErrorKind::Resource);
            assert_eq!(error.message, "Expression tree nesting budget exceeded");
        }
    });
}

#[test]
fn dynamic_function_grammars_keep_the_structural_depth_limit() {
    small_stack(|| {
        let expression = format!("1{}", "+1".repeat(63));
        for (parameters, body) in [
            (String::new(), format!("return {expression};")),
            (format!("p={expression}"), String::new()),
        ] {
            let error = parse_dynamic_function(&parameters, &body).unwrap_err();
            assert_eq!(error.kind, ErrorKind::Resource);
            assert_eq!(error.message, "Expression tree nesting budget exceeded");
        }
    });
}

#[test]
fn declaration_and_default_export_roots_cannot_hide_deep_expression_trees() {
    small_stack(|| {
        let leaf = format!("1{};", "+1".repeat(60));
        let source = nested("function f(){", &leaf, "}", 30);
        let error = parse(&source).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert_eq!(error.message, "Expression tree nesting budget exceeded");
        assert_eq!(parse_module(&source).unwrap_err().kind, ErrorKind::Resource);
        let expression = format!("1{}", "+1".repeat(63));
        for source in [
            format!("export default function(){{return {expression};}}"),
            format!("export default class {{field={expression};}}"),
        ] {
            let error = parse_module(&source).unwrap_err();
            assert_eq!(error.kind, ErrorKind::Resource);
            assert_eq!(error.message, "Expression tree nesting budget exceeded");
        }
    });
}

#[test]
fn wide_literals_and_function_bodies_keep_depth_walk_bounded() {
    small_stack(|| {
        let wide = format!("[{}1]", "1,".repeat(4095));
        assert!(parse(&nested("[", &wide, "]", 60)).is_ok());
        let body = ";".repeat(4096);
        assert!(
            parse(&format!(
                "let f={};",
                nested(
                    "function(){return ",
                    &format!("function(){{{body}}}"),
                    ";}",
                    20
                )
            ))
            .is_ok()
        );
    });
}

struct EmptyHost;
impl crate::Host for EmptyHost {
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
fn parsed_function_and_class_trees_validate_and_execute_on_one_mib_stack() {
    small_stack(|| {
        for source in [
            nested("function f(){", ";", "}", 63),
            nested("function f(a=1){", ";", "}", 60),
            format!("let x={};", nested("class { field = ", "1", ";}", 60)),
            format!(
                "async function f(){{return {};}}",
                nested("await ", "1", "", 60)
            ),
            format!("function* f(){{return {};}}", nested("yield ", "1", "", 60)),
        ] {
            let mut runtime = crate::Runtime::default();
            runtime.execute(&source, &mut EmptyHost).unwrap();
        }
    });
}
