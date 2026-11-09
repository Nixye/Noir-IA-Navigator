use crate::{JsError, Value};
use std::sync::Arc;

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
    Assign(String, Box<Expr>, Box<Expr>),
    Update(Box<Expr>, bool, bool),
    Conditional(Box<Expr>, Box<Expr>, Box<Expr>),
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

#[derive(Debug, Clone, PartialEq)]
enum Kind {
    Name(String),
    Number(f64),
    BigInt(Arc<crate::BigInt>),
    String(String),
    Template(TemplateParts),
    RegExp(String, String),
    Symbol(String),
    End,
}
#[derive(Debug, Clone)]
struct Token {
    kind: Kind,
    offset: usize,
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
        function_depth: 0,
        function_kind: FunctionKind::Ordinary,
        module_context: false,
        loop_depth: 0,
        switch_depth: 0,
        labels: Vec::new(),
        nodes: 0,
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
        function_depth: 0,
        function_kind: FunctionKind::Ordinary,
        module_context: false,
        loop_depth: 0,
        switch_depth: 0,
        labels: Vec::new(),
        nodes: 0,
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
            Kind::RegExp(pattern, flags)
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
                Kind::Symbol(op.into())
            } else if "{}[]().,;:?# +-*/%!=<>&|^~".contains(ch) {
                i += ch.len_utf8();
                Kind::Symbol(ch.to_string())
            } else {
                return Err(JsError::syntax(i, format!("Unsupported token {ch:?}")));
            }
        };
        context.update(&kind, result.last());
        result.push(Token {
            kind,
            offset: start,
            newline,
        });
        newline = false;
        if result.len() > 32_768 {
            return Err(JsError::resource("Script token budget exceeded"));
        }
    }
    result.push(Token {
        kind: Kind::End,
        offset: source.len(),
        newline,
    });
    Ok(result)
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
            Kind::Name(text) | Kind::Symbol(text) => Some(text.as_str()),
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
            Kind::Symbol(symbol) => match symbol.as_str() {
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
    function_depth: usize,
    function_kind: FunctionKind,
    module_context: bool,
    loop_depth: usize,
    switch_depth: usize,
    labels: Vec<(String, bool)>,
    nodes: usize,
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
        matches!(&self.token().kind,Kind::Symbol(v)|Kind::Name(v) if v==s)
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
        match self.token().kind.clone() {
            Kind::Name(name) | Kind::String(name) => {
                self.index += 1;
                Ok(name)
            }
            Kind::BigInt(n) => {
                self.index += 1;
                Ok(n.text(10))
            }
            Kind::Number(n) => {
                self.index += 1;
                Ok(n.to_string())
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
        self.nodes += 1;
        if self.nodes > 16_384 {
            Err(JsError::resource("Syntax tree node budget exceeded"))
        } else {
            Ok(())
        }
    }
    fn statement(&mut self) -> Result<Stmt, JsError> {
        self.count()?;
        self.depth += 1;
        if self.depth > 64 {
            return Err(JsError::resource("Syntax nesting budget exceeded"));
        }
        let result = self.statement_inner();
        self.depth -= 1;
        result
    }
    fn statement_inner(&mut self) -> Result<Stmt, JsError> {
        if matches!(self.token().kind, Kind::Name(_))
            && self
                .tokens
                .get(self.index + 1)
                .is_some_and(|token| matches!(&token.kind, Kind::Symbol(s) if s == ":"))
        {
            let name = self.name()?;
            self.require(":")?;
            if self.labels.iter().any(|(label, _)| label == &name) {
                return Err(self.error("Duplicate active label"));
            }
            let mut next = self.index;
            while matches!(self.tokens.get(next).map(|t| &t.kind), Some(Kind::Name(_)))
                && self
                    .tokens
                    .get(next + 1)
                    .is_some_and(|t| matches!(&t.kind, Kind::Symbol(s) if s == ":"))
            {
                next += 2;
            }
            let iteration = matches!(self.tokens.get(next).map(|t| &t.kind), Some(Kind::Name(s)) if matches!(s.as_str(), "for" | "while" | "do"));
            self.labels.push((name.clone(), iteration));
            let body = self.statement()?;
            self.labels.pop();
            return Ok(Stmt::Label(name, Box::new(body)));
        }
        if self.eat(";") {
            return Ok(Stmt::Empty);
        }
        if self.eat("{") {
            return Ok(Stmt::Block(self.block()?));
        }
        if self.is("let") || self.is("const") || self.is("var") {
            let stmt = self.variables()?;
            self.semicolon()?;
            return Ok(stmt);
        }
        if let Some(kind) = self.function_prefix()? {
            let name = self.name()?;
            let (params, body, parameters) = self.function_body(kind)?;
            return Ok(Stmt::FunctionDefinition(
                name,
                Expr::Function {
                    kind,
                    method: false,
                    name: None,
                    params,
                    body,
                    arrow: false,
                    parameters: Some(parameters),
                },
            ));
        }
        if self.eat("class") {
            let name = self.name()?;
            let definition = self.class_body(Some(name.clone()))?;
            return Ok(Stmt::ClassDefinition(name, definition));
        }
        if self.eat("if") {
            self.require("(")?;
            let c = self.expression(0)?;
            self.require(")")?;
            let yes = Box::new(self.statement()?);
            let no = if self.eat("else") {
                Some(Box::new(self.statement()?))
            } else {
                None
            };
            return Ok(Stmt::If(c, yes, no));
        }
        if self.eat("while") {
            self.require("(")?;
            let c = self.expression(0)?;
            self.require(")")?;
            self.loop_depth += 1;
            let body = self.statement()?;
            self.loop_depth -= 1;
            return Ok(Stmt::While(c, Box::new(body)));
        }
        if self.eat("do") {
            self.loop_depth += 1;
            let body = self.statement()?;
            self.loop_depth -= 1;
            self.require("while")?;
            self.require("(")?;
            let condition = self.expression(0)?;
            self.require(")")?;
            self.eat(";");
            return Ok(Stmt::DoWhile(Box::new(body), condition));
        }
        if self.eat("try") {
            self.require("{")?;
            let body = self.block()?;
            let catch = if self.eat("catch") {
                let binding = if self.eat("(") {
                    let name = self.name()?;
                    self.require(")")?;
                    Some(name)
                } else {
                    None
                };
                self.require("{")?;
                Some((binding, self.block()?))
            } else {
                None
            };
            let finally = if self.eat("finally") {
                self.require("{")?;
                Some(self.block()?)
            } else {
                None
            };
            if catch.is_none() && finally.is_none() {
                return Err(self.error("Try requires catch or finally"));
            }
            return Ok(Stmt::Try {
                body,
                catch,
                finally,
            });
        }
        if self.eat("switch") {
            self.require("(")?;
            let discriminant = self.expression(0)?;
            self.require(")")?;
            self.require("{")?;
            self.switch_depth += 1;
            let mut clauses = Vec::new();
            let mut has_default = false;
            while !self.eat("}") {
                self.count()?;
                let test = if self.eat("case") {
                    Some(self.expression(0)?)
                } else if self.eat("default") && !has_default {
                    has_default = true;
                    None
                } else {
                    return Err(self.error("Expected case or a single default clause"));
                };
                self.require(":")?;
                let mut body = Vec::new();
                while !self.is("case") && !self.is("default") && !self.is("}") {
                    if matches!(self.token().kind, Kind::End) {
                        return Err(self.error("Unclosed switch"));
                    }
                    body.push(self.statement()?);
                }
                clauses.push((test, body));
            }
            self.switch_depth -= 1;
            return Ok(Stmt::Switch(discriminant, clauses));
        }
        if self.eat("for") {
            let for_await = self.eat("await");
            if for_await
                && !matches!(
                    self.function_kind,
                    FunctionKind::Async | FunctionKind::AsyncGenerator
                )
            {
                return Err(self.error("For-await-of requires an async function"));
            }
            self.require("(")?;
            // Recognize a single uninitialized declaration/reference before
            // the relational parser consumes `in`. The fallback is the usual
            // three-clause loop; speculative syntax work still counts.
            let saved = self.index;
            let mut for_of = false;
            let binding = if self.is("var") || self.is("let") || self.is("const") {
                let kind = if self.eat("var") {
                    Declaration::Var
                } else if self.eat("let") {
                    Declaration::Let
                } else {
                    self.require("const")?;
                    Declaration::Const
                };
                let pattern = self.pattern(false)?;
                let found = if self.eat("in") {
                    true
                } else if self.eat("of") {
                    for_of = true;
                    true
                } else {
                    false
                };
                found.then_some(match pattern {
                    Pattern::Name(name) => ForBinding::Declaration(kind, name),
                    pattern => ForBinding::Pattern(kind, pattern),
                })
            } else if !self.is(";") {
                // A bare yield is a valid classical-for initializer but not a
                // candidate for an in/of reference. Probe without committing
                // syntax errors; resource failures must remain fatal.
                let bare_yield = self.is("yield");
                let target = match self.expression(10) {
                    Ok(target) => Some(target),
                    Err(error) if bare_yield && error.kind == crate::ErrorKind::Syntax => None,
                    Err(error) => return Err(error),
                };
                if let Some(target) = target
                    && (self.eat("in") || {
                        let found = self.eat("of");
                        for_of = found;
                        found
                    })
                {
                    if !matches!(target, Expr::Name(_) | Expr::Member(..)) {
                        return Err(self.error("Invalid for-in assignment target"));
                    }
                    Some(ForBinding::Reference(target))
                } else {
                    None
                }
            } else {
                None
            };
            if for_await && (!for_of || binding.is_none()) {
                return Err(self.error("For-await-of requires a binding followed by of"));
            }
            if let Some(binding) = binding {
                let source = self.expression(if for_await { 1 } else { 0 })?;
                self.require(")")?;
                self.loop_depth += 1;
                let body = self.statement()?;
                self.loop_depth -= 1;
                if let ForBinding::Declaration(kind, name) = &binding
                    && *kind != Declaration::Var
                    && self.contains_var_declaration(&body, name)?
                {
                    return Err(
                        self.error("For-in lexical binding conflicts with a body var declaration")
                    );
                }
                if let ForBinding::Pattern(kind, pattern) = &binding
                    && *kind != Declaration::Var
                {
                    let mut names = Vec::new();
                    pattern.names(&mut names);
                    for name in names {
                        if self.contains_var_declaration(&body, &name)? {
                            return Err(self.error("Loop lexical pattern conflicts with var"));
                        }
                    }
                }
                return Ok(if for_await {
                    Stmt::ForAwaitOf(binding, source, Box::new(body))
                } else if for_of {
                    Stmt::ForOf(binding, source, Box::new(body))
                } else {
                    Stmt::ForIn(binding, source, Box::new(body))
                });
            }
            self.index = saved;
            let initial = if self.eat(";") {
                None
            } else {
                let s = if self.is("let") || self.is("const") || self.is("var") {
                    self.variables()?
                } else {
                    Stmt::Expression(self.expression(0)?)
                };
                self.require(";")?;
                Some(Box::new(s))
            };
            let condition = if self.eat(";") {
                None
            } else {
                let e = self.expression(0)?;
                self.require(";")?;
                Some(e)
            };
            let update = if self.eat(")") {
                None
            } else {
                let e = self.expression(0)?;
                self.require(")")?;
                Some(e)
            };
            self.loop_depth += 1;
            let body = self.statement()?;
            self.loop_depth -= 1;
            return Ok(Stmt::For(initial, condition, update, Box::new(body)));
        }
        if self.eat("return") {
            if self.function_depth == 0 {
                return Err(self.error("Return outside function"));
            }
            let value = if self.token().newline
                || self.is(";")
                || self.is("}")
                || matches!(self.token().kind, Kind::End)
            {
                None
            } else {
                Some(self.expression(0)?)
            };
            self.semicolon()?;
            return Ok(Stmt::Return(value));
        }
        if self.eat("throw") {
            if self.token().newline {
                return Err(self.error("Line break after throw"));
            }
            let e = self.expression(0)?;
            self.semicolon()?;
            return Ok(Stmt::Throw(e));
        }
        if self.eat("break") {
            let label = if !self.token().newline && matches!(self.token().kind, Kind::Name(_)) {
                Some(self.name()?)
            } else {
                None
            };
            if let Some(label) = &label {
                if !self.labels.iter().any(|(name, _)| name == label) {
                    return Err(self.error("Unknown break label"));
                }
            } else if self.loop_depth == 0 && self.switch_depth == 0 {
                return Err(self.error("Break outside loop or switch"));
            }
            self.semicolon()?;
            return Ok(Stmt::Break(label));
        }
        if self.eat("continue") {
            let label = if !self.token().newline && matches!(self.token().kind, Kind::Name(_)) {
                Some(self.name()?)
            } else {
                None
            };
            if let Some(label) = &label {
                if !self
                    .labels
                    .iter()
                    .any(|(name, iteration)| name == label && *iteration)
                {
                    return Err(self.error("Continue label must target an iteration statement"));
                }
            } else if self.loop_depth == 0 {
                return Err(self.error("Continue outside loop"));
            }
            self.semicolon()?;
            return Ok(Stmt::Continue(label));
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
        let mut declarations = Vec::new();
        loop {
            let name = self.pattern(false)?;
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
        self.depth += 1;
        if self.depth > 64 {
            return Err(JsError::resource("Binding pattern nesting exceeded"));
        }
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
            let rest = self.eat("...");
            let pattern = self.pattern(!rest)?;
            let mut binding_names = Vec::new();
            pattern.names(&mut binding_names);
            for name in binding_names {
                if matches!(name.as_str(), "await" | "yield") {
                    return Err(self.error("Suspension keywords are not parameter bindings"));
                }
                if !names.insert(name) {
                    return Err(self.error("Duplicate parameter binding"));
                }
            }
            result.push(Parameter { pattern, rest });
            if result.len() > 32 {
                return Err(JsError::resource("Function parameter limit exceeded"));
            }
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
            !matches!(&token.kind, Kind::Symbol(symbol) if matches!(symbol.as_str(), "(" | ":" | "," | "=" | ";" | "}"))
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
            let static_member=self.is("static")&&!self.tokens.get(self.index+1).is_some_and(|token|matches!(&token.kind,Kind::Symbol(symbol)if matches!(symbol.as_str(),"("|"="|";"|"}")));
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
        if params.len() > 32 {
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
        self.count()?;
        self.depth += 1;
        if self.depth > 64 {
            return Err(JsError::resource("Expression nesting budget exceeded"));
        }
        let result = self.expression_inner(min);
        self.depth -= 1;
        result
    }
    fn expression_inner(&mut self, min: u8) -> Result<Expr, JsError> {
        let mut left = if self.eat("await") {
            if !matches!(
                self.function_kind,
                FunctionKind::Async | FunctionKind::AsyncGenerator
            ) {
                return Err(self.error("await is only supported in async function bodies"));
            }
            Expr::Await(Box::new(self.expression(14)?))
        } else if self.eat("yield") {
            if !matches!(
                self.function_kind,
                FunctionKind::Generator | FunctionKind::AsyncGenerator
            ) || min > 1
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
                return Ok(Expr::Yield(None));
            }
            if self.eat("*") {
                Expr::YieldDelegate(Box::new(self.expression(1)?))
            } else {
                Expr::Yield(Some(Box::new(self.expression(1)?)))
            }
        } else if self.is("!")
            || self.is("-")
            || self.is("+")
            || self.is("~")
            || self.is("typeof")
            || self.is("void")
            || self.is("delete")
        {
            let op = match self.token().kind.clone() {
                Kind::Name(v) | Kind::Symbol(v) => v,
                _ => unreachable!(),
            };
            self.index += 1;
            Expr::Unary(op, Box::new(self.expression(14)?))
        } else if self.eat("++") {
            let target = self.expression(14)?;
            if !assignment_target(&target, false) {
                return Err(self.error("Invalid update target"));
            }
            Expr::Update(Box::new(target), true, true)
        } else if self.eat("--") {
            let target = self.expression(14)?;
            if !assignment_target(&target, false) {
                return Err(self.error("Invalid update target"));
            }
            Expr::Update(Box::new(target), false, true)
        } else {
            self.atom()?
        };
        let mut chain_length = 0;
        loop {
            chain_length += 1;
            if chain_length > 64 {
                return Err(JsError::resource("Expression chain budget exceeded"));
            }
            if min <= 16 && self.eat(".") {
                if self.eat("#") {
                    let name = self.name()?;
                    left = Expr::PrivateMember(Box::new(left), name);
                    continue;
                }
                let property = self.property_name()?;
                left = Expr::Member(
                    Box::new(left),
                    Box::new(Expr::Literal(Value::string(property))),
                );
                continue;
            }
            if min <= 16 && self.eat("[") {
                let property = self.expression(0)?;
                self.require("]")?;
                left = Expr::Member(Box::new(left), Box::new(property));
                continue;
            }
            if min <= 16 && self.eat("(") {
                let mut args = Vec::new();
                if !self.eat(")") {
                    loop {
                        args.push(if self.eat("...") {
                            Expr::Spread(Box::new(self.expression(1)?))
                        } else {
                            self.expression(1)?
                        });
                        if args.len() > 32 {
                            return Err(JsError::resource("Call argument limit exceeded"));
                        }
                        if self.eat(")") {
                            break;
                        }
                        self.require(",")?;
                    }
                }
                left = Expr::Call(Box::new(left), args);
                continue;
            }
            if min <= 15 && !self.token().newline && (self.is("++") || self.is("--")) {
                let increment = self.eat("++");
                if !increment {
                    self.require("--")?;
                }
                if !assignment_target(&left, false) {
                    return Err(self.error("Invalid update target"));
                }
                left = Expr::Update(Box::new(left), increment, false);
                continue;
            }
            if min <= 2 && self.eat("?") {
                let yes = self.expression(1)?;
                self.require(":")?;
                let no = self.expression(1)?;
                left = Expr::Conditional(Box::new(left), Box::new(yes), Box::new(no));
                continue;
            }
            let op = match &self.token().kind {
                Kind::Symbol(op) => op.clone(),
                Kind::Name(op) if matches!(op.as_str(), "in" | "instanceof") => op.clone(),
                _ => break,
            };
            let precedence = match op.as_str() {
                "," => 0,
                "=" | "+=" | "-=" | "*=" | "/=" | "%=" | "**=" | "&=" | "|=" | "^=" | "<<="
                | ">>=" | ">>>=" => 1,
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
                _ => break,
            };
            if precedence < min {
                break;
            }
            self.index += 1;
            let right = self.expression(if precedence == 1 || op == "**" {
                precedence
            } else {
                precedence + 1
            })?;
            left = if precedence == 1 {
                if !assignment_target(&left, op == "=") {
                    return Err(self.error("Invalid assignment target"));
                }
                Expr::Assign(op, Box::new(left), Box::new(right))
            } else {
                Expr::Binary(op, Box::new(left), Box::new(right))
            };
        }
        Ok(left)
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
                    function_depth: self.function_depth,
                    function_kind: self.function_kind,
                    module_context: self.module_context,
                    loop_depth: 0,
                    switch_depth: 0,
                    labels: Vec::new(),
                    nodes: self.nodes,
                };
                let expr = nested
                    .expression(0)
                    .map_err(JsError::without_input_source_offset)?;
                if !matches!(nested.token().kind, Kind::End) {
                    return Err(nested
                        .error("Unexpected template interpolation token")
                        .without_input_source_offset());
                }
                self.nodes = nested.nodes;
                output.push(expr);
            }
        }
        Ok(Expr::Template(output))
    }
    // Grouping must not carry unrelated literal/constructor temporaries on
    // every recursive expression frame; keep the existing depth check intact.
    fn atom(&mut self) -> Result<Expr, JsError> {
        if self.eat("(") {
            let saved = self.index;
            let extended = self.parameters(false);
            if let Err(error) = &extended
                && error.kind == crate::ErrorKind::Resource
            {
                return Err(error.clone());
            }
            if let Ok(parameters) = extended
                && !self.token().newline
                && self.eat("=>")
            {
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
            self.index = saved;
            let e = self.expression(0)?;
            self.require(")")?;
            return Ok(e);
        }
        self.atom_other()
    }
    #[inline(never)]
    fn atom_other(&mut self) -> Result<Expr, JsError> {
        if self.eat("import") {
            if !self.module_context {
                return Err(self.error("Import expressions are only supported in module source"));
            }
            if self.eat(".") {
                if !matches!(&self.token().kind, Kind::Name(name) if name == "meta") {
                    return Err(self.error("Expected import.meta"));
                }
                self.index += 1;
                return Ok(Expr::ImportMeta);
            }
            self.require("(")?;
            let specifier = Box::new(self.expression(1)?);
            let options = if self.eat(",") && !self.is(")") {
                let options = Box::new(self.expression(1)?);
                self.eat(",");
                Some(options)
            } else {
                None
            };
            self.require(")")?;
            return Ok(Expr::ImportCall { specifier, options });
        }
        if self.eat("new") {
            if self.is("import")
                && self.tokens.get(self.index + 1).is_some_and(
                    |token| matches!(&token.kind, Kind::Symbol(symbol) if symbol == "("),
                )
            {
                return Err(self.error("Import calls cannot be constructor targets"));
            }
            self.depth += 1;
            if self.depth > 64 {
                return Err(JsError::resource("Constructor nesting budget exceeded"));
            }
            let target = self.atom();
            self.depth -= 1;
            let mut target = target?;
            let mut members = 0;
            loop {
                members += 1;
                if members > 64 {
                    return Err(JsError::resource("Constructor chain budget exceeded"));
                }
                if self.eat(".") {
                    let name = self.property_name()?;
                    target = Expr::Member(
                        Box::new(target),
                        Box::new(Expr::Literal(Value::string(name))),
                    );
                } else if self.eat("[") {
                    let property = self.expression(0)?;
                    self.require("]")?;
                    target = Expr::Member(Box::new(target), Box::new(property));
                } else {
                    break;
                }
            }
            let mut args = Vec::new();
            if self.eat("(") && !self.eat(")") {
                loop {
                    args.push(if self.eat("...") {
                        Expr::Spread(Box::new(self.expression(1)?))
                    } else {
                        self.expression(1)?
                    });
                    if args.len() > 32 {
                        return Err(JsError::resource("Constructor argument limit exceeded"));
                    }
                    if self.eat(")") {
                        break;
                    }
                    self.require(",")?;
                }
            }
            return Ok(Expr::Construct(Box::new(target), args));
        }
        if let Some(kind) = self.function_prefix()? {
            let name = if self.is("(") {
                None
            } else {
                Some(self.name()?)
            };
            let (params, body, parameters) = self.function_body(kind)?;
            return Ok(Expr::Function {
                kind,
                method: false,
                name,
                params,
                body,
                arrow: false,
                parameters: Some(parameters),
            });
        }
        if self.eat("async") {
            if self.token().newline {
                return Err(self.error("Line break after async is not supported"));
            }
            let parameters = if self.eat("(") {
                self.parameters(false)?
            } else {
                vec![Parameter {
                    pattern: Pattern::Name(self.name()?),
                    rest: false,
                }]
            };
            if self.token().newline {
                return Err(self.error("Line break before arrow is forbidden"));
            }
            self.require("=>")?;
            let params = parameter_length_names(&parameters);
            let mut function = self.arrow(params, FunctionKind::Async)?;
            if let Expr::Function {
                parameters: stored, ..
            } = &mut function
            {
                *stored = Some(Arc::new(parameters));
            }
            return Ok(function);
        }
        if self.eat("[") {
            let mut values = Vec::new();
            if !self.eat("]") {
                loop {
                    if self.eat(",") {
                        values.push(Expr::Hole);
                        if values.len() > 4096 {
                            return Err(JsError::resource("Array literal width exceeded"));
                        }
                        if self.eat("]") {
                            break;
                        }
                        continue;
                    }
                    values.push(if self.eat("...") {
                        Expr::Spread(Box::new(self.expression(1)?))
                    } else {
                        self.expression(1)?
                    });
                    if self.eat("]") {
                        break;
                    }
                    self.require(",")?;
                    if self.eat("]") {
                        break;
                    }
                }
            }
            return Ok(Expr::Array(values));
        }
        if self.eat("{") {
            let mut pairs = Vec::new();
            if !self.eat("}") {
                loop {
                    if self.eat("...") {
                        pairs.push((String::new(), Expr::Spread(Box::new(self.expression(1)?))));
                        if self.eat("}") {
                            break;
                        }
                        self.require(",")?;
                        if self.eat("}") {
                            break;
                        }
                        continue;
                    }
                    let kind = self.method_kind()?;
                    let key = self.property_name()?;
                    let value = if self.is("(") {
                        let start = self.index;
                        let (params, body, parameters) = self.function_body(kind)?;
                        // Object literal methods do not yet carry a home object.
                        // Refuse super rather than accidentally capturing an outer class home.
                        if Self::object_method_has_super(&self.tokens[start..self.index], 0)? {
                            return Err(
                                self.error("super in object literal methods is not supported")
                            );
                        }
                        Expr::Function {
                            kind,
                            method: true,
                            name: None,
                            params,
                            body,
                            arrow: false,
                            parameters: Some(parameters),
                        }
                    } else if kind != FunctionKind::Ordinary {
                        return Err(self.error("Async/generator modifier requires a method body"));
                    } else if self.eat(":") {
                        self.expression(1)?
                    } else {
                        Expr::Name(key.clone())
                    };
                    pairs.push((key, value));
                    if self.eat("}") {
                        break;
                    }
                    self.require(",")?;
                    if self.eat("}") {
                        break;
                    }
                }
            }
            return Ok(Expr::Object(pairs));
        }
        let token = self.token().kind.clone();
        self.index += 1;
        match token {
            Kind::Number(n) => Ok(Expr::Literal(Value::Number(n))),
            Kind::BigInt(n) => Ok(Expr::Literal(Value::BigInt(n))),
            Kind::String(s) => Ok(Expr::Literal(Value::string(s))),
            Kind::Template(parts) => self.template_literal(parts),
            Kind::RegExp(pattern, flags) => Ok(Expr::RegExp(pattern, flags)),
            Kind::Name(name) => {
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
        let expression = format!("{}1{}", "(".repeat(70), ")".repeat(70));
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
