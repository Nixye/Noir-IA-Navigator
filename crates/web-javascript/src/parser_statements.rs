//! Keep each statement grammar in its own frame: unrelated statement
//! temporaries must not multiply at every level of recursive parsing.
use super::*;

impl Parser {
    pub(super) fn statement_starts_label(&self) -> bool {
        matches!(self.token().kind, Kind::Name(_))
            && self
                .tokens
                .get(self.index + 1)
                .is_some_and(|token| matches!(&token.kind, Kind::Symbol(s) if *s == ":"))
    }

    #[inline(never)]
    pub(super) fn statement_label(&mut self) -> Result<Stmt, JsError> {
        let name = self.name()?;
        self.require(":")?;
        if self.labels.iter().any(|(label, _)| label == &name) {
            return Err(self.error("Duplicate active label"));
        }
        let mut next = self.index;
        while matches!(self.tokens.get(next).map(|t| &t.kind), Some(Kind::Name(_)))
            && self
                .tokens
                .get(next + 1)
                .is_some_and(|t| matches!(&t.kind, Kind::Symbol(s) if *s == ":"))
        {
            next += 2;
        }
        let iteration = matches!(self.tokens.get(next).map(|t| &t.kind), Some(Kind::Name(s)) if matches!(s.as_str(), "for" | "while" | "do"));
        self.labels.push((name.clone(), iteration));
        let body = self.statement()?;
        self.labels.pop();
        Ok(Stmt::Label(name, Box::new(body)))
    }

    #[inline(never)]
    pub(super) fn statement_variables(&mut self) -> Result<Stmt, JsError> {
        let stmt = self.variables()?;
        self.semicolon()?;
        Ok(stmt)
    }

    #[inline(never)]
    pub(super) fn statement_function(&mut self, kind: FunctionKind) -> Result<Stmt, JsError> {
        let name = self.name()?;
        let (params, body, parameters) = self.function_body(kind)?;
        let expression = Expr::Function {
            kind,
            method: false,
            name: None,
            params,
            body,
            arrow: false,
            parameters: Some(parameters),
        };
        depth::validate_expression_depth(&expression)?;
        Ok(Stmt::FunctionDefinition(name, expression))
    }

    #[inline(never)]
    pub(super) fn statement_class(&mut self) -> Result<Stmt, JsError> {
        let name = self.name()?;
        let definition = self.class_body(Some(name.clone()))?;
        depth::validate_class_depth(&definition)?;
        Ok(Stmt::ClassDefinition(name, definition))
    }

    #[inline(never)]
    pub(super) fn statement_while(&mut self) -> Result<Stmt, JsError> {
        self.require("(")?;
        let c = self.expression(0)?;
        self.require(")")?;
        self.loop_depth += 1;
        let body = self.statement()?;
        self.loop_depth -= 1;
        Ok(Stmt::While(c, Box::new(body)))
    }

    #[inline(never)]
    pub(super) fn statement_do_while(&mut self) -> Result<Stmt, JsError> {
        self.loop_depth += 1;
        let body = self.statement()?;
        self.loop_depth -= 1;
        self.require("while")?;
        self.require("(")?;
        let condition = self.expression(0)?;
        self.require(")")?;
        self.eat(";");
        Ok(Stmt::DoWhile(Box::new(body), condition))
    }

    #[inline(never)]
    pub(super) fn statement_try(&mut self) -> Result<Stmt, JsError> {
        self.require("{")?;
        let body = self.block()?;
        let catch = if self.eat("catch") {
            let binding = if self.eat("(") {
                let name = self.name()?;
                self.require(")")?;
                Some(name)
            } else {
                None
            };
            self.require("{")?;
            Some((binding, self.block()?))
        } else {
            None
        };
        let finally = if self.eat("finally") {
            self.require("{")?;
            Some(self.block()?)
        } else {
            None
        };
        if catch.is_none() && finally.is_none() {
            return Err(self.error("Try requires catch or finally"));
        }
        Ok(Stmt::Try {
            body,
            catch,
            finally,
        })
    }

    #[inline(never)]
    pub(super) fn statement_switch(&mut self) -> Result<Stmt, JsError> {
        self.require("(")?;
        let discriminant = self.expression(0)?;
        self.require(")")?;
        self.require("{")?;
        self.switch_depth += 1;
        let mut clauses = Vec::new();
        let mut has_default = false;
        while !self.eat("}") {
            self.count()?;
            let test = if self.eat("case") {
                Some(self.expression(0)?)
            } else if self.eat("default") && !has_default {
                has_default = true;
                None
            } else {
                return Err(self.error("Expected case or a single default clause"));
            };
            self.require(":")?;
            let mut body = Vec::new();
            while !self.is("case") && !self.is("default") && !self.is("}") {
                if matches!(self.token().kind, Kind::End) {
                    return Err(self.error("Unclosed switch"));
                }
                body.push(self.statement()?);
            }
            clauses.push((test, body));
        }
        self.switch_depth -= 1;
        Ok(Stmt::Switch(discriminant, clauses))
    }

    #[inline(never)]
    pub(super) fn statement_for(&mut self) -> Result<Stmt, JsError> {
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
        // Recognize an uninitialized binding before relational `in` is
        // consumed, retaining the first pattern or Pratt context for the
        // classical initializer instead of constructing and discarding it.
        let mut for_of = false;
        let mut initial = None;
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
            if self.eat("in") || {
                for_of = self.eat("of");
                for_of
            } {
                Some(match pattern {
                    Pattern::Name(name) => ForBinding::Declaration(kind, name),
                    pattern => ForBinding::Pattern(kind, pattern),
                })
            } else {
                // For-await rejects here, before any initializer or additional
                // declarator, exactly as its former binding probe did.
                if !for_await {
                    initial = Some(self.variables_after_first(kind, pattern)?);
                }
                None
            }
        } else if !self.is(";") {
            let parsed = self.expression_goal(pratt::Goal::ForInitializer { for_await })?;
            let target = parsed.value;
            if parsed.for_binding {
                for_of = !self.eat("in");
                if for_of {
                    self.require("of")?;
                }
                if !matches!(target, Expr::Name(_) | Expr::Member(..)) {
                    return Err(self.error("Invalid for-in assignment target"));
                }
                Some(ForBinding::Reference(target))
            } else {
                initial = Some(Stmt::Expression(target));
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
        self.require(";")?;
        let initial = initial.map(Box::new);
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

    #[inline(never)]
    pub(super) fn statement_return(&mut self) -> Result<Stmt, JsError> {
        if self.function_depth == 0 {
            return Err(self.error("Return outside function"));
        }
        let value = if self.token().newline
            || self.is(";")
            || self.is("}")
            || matches!(self.token().kind, Kind::End)
        {
            None
        } else {
            Some(self.expression(0)?)
        };
        self.semicolon()?;
        Ok(Stmt::Return(value))
    }

    #[inline(never)]
    pub(super) fn statement_throw(&mut self) -> Result<Stmt, JsError> {
        if self.token().newline {
            return Err(self.error("Line break after throw"));
        }
        let e = self.expression(0)?;
        self.semicolon()?;
        Ok(Stmt::Throw(e))
    }

    #[inline(never)]
    pub(super) fn statement_break(&mut self) -> Result<Stmt, JsError> {
        let label = if !self.token().newline && matches!(self.token().kind, Kind::Name(_)) {
            Some(self.name()?)
        } else {
            None
        };
        if let Some(label) = &label {
            if !self.labels.iter().any(|(name, _)| name == label) {
                return Err(self.error("Unknown break label"));
            }
        } else if self.loop_depth == 0 && self.switch_depth == 0 {
            return Err(self.error("Break outside loop or switch"));
        }
        self.semicolon()?;
        Ok(Stmt::Break(label))
    }

    #[inline(never)]
    pub(super) fn statement_continue(&mut self) -> Result<Stmt, JsError> {
        let label = if !self.token().newline && matches!(self.token().kind, Kind::Name(_)) {
            Some(self.name()?)
        } else {
            None
        };
        if let Some(label) = &label {
            if !self
                .labels
                .iter()
                .any(|(name, iteration)| name == label && *iteration)
            {
                return Err(self.error("Continue label must target an iteration statement"));
            }
        } else if self.loop_depth == 0 {
            return Err(self.error("Continue outside loop"));
        }
        self.semicolon()?;
        Ok(Stmt::Continue(label))
    }
}
