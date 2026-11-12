//! Differential oracle for the former two-pass for-head parser. Keep this
//! test-only copy independent of the retained-head production path. Shared
//! grammar helpers and work ledgers remain current, isolating head reuse.
use super::*;
use crate::{ErrorKind, Host, Runtime};

fn parser(source: &str, kind: FunctionKind) -> Parser {
    Parser {
        tokens: lex(source).unwrap(),
        index: 0,
        depth: 0,
        expression_frames: 0,
        function_depth: usize::from(kind != FunctionKind::Ordinary),
        function_kind: kind,
        module_context: false,
        loop_depth: 0,
        switch_depth: 0,
        labels: Vec::new(),
        visits: 0,
        group_visits: 0,
        flat_storage_bytes: 0,
    }
}

impl Parser {
    #[inline(never)]
    fn legacy_statement_for(&mut self) -> Result<Stmt, JsError> {
        let for_await = self.eat("await");
        if for_await
            && !matches!(
                self.function_kind,
                FunctionKind::Async | FunctionKind::AsyncGenerator
            )
        {
            return Err(self.error("For-await-of requires an async function"));
        }
        self.require("(")?;
        // Recognize a single uninitialized declaration/reference before
        // the relational parser consumes `in`. The fallback is the usual
        // three-clause loop; speculative syntax work still counts.
        let saved = self.index;
        let mut for_of = false;
        let binding = if self.is("var") || self.is("let") || self.is("const") {
            let kind = if self.eat("var") {
                Declaration::Var
            } else if self.eat("let") {
                Declaration::Let
            } else {
                self.require("const")?;
                Declaration::Const
            };
            let pattern = self.pattern(false)?;
            let found = if self.eat("in") {
                true
            } else if self.eat("of") {
                for_of = true;
                true
            } else {
                false
            };
            found.then_some(match pattern {
                Pattern::Name(name) => ForBinding::Declaration(kind, name),
                pattern => ForBinding::Pattern(kind, pattern),
            })
        } else if !self.is(";") {
            // A bare yield is a valid classical-for initializer but not a
            // candidate for an in/of reference. Probe without committing
            // syntax errors; resource failures must remain fatal.
            let bare_yield = self.is("yield");
            let target = match self.expression(10) {
                Ok(target) => Some(target),
                Err(error) if bare_yield && error.kind == crate::ErrorKind::Syntax => None,
                Err(error) => return Err(error),
            };
            if let Some(target) = target
                && (self.eat("in") || {
                    let found = self.eat("of");
                    for_of = found;
                    found
                })
            {
                if !matches!(target, Expr::Name(_) | Expr::Member(..)) {
                    return Err(self.error("Invalid for-in assignment target"));
                }
                Some(ForBinding::Reference(target))
            } else {
                None
            }
        } else {
            None
        };
        if for_await && (!for_of || binding.is_none()) {
            return Err(self.error("For-await-of requires a binding followed by of"));
        }
        if let Some(binding) = binding {
            let source = self.expression(if for_await { 1 } else { 0 })?;
            self.require(")")?;
            self.loop_depth += 1;
            let body = self.statement()?;
            self.loop_depth -= 1;
            if let ForBinding::Declaration(kind, name) = &binding
                && *kind != Declaration::Var
                && self.contains_var_declaration(&body, name)?
            {
                return Err(
                    self.error("For-in lexical binding conflicts with a body var declaration")
                );
            }
            if let ForBinding::Pattern(kind, pattern) = &binding
                && *kind != Declaration::Var
            {
                let mut names = Vec::new();
                pattern.names(&mut names);
                for name in names {
                    if self.contains_var_declaration(&body, &name)? {
                        return Err(self.error("Loop lexical pattern conflicts with var"));
                    }
                }
            }
            return Ok(if for_await {
                Stmt::ForAwaitOf(binding, source, Box::new(body))
            } else if for_of {
                Stmt::ForOf(binding, source, Box::new(body))
            } else {
                Stmt::ForIn(binding, source, Box::new(body))
            });
        }
        self.index = saved;
        let initial = if self.eat(";") {
            None
        } else {
            let s = if self.is("let") || self.is("const") || self.is("var") {
                self.variables()?
            } else {
                Stmt::Expression(self.expression(0)?)
            };
            self.require(";")?;
            Some(Box::new(s))
        };
        let condition = if self.eat(";") {
            None
        } else {
            let e = self.expression(0)?;
            self.require(";")?;
            Some(e)
        };
        let update = if self.eat(")") {
            None
        } else {
            let e = self.expression(0)?;
            self.require(")")?;
            Some(e)
        };
        self.loop_depth += 1;
        let body = self.statement()?;
        self.loop_depth -= 1;
        Ok(Stmt::For(initial, condition, update, Box::new(body)))
    }
}

fn compare(source: &str, kind: FunctionKind) {
    let mut retained = parser(source, kind);
    let mut reparsed = parser(source, kind);
    assert!(retained.eat("for"));
    assert!(reparsed.eat("for"));
    let actual = retained.statement_for();
    let expected = reparsed.legacy_statement_for();
    assert_eq!(
        format!("{actual:?}"),
        format!("{expected:?}"),
        "{source} ({kind:?})"
    );
    assert_eq!(retained.index, reparsed.index, "{source} ({kind:?})");
    assert_eq!(retained.depth, 0, "{source}");
    assert_eq!(retained.expression_frames, 0, "{source}");
    assert!(retained.visits <= reparsed.visits, "{source}");
    assert!(
        retained.flat_storage_bytes <= reparsed.flat_storage_bytes,
        "{source}"
    );
}

#[test]
fn retained_for_head_matches_former_parser_grammar_errors_and_offsets() {
    for source in [
        "for(;;){}",
        "for(;false;){}",
        "for(a;false;a++){}",
        "for(a=1;false;){}",
        "for(a,b,c;false;){}",
        "for(a.b[0](1)+2<3?4:5;false;){}",
        "for(a=1 in b;false;){}",
        "for(a=1 of b;false;){}",
        "for(a?b:c of d;false;){}",
        "for(a<b in c;false;){}",
        "for(a+b in c){}",
        "for(a.b in c){}",
        "for(a[0] of c){}",
        "for(this in c){}",
        "for([a] of c){}",
        "for({a} in c){}",
        "for(var a in b){}",
        "for(let a of b){}",
        "for(const a of b){}",
        "for(let [a,b] of c){}",
        "for(const {a:b} in c){}",
        "for(var a,b=2;false;){}",
        "for(let a=1,b=2;false;){}",
        "for(const [a=1]=[];false;){}",
        "for(let {a:(b)}=c;false;){}",
        "for(let [a=(1,2,3)]=[];false;){}",
        "for(const {a=1}=b;false;){}",
        "for(const a;false;){}",
        "for(let [a];false;){}",
        "for(var ;false;){}",
        "for(let a of b){var a;}",
        "for(let [a] of b){var a;}",
        "for((a) of b){}",
        "for((a in b);false;){}",
        "for(((a+b));false;){}",
        "for(((a))+b=1;false;){}",
        "for((a=>a)(1);false;){}",
        "for((a=1)=>a;false;){}",
        "for((a)\n=>a;false;){}",
        "for(a\n++;false;){}",
        "for(a\n++b;false;){}",
        "for(a\n;false;){}",
        "for(yield;false;){}",
        "for(yield 1;false;){}",
        "for(yield* a;false;){}",
        "for(yield\n1;false;){}",
        "for(yield,1;false;){}",
        "for((yield)+1;false;){}",
        "for((yield) of a){}",
        "for(await a;false;){}",
        "for((await a) of b){}",
        "for await(a of b){}",
        "for await(var a of b){}",
        "for await(const [a] of b){}",
        "for await(a in b){}",
        "for await(;;){}",
        "for await(a=;false;){}",
        "for await(let a=?;false;){}",
        "for await(let a,b of c){}",
        "for await(a+b of c){}",
        "for await(yield;false;){}",
        "for await(yield ?;false;){}",
        "for await((yield);false;){}",
        "for await(a of b,c){}",
        "for await(a of (b,c)){}",
        "for(a+;false;){}",
        "for(a[?];false;){}",
        "for(a(,);false;){}",
        "for(a=1;false;)",
        "for(a=1 false;){}",
        "for(a=1;false){}",
    ] {
        for kind in [
            FunctionKind::Ordinary,
            FunctionKind::Generator,
            FunctionKind::Async,
            FunctionKind::AsyncGenerator,
        ] {
            compare(source, kind);
        }
    }
    // Cross products catch accidental in/of recognition after a lower-
    // precedence classical tail, rather than at the original min-10 boundary.
    for left in [
        "a", "(a)", "(a,b,c)", "a.b", "a[0]", "a+1", "a<<1", "(()=>1)",
    ] {
        for middle in ["", "=1", ",b", "<b", "==b", "&&b", "?b:c", "instanceof b"] {
            for end in [";false;){}", " in c){}", " of c){}", " of c;false;){}"] {
                compare(&format!("for({left} {middle}{end}"), FunctionKind::Ordinary);
            }
        }
    }
}

#[test]
fn retained_for_head_keeps_chain_frame_and_tree_boundaries() {
    for count in [61, 62, 63, 64] {
        for source in [
            format!("for(a{};false;){{}}", ".x".repeat(count)),
            format!("for(a{}=0;false;){{}}", ".x".repeat(count)),
            format!("for(a{}+0<1;false;){{}}", ".x".repeat(count)),
            format!("for(a{} in b){{}}", ".x".repeat(count)),
            format!("for await(a{}=?;false;){{}}", ".x".repeat(count)),
            format!("for({}1;false;){{}}", "!".repeat(count)),
            format!(
                "for({}1{};false;){{}}",
                "f(".repeat(count),
                ")".repeat(count)
            ),
            format!(
                "for((a{}){}=0;false;){{}}",
                ".x".repeat(count / 2),
                ".x".repeat(count - count / 2)
            ),
        ] {
            compare(&source, FunctionKind::AsyncGenerator);
        }
    }
    parse(&format!("for(a{}=0;false;){{}}", ".x".repeat(62))).unwrap();
    let error = parse(&format!("for(a{}=0;false;){{}}", ".x".repeat(63))).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert_eq!(error.message, "Expression chain budget exceeded");
    // The first phase's Resource failure must beat the for-await binding error.
    let error = parse(&format!(
        "async function f(){{for await(a{}=?;false;){{}}}}",
        ".x".repeat(64)
    ))
    .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert_eq!(error.message, "Expression chain budget exceeded");
}

#[test]
fn retained_for_initializer_parses_wide_function_body_once() {
    for count in [4_000, 5_000] {
        let initial = format!("(function(){{{}}})()", "0;".repeat(count));
        let mut once = parser(&initial, FunctionKind::Ordinary);
        once.expression(0).unwrap();
        let source = format!("for({initial};false;){{}}");
        let mut retained = parser(&source, FunctionKind::Ordinary);
        retained.statement().unwrap();
        assert_eq!(retained.visits, once.visits + 3);
        assert_eq!(retained.visits, 2 * count + 4);
        assert_eq!(MAX_PARSER_VISITS, 16_384);
        let mut reparsed = parser(&source, FunctionKind::Ordinary);
        // Match statement()'s entry charge and native depth on both paths.
        reparsed.count().unwrap();
        reparsed.depth = 1;
        assert!(reparsed.eat("for"));
        if count == 4_000 {
            reparsed.legacy_statement_for().unwrap();
            assert_eq!(reparsed.visits, 2 * once.visits + 3);
        } else {
            let error = reparsed.legacy_statement_for().unwrap_err();
            assert_eq!(error.kind, ErrorKind::Resource);
            assert_eq!(error.message, "Parser visit budget exceeded");
        }
    }
}

#[test]
fn retained_for_declaration_reuses_the_first_destructuring_pattern() {
    for declaration in [
        "let [a=(1,2,3)]=[],b=4",
        "var {a:b=(1,2,3)}={},c=4",
        "const [a=(1,2,3),...rest]=[]",
    ] {
        let mut once = parser(declaration, FunctionKind::Ordinary);
        let expected = once.variables().unwrap();
        let mut retained = parser(
            &format!("for({declaration};false;){{}}"),
            FunctionKind::Ordinary,
        );
        let Stmt::For(Some(initial), _, _, _) = retained.statement().unwrap() else {
            panic!()
        };
        assert_eq!(format!("{initial:?}"), format!("{expected:?}"));
        assert_eq!(retained.visits, once.visits + 3);
        assert_eq!(retained.group_visits, once.group_visits);
        assert!(once.flat_storage_bytes > 0);
        assert_eq!(retained.flat_storage_bytes, once.flat_storage_bytes);
    }
}

#[test]
fn retained_for_initializer_preserves_monotonic_work_and_storage_on_errors() {
    for source in [
        "for((1,2,3)=?;false;){}",
        "for((1,2,3);false;)",
        "for(let [a=(1,2,3)]=?;false;){}",
        "for await((1,2,3);false;){}",
    ] {
        let mut failed = parser(source, FunctionKind::AsyncGenerator);
        let error = failed.statement().unwrap_err();
        assert_eq!(error.kind, ErrorKind::Syntax);
        assert!(failed.flat_storage_bytes > 0);
        let storage = failed.flat_storage_bytes;
        let visits = failed.visits;
        failed.tokens = lex("(4,5,6)").unwrap();
        failed.index = 0;
        failed.expression(0).unwrap();
        assert_eq!(failed.flat_storage_bytes, 2 * storage);
        assert!(failed.visits > visits);
        assert_eq!(failed.expression_frames, 0);
        assert_eq!(failed.depth, 0);
    }
    // A failed root entry must not be hidden by direct-yield handling.
    let mut failed = parser("for(yield;false;){}", FunctionKind::Generator);
    failed.visits = MAX_PARSER_VISITS;
    assert!(failed.eat("for"));
    assert_eq!(
        failed.statement_for().unwrap_err().kind,
        ErrorKind::Resource
    );
    assert_eq!(failed.visits, MAX_PARSER_VISITS + 1);
}

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

#[test]
fn retained_for_initializers_execute_once_in_source_order() {
    for (source, expected) in [
        (
            "var n=0;for((function(){n++;})();false;){}n",
            Value::Number(1.),
        ),
        (
            "var s='';function f(x){s+=x;return x;}for(f('a'),f('b');false;f('c')){f('d');}s",
            Value::string("ab"),
        ),
        (
            "var s='';var o={};Object.defineProperty(o,'x',{get(){s+='g';return 1;},set(v){s+='s';}});for(o.x+=2;false;){}s",
            Value::string("gs"),
        ),
        (
            "var n=0;for(let [a=(n++,3)]=[];n<3;n++){n+=a;}n",
            Value::Number(5.),
        ),
        (
            "var n=0;for(var a=1,b=2;a<3;a++){n+=a+b;}n",
            Value::Number(7.),
        ),
        ("var s='';for(var x of [1,2]){s+=x;}s", Value::string("12")),
        (
            "var s='';for(var x in {a:1,b:2}){s+=x;}s",
            Value::string("ab"),
        ),
        (
            "function* f(){for(yield 3;false;){}return 4;}var g=f();var a=g.next().value; a+g.next().value",
            Value::Number(7.),
        ),
        (
            "var n=0;async function f(){for(await (n++,1);false;){}return n;}f();n",
            Value::Number(1.),
        ),
    ] {
        assert_eq!(
            Runtime::default().execute(source, &mut Empty).unwrap(),
            expected,
            "{source}"
        );
    }
    let mut runtime = Runtime::default();
    runtime
        .execute(
            "var n=0;async function f(){for await(var x of [1,2]){n+=x;}}f();",
            &mut Empty,
        )
        .unwrap();
    // Async iteration finishes at the script's job checkpoint, after its
    // immediate completion value has already been captured.
    assert_eq!(runtime.execute("n", &mut Empty).unwrap(), Value::Number(3.));
}

#[test]
fn retained_for_initializer_resource_failures_bypass_javascript_handlers() {
    let mut runtime = Runtime::default();
    let source = format!(
        "var caught=0;try{{eval('for(a{};false;){{}}')}}catch(e){{caught=1;}}finally{{caught+=2;}}",
        ".x".repeat(64),
    );
    let error = runtime.execute(&source, &mut Empty).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert_eq!(error.message, "Expression chain budget exceeded");
    assert_eq!(
        runtime.execute("caught", &mut Empty).unwrap(),
        Value::Number(0.)
    );
}

#[test]
fn retained_for_initializers_fit_one_mib_native_stack() {
    std::thread::Builder::new()
        .name("retained-for-initializer-1mib".into())
        .stack_size(1024 * 1024)
        .spawn(|| {
            retained_for_head_keeps_chain_frame_and_tree_boundaries();
            retained_for_initializer_parses_wide_function_body_once();
            retained_for_declaration_reuses_the_first_destructuring_pattern();
            retained_for_initializer_preserves_monotonic_work_and_storage_on_errors();
            retained_for_initializers_execute_once_in_source_order();
        })
        .unwrap()
        .join()
        .unwrap();
}
