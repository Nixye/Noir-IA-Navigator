//! Only contiguous Conditional.no edges become flat. Reversed storage makes
//! Pratt's inside-out construction amortized linear, without copying suffixes.
use super::*;

// A local representation bound, not permission to bypass Pratt's 64 frames.
const MAX_CLAUSES: usize = 64;
#[derive(Clone, Debug)]
pub struct ConditionalChain(Arc<Data>);
#[derive(Debug)]
struct Data {
    clauses_rev: Vec<(Expr, Expr)>,
    fallback: Expr,
    height: usize,
}
impl ConditionalChain {
    pub(crate) fn clauses_rev(&self) -> &[(Expr, Expr)] {
        &self.0.clauses_rev
    }
    pub(crate) fn fallback(&self) -> &Expr {
        &self.0.fallback
    }
    pub(super) fn height(&self) -> usize {
        self.0.height
    }
}
fn reserve(
    clauses: &mut Vec<(Expr, Expr)>,
    additional: usize,
    bytes: &mut usize,
) -> Result<(), JsError> {
    flat_storage::reserve(
        clauses,
        additional,
        2,
        MAX_CLAUSES,
        bytes,
        "Conditional-chain storage budget exceeded",
        "Conditional-chain length budget exceeded",
        "Conditional-chain storage unavailable",
    )
}
fn joined_height(children: impl IntoIterator<Item = usize>) -> Result<usize, JsError> {
    let height = 1 + children.into_iter().max().unwrap_or(0);
    if height > 64 {
        return Err(JsError::resource("Expression tree nesting budget exceeded"));
    }
    Ok(height)
}
fn conditional(condition: Expr, yes: Expr, no: Expr, bytes: &mut usize) -> Result<Expr, JsError> {
    match no {
        Expr::ConditionalChain(mut chain) => {
            let data = Arc::get_mut(&mut chain.0)
                .ok_or_else(|| JsError::resource("Shared conditional chain cannot be extended"))?;
            let height = joined_height([
                data.height - 1,
                depth::expression_height(&condition)?,
                depth::expression_height(&yes)?,
            ])?;
            reserve(&mut data.clauses_rev, 1, bytes)?;
            data.clauses_rev.push((condition, yes));
            data.height = height;
            Ok(Expr::ConditionalChain(chain))
        }
        Expr::Conditional(inner_condition, inner_yes, fallback) => {
            // Release all three removed Boxes before reserving the flat slots.
            let (inner_condition, inner_yes, fallback) = (*inner_condition, *inner_yes, *fallback);
            let height = joined_height([
                depth::expression_height(&condition)?,
                depth::expression_height(&yes)?,
                depth::expression_height(&inner_condition)?,
                depth::expression_height(&inner_yes)?,
                depth::expression_height(&fallback)?,
            ])?;
            flat_storage::charge(
                bytes,
                flat_storage::header_bytes::<Data>(),
                "Conditional-chain storage budget exceeded",
            )?;
            let mut clauses_rev = Vec::new();
            reserve(&mut clauses_rev, 2, bytes)?;
            clauses_rev.extend([(inner_condition, inner_yes), (condition, yes)]);
            Ok(Expr::ConditionalChain(ConditionalChain(Arc::new(Data {
                clauses_rev,
                fallback,
                height,
            }))))
        }
        no => Ok(Expr::Conditional(
            Box::new(condition),
            Box::new(yes),
            Box::new(no),
        )),
    }
}
impl Parser {
    pub(super) fn expression_conditional(
        &mut self,
        condition: Expr,
        yes: Expr,
        no: Expr,
    ) -> Result<Expr, JsError> {
        // Pratt has already charged every grammar entry. Storage construction
        // neither reparses nor adds visits to the cumulative work ledger.
        conditional(condition, yes, no, &mut self.flat_storage_bytes)
    }
}

#[cfg(test)]
#[path = "parser_conditional_chain_storage_tests.rs"]
mod tests;
