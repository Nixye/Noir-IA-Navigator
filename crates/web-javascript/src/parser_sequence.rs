//! Only left comma spines become flat. Children and cached physical height are
//! immutable after construction; readers cannot obtain a mutable child or Arc.
use super::*;

const MAX_ITEMS: usize = 4096;
// All flat AST families share one cumulative flat-storage ledger.
#[cfg(test)]
const MAX_BYTES: usize = flat_storage::MAX_BYTES;

#[derive(Clone, Debug)]
pub struct Sequence(Arc<Data>);
#[derive(Debug)]
struct Data {
    items: Vec<Expr>,
    height: usize,
}
impl Sequence {
    pub(crate) fn items(&self) -> &[Expr] {
        &self.0.items
    }
    pub(super) fn height(&self) -> usize {
        self.0.height
    }
    pub(crate) fn removed_binary_nodes(&self) -> usize {
        self.0.items.len() - 2
    }
}

fn header_bytes() -> usize {
    flat_storage::header_bytes::<Data>()
}
fn charge(bytes: &mut usize, amount: usize) -> Result<(), JsError> {
    flat_storage::charge(bytes, amount, "Sequence storage budget exceeded")
}
fn reserve(items: &mut Vec<Expr>, additional: usize, bytes: &mut usize) -> Result<(), JsError> {
    flat_storage::reserve(
        items,
        additional,
        3,
        MAX_ITEMS,
        bytes,
        "Sequence storage budget exceeded",
        "Sequence length budget exceeded",
        "Sequence storage unavailable",
    )
}
fn joined_height(children: impl IntoIterator<Item = usize>) -> Result<usize, JsError> {
    let height = 1 + children.into_iter().max().unwrap_or(0);
    if height > 64 {
        return Err(JsError::resource("Expression tree nesting budget exceeded"));
    }
    Ok(height)
}
fn comma(left: Expr, right: Expr, bytes: &mut usize) -> Result<Expr, JsError> {
    match left {
        Expr::Sequence(mut sequence) => {
            // Parsing moves owned expressions; it never shares a prefix before
            // extending it. Refuse an unexpected alias instead of hidden COW.
            let data = Arc::get_mut(&mut sequence.0)
                .ok_or_else(|| JsError::resource("Shared sequence cannot be extended"))?;
            let height = joined_height([data.height - 1, depth::expression_height(&right)?])?;
            reserve(&mut data.items, 1, bytes)?;
            data.items.push(right);
            data.height = height;
            Ok(Expr::Sequence(sequence))
        }
        Expr::Binary(op, first, second) if op == "," => {
            // Release the old operand Boxes before reserving their flat slots.
            let (first, second) = {
                let old_first = first;
                let old_second = second;
                (*old_first, *old_second)
            };
            let height = joined_height([
                depth::expression_height(&first)?,
                depth::expression_height(&second)?,
                depth::expression_height(&right)?,
            ])?;
            charge(bytes, header_bytes())?;
            let mut items = Vec::new();
            reserve(&mut items, 3, bytes)?;
            items.extend([first, second, right]);
            Ok(Expr::Sequence(Sequence(Arc::new(Data { items, height }))))
        }
        left => Ok(Expr::Binary(",".into(), Box::new(left), Box::new(right))),
    }
}
impl Parser {
    pub(super) fn expression_sequence(&mut self, left: Expr, right: Expr) -> Result<Expr, JsError> {
        comma(left, right, &mut self.flat_storage_bytes)
    }
}

#[cfg(test)]
#[path = "parser_sequence_storage_tests.rs"]
mod tests;
