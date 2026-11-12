//! Borrowed, constant-width argument continuation shared by every call kind.
//!
//! The call provider owns the already captured callee/receiver and reserves its
//! continuation slot before starting or applying this cursor. No argument AST is
//! copied and no spread is first materialized as a second argument list.
use crate::{Expr, Host, JsError, Runtime, Value};

pub(crate) struct ArgumentsCursor<'ast> {
    arguments: &'ast [Expr],
    // The pending expression is arguments[next - 1]. The Spread wrapper itself
    // is not evaluated: only its operand has an expression-entry tick.
    next: usize,
    values: Vec<Value>,
}

pub(crate) enum ArgumentsStep<'ast> {
    Evaluate {
        expression: &'ast Expr,
        cursor: ArgumentsCursor<'ast>,
    },
    Complete(Vec<Value>),
}

impl Runtime {
    pub(crate) fn sync_start_arguments<'ast>(
        &mut self,
        arguments: &'ast [Expr],
    ) -> Result<ArgumentsStep<'ast>, JsError> {
        let values = self.argument_buffer(arguments)?;
        self.sync_next_argument(ArgumentsCursor {
            arguments,
            next: 0,
            values,
        })
    }

    pub(crate) fn sync_apply_argument<'ast>(
        &mut self,
        mut cursor: ArgumentsCursor<'ast>,
        value: Value,
        host: &mut impl Host,
    ) -> Result<ArgumentsStep<'ast>, JsError> {
        if matches!(cursor.arguments[cursor.next - 1], Expr::Spread(_)) {
            self.append_argument_spread(&mut cursor.values, value, host)?;
        } else {
            // The slot was reserved before this child's evaluation. Moving the
            // result into it must not consume another allocation reservation.
            cursor.values.push(value);
        }
        self.sync_next_argument(cursor)
    }

    fn sync_next_argument<'ast>(
        &mut self,
        mut cursor: ArgumentsCursor<'ast>,
    ) -> Result<ArgumentsStep<'ast>, JsError> {
        let Some(expression) = cursor.arguments.get(cursor.next) else {
            return Ok(ArgumentsStep::Complete(cursor.values));
        };
        let expression = if let Expr::Spread(operand) = expression {
            operand.as_ref()
        } else {
            // Parser-bypassing callers still stop before argument 65 executes.
            self.reserve_argument_slot(&mut cursor.values)?;
            expression
        };
        cursor.next += 1;
        Ok(ArgumentsStep::Evaluate { expression, cursor })
    }
}

#[cfg(test)]
#[path = "sync_argument_tests.rs"]
mod tests;
