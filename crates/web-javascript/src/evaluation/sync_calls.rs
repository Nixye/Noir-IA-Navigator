//! Callee/receiver capture precedes argument storage and evaluation. Calls keep
//! their special semantics through the final invocation, never a generic eval
//! fallback. No borrowed expression is stored in a suspended/owned machine.
use crate::{Expr, Host, JsError, Runtime, Value, callables, prepared_call::PreparedCall};

use super::{
    storage::SyncStack,
    sync_arguments::{ArgumentsCursor, ArgumentsStep},
    sync_frames::SyncFrame,
    sync_machine::Step,
    sync_references::ReferenceGoal,
};

pub(crate) enum CallTarget {
    Prepared(PreparedCall),
    DirectEval(Value),
    SuperConstructor,
    Construct(Value),
}

pub(crate) enum CallFrame<'ast> {
    Callee {
        expression: &'ast Expr,
        arguments: &'ast [Expr],
        construct: bool,
    },
    Arguments {
        target: CallTarget,
        cursor: ArgumentsCursor<'ast>,
    },
    ImportSpecifier {
        referrer: String,
        options: Option<&'ast Expr>,
    },
    ImportOptions {
        referrer: String,
        specifier: Value,
    },
}

impl Runtime {
    pub(crate) fn sync_start_call<'ast>(
        &mut self,
        node: &'ast Expr,
        scope: usize,
        host: &mut impl Host,
        stack: &mut SyncStack<'ast>,
    ) -> Result<Step<'ast>, JsError> {
        match node {
            Expr::Call(callee, arguments) => match callee.as_ref() {
                Expr::Super => self.sync_begin_call_arguments(
                    CallTarget::SuperConstructor,
                    arguments,
                    scope,
                    host,
                    stack,
                ),
                Expr::Member(..) | Expr::PrivateMember(..) => self.sync_start_reference_target(
                    callee,
                    ReferenceGoal::Call { arguments },
                    scope,
                    host,
                    stack,
                ),
                _ => {
                    stack.push(
                        self,
                        SyncFrame::Calls(CallFrame::Callee {
                            expression: callee,
                            arguments,
                            construct: false,
                        }),
                    )?;
                    Ok(Step::Eval(callee))
                }
            },
            Expr::Construct(callee, arguments) => {
                // Unlike an ordinary member call, new gets the callee's value
                // and charges its expression entry, including Member itself.
                stack.push(
                    self,
                    SyncFrame::Calls(CallFrame::Callee {
                        expression: callee,
                        arguments,
                        construct: true,
                    }),
                )?;
                Ok(Step::Eval(callee))
            }
            Expr::ImportCall { specifier, options } => {
                stack.ensure_slot(self)?;
                let referrer = self.prepare_import_referrer(scope)?;
                stack.push(
                    self,
                    SyncFrame::Calls(CallFrame::ImportSpecifier {
                        referrer,
                        options: options.as_deref(),
                    }),
                )?;
                Ok(Step::Eval(specifier))
            }
            _ => Err(JsError::runtime("Invalid synchronous call operation")),
        }
    }

    pub(crate) fn sync_apply_call<'ast>(
        &mut self,
        frame: CallFrame<'ast>,
        value: Value,
        scope: usize,
        host: &mut impl Host,
        stack: &mut SyncStack<'ast>,
    ) -> Result<Step<'ast>, JsError> {
        match frame {
            CallFrame::Callee {
                expression,
                arguments,
                construct,
            } => {
                // Pop retains the Vec capacity. Reconfirm the available slot
                // before allocating retained context metadata for this phase.
                stack.ensure_slot(self)?;
                let target = if construct {
                    CallTarget::Construct(value)
                } else if matches!(expression, Expr::Name(name) if name == "eval") {
                    CallTarget::DirectEval(value)
                } else {
                    CallTarget::Prepared(self.prepare_value_call(value, expression)?)
                };
                self.sync_begin_call_arguments(target, arguments, scope, host, stack)
            }
            CallFrame::Arguments { target, cursor } => {
                stack.ensure_slot(self)?;
                let next = self.sync_apply_argument(cursor, value, host)?;
                self.sync_advance_call_arguments(target, next, scope, host, stack)
            }
            CallFrame::ImportSpecifier { referrer, options } => {
                if let Some(options) = options {
                    stack.push(
                        self,
                        SyncFrame::Calls(CallFrame::ImportOptions {
                            referrer,
                            specifier: value,
                        }),
                    )?;
                    Ok(Step::Eval(options))
                } else {
                    self.import_call_values(referrer, value, Value::Undefined, host)
                        .map(Step::Value)
                }
            }
            CallFrame::ImportOptions {
                referrer,
                specifier,
            } => self
                .import_call_values(referrer, specifier, value, host)
                .map(Step::Value),
        }
    }

    pub(crate) fn sync_begin_call_arguments<'ast>(
        &mut self,
        target: CallTarget,
        arguments: &'ast [Expr],
        scope: usize,
        host: &mut impl Host,
        stack: &mut SyncStack<'ast>,
    ) -> Result<Step<'ast>, JsError> {
        stack.ensure_slot(self)?;
        let next = self.sync_start_arguments(arguments)?;
        self.sync_advance_call_arguments(target, next, scope, host, stack)
    }

    fn sync_advance_call_arguments<'ast>(
        &mut self,
        target: CallTarget,
        next: ArgumentsStep<'ast>,
        scope: usize,
        host: &mut impl Host,
        stack: &mut SyncStack<'ast>,
    ) -> Result<Step<'ast>, JsError> {
        match next {
            ArgumentsStep::Evaluate { expression, cursor } => {
                stack.push(
                    self,
                    SyncFrame::Calls(CallFrame::Arguments { target, cursor }),
                )?;
                Ok(Step::Eval(expression))
            }
            ArgumentsStep::Complete(arguments) => {
                // Explicit invocation bridges: call/construct enforce call and
                // constructor guards; direct eval enforces its dynamic guard.
                // Bodies/defaults/fields can enter another synchronous dispatcher
                // under the same active lease and aggregate budgets. Host named
                // dispatch remains the existing LegacyMethod path. None of these
                // paths validate callability before all arguments have completed.
                let value = match target {
                    CallTarget::Prepared(target) => {
                        self.invoke_prepared_call(target, arguments, host)
                    }
                    CallTarget::DirectEval(Value::Native("eval")) => {
                        self.dynamic_eval(&arguments, scope, true, host)
                    }
                    CallTarget::DirectEval(callable) => self
                        .call(callable, arguments, Value::Undefined, host)
                        .map_err(|error| callables::error_context(error, "eval")),
                    CallTarget::SuperConstructor => self.super_construct(arguments, scope, host),
                    CallTarget::Construct(callable) => self.construct(callable, arguments, host),
                }?;
                Ok(Step::Value(value))
            }
        }
    }
}

#[cfg(test)]
mod tests;
