//! Only immediate `else if` edges are flat. Clause bodies and fallback retain
//! their original scopes and independently guarded, recursive structure.
use super::*;

const MAX_CLAUSES: usize = 4096;
#[derive(Clone, Debug)]
pub struct IfChain(Arc<Data>);
#[derive(Debug)]
struct Data {
    clauses: Vec<(Expr, Stmt)>,
    fallback: Option<Stmt>,
}
impl IfChain {
    pub(crate) fn clauses(&self) -> &[(Expr, Stmt)] {
        &self.0.clauses
    }
    pub(crate) fn fallback(&self) -> Option<&Stmt> {
        self.0.fallback.as_ref()
    }
}
fn reserve(
    clauses: &mut Vec<(Expr, Stmt)>,
    additional: usize,
    bytes: &mut usize,
) -> Result<(), JsError> {
    flat_storage::reserve(
        clauses,
        additional,
        2,
        MAX_CLAUSES,
        bytes,
        "If-chain storage budget exceeded",
        "If-chain length budget exceeded",
        "If-chain storage unavailable",
    )
}
impl Parser {
    #[inline(never)]
    fn if_clause(&mut self) -> Result<(Expr, Stmt), JsError> {
        self.require("(")?;
        let condition = self.expression(0)?;
        self.require(")")?;
        let body = self.statement()?;
        Ok((condition, body))
    }
    #[inline(never)]
    pub(super) fn statement_if(&mut self) -> Result<Stmt, JsError> {
        let (condition, body) = self.if_clause()?;
        if !self.eat("else") {
            return Ok(Stmt::If(condition, Box::new(body), None));
        }
        if !self.is("if") || self.statement_starts_label() {
            let fallback = self.statement()?;
            return Ok(Stmt::If(
                condition,
                Box::new(body),
                Some(Box::new(fallback)),
            ));
        }
        // Label recognition precedes keywords in statement_inner. Leave even
        // invalid reserved labels (`else if:`) on that path for its diagnostic.
        // No extra native statement frame for immediate else-if edges. Count
        // each former statement entry before its grammar and retained storage.
        self.count()?;
        self.require("if")?;
        flat_storage::charge(
            &mut self.flat_storage_bytes,
            flat_storage::header_bytes::<Data>(),
            "If-chain storage budget exceeded",
        )?;
        let mut clauses = Vec::new();
        reserve(&mut clauses, 2, &mut self.flat_storage_bytes)?;
        clauses.push((condition, body));
        loop {
            clauses.push(self.if_clause()?);
            let fallback = if self.eat("else") {
                if self.is("if") && !self.statement_starts_label() {
                    self.count()?;
                    self.require("if")?;
                    reserve(&mut clauses, 1, &mut self.flat_storage_bytes)?;
                    continue;
                }
                Some(self.statement()?)
            } else {
                None
            };
            return Ok(Stmt::IfChain(IfChain(Arc::new(Data { clauses, fallback }))));
        }
    }
}

#[cfg(test)]
#[path = "parser_if_chain_storage_tests.rs"]
mod tests;
