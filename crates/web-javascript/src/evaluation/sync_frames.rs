//! Borrowed continuations. One parent expression owns at most one live frame.
//! Providers replace their phase rather than stacking a second parent frame.
use super::{sync_calls::CallFrame, sync_literals::LiteralFrame, sync_references::ReferenceFrame};
use crate::{Expr, Value, parser::ConditionalChain};

pub(crate) enum SyncFrame<'ast> {
    Core(CoreFrame<'ast>),
    References(ReferenceFrame<'ast>),
    Calls(CallFrame<'ast>),
    Literals(LiteralFrame<'ast>),
}

pub(crate) enum CoreFrame<'ast> {
    Unary(&'ast str),
    BinaryLeft {
        op: &'ast str,
        right: &'ast Expr,
    },
    BinaryRight {
        op: &'ast str,
        left: Value,
    },
    Conditional {
        yes: &'ast Expr,
        no: &'ast Expr,
    },
    ConditionalChain {
        chain: &'ast ConditionalChain,
        index: usize,
    },
    Sequence {
        items: &'ast [Expr],
        index: usize,
        // Keep the preceding value alive while the next expression runs.
        previous: Value,
    },
}
