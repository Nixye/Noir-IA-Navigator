//! Nonrecursive expression builders shared by the Pratt continuation machine.
//! Keep AST construction separate from scheduling child expressions.
use super::*;

impl Parser {
    #[inline(never)]
    pub(super) fn expression_dot(&mut self, left: Expr) -> Result<Expr, JsError> {
        if self.eat("#") {
            return Ok(Expr::PrivateMember(Box::new(left), self.name()?));
        }
        let property = self.property_name()?;
        Ok(Expr::Member(
            Box::new(left),
            Box::new(Expr::Literal(Value::string(property))),
        ))
    }

    #[inline(never)]
    pub(super) fn expression_postfix(&mut self, left: Expr) -> Result<Expr, JsError> {
        let increment = self.eat("++");
        if !increment {
            self.require("--")?;
        }
        if !assignment_target(&left, false) {
            return Err(self.error("Invalid update target"));
        }
        Ok(Expr::Update(Box::new(left), increment, false))
    }

    pub(super) fn expression_operator(&self, min: u8) -> Option<(String, u8)> {
        let op = match &self.token().kind {
            Kind::Symbol(op) => *op,
            Kind::Name(op) if matches!(op.as_str(), "in" | "instanceof") => op.as_str(),
            _ => return None,
        };
        let precedence = match op {
            "," => 0,
            "=" | "+=" | "-=" | "*=" | "/=" | "%=" | "**=" | "&=" | "|=" | "^=" | "<<=" | ">>="
            | ">>>=" => 1,
            "??" | "||" => 3,
            "&&" => 4,
            "|" => 5,
            "^" => 6,
            "&" => 7,
            "==" | "!=" | "===" | "!==" => 8,
            "<" | ">" | "<=" | ">=" | "in" | "instanceof" => 9,
            "<<" | ">>" | ">>>" => 10,
            "+" | "-" => 11,
            "*" | "/" | "%" => 12,
            "**" => 13,
            _ => return None,
        };
        (precedence >= min).then(|| (op.to_owned(), precedence))
    }

    #[inline(never)]
    pub(super) fn expression_binary(
        &mut self,
        left: Expr,
        op: String,
        precedence: u8,
        right: Expr,
    ) -> Result<Expr, JsError> {
        if op == "," {
            return self.expression_sequence(left, right);
        }
        Ok(if precedence == 1 {
            if !assignment_target(&left, op == "=") {
                return Err(self.error("Invalid assignment target"));
            }
            Expr::Assign(op, Box::new(left), Box::new(right))
        } else {
            Expr::Binary(op, Box::new(left), Box::new(right))
        })
    }
}
