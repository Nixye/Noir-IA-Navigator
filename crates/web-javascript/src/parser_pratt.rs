//! Bounded, nonrecursive Pratt continuations. Complex atoms still reenter the
//! native parser, retaining both its depth guard and the shared context balance.
use super::*;

pub(super) const MAX_EXPRESSION_FRAMES: usize = 64;

pub(super) enum Goal {
    Expression(u8),
    ForInitializer { for_await: bool },
}

pub(super) struct Parsed {
    pub(super) value: Expr,
    // Only the original min-10 boundary may identify an in/of binding.
    // A later `of` after a classical tail must remain a syntax error.
    pub(super) for_binding: bool,
}

struct Context {
    outer_min: u8,
    groups: usize,
    chain_length: usize,
    // Only changed resumed group tails need another structural walk.
    checked: bool,
    tail_start: usize,
}
impl Context {
    fn min(&self) -> u8 {
        if self.groups == 0 { self.outer_min } else { 0 }
    }
}

enum Prefix {
    Unary(String),
    Update(bool),
    Await,
    Yield(bool),
}

enum Continuation {
    Prefix(Context, Prefix),
    Index(Context, Expr),
    Call {
        context: Context,
        target: Expr,
        arguments: Vec<Expr>,
        spread: bool,
    },
    ConditionalYes(Context, Expr),
    ConditionalNo(Context, Expr, Expr),
    Binary(Context, Expr, String, u8),
}

enum State {
    Begin(u8),
    Atom(Context),
    Tail(Context, Expr),
    Complete(Context, Expr),
}

impl Parser {
    #[inline(never)]
    pub(super) fn expression_pratt(&mut self, goal: Goal) -> Result<Parsed, JsError> {
        let entry_frames = self.expression_frames;
        // All pending continuations and partial trees are owned by this call.
        // They have returned or actually dropped before its balance is restored.
        let result = self.expression_pratt_inner(goal);
        self.expression_frames = entry_frames;
        result
    }

    #[inline(never)]
    fn expression_pratt_inner(&mut self, goal: Goal) -> Result<Parsed, JsError> {
        let (min, mut for_head) = match goal {
            Goal::Expression(min) => (min, None),
            // A direct yield cannot be an in/of reference. Previously its
            // min-10 attempt consumed only `yield`, failed, then reparsed at
            // min 0. Choose that grammar before constructing any subtree.
            Goal::ForInitializer { for_await } if self.is("yield") => {
                if for_await {
                    self.index += 1;
                    return Err(self.error("For-await-of requires a binding followed by of"));
                }
                (0, None)
            }
            Goal::ForInitializer { for_await } => (10, Some(for_await)),
        };
        let mut pending = Vec::new();
        let mut state = State::Begin(min);
        loop {
            state = match state {
                State::Begin(min) => self.pratt_begin(&mut pending, min)?,
                // Leave prefix/continuation temporaries behind before a
                // complex atom can reenter this native expression dispatcher.
                State::Atom(context) => State::Tail(context, self.atom()?),
                State::Tail(context, left) => self.pratt_tail(&mut pending, context, left)?,
                State::Complete(mut context, value) => {
                    // Every logical child returns a checked tree. A local tail
                    // can add at most 63 links, just as before the trampoline.
                    // Never defer these checks until the whole machine returns.
                    if !context.checked || self.index != context.tail_start {
                        depth::validate_expression_depth(&value)?;
                    }
                    if context.groups != 0 {
                        self.require(")")?;
                        context.groups -= 1;
                        context.chain_length = 0;
                        context.checked = true;
                        context.tail_start = self.index;
                        State::Tail(context, value)
                    } else if let Some(continuation) = pending.pop() {
                        self.expression_frames -= 1;
                        self.pratt_resume(&mut pending, continuation, value)?
                    } else if let Some(for_await) = for_head.take() {
                        if self.is("in") || self.is("of") {
                            return Ok(Parsed {
                                value,
                                for_binding: true,
                            });
                        }
                        if for_await {
                            return Err(
                                self.error("For-await-of requires a binding followed by of")
                            );
                        }
                        // Keep the AST, groups and local chain count. The
                        // min-10 terminal attempt already charged this tail
                        // iteration; continuing at min 0 must not charge it
                        // twice or reset the 63-link limit. Unchanged trees
                        // also reuse the structural check immediately above.
                        context.outer_min = 0;
                        context.checked = true;
                        context.tail_start = self.index;
                        self.pratt_tail_step(&mut pending, context, value)?
                    } else {
                        return Ok(Parsed {
                            value,
                            for_binding: false,
                        });
                    }
                }
            };
        }
    }

    fn pratt_child(
        &mut self,
        pending: &mut Vec<Continuation>,
        continuation: Continuation,
        min: u8,
    ) -> Result<State, JsError> {
        // Match the former expression() entry, including failed-visit charges.
        // The aggregate guard is checked before reserving a continuation slot.
        self.count()?;
        if self.expression_frames >= MAX_EXPRESSION_FRAMES {
            return Err(JsError::resource("Expression nesting budget exceeded"));
        }
        pending
            .try_reserve(1)
            .map_err(|_| JsError::resource("Expression continuation storage unavailable"))?;
        self.expression_frames += 1;
        pending.push(continuation);
        Ok(State::Begin(min))
    }

    #[inline(never)]
    fn pratt_begin(&mut self, pending: &mut Vec<Continuation>, min: u8) -> Result<State, JsError> {
        let context = Context {
            outer_min: min,
            groups: self.expression_groups()?,
            chain_length: 0,
            checked: false,
            tail_start: self.index,
        };
        if self.eat("await") {
            if !matches!(
                self.function_kind,
                FunctionKind::Async | FunctionKind::AsyncGenerator
            ) {
                return Err(self.error("await is only supported in async function bodies"));
            }
            return self.pratt_child(pending, Continuation::Prefix(context, Prefix::Await), 14);
        }
        if self.eat("yield") {
            if !matches!(
                self.function_kind,
                FunctionKind::Generator | FunctionKind::AsyncGenerator
            ) || context.min() > 1
            {
                return Err(
                    self.error("yield is only supported as an assignment expression in generators")
                );
            }
            if self.token().newline
                || self.is(";")
                || self.is("}")
                || self.is(")")
                || self.is("]")
                || self.is(",")
                || self.is(":")
                || matches!(self.token().kind, Kind::End)
            {
                // Bare yield skips this tail, but not a tail after a group.
                return Ok(State::Complete(context, Expr::Yield(None)));
            }
            let delegate = self.eat("*");
            return self.pratt_child(
                pending,
                Continuation::Prefix(context, Prefix::Yield(delegate)),
                1,
            );
        }
        if self.is("!")
            || self.is("-")
            || self.is("+")
            || self.is("~")
            || self.is("typeof")
            || self.is("void")
            || self.is("delete")
        {
            let op = match &self.token().kind {
                Kind::Name(value) => value.clone(),
                Kind::Symbol(value) => (*value).to_owned(),
                _ => unreachable!(),
            };
            self.index += 1;
            return self.pratt_child(
                pending,
                Continuation::Prefix(context, Prefix::Unary(op)),
                14,
            );
        }
        for (token, increment) in [("++", true), ("--", false)] {
            if self.eat(token) {
                return self.pratt_child(
                    pending,
                    Continuation::Prefix(context, Prefix::Update(increment)),
                    14,
                );
            }
        }
        Ok(State::Atom(context))
    }

    #[inline(never)]
    fn pratt_tail(
        &mut self,
        pending: &mut Vec<Continuation>,
        mut context: Context,
        left: Expr,
    ) -> Result<State, JsError> {
        context.chain_length += 1;
        if context.chain_length > 64 {
            return Err(JsError::resource("Expression chain budget exceeded"));
        }
        self.pratt_tail_step(pending, context, left)
    }

    fn pratt_tail_step(
        &mut self,
        pending: &mut Vec<Continuation>,
        context: Context,
        left: Expr,
    ) -> Result<State, JsError> {
        let min = context.min();
        if min <= 16 && self.eat(".") {
            Ok(State::Tail(context, self.expression_dot(left)?))
        } else if min <= 16 && self.eat("[") {
            self.pratt_child(pending, Continuation::Index(context, left), 0)
        } else if min <= 16 && self.eat("(") {
            if self.eat(")") {
                Ok(State::Tail(context, Expr::Call(Box::new(left), Vec::new())))
            } else {
                self.pratt_argument(pending, context, left, Vec::new())
            }
        } else if min <= 15 && !self.token().newline && (self.is("++") || self.is("--")) {
            Ok(State::Tail(context, self.expression_postfix(left)?))
        } else if min <= 2 && self.eat("?") {
            self.pratt_child(pending, Continuation::ConditionalYes(context, left), 1)
        } else if let Some((op, precedence)) = self.expression_operator(min) {
            self.index += 1;
            let right_min = if precedence == 1 || op == "**" {
                precedence
            } else {
                precedence + 1
            };
            self.pratt_child(
                pending,
                Continuation::Binary(context, left, op, precedence),
                right_min,
            )
        } else {
            Ok(State::Complete(context, left))
        }
    }

    fn pratt_argument(
        &mut self,
        pending: &mut Vec<Continuation>,
        context: Context,
        target: Expr,
        arguments: Vec<Expr>,
    ) -> Result<State, JsError> {
        crate::call_storage::check_arguments(arguments.len().saturating_add(1))?;
        let spread = self.eat("...");
        self.pratt_child(
            pending,
            Continuation::Call {
                context,
                target,
                arguments,
                spread,
            },
            1,
        )
    }

    #[inline(never)]
    fn pratt_resume(
        &mut self,
        pending: &mut Vec<Continuation>,
        continuation: Continuation,
        value: Expr,
    ) -> Result<State, JsError> {
        match continuation {
            Continuation::Prefix(context, prefix) => {
                let value = match prefix {
                    Prefix::Unary(op) => Expr::Unary(op, Box::new(value)),
                    Prefix::Update(increment) => {
                        if !assignment_target(&value, false) {
                            return Err(self.error("Invalid update target"));
                        }
                        Expr::Update(Box::new(value), increment, true)
                    }
                    Prefix::Await => Expr::Await(Box::new(value)),
                    Prefix::Yield(true) => Expr::YieldDelegate(Box::new(value)),
                    Prefix::Yield(false) => Expr::Yield(Some(Box::new(value))),
                };
                Ok(State::Tail(context, value))
            }
            Continuation::Index(context, left) => {
                self.require("]")?;
                Ok(State::Tail(
                    context,
                    Expr::Member(Box::new(left), Box::new(value)),
                ))
            }
            Continuation::Call {
                context,
                target,
                mut arguments,
                spread,
            } => {
                arguments.push(if spread {
                    Expr::Spread(Box::new(value))
                } else {
                    value
                });
                if self.eat(")") {
                    Ok(State::Tail(
                        context,
                        Expr::Call(Box::new(target), arguments),
                    ))
                } else {
                    self.require(",")?;
                    self.pratt_argument(pending, context, target, arguments)
                }
            }
            Continuation::ConditionalYes(context, condition) => {
                self.require(":")?;
                self.pratt_child(
                    pending,
                    Continuation::ConditionalNo(context, condition, value),
                    1,
                )
            }
            Continuation::ConditionalNo(context, condition, yes) => Ok(State::Tail(
                context,
                self.expression_conditional(condition, yes, value)?,
            )),
            Continuation::Binary(context, left, op, precedence) => Ok(State::Tail(
                context,
                self.expression_binary(left, op, precedence, value)?,
            )),
        }
    }
}

#[cfg(test)]
mod storage_tests {
    use super::*;
    use std::mem::{MaybeUninit, size_of};

    #[test]
    fn retained_continuation_capacity_has_a_separate_bounded_high_water_mark() {
        let mut total_slots = 0;
        for native_dispatchers in 1..=MAX_EXPRESSION_FRAMES {
            // Every live native dispatcher owns one root context. Its vector
            // can retain a former peak after children return, but could never
            // have held more than the remaining shared logical slots.
            let maximum_pending = MAX_EXPRESSION_FRAMES - native_dispatchers;
            let bound = if maximum_pending == 0 {
                0
            } else {
                maximum_pending.next_power_of_two().max(4)
            };
            // Same layout, allocation and growth as Vec<Continuation>, without
            // manufacturing AST values or relying on recursive Clone/Drop.
            let mut pending: Vec<MaybeUninit<Continuation>> = Vec::new();
            for _ in 0..maximum_pending {
                pending.try_reserve(1).unwrap();
                pending.push(MaybeUninit::uninit());
            }
            assert!(pending.capacity() <= bound);
            pending.clear();
            assert!(pending.capacity() <= bound);
            total_slots += bound;
        }
        assert_eq!(total_slots, 31 * 64 + 16 * 32 + 8 * 16 + 4 * 8 + 4 * 4);
        assert_eq!(total_slots, 2672);
        println!(
            "Context={} Continuation={} State={} retained_slots={} payload_bytes={} Vec_headers={}",
            size_of::<Context>(),
            size_of::<Continuation>(),
            size_of::<State>(),
            total_slots,
            total_slots * size_of::<Continuation>(),
            MAX_EXPRESSION_FRAMES * size_of::<Vec<Continuation>>(),
        );
    }
}
