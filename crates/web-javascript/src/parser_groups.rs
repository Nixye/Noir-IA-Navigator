//! Parentheses carry grammar context, but are not AST nodes. The Pratt machine
//! keeps their count in each context rather than allocating one frame per group.
use super::*;

impl Parser {
    pub(super) fn expression_groups(&mut self) -> Result<usize, JsError> {
        let mut groups = 0;
        while self.is("(") && !self.group_starts_arrow() {
            self.index += 1;
            // This work has no AST node or native/continuation frame. Keep
            // it cumulative even across templates, failed calls and rewinds.
            self.count_group()?;
            groups += 1;
        }
        Ok(groups)
    }

    fn count_group(&mut self) -> Result<(), JsError> {
        self.group_visits += 1;
        if self.group_visits > MAX_GROUP_VISITS {
            let mut error = JsError::resource("Parser group visit budget exceeded");
            error.add_diagnostic_context(&format!(
                "parser group-visits={} limit={}",
                self.group_visits, MAX_GROUP_VISITS,
            ));
            Err(error)
        } else {
            Ok(())
        }
    }

    fn group_starts_arrow(&self) -> bool {
        let close = self.token().close_paren;
        close != NO_TOKEN
            && self
                .tokens
                .get(close as usize + 1)
                .is_some_and(|token| !token.newline && matches!(token.kind, Kind::Symbol("=>")))
    }
}
