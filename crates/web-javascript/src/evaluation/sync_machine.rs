//! Synchronous borrowed-AST dispatcher. It cannot suspend or escape eval().
use super::{
    storage::SyncStack,
    sync_frames::{CoreFrame, SyncFrame},
};
use crate::{ErrorKind, Expr, Host, JsError, Runtime, Value, intrinsic_global};

pub(crate) enum Step<'ast> {
    Eval(&'ast Expr),
    Value(Value),
}

impl Runtime {
    #[inline(never)]
    pub(crate) fn eval_sync(
        &mut self,
        expression: &Expr,
        scope: usize,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let mut stack = SyncStack::new();
        let mut operation = expression_name(expression);
        let result: Result<Value, JsError> = (|| {
            let mut step = Step::Eval(expression);
            // eval() charged the root before checking native admission.
            let mut root = true;
            loop {
                operation = match &step {
                    Step::Eval(node) => expression_name(node),
                    Step::Value(_) => "pop",
                };
                self.sync_transition()?;
                step = match step {
                    Step::Eval(node) => {
                        operation = expression_name(node);
                        if root {
                            root = false;
                        } else {
                            self.tick()?;
                        }
                        self.sync_start(node, scope, host, &mut stack)?
                    }
                    Step::Value(value) => match stack.pop(self)? {
                        Some(SyncFrame::Core(frame)) => {
                            operation = "operator-apply";
                            self.sync_apply_core(frame, value, host, &mut stack)?
                        }
                        Some(SyncFrame::References(frame)) => {
                            operation = "reference-apply";
                            self.sync_apply_reference(frame, value, scope, host, &mut stack)?
                        }
                        Some(SyncFrame::Calls(frame)) => {
                            operation = "call-apply";
                            self.sync_apply_call(frame, value, scope, host, &mut stack)?
                        }
                        Some(SyncFrame::Literals(frame)) => {
                            operation = "literal-apply";
                            self.sync_apply_literal(frame, value, scope, host, &mut stack)?
                        }
                        None => return Ok(value),
                    },
                };
            }
        })();
        let result = result.map_err(|mut error| {
            if error.kind == ErrorKind::Resource {
                self.sync_error_context(&mut error, operation);
            }
            error
        });
        stack.finish(self);
        result
    }

    #[inline(never)]
    fn sync_start<'ast>(
        &mut self,
        node: &'ast Expr,
        scope: usize,
        host: &mut impl Host,
        stack: &mut SyncStack<'ast>,
    ) -> Result<Step<'ast>, JsError> {
        Ok(match node {
            Expr::Literal(value) => Step::Value(value.clone()),
            Expr::Name(name) => Step::Value(self.read(scope, name, host)?),
            Expr::RegExp(pattern, flags) => Step::Value(self.make_regexp(pattern, flags)?),
            Expr::ImportMeta => Step::Value(self.eval_import_meta(scope)?),
            Expr::Function { .. } => Step::Value(self.eval_function_expression(node, scope, host)?),
            // Explicit class bridge: extends/computed defaults may enter eval;
            // all such entries share native/active/storage/work guards. Class
            // activation owns its strict/private state restoration and values
            // stay protected by the dispatch-wide GC barrier.
            Expr::Class(definition) => Step::Value(self.eval_class(definition, scope, host)?),
            Expr::Await(_) | Expr::Yield(_) | Expr::YieldDelegate(_) => {
                return Err(JsError::syntax(
                    0,
                    "Suspension requires a resumable function frame",
                ));
            }
            Expr::Spread(_) | Expr::Hole | Expr::Super => {
                return Err(JsError::runtime("Invalid standalone language expression"));
            }
            Expr::Array(_) | Expr::Object(_) | Expr::Template(_) => {
                return self.sync_start_literal(node, scope, host, stack);
            }
            Expr::Member(..) | Expr::PrivateMember(..) | Expr::Assign(..) | Expr::Update(..) => {
                return self.sync_start_references(node, scope, host, stack);
            }
            Expr::Call(..) | Expr::Construct(..) | Expr::ImportCall { .. } => {
                return self.sync_start_call(node, scope, host, stack);
            }
            Expr::Unary(op, _) if op == "delete" => {
                return self.sync_start_references(node, scope, host, stack);
            }
            Expr::Unary(op, expr) => {
                if op == "typeof"
                    && let Expr::Name(name) = expr.as_ref()
                    && self.lookup_binding(scope, name).is_none()
                    && self.global_own_property(name).is_none()
                    && (self.global_deleted(name)
                        || (host.global(name).is_none() && intrinsic_global(name).is_none()))
                {
                    return self.string("undefined".into()).map(Step::Value);
                }
                stack.push(self, SyncFrame::Core(CoreFrame::Unary(op)))?;
                Step::Eval(expr)
            }
            Expr::Binary(op, left, right) => {
                stack.push(self, SyncFrame::Core(CoreFrame::BinaryLeft { op, right }))?;
                Step::Eval(left)
            }
            Expr::Conditional(condition, yes, no) => {
                stack.push(self, SyncFrame::Core(CoreFrame::Conditional { yes, no }))?;
                Step::Eval(condition)
            }
            Expr::ConditionalChain(chain) => {
                let index = chain.clauses_rev().len() - 1;
                stack.push(
                    self,
                    SyncFrame::Core(CoreFrame::ConditionalChain { chain, index }),
                )?;
                Step::Eval(&chain.clauses_rev()[index].0)
            }
            Expr::Sequence(sequence) => {
                for _ in 0..sequence.removed_binary_nodes() {
                    self.tick()?;
                }
                let items = sequence.items();
                stack.push(
                    self,
                    SyncFrame::Core(CoreFrame::Sequence {
                        items,
                        index: 0,
                        previous: Value::Undefined,
                    }),
                )?;
                Step::Eval(&items[0])
            }
        })
    }

    #[inline(never)]
    fn sync_apply_core<'ast>(
        &mut self,
        frame: CoreFrame<'ast>,
        value: Value,
        host: &mut impl Host,
        stack: &mut SyncStack<'ast>,
    ) -> Result<Step<'ast>, JsError> {
        Ok(match frame {
            CoreFrame::Unary(op) => Step::Value(self.unary_value(op, value, host)?),
            CoreFrame::BinaryLeft { op, right } => {
                if op == "&&" && !value.truthy()
                    || op == "||" && value.truthy()
                    || op == "??" && !matches!(value, Value::Undefined | Value::Null)
                {
                    Step::Value(value)
                } else {
                    stack.push(
                        self,
                        SyncFrame::Core(CoreFrame::BinaryRight { op, left: value }),
                    )?;
                    Step::Eval(right)
                }
            }
            CoreFrame::BinaryRight { op, left } => {
                Step::Value(self.binary_value(op, left, value, host)?)
            }
            CoreFrame::Conditional { yes, no } => Step::Eval(if value.truthy() { yes } else { no }),
            CoreFrame::ConditionalChain { chain, index } => {
                if value.truthy() {
                    Step::Eval(&chain.clauses_rev()[index].1)
                } else if index == 0 {
                    Step::Eval(chain.fallback())
                } else {
                    self.tick()?;
                    stack.push(
                        self,
                        SyncFrame::Core(CoreFrame::ConditionalChain {
                            chain,
                            index: index - 1,
                        }),
                    )?;
                    Step::Eval(&chain.clauses_rev()[index - 1].0)
                }
            }
            CoreFrame::Sequence {
                items,
                index,
                previous,
            } => {
                drop(previous);
                if index + 1 == items.len() {
                    Step::Value(value)
                } else {
                    stack.push(
                        self,
                        SyncFrame::Core(CoreFrame::Sequence {
                            items,
                            index: index + 1,
                            previous: value,
                        }),
                    )?;
                    Step::Eval(&items[index + 1])
                }
            }
        })
    }

    pub(crate) fn sync_error_context(&self, error: &mut JsError, operation: &'static str) {
        error.add_diagnostic_context(&format!(
            "sync {operation} retained={} native={} call={} statement={} conversion={}",
            self.sync_execution.live_frames,
            self.eval_depth,
            self.call_depth,
            self.statement_depth,
            self.conversion_depth,
        ));
    }
}

fn expression_name(node: &Expr) -> &'static str {
    match node {
        Expr::Literal(_) => "literal",
        Expr::Name(_) => "name",
        Expr::RegExp(..) => "regexp",
        Expr::ImportMeta => "import.meta",
        Expr::ImportCall { .. } => "import",
        Expr::Function { .. } => "function",
        Expr::Class(_) => "class-bridge",
        Expr::Await(_) => "await",
        Expr::Yield(_) => "yield",
        Expr::YieldDelegate(_) => "yield-delegate",
        Expr::Spread(_) => "spread",
        Expr::Hole => "hole",
        Expr::Super => "super",
        Expr::Array(_) => "array",
        Expr::Object(_) => "object",
        Expr::Template(_) => "template",
        Expr::Member(..) => "member",
        Expr::PrivateMember(..) => "private-member",
        Expr::Assign(..) => "assign",
        Expr::Update(..) => "update",
        Expr::Call(..) => "call",
        Expr::Construct(..) => "construct",
        Expr::Unary(..) => "unary",
        Expr::Binary(..) => "binary",
        Expr::Conditional(..) => "conditional",
        Expr::ConditionalChain(_) => "conditional-chain",
        Expr::Sequence(_) => "sequence",
    }
}
