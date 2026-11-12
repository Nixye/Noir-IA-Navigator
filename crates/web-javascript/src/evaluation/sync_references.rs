//! Borrowed reference phases. Resolving a target never evaluates that target as
//! a value: it evaluates only its base/key and retains the resulting Place once.
//! Getters, setters and key conversion remain guarded synchronous bridges. The
//! enclosing dispatcher keeps its active lease even while a frame is applied.
use crate::{
    Expr, Host, JsError, Place, Runtime, Value, intrinsic_global, prepared_call::PreparedCall,
};

use super::{
    storage::SyncStack, sync_calls::CallTarget, sync_frames::SyncFrame, sync_machine::Step,
};

pub(crate) enum ReferenceGoal<'ast> {
    Value,
    Assign { op: &'ast str, right: &'ast Expr },
    Update { increment: bool, prefix: bool },
    Delete,
    Call { arguments: &'ast [Expr] },
}

pub(crate) enum ReferenceFrame<'ast> {
    Base {
        key: &'ast Expr,
        goal: ReferenceGoal<'ast>,
    },
    Key {
        base: Value,
        goal: ReferenceGoal<'ast>,
    },
    PrivateBase {
        name: &'ast str,
        goal: ReferenceGoal<'ast>,
    },
    SuperKey {
        base: Value,
        receiver: Value,
        goal: ReferenceGoal<'ast>,
    },
    AssignRight {
        place: Place,
        previous: Option<Value>,
        op: &'ast str,
    },
    DestructureRight {
        target: &'ast Expr,
    },
    DeleteValue,
}

impl Runtime {
    pub(crate) fn sync_start_references<'ast>(
        &mut self,
        node: &'ast Expr,
        scope: usize,
        host: &mut impl Host,
        stack: &mut SyncStack<'ast>,
    ) -> Result<Step<'ast>, JsError> {
        match node {
            Expr::Member(..) | Expr::PrivateMember(..) => {
                self.sync_start_reference_target(node, ReferenceGoal::Value, scope, host, stack)
            }
            Expr::Assign(op, target, right) => {
                if op == "=" && matches!(target.as_ref(), Expr::Array(_) | Expr::Object(_)) {
                    stack.push(
                        self,
                        SyncFrame::References(ReferenceFrame::DestructureRight { target }),
                    )?;
                    return Ok(Step::Eval(right));
                }
                self.sync_start_reference_target(
                    target,
                    ReferenceGoal::Assign { op, right },
                    scope,
                    host,
                    stack,
                )
            }
            Expr::Update(target, increment, prefix) => self.sync_start_reference_target(
                target,
                ReferenceGoal::Update {
                    increment: *increment,
                    prefix: *prefix,
                },
                scope,
                host,
                stack,
            ),
            Expr::Unary(op, target) if op == "delete" => match target.as_ref() {
                // These two leaf cases deliberately do not evaluate a child or
                // debit an expression-entry tick. Keep binding/global behavior.
                Expr::Name(_) | Expr::PrivateMember(..) => {
                    self.delete_expression(target, scope, host).map(Step::Value)
                }
                Expr::Member(..) => self.sync_start_reference_target(
                    target,
                    ReferenceGoal::Delete,
                    scope,
                    host,
                    stack,
                ),
                _ => {
                    stack.push(self, SyncFrame::References(ReferenceFrame::DeleteValue))?;
                    Ok(Step::Eval(target))
                }
            },
            _ => Err(JsError::runtime("Invalid synchronous reference operation")),
        }
    }

    pub(crate) fn sync_start_reference_target<'ast>(
        &mut self,
        target: &'ast Expr,
        goal: ReferenceGoal<'ast>,
        scope: usize,
        host: &mut impl Host,
        stack: &mut SyncStack<'ast>,
    ) -> Result<Step<'ast>, JsError> {
        match target {
            Expr::Name(name) => {
                // A leaf lookup only. TDZ and module bindings remain in
                // read_place/write_place, not in reference capture.
                stack.ensure_slot(self)?;
                let place = self.sync_name_place(name, scope, host)?;
                self.sync_finish_place(place, goal, scope, host, stack)
            }
            Expr::Member(base, key) => {
                if matches!(base.as_ref(), Expr::Super) && !matches!(goal, ReferenceGoal::Delete) {
                    stack.ensure_slot(self)?;
                    // Preserve lookup of home and this before the key operand.
                    let base = self.super_base(scope, host)?;
                    let receiver = self.read(scope, "this", host)?;
                    stack.push(
                        self,
                        SyncFrame::References(ReferenceFrame::SuperKey {
                            base,
                            receiver,
                            goal,
                        }),
                    )?;
                    return Ok(Step::Eval(key));
                }
                stack.push(
                    self,
                    SyncFrame::References(ReferenceFrame::Base { key, goal }),
                )?;
                Ok(Step::Eval(base))
            }
            Expr::PrivateMember(base, name) => {
                stack.push(
                    self,
                    SyncFrame::References(ReferenceFrame::PrivateBase { name, goal }),
                )?;
                Ok(Step::Eval(base))
            }
            _ => Err(JsError::runtime("Invalid assignment target")),
        }
    }

    pub(crate) fn sync_apply_reference<'ast>(
        &mut self,
        frame: ReferenceFrame<'ast>,
        value: Value,
        scope: usize,
        host: &mut impl Host,
        stack: &mut SyncStack<'ast>,
    ) -> Result<Step<'ast>, JsError> {
        match frame {
            ReferenceFrame::Base { key, goal } => {
                stack.push(
                    self,
                    SyncFrame::References(ReferenceFrame::Key { base: value, goal }),
                )?;
                Ok(Step::Eval(key))
            }
            ReferenceFrame::Key { base, goal } => {
                stack.ensure_slot(self)?;
                match goal {
                    ReferenceGoal::Value => {
                        self.read_member_values(base, value, host).map(Step::Value)
                    }
                    ReferenceGoal::Call { arguments } => {
                        let target = self.prepare_member_call_values(base, value, host)?;
                        self.sync_begin_call_arguments(
                            CallTarget::Prepared(target),
                            arguments,
                            scope,
                            host,
                            stack,
                        )
                    }
                    goal => {
                        // Key expression has completed. The shared helper checks
                        // nullish before ToPropertyKey, with no duplicated get.
                        let place = self.ordinary_member_place_values(base, value, host)?;
                        self.sync_finish_place(place, goal, scope, host, stack)
                    }
                }
            }
            ReferenceFrame::PrivateBase { name, goal } => {
                stack.ensure_slot(self)?;
                let class = self.private_class(scope)?;
                match goal {
                    ReferenceGoal::Value => {
                        self.private_get(value, class, name, host).map(Step::Value)
                    }
                    ReferenceGoal::Call { arguments } => {
                        let callable = self.private_get(value.clone(), class, name, host)?;
                        let target = PreparedCall::Callable {
                            callable,
                            receiver: value,
                            context: None,
                        };
                        self.sync_begin_call_arguments(
                            CallTarget::Prepared(target),
                            arguments,
                            scope,
                            host,
                            stack,
                        )
                    }
                    goal => {
                        let name = self.sync_owned_metadata(name)?;
                        self.sync_finish_place(
                            Place::PrivateMember(value, class, name),
                            goal,
                            scope,
                            host,
                            stack,
                        )
                    }
                }
            }
            ReferenceFrame::SuperKey {
                base,
                receiver,
                goal,
            } => {
                stack.ensure_slot(self)?;
                match goal {
                    ReferenceGoal::Value => self
                        .super_get_values(base, receiver, value, host)
                        .map(Step::Value),
                    ReferenceGoal::Call { arguments } => {
                        let callable = self.super_get_values(base, receiver, value, host)?;
                        // Existing super-call semantics read this after the getter
                        // too. Do not silently change this into an earlier capture.
                        let receiver = self.read(scope, "this", host)?;
                        let target = PreparedCall::Callable {
                            callable,
                            receiver,
                            context: None,
                        };
                        self.sync_begin_call_arguments(
                            CallTarget::Prepared(target),
                            arguments,
                            scope,
                            host,
                            stack,
                        )
                    }
                    goal => {
                        let place = self.super_place_values(base, receiver, value, host)?;
                        self.sync_finish_place(place, goal, scope, host, stack)
                    }
                }
            }
            ReferenceFrame::AssignRight {
                place,
                previous,
                op,
            } => {
                let value = match previous {
                    Some(_) if matches!(op, "&&=" | "||=" | "??=") => value,
                    Some(left) => self.binary(&op[..op.len() - 1], left, value, host)?,
                    None => value,
                };
                self.write_place(place, value.clone(), host)?;
                Ok(Step::Value(value))
            }
            ReferenceFrame::DestructureRight { target } => {
                // Explicit pattern bridge: parser-bounded pattern traversal may
                // evaluate defaults/references and invoke iterators/accessors.
                // Those eval/call/conversion entries retain their native guards;
                // Resource skips IteratorClose in the existing pattern helper.
                self.assign_destructure(target, value.clone(), scope, host)?;
                Ok(Step::Value(value))
            }
            ReferenceFrame::DeleteValue => Ok(Step::Value(Value::Bool(true))),
        }
    }

    pub(crate) fn sync_name_place(
        &mut self,
        name: &str,
        scope: usize,
        host: &mut impl Host,
    ) -> Result<Place, JsError> {
        if let Some(id) = self.lookup_binding(scope, name) {
            return Ok(Place::Binding(id, self.sync_owned_metadata(name)?));
        }
        if let Some(id) = host.global_object() {
            if self.strict
                && self.global_own_property(name).is_none()
                && (self.global_deleted(name)
                    || (intrinsic_global(name).is_none() && !host.has_property(id, name)?))
            {
                return Err(JsError::runtime(format!(
                    "Assignment to undeclared identifier {name}"
                )));
            }
            return Ok(Place::Global(id, self.sync_owned_metadata(name)?));
        }
        Err(JsError::runtime(format!(
            "Assignment to undeclared identifier {name}"
        )))
    }

    fn sync_finish_place<'ast>(
        &mut self,
        place: Place,
        goal: ReferenceGoal<'ast>,
        scope: usize,
        host: &mut impl Host,
        stack: &mut SyncStack<'ast>,
    ) -> Result<Step<'ast>, JsError> {
        match goal {
            ReferenceGoal::Value => self.read_place(&place, host).map(Step::Value),
            ReferenceGoal::Assign { op, right } => {
                stack.ensure_slot(self)?;
                let previous = if op == "=" {
                    None
                } else {
                    Some(self.read_place(&place, host)?)
                };
                if let Some(previous) = &previous
                    && (op == "&&=" && !previous.truthy()
                        || op == "||=" && previous.truthy()
                        || op == "??=" && !matches!(previous, Value::Undefined | Value::Null))
                {
                    return Ok(Step::Value(previous.clone()));
                }
                stack.push(
                    self,
                    SyncFrame::References(ReferenceFrame::AssignRight {
                        place,
                        previous,
                        op,
                    }),
                )?;
                Ok(Step::Eval(right))
            }
            ReferenceGoal::Update { increment, prefix } => self
                .update_place(place, increment, prefix, host)
                .map(Step::Value),
            ReferenceGoal::Delete => self.delete_ordinary_place(place, host).map(Step::Value),
            ReferenceGoal::Call { arguments } => {
                let callable = self.read_place(&place, host)?;
                let target = PreparedCall::Callable {
                    callable,
                    receiver: Value::Undefined,
                    context: None,
                };
                self.sync_begin_call_arguments(
                    CallTarget::Prepared(target),
                    arguments,
                    scope,
                    host,
                    stack,
                )
            }
        }
    }
}

#[cfg(test)]
mod tests;
