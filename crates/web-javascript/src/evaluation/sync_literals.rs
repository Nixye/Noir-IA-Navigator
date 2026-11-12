//! Borrowed literal cursors. Each literal suspends in at most one frame,
//! regardless of its width; partial buffers move rather than being copied.
//!
//! Iterator steps, property copies and ToString are explicit native bridges.
//! They retain their existing call/conversion guards and may reenter evaluation.
//! The synchronous machine's active-execution barrier protects local Values
//! through these bridges and through frame teardown, including Resource errors.
//!
//! ArrayHoles keeps the existing bounded bitset: at most 128 u64 words, and
//! every represented hole first consumes an array Value slot. Even a singleton
//! word is smaller than that reserved slot, so this auxiliary storage remains
//! bounded by the existing cumulative slot ledger without charging it twice.
use super::{storage::SyncStack, sync_frames::SyncFrame, sync_machine::Step};
use crate::{ArrayHoles, Expr, Host, JsError, Runtime, Value};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) enum LiteralFrame<'ast> {
    Array {
        items: &'ast [Expr],
        next: usize,
        values: Vec<Value>,
        holes: ArrayHoles,
    },
    Object {
        pairs: &'ast [(String, Expr)],
        next: usize,
        object: Value,
    },
    Template {
        parts: &'ast [Expr],
        next: usize,
        units: Vec<u16>,
    },
}

impl Runtime {
    pub(crate) fn sync_start_literal<'ast>(
        &mut self,
        node: &'ast Expr,
        _scope: usize,
        _host: &mut impl Host,
        stack: &mut SyncStack<'ast>,
    ) -> Result<Step<'ast>, JsError> {
        let frame = match node {
            Expr::Array(items) => {
                if items.len() > 4096 {
                    return Err(JsError::resource("Array limit exceeded"));
                }
                LiteralFrame::Array {
                    items,
                    next: 0,
                    values: Vec::new(),
                    holes: ArrayHoles::new(),
                }
            }
            Expr::Object(pairs) => {
                Self::check_named_property_count(pairs.len())?;
                if !pairs.is_empty() {
                    // Reserve the continuation before allocating the target.
                    stack.ensure_slot(self)?;
                }
                let object = self.ordinary_object(BTreeMap::new())?;
                LiteralFrame::Object {
                    pairs,
                    next: 0,
                    object,
                }
            }
            Expr::Template(parts) => LiteralFrame::Template {
                parts,
                next: 0,
                units: Vec::new(),
            },
            _ => unreachable!("Non-literal sent to literal provider"),
        };
        self.sync_next_literal(frame, stack)
    }

    pub(crate) fn sync_apply_literal<'ast>(
        &mut self,
        frame: LiteralFrame<'ast>,
        value: Value,
        _scope: usize,
        host: &mut impl Host,
        stack: &mut SyncStack<'ast>,
    ) -> Result<Step<'ast>, JsError> {
        let frame = match frame {
            LiteralFrame::Array {
                items,
                next,
                mut values,
                mut holes,
            } => {
                if next < items.len() {
                    stack.ensure_slot(self)?;
                }
                if matches!(items[next - 1], Expr::Spread(_)) {
                    self.literal_array_spread(&mut values, value, host)?;
                } else {
                    self.literal_array_append(&mut values, &mut holes, value, false)?;
                }
                LiteralFrame::Array {
                    items,
                    next,
                    values,
                    holes,
                }
            }
            LiteralFrame::Object {
                pairs,
                next,
                object,
            } => {
                if next < pairs.len() {
                    stack.ensure_slot(self)?;
                }
                let (name, expression) = &pairs[next - 1];
                if matches!(expression, Expr::Spread(_)) {
                    self.copy_properties(object.clone(), value, &BTreeSet::new(), host)?;
                } else {
                    self.literal_object_property(
                        &object,
                        name,
                        value,
                        matches!(expression, Expr::Function { method: true, .. }),
                        host,
                    )?;
                }
                LiteralFrame::Object {
                    pairs,
                    next,
                    object,
                }
            }
            LiteralFrame::Template {
                parts,
                next,
                mut units,
            } => {
                if next < parts.len() {
                    stack.ensure_slot(self)?;
                }
                // ToString and UTF-16 assembly finish before the next child.
                self.literal_template_append(&mut units, value, host)?;
                LiteralFrame::Template { parts, next, units }
            }
        };
        self.sync_next_literal(frame, stack)
    }

    fn sync_next_literal<'ast>(
        &mut self,
        frame: LiteralFrame<'ast>,
        stack: &mut SyncStack<'ast>,
    ) -> Result<Step<'ast>, JsError> {
        let (expression, frame) = match frame {
            LiteralFrame::Array {
                items,
                mut next,
                mut values,
                mut holes,
            } => {
                // Holes have only the existing append tick, not an eval tick.
                while matches!(items.get(next), Some(Expr::Hole)) {
                    self.literal_array_append(&mut values, &mut holes, Value::Undefined, true)?;
                    next += 1;
                }
                let Some(expression) = items.get(next) else {
                    return self.literal_array_finish(values, holes).map(Step::Value);
                };
                stack.ensure_slot(self)?;
                let expression = if let Expr::Spread(operand) = expression {
                    operand.as_ref()
                } else {
                    expression
                };
                (
                    expression,
                    LiteralFrame::Array {
                        items,
                        next: next + 1,
                        values,
                        holes,
                    },
                )
            }
            LiteralFrame::Object {
                pairs,
                next,
                object,
            } => {
                let Some((_, expression)) = pairs.get(next) else {
                    return Ok(Step::Value(object));
                };
                stack.ensure_slot(self)?;
                let expression = if let Expr::Spread(operand) = expression {
                    operand.as_ref()
                } else {
                    expression
                };
                (
                    expression,
                    LiteralFrame::Object {
                        pairs,
                        next: next + 1,
                        object,
                    },
                )
            }
            LiteralFrame::Template { parts, next, units } => {
                let Some(expression) = parts.get(next) else {
                    return self.literal_template_finish(units).map(Step::Value);
                };
                stack.ensure_slot(self)?;
                (
                    expression,
                    LiteralFrame::Template {
                        parts,
                        next: next + 1,
                        units,
                    },
                )
            }
        };
        stack.push(self, SyncFrame::Literals(frame))?;
        Ok(Step::Eval(expression))
    }
}

#[cfg(test)]
#[path = "sync_literal_tests.rs"]
mod tests;
