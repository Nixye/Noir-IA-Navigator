use crate::{JsError, Value};
use std::sync::Arc;

// Preserve the old ceiling for payload-bearing tokens and context pushes.
// Extra compact slots are available only for static, context-free punctuation.
const MAX_COSTLY_TOKENS: usize = 32_768;
// The previous two-String token occupied 64 bytes on supported x64 builds.
// Use a conservative layout floor rather than assuming that on every target.
const LEGACY_TOKEN_SLOT_BYTES: usize =
    2 * std::mem::size_of::<String>() + 2 * std::mem::size_of::<usize>();
const MAX_TOKEN_STORAGE_BYTES: usize = (MAX_COSTLY_TOKENS + 1) * LEGACY_TOKEN_SLOT_BYTES;
const REGEXP_BOX_BYTES: usize = crate::regexp::MAX_REGEXES * std::mem::size_of::<RegExpLiteral>();
const MAX_SCRIPT_TOKENS: usize = {
    let storage_limit =
        (MAX_TOKEN_STORAGE_BYTES - REGEXP_BOX_BYTES) / std::mem::size_of::<Token>() - 1;
    if storage_limit < 40_960 {
        storage_limit
    } else {
        40_960
    }
};
// Cumulative grammar/validation entries, not an all-AST node or byte count.
const MAX_PARSER_VISITS: usize = 16_384;
// Allocation-free grouping work has its own inherited, non-refundable ledger.
const MAX_GROUP_VISITS: usize = 16_384;
const NO_TOKEN: u32 = u32::MAX;
const _: () = assert!(MAX_SCRIPT_TOKENS < NO_TOKEN as usize);

#[cfg(test)]
#[path = "parser_stack_tests.rs"]
mod stack_tests;

#[cfg(test)]
#[path = "parser_token_budget_tests.rs"]
mod token_budget_tests;

#[cfg(test)]
#[path = "parser_lookahead_tests.rs"]
mod lookahead_tests;

#[path = "parser_expressions.rs"]
mod expressions;

#[path = "parser_pratt.rs"]
mod pratt;

#[cfg(test)]
#[path = "parser_pratt_tests.rs"]
mod pratt_tests;

#[path = "parser_depth.rs"]
mod depth;

#[path = "parser_flat_storage.rs"]
mod flat_storage;

#[path = "parser_if_chain.rs"]
mod if_chain;
pub(super) use if_chain::IfChain;

#[cfg(test)]
#[path = "parser_if_chain_tests.rs"]
mod if_chain_tests;

#[path = "parser_conditional_chain.rs"]
mod conditional_chain;
pub(super) use conditional_chain::ConditionalChain;

#[cfg(test)]
#[path = "parser_conditional_chain_tests.rs"]
mod conditional_chain_tests;

#[path = "parser_sequence.rs"]
mod sequence;
pub(super) use sequence::Sequence;

#[cfg(test)]
#[path = "parser_sequence_tests.rs"]
mod sequence_tests;

#[path = "parser_groups.rs"]
mod groups;

#[cfg(test)]
#[path = "parser_group_tests.rs"]
mod group_tests;

#[cfg(test)]
#[path = "parser_visit_budget_tests.rs"]
mod visit_budget_tests;

#[path = "parser_atoms.rs"]
mod atoms;

#[path = "parser_statements.rs"]
mod statements;

#[cfg(test)]
#[path = "parser_for_tests.rs"]
mod for_tests;

#[path = "parser_modules.rs"]
mod modules;
pub use modules::{Module, parse_module};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionKind {
    Ordinary,
    Async,
    Generator,
    AsyncGenerator,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Literal(Value),
    RegExp(String, String),
    Name(String),
    Array(Vec<Expr>),
    Object(Vec<(String, Expr)>),
    Member(Box<Expr>, Box<Expr>),
    Call(Box<Expr>, Vec<Expr>),
    ImportMeta,
    ImportCall {
        specifier: Box<Expr>,
        options: Option<Box<Expr>>,
    },
    Construct(Box<Expr>, Vec<Expr>),
    Await(Box<Expr>),
    Yield(Option<Box<Expr>>),
    YieldDelegate(Box<Expr>),
    Unary(String, Box<Expr>),
    Binary(String, Box<Expr>, Box<Expr>),
    Sequence(Sequence),
    Assign(String, Box<Expr>, Box<Expr>),
    Update(Box<Expr>, bool, bool),
    Conditional(Box<Expr>, Box<Expr>, Box<Expr>),
    ConditionalChain(ConditionalChain),
    Spread(Box<Expr>),
    Hole,
    Template(Vec<Expr>),
    PrivateMember(Box<Expr>, String),
    Super,
    Class(ClassDefinition),
    Function {
        kind: FunctionKind,
        method: bool,
        name: Option<String>,
        params: Arc<Vec<String>>,
        body: Arc<Vec<Stmt>>,
        arrow: bool,
        parameters: Option<Arc<Vec<Parameter>>>,
    },
}
fn assignment_target(expr: &Expr, pattern: bool) -> bool {
    match expr {
        Expr::Name(name) => name != "this",
        Expr::Member(..) | Expr::PrivateMember(..) => true,
        Expr::Array(items) if pattern => items.iter().enumerate().all(|(index, item)| match item {
            Expr::Hole => true,
            Expr::Spread(target) => index + 1 == items.len() && assignment_target(target, true),
            Expr::Assign(op, target, _) if op == "=" => assignment_target(target, true),
            target => assignment_target(target, true),
        }),
        Expr::Object(items) if pattern => {
            items
                .iter()
                .enumerate()
                .all(|(index, (_, item))| match item {
                    Expr::Spread(target) => {
                        index + 1 == items.len() && assignment_target(target, false)
                    }
                    Expr::Assign(op, target, _) if op == "=" => assignment_target(target, true),
                    target => assignment_target(target, true),
                })
        }
        _ => false,
    }
}
#[derive(Debug, Clone)]
pub enum Pattern {
    Name(String),
    Array(Vec<Option<Pattern>>, Option<Box<Pattern>>),
    Object(Vec<(String, Pattern)>, Option<Box<Pattern>>),
    Default(Box<Pattern>, Box<Expr>),
}
#[derive(Debug, Clone)]
pub struct Parameter {
    pub pattern: Pattern,
    pub rest: bool,
}
#[derive(Debug, Clone)]
pub struct ClassDefinition {
    pub name: Option<String>,
    pub base: Option<Box<Expr>>,
    pub members: Vec<ClassMember>,
}
#[derive(Debug, Clone)]
pub struct ClassMember {
    pub name: String,
    pub value: Option<Expr>,
    pub static_member: bool,
    pub private: bool,
    pub method: bool,
    pub accessor: Option<bool>,
}
impl Pattern {
    pub(super) fn visit_names<'a>(
        &'a self,
        visit: &mut impl FnMut(&'a str) -> Result<(), JsError>,
    ) -> Result<(), JsError> {
        match self {
            Self::Name(name) => visit(name),
            Self::Array(values, rest) => {
                for value in values.iter().flatten() {
                    value.visit_names(visit)?;
                }
                if let Some(rest) = rest {
                    rest.visit_names(visit)?;
                }
                Ok(())
            }
            Self::Object(values, rest) => {
                for (_, value) in values {
                    value.visit_names(visit)?;
                }
                if let Some(rest) = rest {
                    rest.visit_names(visit)?;
                }
                Ok(())
            }
            Self::Default(pattern, _) => pattern.visit_names(visit),
        }
    }
    pub fn names(&self, output: &mut Vec<String>) {
        match self {
            Self::Name(name) => output.push(name.clone()),
            Self::Array(values, rest) => {
                for value in values.iter().flatten() {
                    value.names(output);
                }
                if let Some(rest) = rest {
                    rest.names(output);
                }
            }
            Self::Object(values, rest) => {
                for (_, value) in values {
                    value.names(output);
                }
                if let Some(rest) = rest {
                    rest.names(output);
                }
            }
            Self::Default(pattern, _) => pattern.names(output),
        }
    }
}
fn parameter_length_names(parameters: &[Parameter]) -> Vec<String> {
    parameters
        .iter()
        .take_while(|parameter| {
            !parameter.rest && !matches!(parameter.pattern, Pattern::Default(..))
        })
        .enumerate()
        .map(|(index, parameter)| match &parameter.pattern {
            Pattern::Name(name) => name.clone(),
            _ => format!("[[parameter{index}]]"),
        })
        .collect()
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Declaration {
    Var,
    Let,
    Const,
}
#[derive(Debug, Clone)]
pub enum ForBinding {
    Declaration(Declaration, String),
    Reference(Expr),
    Pattern(Declaration, Pattern),
}
#[derive(Debug, Clone)]
pub enum Stmt {
    Empty,
    Expression(Expr),
    Variables(Declaration, Vec<(String, Option<Expr>)>),
    PatternVariables(Declaration, Vec<(Pattern, Option<Expr>)>),
    FunctionDefinition(String, Expr),
    ClassDefinition(String, ClassDefinition),
    Block(Vec<Stmt>),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    IfChain(IfChain),
    While(Expr, Box<Stmt>),
    DoWhile(Box<Stmt>, Expr),
    For(Option<Box<Stmt>>, Option<Expr>, Option<Expr>, Box<Stmt>),
    ForIn(ForBinding, Expr, Box<Stmt>),
    ForOf(ForBinding, Expr, Box<Stmt>),
    ForAwaitOf(ForBinding, Expr, Box<Stmt>),
    Try {
        body: Vec<Stmt>,
        catch: Option<(Option<String>, Vec<Stmt>)>,
        finally: Option<Vec<Stmt>>,
    },
    Switch(Expr, Vec<(Option<Expr>, Vec<Stmt>)>),
    Label(String, Box<Stmt>),
    Return(Option<Expr>),
    Throw(Expr),
    Break(Option<String>),
    Continue(Option<String>),
}
type TemplateParts = Vec<(String, Option<String>)>;

#[derive(Debug, PartialEq)]
struct RegExpLiteral {
    pattern: String,
    flags: String,
}

// Rare multi-string payloads must not inflate every punctuation/number slot.
#[derive(Debug, PartialEq)]
enum Kind {
    Name(String),
    Number(f64),
    BigInt(Arc<crate::BigInt>),
    String(String),
    Template(TemplateParts),
    RegExp(Box<RegExpLiteral>),
    Symbol(&'static str),
    End,
}
#[derive(Debug)]
struct Token {
    kind: Kind,
    offset: usize,
    close_paren: u32,
    newline: bool,
}

pub fn parse(source: &str) -> Result<Vec<Stmt>, JsError> {
    if source.len() > 128 * 1024 {
        return Err(JsError::resource("Script exceeds 128 KiB"));
    }
    let tokens = lex(source)?;
    let mut p = Parser {
        tokens,
        index: 0,
        depth: 0,
        expression_frames: 0,
        function_depth: 0,
        function_kind: FunctionKind::Ordinary,
        module_context: false,
        loop_depth: 0,
        switch_depth: 0,
        labels: Vec::new(),
        visits: 0,
        group_visits: 0,
        flat_storage_bytes: 0,
    };
    let mut output = Vec::new();
    while !matches!(p.token().kind, Kind::End) {
        output.push(p.statement()?);
    }
    Ok(output)
}

/// Parse the two Function constructor grammars independently. In particular,
/// parameter text cannot close a synthetic wrapper and inject a statement.
pub fn parse_dynamic_function(parameters: &str, body: &str) -> Result<FunctionBody, JsError> {
    let mut p = Parser {
        tokens: lex(parameters)?,
        index: 0,
        depth: 0,
        expression_frames: 0,
        function_depth: 0,
        function_kind: FunctionKind::Ordinary,
        module_context: false,
        loop_depth: 0,
        switch_depth: 0,
        labels: Vec::new(),
        visits: 0,
        group_visits: 0,
        flat_storage_bytes: 0,
    };
    let parameters = p.parameters(true)?;
    let params = parameter_length_names(&parameters);
    p.tokens = lex(body)?;
    p.index = 0;
    p.function_depth = 1;
    let mut statements = Vec::new();
    while !matches!(p.token().kind, Kind::End) {
        statements.push(p.statement()?);
    }
    depth::validate_function_depth(&statements, &parameters)?;
    Ok((Arc::new(params), Arc::new(statements), Arc::new(parameters)))
}

fn lex_template(
    source: &str,
    start: usize,
    depth: usize,
) -> Result<(TemplateParts, usize), JsError> {
    if depth > 32 {
        return Err(JsError::resource("Template nesting exceeded"));
    }
    let mut parts = Vec::new();
    let mut text = String::new();
    let mut i = start + 1;
    while i < source.len() {
        let c = source[i..].chars().next().unwrap();
        i += c.len_utf8();
        if c == '`' {
            parts.push((text, None));
            return Ok((parts, i));
        }
        if c == '$' && source.as_bytes().get(i) == Some(&b'{') {
            let begin = i + 1;
            let end = template_expression_end(source, begin, depth)?;
            parts.push((
                std::mem::take(&mut text),
                Some(source[begin..end].to_owned()),
            ));
            if parts.len() > 128 {
                return Err(JsError::resource("Template interpolation count exceeded"));
            }
            i = end + 1;
            continue;
        }
        if c == '\\' {
            let escaped = source
                .get(i..)
                .and_then(|s| s.chars().next())
                .ok_or_else(|| JsError::syntax(i, "Unclosed template escape"))?;
            i += escaped.len_utf8();
            match escaped {
                'n' => text.push('\n'),
                'r' => text.push('\r'),
                't' => text.push('\t'),
                'b' => text.push('\u{8}'),
                'f' => text.push('\u{c}'),
                'v' => text.push('\u{b}'),
                '0' => text.push('\0'),
                '\n' => {}
                '\r' => {
                    if source.as_bytes().get(i) == Some(&b'\n') {
                        i += 1;
                    }
                }
                'u' | 'x' => {
                    let digits = if escaped == 'u' { 4 } else { 2 };
                    let end = i + digits;
                    let hex = source
                        .get(i..end)
                        .filter(|s| s.is_ascii())
                        .ok_or_else(|| JsError::syntax(i, "Invalid template escape"))?;
                    let scalar = u32::from_str_radix(hex, 16)
                        .ok()
                        .and_then(char::from_u32)
                        .ok_or_else(|| JsError::syntax(i, "Unsupported template Unicode escape"))?;
                    text.push(scalar);
                    i = end;
                }
                other => text.push(other),
            }
        } else if c == '\r' {
            text.push('\n');
            if source.as_bytes().get(i) == Some(&b'\n') {
                i += 1;
            }
        } else {
            text.push(c);
        }
        if text.len() > 65_536 {
            return Err(JsError::resource("Template string exceeds 64 KiB"));
        }
    }
    Err(JsError::syntax(start, "Unclosed template"))
}
fn template_expression_end(source: &str, mut i: usize, depth: usize) -> Result<usize, JsError> {
    let mut braces = 0usize;
    let mut regex_allowed = true;
    while i < source.len() {
        let c = source[i..].chars().next().unwrap();
        if source[i..].starts_with("//") {
            i = source[i..].find('\n').map_or(source.len(), |n| i + n);
            continue;
        }
        if source[i..].starts_with("/*") {
            i = source[i + 2..]
                .find("*/")
                .map(|n| i + 2 + n + 2)
                .ok_or_else(|| JsError::syntax(i, "Unclosed interpolation comment"))?;
            continue;
        }
        if c == '`' {
            i = lex_template(source, i, depth + 1)?.1;
            regex_allowed = false;
            continue;
        }
        if matches!(c, '\'' | '"') {
            let quote = c;
            i += 1;
            let mut closed = false;
            while i < source.len() {
                let x = source[i..].chars().next().unwrap();
                i += x.len_utf8();
                if x == '\\' {
                    let escaped = source[i..]
                        .chars()
                        .next()
                        .ok_or_else(|| JsError::syntax(i, "Unclosed interpolation string"))?;
                    i += escaped.len_utf8();
                } else if x == quote {
                    closed = true;
                    break;
                }
            }
            if !closed {
                return Err(JsError::syntax(i, "Unclosed interpolation string"));
            }
            regex_allowed = false;
            continue;
        }
        if c == '/' && regex_allowed {
            i = lex_regexp(source, i)?.2;
            regex_allowed = false;
            continue;
        }
        if c == '}' {
            if braces == 0 {
                return Ok(i);
            }
            braces -= 1;
        } else if c == '{' {
            braces += 1;
            if braces > 64 {
                return Err(JsError::resource("Template expression nesting exceeded"));
            }
        }
        if !c.is_whitespace() {
            regex_allowed = "([{,:;=!?&|+-*%".contains(c);
        }
        i += c.len_utf8();
    }
    Err(JsError::syntax(i, "Unclosed template interpolation"))
}
// Keep bounded geometric growth for real tokens. EOF is the final insertion,
// so it needs only one extra slot instead of doubling a full vector.
fn push_token(tokens: &mut Vec<Token>, token: Token) -> Result<(), JsError> {
    if tokens.len() > MAX_SCRIPT_TOKENS {
        return Err(JsError::resource("Script token budget exceeded"));
    }
    if tokens.len() == tokens.capacity() {
        let capacity = if matches!(token.kind, Kind::End) {
            tokens.len() + 1
        } else {
            (tokens.capacity() * 2).clamp(4, MAX_SCRIPT_TOKENS + 1)
        };
        // Charge requested capacity, including EOF and the maximum number of
        // boxed regexp payloads, before allocating. Owned literal bytes retain
        // their separate source/literal limits and are not included here.
        if capacity * std::mem::size_of::<Token>() + REGEXP_BOX_BYTES > MAX_TOKEN_STORAGE_BYTES {
            return Err(JsError::resource("Script token storage budget exceeded"));
        }
        tokens
            .try_reserve_exact(capacity - tokens.len())
            .map_err(|_| JsError::resource("Script token allocation failed"))?;
    }
    tokens.push(token);
    Ok(())
}

fn single_symbol(ch: char) -> Option<&'static str> {
    Some(match ch {
        '{' => "{",
        '}' => "}",
        '[' => "[",
        ']' => "]",
        '(' => "(",
        ')' => ")",
        '.' => ".",
        ',' => ",",
        ';' => ";",
        ':' => ":",
        '?' => "?",
        '#' => "#",
        '+' => "+",
        '-' => "-",
        '*' => "*",
        '/' => "/",
        '%' => "%",
        '!' => "!",
        '=' => "=",
        '<' => "<",
        '>' => ">",
        '&' => "&",
        '|' => "|",
        '^' => "^",
        '~' => "~",
        _ => return None,
    })
}

fn costly_token(kind: &Kind) -> bool {
    match kind {
        // These are the only punctuation tokens that push RegexContext frames.
        Kind::Symbol(symbol) => matches!(*symbol, "(" | "{" | "?"),
        Kind::End => false,
        _ => true,
    }
}

fn lex(source: &str) -> Result<Vec<Token>, JsError> {
    let mut result = Vec::new();
    let mut i = 0;
    let mut newline = false;
    let mut context = RegexContext {
        allowed: true,
        ..Default::default()
    };
    let mut regexp_count = 0usize;
    let mut regexp_bytes = 0usize;
    let mut costly_tokens = 0usize;
    while i < source.len() {
        let ch = source[i..].chars().next().expect("valid UTF-8 boundary");
        if ch.is_whitespace() {
            newline |= matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}');
            i += ch.len_utf8();
            continue;
        }
        if source[i..].starts_with("//") {
            i = source[i..].find('\n').map_or(source.len(), |n| i + n);
            continue;
        }
        if source[i..].starts_with("/*") {
            let end = source[i + 2..]
                .find("*/")
                .ok_or_else(|| JsError::syntax(i, "Unterminated comment"))?
                + i
                + 2;
            newline |= source[i..end].contains(['\n', '\r']);
            i = end + 2;
            continue;
        }
        let start = i;
        let kind = if ch == '/' && context.allowed {
            let (pattern, flags, end) = lex_regexp(source, i)?;
            regexp_count += 1;
            regexp_bytes += pattern.len();
            if regexp_count > crate::regexp::MAX_REGEXES
                || regexp_bytes > crate::regexp::MAX_PATTERN_TOTAL
            {
                return Err(JsError::resource("RegExp literal budget exceeded"));
            }
            crate::regexp::Compiled::compile(&pattern, &flags).map_err(|error| {
                if error.kind == crate::ErrorKind::Resource {
                    error
                } else {
                    JsError::syntax(start, error.message)
                }
            })?;
            i = end;
            Kind::RegExp(Box::new(RegExpLiteral { pattern, flags }))
        } else if ch.is_alphabetic() || matches!(ch, '_' | '$') {
            i += ch.len_utf8();
            while i < source.len() {
                let c = source[i..].chars().next().unwrap();
                if !c.is_alphanumeric() && !matches!(c, '_' | '$') {
                    break;
                }
                i += c.len_utf8();
            }
            Kind::Name(source[start..i].into())
        } else if ch.is_ascii_digit()
            || (ch == '.' && source.as_bytes().get(i + 1).is_some_and(u8::is_ascii_digit))
        {
            let radix = if source[i..].starts_with("0x") || source[i..].starts_with("0X") {
                16
            } else if source[i..].starts_with("0b") || source[i..].starts_with("0B") {
                2
            } else if source[i..].starts_with("0o") || source[i..].starts_with("0O") {
                8
            } else {
                10
            };
            if radix != 10 {
                i += 2;
            }
            let digit_start = i;
            while source
                .as_bytes()
                .get(i)
                .is_some_and(|c| (*c as char).is_digit(radix))
            {
                i += 1;
            }
            let integer_end = i;
            if radix == 10 {
                if source.as_bytes().get(i) == Some(&b'.') {
                    i += 1;
                    while source.as_bytes().get(i).is_some_and(u8::is_ascii_digit) {
                        i += 1;
                    }
                }
                if source
                    .as_bytes()
                    .get(i)
                    .is_some_and(|c| matches!(c, b'e' | b'E'))
                {
                    i += 1;
                    if source
                        .as_bytes()
                        .get(i)
                        .is_some_and(|c| matches!(c, b'+' | b'-'))
                    {
                        i += 1;
                    }
                    let before = i;
                    while source.as_bytes().get(i).is_some_and(u8::is_ascii_digit) {
                        i += 1;
                    }
                    if before == i {
                        return Err(JsError::syntax(start, "Invalid numeric exponent"));
                    }
                }
            } else if digit_start == i {
                return Err(JsError::syntax(start, "Missing integer digits"));
            }
            let big = source.as_bytes().get(i) == Some(&b'n');
            let kind = if big {
                if i != integer_end
                    || (radix == 10 && i - start > 1 && source.as_bytes()[start] == b'0')
                {
                    return Err(JsError::syntax(start, "Invalid BigInt literal"));
                }
                let value = crate::BigInt::parse(&source[start..i]).map_err(|e| {
                    if e.kind == crate::ErrorKind::Resource {
                        e
                    } else {
                        JsError::syntax(start, e.message)
                    }
                })?;
                i += 1;
                Kind::BigInt(Arc::new(value))
            } else if radix == 10 {
                Kind::Number(
                    source[start..i]
                        .parse()
                        .map_err(|_| JsError::syntax(start, "Invalid number"))?,
                )
            } else {
                let mut value = 0.;
                for c in source[digit_start..i].chars() {
                    value = value * radix as f64 + c.to_digit(radix).unwrap() as f64;
                }
                Kind::Number(value)
            };
            if source[i..]
                .chars()
                .next()
                .is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '$'))
            {
                return Err(JsError::syntax(
                    i,
                    "Invalid character after numeric literal",
                ));
            }
            kind
        } else if ch == '`' {
            let (parts, end) = lex_template(source, i, 0)?;
            i = end;
            Kind::Template(parts)
        } else if matches!(ch, '\'' | '"') {
            let quote = ch;
            i += ch.len_utf8();
            let mut text = String::new();
            let mut closed = false;
            while i < source.len() {
                let c = source[i..].chars().next().unwrap();
                i += c.len_utf8();
                if c == quote {
                    closed = true;
                    break;
                }
                if quote != '`' && matches!(c, '\n' | '\r') {
                    return Err(JsError::syntax(i, "Unescaped line break in string"));
                }
                if quote == '`' && c == '$' && source.as_bytes().get(i) == Some(&b'{') {
                    return Err(JsError::syntax(
                        i,
                        "Template interpolation is not supported",
                    ));
                }
                if c == '\\' {
                    let escaped = source[i..]
                        .chars()
                        .next()
                        .ok_or_else(|| JsError::syntax(i, "Unterminated string escape"))?;
                    i += escaped.len_utf8();
                    match escaped {
                        'n' => text.push('\n'),
                        'r' => text.push('\r'),
                        't' => text.push('\t'),
                        'b' => text.push('\u{8}'),
                        'f' => text.push('\u{c}'),
                        '0' => text.push('\0'),
                        '\n' => {}
                        '\r' => {
                            if source.as_bytes().get(i) == Some(&b'\n') {
                                i += 1;
                            }
                        }
                        'u' | 'x' => {
                            let digits = if escaped == 'u' { 4 } else { 2 };
                            let end = i + digits;
                            let hex = source
                                .get(i..end)
                                .filter(|s| s.is_ascii())
                                .ok_or_else(|| JsError::syntax(i, "Invalid Unicode escape"))?;
                            let n = u32::from_str_radix(hex, 16)
                                .map_err(|_| JsError::syntax(i, "Invalid string escape"))?;
                            text.push(char::from_u32(n).ok_or_else(|| {
                                JsError::syntax(i, "Surrogate escapes are not supported")
                            })?);
                            i = end;
                        }
                        other => text.push(other),
                    }
                } else {
                    text.push(c);
                }
            }
            if !closed {
                return Err(JsError::syntax(start, "Unterminated string"));
            }
            if text.len() > 65_536 {
                return Err(JsError::resource("String literal exceeds 64 KiB"));
            }
            Kind::String(text)
        } else {
            let operator = [
                ">>>=", "...", "===", "!==", "**=", "<<=", ">>=", ">>>", "=>", "==", "!=", "<=",
                ">=", "&&", "||", "??", "++", "--", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=",
                "**", "<<", ">>",
            ]
            .into_iter()
            .find(|op| source[i..].starts_with(op));
            if let Some(op) = operator {
                i += op.len();
                Kind::Symbol(op)
            } else if let Some(symbol) = single_symbol(ch) {
                i += ch.len_utf8();
                Kind::Symbol(symbol)
            } else {
                return Err(JsError::syntax(i, format!("Unsupported token {ch:?}")));
            }
        };
        if result.len() == MAX_SCRIPT_TOKENS {
            let mut error = JsError::resource("Script token budget exceeded");
            error.add_diagnostic_context(&format!(
                "lexer tokens={} limit={} input-bytes={} scanned-bytes={}",
                result.len() + 1,
                MAX_SCRIPT_TOKENS,
                source.len(),
                i,
            ));
            return Err(error);
        }
        if costly_token(&kind) {
            costly_tokens += 1;
            if costly_tokens > MAX_COSTLY_TOKENS {
                let mut error = JsError::resource("Script token payload budget exceeded");
                error.add_diagnostic_context(&format!(
                    "lexer payload-context-tokens={} limit={} input-bytes={} scanned-bytes={}",
                    costly_tokens,
                    MAX_COSTLY_TOKENS,
                    source.len(),
                    i,
                ));
                return Err(error);
            }
        }
        context.update(&kind, result.last());
        push_token(
            &mut result,
            Token {
                kind,
                offset: start,
                close_paren: NO_TOKEN,
                newline,
            },
        )?;
        newline = false;
    }
    push_token(
        &mut result,
        Token {
            kind: Kind::End,
            offset: source.len(),
            close_paren: NO_TOKEN,
            newline,
        },
    )?;
    link_parentheses(&mut result);
    Ok(result)
}

// During this pass an unmatched opener stores the previous opener. Reuse that
// field as the closing-token index when matched, then erase links left unmatched.
// This needs no auxiliary stack and occupies existing Token padding on x64.
fn link_parentheses(tokens: &mut [Token]) {
    let mut open = NO_TOKEN;
    for index in 0..tokens.len() {
        match tokens[index].kind {
            Kind::Symbol("(") => {
                tokens[index].close_paren = open;
                open = index as u32;
            }
            Kind::Symbol(")") if open != NO_TOKEN => {
                let previous = tokens[open as usize].close_paren;
                tokens[open as usize].close_paren = index as u32;
                open = previous;
            }
            _ => {}
        }
    }
    while open != NO_TOKEN {
        let previous = tokens[open as usize].close_paren;
        tokens[open as usize].close_paren = NO_TOKEN;
        open = previous;
    }
}

fn lex_regexp(source: &str, start: usize) -> Result<(String, String, usize), JsError> {
    let mut index = start + 1;
    let begin = index;
    let mut class = false;
    let mut escaped = false;
    while index < source.len() {
        let c = source[index..].chars().next().unwrap();
        if matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}') {
            return Err(JsError::syntax(index, "Line terminator in RegExp literal"));
        }
        if !escaped && c == '/' && !class {
            let pattern = source[begin..index].to_owned();
            index += 1;
            let flags_begin = index;
            while source
                .as_bytes()
                .get(index)
                .is_some_and(u8::is_ascii_alphabetic)
            {
                index += 1;
            }
            return Ok((pattern, source[flags_begin..index].into(), index));
        }
        if !escaped {
            if c == '[' {
                class = true;
            }
            if c == ']' {
                class = false;
            }
        }
        escaped = c == '\\' && !escaped;
        index += c.len_utf8();
    }
    Err(JsError::syntax(start, "Unterminated RegExp literal"))
}

#[derive(Default)]
struct RegexContext {
    allowed: bool,
    parentheses: Vec<(bool, Option<bool>)>,
    braces: Vec<(bool, bool)>,
    questions: Vec<(usize, usize)>,
    colon_statement: bool,
    pending_function: Option<bool>,
    pending_async: bool,
    closed_function: Option<bool>,
    last_block: bool,
    property_name: bool,
}
impl RegexContext {
    fn update(&mut self, kind: &Kind, previous: Option<&Token>) {
        let previous_text = previous.and_then(|token| match &token.kind {
            Kind::Name(text) => Some(text.as_str()),
            Kind::Symbol(text) => Some(*text),
            _ => None,
        });
        let statement_start = previous.is_none()
            || matches!(
                previous_text,
                Some(";" | "else" | "do" | "export" | "default")
            )
            || (previous_text == Some("{") && self.braces.last().is_some_and(|frame| frame.0))
            || (previous_text == Some("}") && self.last_block);
        let async_declaration = std::mem::take(&mut self.pending_async);
        let closed_function = self.closed_function.take();
        let property_name = self.property_name;
        self.property_name = matches!(kind, Kind::Name(_)) && previous_text == Some(".");
        self.last_block = false;
        self.allowed = match kind {
            Kind::Name(name) => {
                if name == "function" && previous_text != Some(".") {
                    self.pending_function = Some(
                        statement_start || (previous_text == Some("async") && async_declaration),
                    );
                }
                if name == "async" {
                    self.pending_async = statement_start;
                }
                (name == "default" && previous_text == Some("export"))
                    || previous_text != Some(".")
                        && matches!(
                            name.as_str(),
                            "return"
                                | "await"
                                | "yield"
                                | "throw"
                                | "case"
                                | "delete"
                                | "void"
                                | "typeof"
                                | "new"
                                | "in"
                                | "instanceof"
                                | "else"
                                | "do"
                        )
            }
            Kind::Symbol(symbol) => match *symbol {
                "(" => {
                    let control = !property_name
                        && matches!(
                            previous_text,
                            Some("if" | "while" | "for" | "with" | "switch" | "catch")
                        );
                    self.parentheses
                        .push((control, self.pending_function.take()));
                    true
                }
                ")" => {
                    let (control, function) = self.parentheses.pop().unwrap_or_default();
                    self.closed_function = function;
                    control
                }
                "{" => {
                    let function = closed_function.is_some() || previous_text == Some("=>");
                    let block = function
                        || statement_start
                        || previous_text == Some(")")
                        || matches!(previous_text, Some("try" | "finally"))
                        || (previous_text == Some(":") && self.colon_statement);
                    let after = if function {
                        closed_function.unwrap_or(false)
                    } else {
                        block
                    };
                    self.braces.push((block, after));
                    true
                }
                "}" => {
                    let (_, after) = self.braces.pop().unwrap_or((true, true));
                    self.last_block = after;
                    after
                }
                "?" => {
                    self.questions
                        .push((self.parentheses.len(), self.braces.len()));
                    true
                }
                ":" => {
                    let conditional =
                        self.questions.last() == Some(&(self.parentheses.len(), self.braces.len()));
                    if conditional {
                        self.questions.pop();
                    }
                    self.colon_statement =
                        !conditional && self.braces.last().is_none_or(|frame| frame.0);
                    true
                }
                "]" | "++" | "--" | "." => false,
                _ => true,
            },
            Kind::End => true,
            _ => false,
        };
    }
}

struct Parser {
    tokens: Vec<Token>,
    index: usize,
    depth: usize,
    // Active logical expression contexts, including suspended Pratt parents
    // across native atom/function/pattern reentry. Templates inherit this.
    expression_frames: usize,
    function_depth: usize,
    function_kind: FunctionKind,
    module_context: bool,
    loop_depth: usize,
    switch_depth: usize,
    labels: Vec<(String, bool)>,
    visits: usize,
    group_visits: usize,
    flat_storage_bytes: usize,
}
type FunctionBody = (Arc<Vec<String>>, Arc<Vec<Stmt>>, Arc<Vec<Parameter>>);
impl Parser {
    fn function_prefix(&mut self) -> Result<Option<FunctionKind>, JsError> {
        if self.is("async")
            && self
                .tokens
                .get(self.index + 1)
                .is_some_and(|token| matches!(&token.kind, Kind::Name(name) if name == "function"))
        {
            self.index += 1;
            if self.token().newline || !self.eat("function") {
                return Err(self.error("Expected function after async without a line break"));
            }
            return Ok(Some(if self.eat("*") {
                FunctionKind::AsyncGenerator
            } else {
                FunctionKind::Async
            }));
        }
        if self.eat("function") {
            return Ok(Some(if self.eat("*") {
                FunctionKind::Generator
            } else {
                FunctionKind::Ordinary
            }));
        }
        Ok(None)
    }
    fn token(&self) -> &Token {
        &self.tokens[self.index]
    }
    fn is(&self, s: &str) -> bool {
        match &self.token().kind {
            Kind::Name(value) => value == s,
            Kind::Symbol(value) => *value == s,
            _ => false,
        }
    }
    fn eat(&mut self, s: &str) -> bool {
        if self.is(s) {
            self.index += 1;
            true
        } else {
            false
        }
    }
    fn require(&mut self, s: &str) -> Result<(), JsError> {
        if self.eat(s) {
            Ok(())
        } else {
            Err(self.error(format!("Expected {s:?}")))
        }
    }
    fn error(&self, message: impl Into<String>) -> JsError {
        JsError::syntax(self.token().offset, message)
    }
    fn name(&mut self) -> Result<String, JsError> {
        if let Kind::Name(name) = &self.token().kind {
            let name = name.clone();
            if matches!(
                name.as_str(),
                "class"
                    | "import"
                    | "export"
                    | "async"
                    | "await"
                    | "new"
                    | "yield"
                    | "delete"
                    | "try"
                    | "catch"
                    | "finally"
                    | "switch"
                    | "with"
                    | "debugger"
                    | "this"
                    | "null"
                    | "true"
                    | "false"
                    | "function"
                    | "let"
                    | "const"
                    | "var"
                    | "return"
                    | "if"
                    | "else"
                    | "while"
                    | "do"
                    | "case"
                    | "default"
                    | "in"
                    | "instanceof"
                    | "super"
                    | "extends"
                    | "for"
                    | "break"
                    | "continue"
                    | "throw"
            ) {
                return Err(self.error("Reserved or unsupported binding name"));
            }
            self.index += 1;
            Ok(name)
        } else {
            Err(self.error("Expected identifier"))
        }
    }
    fn property_name(&mut self) -> Result<String, JsError> {
        match &self.token().kind {
            Kind::Name(name) | Kind::String(name) => {
                let name = name.clone();
                self.index += 1;
                Ok(name)
            }
            Kind::BigInt(n) => {
                let name = n.text(10);
                self.index += 1;
                Ok(name)
            }
            Kind::Number(n) => {
                let name = n.to_string();
                self.index += 1;
                Ok(name)
            }
            _ => Err(self.error("Expected property name")),
        }
    }
    fn semicolon(&mut self) -> Result<(), JsError> {
        if self.eat(";")
            || self.is("}")
            || matches!(self.token().kind, Kind::End)
            || self.token().newline
        {
            Ok(())
        } else {
            Err(self.error("Expected semicolon or line break"))
        }
    }
    fn count(&mut self) -> Result<(), JsError> {
        self.visits += 1;
        if self.visits > MAX_PARSER_VISITS {
            let mut error = JsError::resource("Parser visit budget exceeded");
            error.add_diagnostic_context(&format!(
                "parser visits={} limit={}",
                self.visits, MAX_PARSER_VISITS,
            ));
            Err(error)
        } else {
            Ok(())
        }
    }
    fn statement(&mut self) -> Result<Stmt, JsError> {
        self.count()?;
        if self.depth >= 64 {
            return Err(JsError::resource("Syntax nesting budget exceeded"));
        }
        self.depth += 1;
        let result = self.statement_inner();
        self.depth -= 1;
        result
    }
    fn statement_inner(&mut self) -> Result<Stmt, JsError> {
        if self.statement_starts_label() {
            return self.statement_label();
        }
        if self.eat(";") {
            return Ok(Stmt::Empty);
        }
        if self.eat("{") {
            return Ok(Stmt::Block(self.block()?));
        }
        if self.is("let") || self.is("const") || self.is("var") {
            return self.statement_variables();
        }
        if let Some(kind) = self.function_prefix()? {
            return self.statement_function(kind);
        }
        if self.eat("class") {
            return self.statement_class();
        }
        if self.eat("if") {
            return self.statement_if();
        }
        if self.eat("while") {
            return self.statement_while();
        }
        if self.eat("do") {
            return self.statement_do_while();
        }
        if self.eat("try") {
            return self.statement_try();
        }
        if self.eat("switch") {
            return self.statement_switch();
        }
        if self.eat("for") {
            return self.statement_for();
        }
        if self.eat("return") {
            return self.statement_return();
        }
        if self.eat("throw") {
            return self.statement_throw();
        }
        if self.eat("break") {
            return self.statement_break();
        }
        if self.eat("continue") {
            return self.statement_continue();
        }
        let e = self.expression(0)?;
        self.semicolon()?;
        Ok(Stmt::Expression(e))
    }
    fn block(&mut self) -> Result<Vec<Stmt>, JsError> {
        let mut result = Vec::new();
        while !self.eat("}") {
            if matches!(self.token().kind, Kind::End) {
                return Err(self.error("Unclosed block"));
            }
            result.push(self.statement()?);
        }
        Ok(result)
    }
    fn variables(&mut self) -> Result<Stmt, JsError> {
        let kind = if self.eat("const") {
            Declaration::Const
        } else if self.eat("let") {
            Declaration::Let
        } else {
            self.require("var")?;
            Declaration::Var
        };
        let first = self.pattern(false)?;
        self.variables_after_first(kind, first)
    }
    fn variables_after_first(
        &mut self,
        kind: Declaration,
        first: Pattern,
    ) -> Result<Stmt, JsError> {
        let mut declarations = Vec::new();
        let mut name = first;
        loop {
            let initial = if self.eat("=") {
                Some(self.expression(1)?)
            } else {
                None
            };
            if (kind == Declaration::Const || !matches!(name, Pattern::Name(_)))
                && initial.is_none()
            {
                return Err(
                    self.error("Const or destructuring declaration requires an initializer")
                );
            }
            declarations.push((name, initial));
            if !self.eat(",") {
                break;
            }
            name = self.pattern(false)?;
        }
        if declarations
            .iter()
            .all(|(pattern, _)| matches!(pattern, Pattern::Name(_)))
        {
            Ok(Stmt::Variables(
                kind,
                declarations
                    .into_iter()
                    .map(|(pattern, initial)| {
                        let Pattern::Name(name) = pattern else {
                            unreachable!()
                        };
                        (name, initial)
                    })
                    .collect(),
            ))
        } else {
            Ok(Stmt::PatternVariables(kind, declarations))
        }
    }
    fn contains_var_declaration(&mut self, statement: &Stmt, name: &str) -> Result<bool, JsError> {
        self.count()?;
        Ok(match statement {
            Stmt::Variables(Declaration::Var, values) => {
                values.iter().any(|(declared, _)| declared == name)
            }
            Stmt::PatternVariables(Declaration::Var, values) => {
                values.iter().any(|(pattern, _)| {
                    let mut names = Vec::new();
                    pattern.names(&mut names);
                    names.iter().any(|declared| declared == name)
                })
            }
            Stmt::Block(body) => {
                let mut found = false;
                for child in body {
                    if self.contains_var_declaration(child, name)? {
                        found = true;
                        break;
                    }
                }
                found
            }
            Stmt::If(_, yes, no) => {
                self.contains_var_declaration(yes, name)?
                    || match no {
                        Some(no) => self.contains_var_declaration(no, name)?,
                        None => false,
                    }
            }
            Stmt::IfChain(chain) => {
                let mut found = false;
                for (index, (_, body)) in chain.clauses().iter().enumerate() {
                    // The first If was counted on entry. Later virtual Ifs
                    // are visited only after the preceding then subtree.
                    if index != 0 {
                        self.count()?;
                    }
                    if self.contains_var_declaration(body, name)? {
                        found = true;
                        break;
                    }
                }
                found
                    || match chain.fallback() {
                        Some(body) => self.contains_var_declaration(body, name)?,
                        None => false,
                    }
            }
            Stmt::While(_, body) | Stmt::DoWhile(body, _) | Stmt::Label(_, body) => {
                self.contains_var_declaration(body, name)?
            }
            Stmt::For(initial, _, _, body) => {
                (match initial {
                    Some(initial) => self.contains_var_declaration(initial, name)?,
                    None => false,
                }) || self.contains_var_declaration(body, name)?
            }
            Stmt::ForIn(binding, _, body)
            | Stmt::ForOf(binding, _, body)
            | Stmt::ForAwaitOf(binding, _, body) => {
                matches!(binding,ForBinding::Declaration(Declaration::Var,declared) if declared==name)
                    || matches!(binding,ForBinding::Pattern(Declaration::Var,pattern)if {let mut names=Vec::new();pattern.names(&mut names);names.iter().any(|declared|declared==name)})
                    || self.contains_var_declaration(body, name)?
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                let mut found = false;
                for child in body
                    .iter()
                    .chain(catch.iter().flat_map(|(_, body)| body))
                    .chain(finally.iter().flatten())
                {
                    if self.contains_var_declaration(child, name)? {
                        found = true;
                        break;
                    }
                }
                found
            }
            Stmt::Switch(_, clauses) => {
                let mut found = false;
                for child in clauses.iter().flat_map(|(_, body)| body) {
                    if self.contains_var_declaration(child, name)? {
                        found = true;
                        break;
                    }
                }
                found
            }
            // Declarations inside a nested function belong to another scope.
            _ => false,
        })
    }
    fn pattern(&mut self, defaults: bool) -> Result<Pattern, JsError> {
        self.count()?;
        if self.depth >= 64 {
            return Err(JsError::resource("Binding pattern nesting exceeded"));
        }
        self.depth += 1;
        let result = (|| {
            let pattern = if self.eat("[") {
                let mut values = Vec::new();
                let mut rest = None;
                while !self.eat("]") {
                    if self.eat(",") {
                        values.push(None);
                        if values.len() > 128 {
                            return Err(JsError::resource("Binding pattern width exceeded"));
                        }
                        continue;
                    }
                    if self.eat("...") {
                        rest = Some(Box::new(self.pattern(false)?));
                        self.require("]")?;
                        break;
                    }
                    values.push(Some(self.pattern(true)?));
                    if values.len() > 128 {
                        return Err(JsError::resource("Binding pattern width exceeded"));
                    }
                    if self.eat("]") {
                        break;
                    }
                    self.require(",")?;
                }
                Pattern::Array(values, rest)
            } else if self.eat("{") {
                let mut values = Vec::new();
                let mut rest = None;
                while !self.eat("}") {
                    if self.eat("...") {
                        rest = Some(Box::new(Pattern::Name(self.name()?)));
                        self.require("}")?;
                        break;
                    }
                    let name = self.property_name()?;
                    let pattern = if self.eat(":") {
                        self.pattern(true)?
                    } else {
                        let pattern = Pattern::Name(name.clone());
                        if self.eat("=") {
                            Pattern::Default(Box::new(pattern), Box::new(self.expression(1)?))
                        } else {
                            pattern
                        }
                    };
                    values.push((name, pattern));
                    if values.len() > 128 {
                        return Err(JsError::resource("Binding pattern width exceeded"));
                    }
                    if self.eat("}") {
                        break;
                    }
                    self.require(",")?;
                }
                Pattern::Object(values, rest)
            } else {
                Pattern::Name(self.name()?)
            };
            if defaults && self.eat("=") {
                Ok(Pattern::Default(
                    Box::new(pattern),
                    Box::new(self.expression(1)?),
                ))
            } else {
                Ok(pattern)
            }
        })();
        self.depth -= 1;
        result
    }
    fn parameters(&mut self, end: bool) -> Result<Vec<Parameter>, JsError> {
        let outer_kind = std::mem::replace(&mut self.function_kind, FunctionKind::Ordinary);
        let result = self.parameters_inner(end);
        self.function_kind = outer_kind;
        result
    }
    fn parameters_inner(&mut self, end: bool) -> Result<Vec<Parameter>, JsError> {
        let mut result = Vec::new();
        let mut names = std::collections::BTreeSet::new();
        if (end && matches!(self.token().kind, Kind::End)) || (!end && self.eat(")")) {
            return Ok(result);
        }
        loop {
            if result.len() >= crate::call_storage::MAX_FUNCTION_PARAMETERS {
                return Err(JsError::resource("Function parameter limit exceeded"));
            }
            let rest = self.eat("...");
            let pattern = self.pattern(!rest)?;
            pattern.visit_names(&mut |name| {
                if matches!(name, "await" | "yield") {
                    return Err(self.error("Suspension keywords are not parameter bindings"));
                }
                if !names.insert(name.to_owned()) {
                    return Err(self.error("Duplicate parameter binding"));
                }
                Ok(())
            })?;
            result.push(Parameter { pattern, rest });
            if (end && matches!(self.token().kind, Kind::End)) || (!end && self.eat(")")) {
                break;
            }
            if rest {
                return Err(self.error("Rest parameter must be last"));
            }
            self.require(",")?;
            if !end && self.eat(")") {
                break;
            }
        }
        Ok(result)
    }
    fn object_method_has_super(tokens: &[Token], depth: usize) -> Result<bool, JsError> {
        if depth > 64 {
            return Err(JsError::resource("Object method template nesting exceeded"));
        }
        for token in tokens {
            match &token.kind {
                Kind::Name(name) if name == "super" => return Ok(true),
                Kind::Template(parts) => {
                    for (_, source) in parts {
                        if let Some(source) = source
                            && Self::object_method_has_super(
                                &lex(source).map_err(JsError::without_input_source_offset)?,
                                depth + 1,
                            )
                            .map_err(JsError::without_input_source_offset)?
                        {
                            return Ok(true);
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(false)
    }
    fn method_kind(&mut self) -> Result<FunctionKind, JsError> {
        if self.is("async") && self.tokens.get(self.index + 1).is_some_and(|token| {
            !matches!(&token.kind, Kind::Symbol(symbol) if matches!(*symbol, "(" | ":" | "," | "=" | ";" | "}"))
        }) {
            self.index += 1;
            if self.token().newline {
                return Err(self.error("Line break after async method modifier is forbidden"));
            }
            return Ok(if self.eat("*") {
                FunctionKind::AsyncGenerator
            } else {
                FunctionKind::Async
            });
        }
        Ok(if self.eat("*") {
            FunctionKind::Generator
        } else {
            FunctionKind::Ordinary
        })
    }
    fn class_body(&mut self, name: Option<String>) -> Result<ClassDefinition, JsError> {
        let base = if self.eat("extends") {
            Some(Box::new(self.expression(16)?))
        } else {
            None
        };
        self.require("{")?;
        let mut members = Vec::new();
        let mut constructor = false;
        let mut private = std::collections::BTreeMap::<String, (u8, bool)>::new();
        while !self.eat("}") {
            if self.eat(";") {
                continue;
            }
            if matches!(self.token().kind, Kind::End) {
                return Err(self.error("Unclosed class"));
            }
            let static_member=self.is("static")&&!self.tokens.get(self.index+1).is_some_and(|token|matches!(&token.kind,Kind::Symbol(symbol)if matches!(*symbol,"("|"="|";"|"}")));
            if static_member {
                self.index += 1;
            }
            let kind = self.method_kind()?;
            let mut private_member = self.eat("#");
            let mut key = self.property_name()?;
            let mut accessor = None;
            if kind == FunctionKind::Ordinary
                && !private_member
                && matches!(key.as_str(), "get" | "set")
                && !self.is("(")
                && !self.is("=")
                && !self.is(";")
                && !self.is("}")
            {
                accessor = Some(key == "get");
                private_member = self.eat("#");
                key = self.property_name()?;
            }
            let method = self.is("(");
            if kind != FunctionKind::Ordinary && !method {
                return Err(self
                    .error("Async/generator modifier requires an ordinary method name and body"));
            }
            if private_member {
                if key == "constructor" {
                    return Err(self.error("Invalid private constructor name"));
                }
                let bit = match accessor {
                    Some(true) => 1,
                    Some(false) => 2,
                    None => 4,
                };
                let (seen, is_static) = private.entry(key.clone()).or_insert((0, static_member));
                if *is_static != static_member
                    || *seen & bit != 0
                    || (*seen != 0 && (bit == 4 || *seen & 4 != 0))
                {
                    return Err(self.error("Duplicate private name"));
                }
                *seen |= bit;
            }
            if static_member && key == "prototype" {
                return Err(self.error("Static prototype member is forbidden"));
            }
            if !static_member && !private_member && key == "constructor" {
                if kind != FunctionKind::Ordinary || !method || accessor.is_some() || constructor {
                    return Err(self.error("Invalid or duplicate constructor"));
                }
                constructor = true;
            }
            let value = if method {
                let (params, body, parameters) = self.function_body(kind)?;
                if accessor == Some(true) && !parameters.is_empty() {
                    return Err(self.error("Getter must have no parameters"));
                }
                if accessor == Some(false) && (parameters.len() != 1 || parameters[0].rest) {
                    return Err(self.error("Setter requires one ordinary parameter"));
                }
                Some(Expr::Function {
                    kind,
                    method: true,
                    name: None,
                    params,
                    body,
                    arrow: false,
                    parameters: Some(parameters),
                })
            } else {
                if accessor.is_some() {
                    return Err(self.error("Accessor requires a method body"));
                }
                let value = if self.eat("=") {
                    Some(self.expression(1)?)
                } else {
                    None
                };
                self.semicolon()?;
                value
            };
            members.push(ClassMember {
                name: key,
                value,
                static_member,
                private: private_member,
                method,
                accessor,
            });
            if members.len() > 128 {
                return Err(JsError::resource("Class member count exceeded"));
            }
        }
        Ok(ClassDefinition {
            name,
            base,
            members,
        })
    }
    fn function_body(&mut self, kind: FunctionKind) -> Result<FunctionBody, JsError> {
        self.require("(")?;
        let parameters = self.parameters(false)?;
        let params = parameter_length_names(&parameters);
        self.require("{")?;
        let outer_kind = std::mem::replace(&mut self.function_kind, kind);
        self.function_depth += 1;
        let outer_loop = self.loop_depth;
        let outer_switch = self.switch_depth;
        let outer_labels = std::mem::take(&mut self.labels);
        self.loop_depth = 0;
        self.switch_depth = 0;
        let body = self.block();
        self.loop_depth = outer_loop;
        self.switch_depth = outer_switch;
        self.labels = outer_labels;
        self.function_depth -= 1;
        self.function_kind = outer_kind;
        let body = body?;
        crate::resumable::validate_body(&body, kind)?;
        Ok((Arc::new(params), Arc::new(body), Arc::new(parameters)))
    }
    fn arrow(&mut self, params: Vec<String>, kind: FunctionKind) -> Result<Expr, JsError> {
        if params.len() > crate::call_storage::MAX_FUNCTION_PARAMETERS {
            return Err(JsError::resource("Function parameter limit exceeded"));
        }
        let outer_kind = std::mem::replace(&mut self.function_kind, kind);
        self.function_depth += 1;
        let outer_loop = self.loop_depth;
        let outer_switch = self.switch_depth;
        let outer_labels = std::mem::take(&mut self.labels);
        self.loop_depth = 0;
        self.switch_depth = 0;
        let body = if self.eat("{") {
            self.block()
        } else {
            self.expression(1)
                .map(|value| vec![Stmt::Return(Some(value))])
        };
        self.function_depth -= 1;
        self.function_kind = outer_kind;
        self.loop_depth = outer_loop;
        self.switch_depth = outer_switch;
        self.labels = outer_labels;
        let body = body?;
        crate::resumable::validate_body(&body, kind)?;
        Ok(Expr::Function {
            kind,
            method: false,
            name: None,
            params: Arc::new(params),
            body: Arc::new(body),
            arrow: true,
            parameters: None,
        })
    }
    fn expression(&mut self, min: u8) -> Result<Expr, JsError> {
        self.expression_goal(pratt::Goal::Expression(min))
            .map(|parsed| parsed.value)
    }
    fn expression_goal(&mut self, goal: pratt::Goal) -> Result<pratt::Parsed, JsError> {
        self.count()?;
        if self.depth >= 64 || self.expression_frames >= pratt::MAX_EXPRESSION_FRAMES {
            return Err(JsError::resource("Expression nesting budget exceeded"));
        }
        self.depth += 1;
        self.expression_frames += 1;
        let result = self.expression_pratt(goal);
        self.expression_frames -= 1;
        self.depth -= 1;
        result
    }
    // Keep template subparser temporaries out of every recursive atom frame.
    // This preserves the same nesting budget on small native thread stacks.
    #[inline(never)]
    fn template_literal(&mut self, parts: TemplateParts) -> Result<Expr, JsError> {
        let mut output = Vec::new();
        for (text, source) in parts {
            output.push(Expr::Literal(Value::string(text)));
            if let Some(source) = source {
                let mut nested = Parser {
                    tokens: lex(&source).map_err(JsError::without_input_source_offset)?,
                    index: 0,
                    depth: self.depth,
                    expression_frames: self.expression_frames,
                    function_depth: self.function_depth,
                    function_kind: self.function_kind,
                    module_context: self.module_context,
                    loop_depth: 0,
                    switch_depth: 0,
                    labels: Vec::new(),
                    visits: self.visits,
                    group_visits: self.group_visits,
                    flat_storage_bytes: self.flat_storage_bytes,
                };
                let expression = nested.expression(0);
                // Failed template probes still consumed parser work. Never
                // refund visits just because a later Syntax error is caught.
                self.visits = nested.visits;
                self.group_visits = nested.group_visits;
                self.flat_storage_bytes = nested.flat_storage_bytes;
                let expr = expression.map_err(JsError::without_input_source_offset)?;
                if !matches!(nested.token().kind, Kind::End) {
                    return Err(nested
                        .error("Unexpected template interpolation token")
                        .without_input_source_offset());
                }
                output.push(expr);
            }
        }
        Ok(Expr::Template(output))
    }
    // Grouping must not carry unrelated literal/constructor temporaries on
    // every recursive expression frame; keep the existing depth check intact.
    fn atom(&mut self) -> Result<Expr, JsError> {
        if self.eat("(") {
            // The lexer already matched delimiters in a linear, allocation-free
            // pass. Do not build and discard parameter patterns for every group.
            let close = self.tokens[self.index - 1].close_paren;
            let arrow = close != NO_TOKEN
                && self.tokens.get(close as usize + 1).is_some_and(|token| {
                    !token.newline && matches!(token.kind, Kind::Symbol("=>"))
                });
            if arrow {
                let parameters = self.parameters(false)?;
                self.require("=>")?;
                let params = parameter_length_names(&parameters);
                let mut function = self.arrow(params, FunctionKind::Ordinary)?;
                if let Expr::Function {
                    parameters: stored, ..
                } = &mut function
                {
                    *stored = Some(Arc::new(parameters));
                }
                return Ok(function);
            }
            let e = self.expression(0)?;
            self.require(")")?;
            return Ok(e);
        }
        self.atom_other()
    }
    #[inline(never)]
    fn atom_other(&mut self) -> Result<Expr, JsError> {
        if self.eat("import") {
            return self.atom_import();
        }
        if self.eat("new") {
            return self.atom_construct();
        }
        if let Some(kind) = self.function_prefix()? {
            return self.atom_function(kind);
        }
        if self.eat("async") {
            return self.atom_async_arrow();
        }
        if self.eat("[") {
            return self.atom_array();
        }
        if self.eat("{") {
            return self.atom_object();
        }
        let token = &self.tokens[self.index].kind;
        self.index += 1;
        match token {
            Kind::Number(n) => Ok(Expr::Literal(Value::Number(*n))),
            Kind::BigInt(n) => Ok(Expr::Literal(Value::BigInt(n.clone()))),
            Kind::String(s) => Ok(Expr::Literal(Value::string(s.clone()))),
            Kind::Template(parts) => {
                let parts = parts.clone();
                self.template_literal(parts)
            }
            Kind::RegExp(literal) => {
                Ok(Expr::RegExp(literal.pattern.clone(), literal.flags.clone()))
            }
            Kind::Name(name) => {
                let name = name.clone();
                if name == "class" {
                    let name = if self.is("{") || self.is("extends") {
                        None
                    } else {
                        Some(self.name()?)
                    };
                    return self.class_body(name).map(Expr::Class);
                }
                if name == "super" {
                    return Ok(Expr::Super);
                }
                if matches!(
                    name.as_str(),
                    "class"
                        | "import"
                        | "export"
                        | "async"
                        | "await"
                        | "new"
                        | "yield"
                        | "delete"
                        | "try"
                        | "switch"
                        | "with"
                ) {
                    return Err(JsError::syntax(
                        self.tokens[self.index - 1].offset,
                        format!("Unsupported syntax {name}"),
                    ));
                }
                if !self.token().newline && self.eat("=>") {
                    return self.arrow(vec![name], FunctionKind::Ordinary);
                }
                Ok(match name.as_str() {
                    "true" => Expr::Literal(Value::Bool(true)),
                    "false" => Expr::Literal(Value::Bool(false)),
                    "null" => Expr::Literal(Value::Null),
                    "undefined" => Expr::Literal(Value::Undefined),
                    _ => Expr::Name(name),
                })
            }
            _ => {
                self.index -= 1;
                Err(self.error("Expected expression"))
            }
        }
    }
}

#[cfg(test)]
mod resumable_syntax_tests {
    use super::*;

    #[test]
    fn classical_for_yield_probe_keeps_resource_errors_fatal() {
        assert!(parse("function* f(){for(yield 1;yield 2;yield 3){yield 4;}}").is_ok());
        let expression = format!("{}1", "!".repeat(70));
        let error = parse(&format!("function* f(){{for({expression};;){{}}}}")).unwrap_err();
        assert_eq!(error.kind, crate::ErrorKind::Resource);
        assert!(parse("function* f(){for(yield 1 of []){}}").is_err());
    }
    #[test]
    fn parses_for_await_only_in_async_function_contexts() {
        let statements =
            parse("async function f() { for await (const x of source) { await x; } }").unwrap();
        let Stmt::FunctionDefinition(_, Expr::Function { body, .. }) = &statements[0] else {
            panic!("expected async function");
        };
        assert!(matches!(
            &body[0],
            Stmt::ForAwaitOf(ForBinding::Declaration(Declaration::Const, _), _, _)
        ));
        for source in [
            "let f = async () => { for await (let x of source) {} };",
            "let o = { async f() { for await (var x of source) {} } };",
            "class C { async f() { for await (obj.x of source) {} } }",
            "async function f() { for await (const [x] of source) {} }",
            "async function f() { for await (let {x} of source) {} }",
            "async function f() { for await (x of (a, b)) {} }",
            "async function f() { for (x of source) {} for (x in source) {} }",
        ] {
            assert!(parse(source).is_ok(), "rejected {source}");
        }
        assert!(
            parse_module("export async function f() { for await (const x of source) {} }").is_ok()
        );
    }

    #[test]
    fn rejects_invalid_for_await_grammar_and_nested_sync_contexts() {
        let labeled = "async function f() { outer: for await (x of source) { continue outer; } }";
        let error = parse(labeled).unwrap_err();
        assert_eq!(error.kind, crate::ErrorKind::Syntax);
        assert!(error.message.starts_with("Unsupported suspension position"));
        for source in [
            "for await (x of source) {}",
            "function f() { for await (x of source) {} }",
            "function* f() { for await (x of source) {} }",
            "async function f() { function g() { for await (x of source) {} } }",
            "async function f() { let g = () => { for await (x of source) {} }; }",
            "async function f() { for await (x in source) {} }",
            "async function f() { for await (let x in source) {} }",
            "async function f() { for await (;;) {} }",
            "async function f() { for await (let x = 0; x < 2; x++) {} }",
            "async function f() { for await (let x = 0 of source) {} }",
            "async function f() { for await (const x, y of source) {} }",
            "async function f() { for await (work() of source) {} }",
            "async function f() { for await (x of a, b) {} }",
            "async function f() { for await (let x of source) { var x; } }",
            "async function f() { for (let x of source) { for await (var x of source) {} } }",
        ] {
            assert!(parse(source).is_err(), "accepted {source}");
        }
        assert!(parse_module("for await (const x of source) {}").is_err());
        assert!(parse_dynamic_function("", "for await (x of source) {}").is_err());
    }

    #[test]
    fn parses_resumable_function_kinds_and_newline_yield() {
        let body = parse("async function a() { let x = await work(); return x; } function* g() { yield\n42; return 3; }").unwrap();
        assert!(matches!(
            &body[0],
            Stmt::FunctionDefinition(
                _,
                Expr::Function {
                    kind: FunctionKind::Async,
                    ..
                }
            )
        ));
        let Stmt::FunctionDefinition(
            _,
            Expr::Function {
                kind: FunctionKind::Generator,
                body,
                ..
            },
        ) = &body[1]
        else {
            panic!("expected generator");
        };
        assert!(matches!(&body[0], Stmt::Expression(Expr::Yield(None))));
        assert!(matches!(
            &body[1],
            Stmt::Expression(Expr::Literal(Value::Number(42.0)))
        ));
        assert!(parse("let a = async function() { return await work(); }; let g = function*() { yield /x/; };").is_ok());
        assert!(parse("async function a() {} /x/.test('x');").is_ok());
    }

    #[test]
    fn rejects_unsupported_and_cross_scope_suspensions() {
        for source in [
            "await work();",
            "yield 1;",
            "async\nfunction a() {}",
            "function* g() { yield*; }",
            "async function a(x = await work()) {}",
            "function* g(x = yield 1) {}",
            "async function a() { function b() { await work(); } }",
            "function* g() { let b = () => yield 1; }",
            "async function a() { let b = (x = await work()) => x; }",
            "async function a() { let b = (await) => 1; }",
            "async function a() { let b = ({await}) => 1; }",
            "let a = (x, x) => 1;",
            "async function a(x, x) {}",
            "function* g(x, x) {}",
        ] {
            assert!(parse(source).is_err(), "accepted {source}");
        }
        assert!(
            parse("async function a() { function b() { return 1; } return await work(); }").is_ok()
        );
        assert!(
            parse("function* g() { let f = async function() { return await work(); }; yield 1; }")
                .is_ok()
        );
    }

    #[test]
    fn module_resumable_exports_remain_hoistable() {
        for source in [
            "export async function a() { return await work(); }",
            "export function* g() { yield 1; }",
            "export default async function() { return await work(); }",
            "export default function*() { yield 1; }",
        ] {
            let module = parse_module(source).unwrap();
            assert!(matches!(&module.body[0], Stmt::FunctionDefinition(_, _)));
            assert_eq!(module.exports.len(), 1);
        }
        assert!(parse_module("export default async\nfunction a() {}").is_err());
        assert!(parse_module("await work();").is_err());
    }
}

#[cfg(test)]
mod async_arrow_syntax_tests {
    use super::*;

    #[test]
    fn parses_async_arrows_with_lexical_function_shape() {
        for source in [
            "let f = async x => await x;",
            "let f = async () => { return await work(); };",
            "let f = async (x = 1, ...rest) => { let y = await x; return this; };",
            "let f = async ({x}, [y]) => await x;",
            "async x => await x;",
        ] {
            let body = parse(source).unwrap();
            let function = match &body[0] {
                Stmt::Variables(_, values) => values[0].1.as_ref().unwrap(),
                Stmt::Expression(function) => function,
                _ => panic!("expected arrow"),
            };
            assert!(matches!(
                function,
                Expr::Function {
                    kind: FunctionKind::Async,
                    arrow: true,
                    ..
                }
            ));
        }
        assert!(parse_module("export default async x => await x;").is_ok());
        assert!(parse("let f = async x => 1 + await x;").is_ok());
    }

    #[test]
    fn rejects_async_arrow_parameter_and_newline_errors() {
        for source in [
            "let f = async\nx => x;",
            "let f = async\n(x) => x;",
            "let f = async x\n=> x;",
            "let f = async (x)\n=> x;",
            "let f = async (x = await work()) => x;",
            "let f = async await => 1;",
            "let f = async (x, x) => x;",
            "let f = async ({yield}) => 1;",
            "let f = async x => { let g = () => await x; };",
        ] {
            assert!(parse(source).is_err(), "accepted {source}");
        }
        assert!(parse_module("export async x => x;").is_err());
    }

    #[test]
    fn accepts_suspension_in_bounded_loop_and_handler_forms() {
        for source in [
            "function* g() { while (ready) { yield 1; } }",
            "async function f() { do { await work(); } while (ready); }",
            "let f = async () => { try { await work(); } catch (e) { await recover(); } finally { await close(); } };",
        ] {
            assert!(parse(source).is_ok(), "rejected {source}");
        }
    }
}

#[cfg(test)]
mod resumable_method_syntax_tests {
    use super::*;

    #[test]
    fn parses_object_and_class_resumable_methods() {
        for source in [
            "let o = { async f(x) { return await x; }, *g() { yield 1; }, async() { return 2; } };",
            "class C { async f(x) { return await x; } *g() { yield 1; } async() {} }",
            "class C { static async f() { await work(); } static *g() { yield 1; } }",
            "class C { async #f() { await work(); } *#g() { yield 1; } }",
            "class C extends B { async f() { return await super.f(); } *g() { yield super.f(); } }",
            "let o = { async get() { await work(); }, *set() { yield 1; } };",
        ] {
            assert!(parse(source).is_ok(), "rejected {source}");
        }
        let body =
            parse("class C { async() {} async f() { await work(); } *g() { yield 1; } }").unwrap();
        let Stmt::ClassDefinition(_, class) = &body[0] else {
            panic!("expected class");
        };
        for (member, expected) in class.members.iter().zip([
            FunctionKind::Ordinary,
            FunctionKind::Async,
            FunctionKind::Generator,
        ]) {
            assert!(
                matches!(&member.value, Some(Expr::Function { kind, method: true, arrow: false, .. }) if *kind == expected)
            );
        }
    }

    #[test]
    fn rejects_invalid_method_modifiers_and_unsupported_object_super() {
        for source in [
            "class C { async constructor() {} }",
            "class C { *constructor() {} }",
            "class C { async get x() {} }",
            "class C { get async x() {} }",
            "class C { get *x() {} }",
            "class C { *set x(v) {} }",
            "let o = { async get x() {} };",
            "let o = { *set x(v) {} };",
            "class C { async\nf() {} }",
            "let o = { async\nf() {} };",
            "class C { async f(x = await work()) {} }",
            "class C { *g(x = yield 1) {} }",
            "class C { async f(x, x) {} }",
            "let o = { *g(x, x) {} };",
            "let o = { async f() { return await super.f(); } };",
            "let o = { async f() { return `${super.f()}`; } };",
            "class C { async f = 1; }",
            "let o = { *g: value };",
        ] {
            assert!(parse(source).is_err(), "accepted {source}");
        }
    }
}

#[cfg(test)]
mod async_generator_syntax_tests {
    use super::*;

    #[test]
    fn parses_async_generator_declarations_expressions_and_newline_yield() {
        let statements = parse("async function* stream() { let x = await work(); yield x; yield\n42; for await (const item of source) { yield item; } }").unwrap();
        let Stmt::FunctionDefinition(
            _,
            Expr::Function {
                kind: FunctionKind::AsyncGenerator,
                body,
                arrow: false,
                method: false,
                ..
            },
        ) = &statements[0]
        else {
            panic!("expected async generator declaration");
        };
        assert!(matches!(&body[0], Stmt::Variables(_, _)));
        assert!(matches!(&body[1], Stmt::Expression(Expr::Yield(Some(_)))));
        assert!(matches!(&body[2], Stmt::Expression(Expr::Yield(None))));
        assert!(matches!(&body[4], Stmt::ForAwaitOf(..)));
        for source in [
            "let f = async function*() { await work(); yield 1; };",
            "let f = async function* named() { yield 1; return 2; };",
            "async function* f() { yield await work(); return await finish(); }",
            "async function* f() { await (yield 1); }",
            "async function* f() {} /x/.test('x');",
            "async function* f() { function* g() { yield* source; } yield 1; }",
            "async function* f() { async function g() { await work(); } yield 1; }",
        ] {
            assert!(parse(source).is_ok(), "rejected {source}");
        }
    }

    #[test]
    fn parses_async_generator_methods_without_changing_async_named_methods() {
        for source in [
            "let o = { async *f() { await work(); yield 1; }, async() {} };",
            "let o = { async *get() { yield 1; }, async *set(x) { yield x; } };",
            "class C { async *f() { await work(); yield 1; } async() {} }",
            "class C { static async *f() { yield 1; } async *#g() { yield 2; } static async *#h() { yield 3; } }",
            "class C extends B { async *f() { let x = await super.f(); yield x; } }",
        ] {
            assert!(parse(source).is_ok(), "rejected {source}");
        }
        let statements = parse("class C { async() {} async *f() { yield 1; } }").unwrap();
        let Stmt::ClassDefinition(_, definition) = &statements[0] else {
            panic!("expected class");
        };
        for (member, expected) in definition
            .members
            .iter()
            .zip([FunctionKind::Ordinary, FunctionKind::AsyncGenerator])
        {
            assert!(matches!(&member.value,
                Some(Expr::Function { kind, method: true, arrow: false, .. }) if *kind == expected));
        }
    }

    #[test]
    fn rejects_invalid_async_generator_contexts_and_modifiers() {
        for source in [
            "async\nfunction* f() {}",
            "class C { async\n*f() {} }",
            "let o = { async\n*f() {} };",
            "class C { async *constructor() {} }",
            "class C { async *get x() {} }",
            "class C { get async *x() {} }",
            "class C { async *set x(v) {} }",
            "let o = { async *get x() {} };",
            "let o = { async *set x(v) {} };",
            "class C { async *f = 1; }",
            "let o = { async *f: 1 };",
            "async function* f(x = await work()) {}",
            "async function* f(x = yield 1) {}",
            "async function* f(await) {}",
            "async function* f({yield}) {}",
            "async function* f(x, x) {}",
            "class C { async *f(x = await work()) {} }",
            "let o = { async *f(x = yield 1) {} };",
            "async function* f() { function g() { await work(); } }",
            "async function* f() { function g() { yield 1; } }",
            "async function* f() { let g = () => yield 1; }",
            "async function* f() { let g = () => await work(); }",
            "async function* f() { let g = (x = yield 1) => x; }",
            "async function* f() { function g() { for await (x of source) {} } }",
            "async function* f() { let o = { g() { yield 1; } }; }",
            "async function* f() { class C { g() { await work(); } } }",
            "async function* f() { async function g() { yield 1; } }",
            "async function* f() { function* g() { await work(); } }",
            "let o = { async *f() { yield super.f(); } };",
        ] {
            assert!(parse(source).is_err(), "accepted {source}");
        }
    }

    #[test]
    fn accepts_async_delegation_only_in_generator_bodies() {
        for source in [
            "async function* f() { yield* source; }",
            "async function* f() { yield* await source; }",
            "let o = { async *f() { yield* source; } };",
            "class C { async *f() { yield* source; } }",
        ] {
            assert!(parse(source).is_ok(), "rejected {source}");
        }
        for source in [
            "async function* f() { yield*; }",
            "async function f() { yield* source; }",
            "async function* f(x = yield* source) {}",
            "async function* f() { function nested() { yield* source; } }",
        ] {
            assert!(parse(source).is_err(), "accepted {source}");
        }
        assert!(parse("function* f() { yield* source; }").is_ok());
    }
}
