//! Original JavaScript adapter over bounded independent pattern-search libraries.
use crate::JsError;
use regex::{Regex, RegexBuilder};
use std::ops::Range;
#[path = "regexp_properties.rs"]
mod unicode_properties;

pub const MAX_PATTERN: usize = 4096;
pub const MAX_REGEXES: usize = 64;
pub const MAX_PATTERN_TOTAL: usize = 16 * 1024;
pub const MAX_SEARCH_WORK: usize = 4 * 1024 * 1024;
pub const ADVANCED_BACKTRACK_LIMIT: usize = 16_384;
const ADVANCED_DELEGATE_LIMIT: usize = 128 * 1024;
const MAX_TRANSLATED: usize = 64 * 1024;

#[derive(Debug, Clone)]
enum Backend {
    Linear(Regex),
    Advanced(fancy_regex::Regex),
}

pub struct Captures {
    pub groups: Vec<Option<Range<usize>>>,
}

#[derive(Debug, Clone)]
pub struct Compiled {
    pub source: String,
    pub flags: String,
    pub global: bool,
    pub sticky: bool,
    pub unicode: bool,
    pub insensitive: bool,
    pub multiline: bool,
    pub has_indices: bool,
    input_insensitive: bool,
    insensitive_backreference: bool,
    pub names: Vec<(String, usize)>,
    backend: Backend,
}

impl Compiled {
    pub fn compile(source: &str, flags: &str) -> Result<Self, JsError> {
        if source.len() > MAX_PATTERN {
            return Err(JsError::resource("RegExp pattern exceeds 4096 bytes"));
        }
        let mut seen = [false; 128];
        for flag in flags.bytes() {
            if !matches!(flag, b'd' | b'g' | b'i' | b'm' | b's' | b'u' | b'y')
                || seen[flag as usize]
            {
                return Err(JsError::runtime("Invalid or unsupported RegExp flags"));
            }
            seen[flag as usize] = true;
        }
        let global = seen[b'g' as usize];
        let sticky = seen[b'y' as usize];
        let unicode = seen[b'u' as usize];
        let insensitive = seen[b'i' as usize];
        let multiline = seen[b'm' as usize];
        let dotall = seen[b's' as usize];
        let translated = translate(source, unicode, dotall, insensitive, multiline)?;
        let backend = if translated.advanced {
            // Limit AST size, repeat expansion and every delegated automaton before
            // compiling. Variable lookbehind support is deliberately not enabled.
            if source.chars().count().saturating_mul(translated.expansion) > 32 * 1024 {
                return Err(JsError::resource(
                    "Advanced RegExp compilation budget exceeded",
                ));
            }
            let pattern = advanced_boundaries(&translated.pattern);
            validate_advanced(&pattern)?;
            Backend::Advanced(
                fancy_regex::RegexBuilder::new(&pattern)
                    .case_insensitive(insensitive)
                    .multi_line(multiline)
                    .dot_matches_new_line(dotall)
                    .backtrack_limit(ADVANCED_BACKTRACK_LIMIT)
                    .delegate_size_limit(ADVANCED_DELEGATE_LIMIT)
                    .delegate_dfa_size_limit(ADVANCED_DELEGATE_LIMIT)
                    .build()
                    .map_err(advanced_error)?,
            )
        } else {
            Backend::Linear(
                RegexBuilder::new(&translated.pattern)
                    .case_insensitive(insensitive)
                    .multi_line(multiline)
                    .dot_matches_new_line(dotall)
                    .size_limit(128 * 1024)
                    .dfa_size_limit(128 * 1024)
                    .nest_limit(64)
                    .build()
                    .map_err(|error| match error {
                        regex::Error::CompiledTooBig(_) => {
                            JsError::resource("RegExp compiled size exceeds 128 KiB")
                        }
                        _ => JsError::runtime(format!("Unsupported or invalid RegExp: {error}")),
                    })?,
            )
        };
        Ok(Self {
            source: source.into(),
            flags: "dgimsuy".chars().filter(|c| seen[*c as usize]).collect(),
            global,
            sticky,
            unicode,
            insensitive,
            multiline,
            has_indices: seen[b'd' as usize],
            input_insensitive: translated.insensitive,
            insensitive_backreference: translated.backreferences && translated.insensitive,
            names: translated.names,
            backend,
        })
    }
    pub fn validate_input(&self, text: &str) -> Result<(), JsError> {
        if text.len() > 65536 {
            return Err(JsError::resource("RegExp input exceeds 64 KiB"));
        }
        if !self.unicode && text.chars().any(|c| c as u32 > 0xffff) {
            return Err(JsError::runtime(
                "RegExp without u on surrogate pairs is not implemented",
            ));
        }
        if self.input_insensitive && !self.unicode && (!text.is_ascii() || !self.source.is_ascii())
        {
            return Err(JsError::runtime(
                "Non-ASCII RegExp case folding requires u in this subset",
            ));
        }
        if self.insensitive_backreference && !text.is_ascii() {
            return Err(JsError::runtime(
                "Non-ASCII input with case-insensitive RegExp backreferences is not implemented",
            ));
        }
        Ok(())
    }

    pub fn extra_search_work(&self) -> usize {
        if matches!(self.backend, Backend::Advanced(_)) {
            ADVANCED_BACKTRACK_LIMIT
        } else {
            0
        }
    }
    pub fn captures_at(&self, text: &str, start: usize) -> Result<Option<Captures>, JsError> {
        self.validate_input(text)?;
        if start > text.len() {
            return Ok(None);
        }
        if !text.is_char_boundary(start) {
            return Err(JsError::runtime("RegExp start is not a character boundary"));
        }
        match &self.backend {
            Backend::Linear(regex) => Ok(regex.captures_at(text, start).map(|captures| Captures {
                groups: captures
                    .iter()
                    .map(|part| part.map(|part| part.range()))
                    .collect(),
            })),
            Backend::Advanced(regex) => regex
                .captures_from_pos(text, start)
                .map(|captures| {
                    captures.map(|captures| Captures {
                        groups: captures
                            .iter()
                            .map(|part| part.map(|part| part.range()))
                            .collect(),
                    })
                })
                .map_err(advanced_error),
        }
    }
}

fn advanced_error(error: fancy_regex::Error) -> JsError {
    match error {
        fancy_regex::Error::RuntimeError(_) => {
            JsError::resource("Advanced RegExp backtracking/stack limit exceeded")
        }
        fancy_regex::Error::CompileError(ref error)
            if matches!(error.as_ref(), fancy_regex::CompileError::InnerError(_)) =>
        {
            JsError::resource("Advanced RegExp delegated compilation limit exceeded")
        }
        fancy_regex::Error::ParseError(_, fancy_regex::ParseError::RecursionExceeded) => {
            JsError::resource("Advanced RegExp parser nesting limit exceeded")
        }
        _ => JsError::runtime(format!("Unsupported or invalid RegExp: {error}")),
    }
}

fn validate_advanced(pattern: &str) -> Result<(), JsError> {
    use fancy_regex::{Expr, LookAround};
    if pattern.len() > MAX_TRANSLATED {
        return Err(JsError::resource(
            "Advanced RegExp translation exceeds 64 KiB",
        ));
    }
    let tree = Expr::parse_tree(pattern).map_err(advanced_error)?;
    let mut pending = vec![(&tree.expr, 0usize)];
    let mut nodes = 0usize;
    while let Some((expr, depth)) = pending.pop() {
        nodes += 1;
        if nodes > 8192 || depth > 64 {
            return Err(JsError::resource("Advanced RegExp AST budget exceeded"));
        }
        if let Expr::LookAround(child, LookAround::LookBehind | LookAround::LookBehindNeg) = expr
            && fixed_width(child).is_none()
        {
            return Err(JsError::runtime(
                "Variable-width RegExp lookbehind is not implemented",
            ));
        }
        if let Expr::Repeat { child, hi, .. } = expr
            && *hi > 1
            && (matches!(child.as_ref(), Expr::Group(_))
                || child.has_descendant(|part| matches!(part, Expr::Group(_))))
        {
            return Err(JsError::runtime(
                "Repeated captures in advanced RegExp are not implemented",
            ));
        }
        pending.extend(expr.children_iter().map(|child| (child, depth + 1)));
    }
    Ok(())
}

fn fixed_width(expr: &fancy_regex::Expr) -> Option<usize> {
    use fancy_regex::Expr;
    match expr {
        Expr::Empty | Expr::Assertion(_) | Expr::LookAround(..) => Some(0),
        Expr::Any { .. } | Expr::Delegate { .. } => Some(1),
        Expr::Literal { val, .. } => Some(val.chars().count()),
        Expr::Group(child) => fixed_width(child),
        Expr::AtomicGroup(child) => fixed_width(child),
        Expr::Concat(parts) => parts
            .iter()
            .try_fold(0usize, |width, part| width.checked_add(fixed_width(part)?)),
        Expr::Alt(parts) => {
            let width = fixed_width(parts.first()?)?;
            parts
                .iter()
                .all(|part| fixed_width(part) == Some(width))
                .then_some(width)
        }
        Expr::Repeat { child, lo, hi, .. } if lo == hi => fixed_width(child)?.checked_mul(*lo),
        _ => None,
    }
}

fn advanced_boundaries(pattern: &str) -> String {
    // The VM parser forbids scoped Unicode mode. These are actual ASCII word
    // assertions, rather than a change to Unicode word-boundary semantics.
    pattern
        .replace("(?-u:\\b\\B)", "(?!)")
        .replace(
            "(?-u:\\b)",
            "(?:(?<![A-Za-z0-9_])(?=[A-Za-z0-9_])|(?<=[A-Za-z0-9_])(?![A-Za-z0-9_]))",
        )
        .replace(
            "(?-u:\\B)",
            "(?:(?<=[A-Za-z0-9_])(?=[A-Za-z0-9_])|(?<![A-Za-z0-9_])(?![A-Za-z0-9_]))",
        )
}

struct Translation {
    pattern: String,
    names: Vec<(String, usize)>,
    advanced: bool,
    backreferences: bool,
    insensitive: bool,
    expansion: usize,
}
struct Group {
    dotall: bool,
    multiline: bool,
    insensitive: bool,
    capture: Option<usize>,
    behind: bool,
    atomic_assertion: bool,
    assertion: bool,
}

fn translate(
    source: &str,
    unicode: bool,
    mut dotall: bool,
    mut insensitive: bool,
    mut multiline: bool,
) -> Result<Translation, JsError> {
    let mut output = String::new();
    let mut chars = source.chars().peekable();
    let mut in_class = false;
    let mut input_insensitive = insensitive;
    let mut groups: Vec<Group> = Vec::new();
    let mut names: Vec<(String, usize)> = Vec::new();
    let mut closed = [false; 25];
    let mut capture_count = 0usize;
    let mut advanced = false;
    let mut backreferences = false;
    let mut expansion = 1usize;
    let mut quantifier_state = 0u8;
    while let Some(c) = chars.next() {
        if output.len() > MAX_TRANSLATED {
            return Err(JsError::resource("RegExp translation exceeds 64 KiB"));
        }
        if c == '\\' {
            quantifier_state = 0;
            let escaped = chars
                .next()
                .ok_or_else(|| JsError::runtime("Unterminated RegExp escape"))?;
            match escaped {
                'd'=>output.push_str("[0-9]"), 'D'=>output.push_str("[^0-9]"),
                'w'=>output.push_str("[A-Za-z0-9_]"), 'W'=>output.push_str("[^A-Za-z0-9_]"),
                's'=>output.push_str("[\\t\\n\\x0b\\x0c\\r \\u{00a0}\\u{1680}\\u{2000}-\\u{200a}\\u{2028}\\u{2029}\\u{202f}\\u{205f}\\u{3000}\\u{feff}]"),
                'S'=>output.push_str("[^\\t\\n\\x0b\\x0c\\r \\u{00a0}\\u{1680}\\u{2000}-\\u{200a}\\u{2028}\\u{2029}\\u{202f}\\u{205f}\\u{3000}\\u{feff}]"),
                'b' if in_class=>output.push_str("\\x08"),
                '-' if in_class=>output.push_str("\\-"),
                'b'|'B'=>{
                    if unicode && insensitive { advanced = true; }
                    output.push_str("(?-u:\\");output.push(escaped);output.push(')');
                },
                '0'=>{if chars.peek().is_some_and(char::is_ascii_digit){return Err(JsError::runtime("RegExp octal escapes are not implemented"));}output.push_str("\\x00");},
                '1'..='9' if !in_class => {
                    let mut digits = escaped.to_string();
                    while chars.peek().is_some_and(char::is_ascii_digit) {
                        digits.push(chars.next().unwrap());
                    }
                    let index = digits.parse::<usize>().ok().filter(|index| *index <= 24)
                        .ok_or_else(|| JsError::runtime("Invalid or unsupported RegExp backreference"))?;
                    backreference(&mut output, index, &closed, &groups)?;
                    advanced = true;
                    backreferences = true;
                },
                '1'..='9' => return Err(JsError::runtime("Legacy RegExp octal escapes are not implemented")),
                'k' if !in_class && chars.peek() == Some(&'<') => {
                    chars.next();
                    let name = group_name(&mut chars)?;
                    let index = names.iter().find(|(key, _)| *key == name).map(|(_, index)| *index)
                        .ok_or_else(|| JsError::runtime("Unknown or forward named RegExp backreference"))?;
                    backreference(&mut output, index, &closed, &groups)?;
                    advanced = true;
                    backreferences = true;
                },
                'p'|'P'=>{
                    if !unicode { output.push(escaped); continue; }
                    if chars.next()!=Some('{'){return Err(JsError::runtime("Invalid Unicode property escape"));}
                    let mut name=String::new();
                    loop {match chars.next(){Some('}')=>break,Some(ch)if name.len()<128=>name.push(ch),_=>return Err(JsError::runtime("Unterminated or excessive Unicode property escape"))}}
                    let name=unicode_properties::property(&name)?;
                    output.push('\\');output.push(escaped);output.push('{');output.push_str(&name);output.push('}');
                },
                'u'=>{
                    let digits=if chars.peek()==Some(&'{') {if !unicode {return Err(JsError::runtime("RegExp code point escape requires u"));} chars.next();let mut digits=String::new();loop{match chars.next(){Some('}')=>break,Some(c) if c.is_ascii_hexdigit()&&digits.len()<6=>digits.push(c),_=>return Err(JsError::runtime("Invalid RegExp code point escape"))}}digits}
                    else {let mut digits=String::new();for _ in 0..4 {digits.push(chars.next().filter(char::is_ascii_hexdigit).ok_or_else(||JsError::runtime("Invalid RegExp Unicode escape"))?);}digits};
                    let scalar=u32::from_str_radix(&digits,16).ok().and_then(char::from_u32).ok_or_else(||JsError::runtime("RegExp surrogate escapes are not implemented"))?;
                    output.push_str(&regex::escape(&scalar.to_string()));
                },
                'c'=>{let letter=chars.next().filter(char::is_ascii_alphabetic).ok_or_else(||JsError::runtime("Invalid RegExp control escape"))?;output.push_str(&format!("\\x{:02x}",(letter as u8)&31));},
                'x'=>{
                    let mut digits = String::new();
                    for _ in 0..2 {
                        digits.push(chars.next().filter(char::is_ascii_hexdigit)
                            .ok_or_else(|| JsError::runtime("Invalid RegExp hexadecimal escape"))?);
                    }
                    output.push_str("\\x"); output.push_str(&digits);
                },
                '/'=>output.push('/'),
                'n'|'r'|'t'|'f'|'v'|'\\'|'^'|'$'|'.'|'*'|'+'|'?'|'('|')'|'['|']'|'{'|'}'|'|'=>{output.push('\\');output.push(escaped);},
                other if !unicode=>output.push_str(&regex::escape(&other.to_string())),
                _=>return Err(JsError::runtime("Invalid RegExp identity escape in u mode")),
            }
        } else {
            if in_class && matches!(c, '&' | '~') {
                output.push_str(if c == '&' { "\\x26" } else { "\\x7e" });
                continue;
            }
            if in_class && c == '-' && chars.peek() == Some(&'-') {
                return Err(JsError::runtime(
                    "Double-hyphen RegExp classes are not implemented",
                ));
            }
            if !in_class && c == '{' {
                let body = chars
                    .clone()
                    .take(33)
                    .take_while(|c| c.is_ascii_digit() || *c == ',')
                    .collect::<String>();
                if body.len() > 32 {
                    return Err(JsError::resource("RegExp quantifier exceeds limit"));
                }
                let mut parts = body.split(',');
                let minimum = parts.next().unwrap_or("");
                let maximum = parts.next();
                let valid = !minimum.is_empty()
                    && minimum.chars().all(|c| c.is_ascii_digit())
                    && maximum.is_none_or(|max| max.chars().all(|c| c.is_ascii_digit()))
                    && parts.next().is_none()
                    && chars.clone().nth(body.len()) == Some('}');
                if valid {
                    if quantifier_state != 0 {
                        return Err(JsError::runtime("Repeated RegExp quantifier"));
                    }
                    quantifier_state = 1;
                    let repeat = maximum
                        .filter(|maximum| !maximum.is_empty())
                        .unwrap_or(minimum)
                        .parse::<usize>()
                        .unwrap_or(usize::MAX)
                        .max(1);
                    expansion = expansion.saturating_mul(repeat);
                    output.push('{');
                    output.push_str(&body);
                    output.push('}');
                    for _ in 0..=body.len() {
                        chars.next();
                    }
                    continue;
                }
                if unicode {
                    return Err(JsError::runtime("Invalid RegExp quantifier in u mode"));
                }
                quantifier_state = 0;
                output.push_str("\\{");
                continue;
            }
            if !in_class && c == '}' {
                quantifier_state = 0;
                if unicode {
                    return Err(JsError::runtime("Unescaped RegExp closing brace in u mode"));
                }
                output.push_str("\\}");
                continue;
            }
            if !in_class && c == '(' {
                quantifier_state = 0;
                if chars.peek() == Some(&'*') {
                    return Err(JsError::runtime("Invalid JavaScript RegExp group"));
                }
                if groups.len() >= 32 {
                    return Err(JsError::resource("RegExp group nesting exceeds 32"));
                }
                let old_dotall = dotall;
                let old_multiline = multiline;
                let old_insensitive = insensitive;
                let mut capture = true;
                let mut behind = false;
                let mut atomic_assertion = false;
                let mut assertion = false;
                let mut prefix = String::from("(");
                if chars.peek() == Some(&'?') {
                    chars.next();
                    capture = false;
                    match chars.next() {
                        Some(':') => prefix.push_str("?:"),
                        Some(operator @ ('=' | '!')) => {
                            assertion = true;
                            if operator == '=' {
                                prefix = String::from("(?>(?=");
                                atomic_assertion = true;
                            } else {
                                prefix.push('?');
                                prefix.push(operator);
                            }
                            advanced = true;
                        }
                        Some('<') => {
                            if let Some(operator @ ('=' | '!')) = chars.peek().copied() {
                                chars.next();
                                prefix.push_str("?<");
                                prefix.push(operator);
                                advanced = true;
                                behind = true;
                                assertion = true;
                            } else {
                                let name = group_name(&mut chars)?;
                                if names.iter().any(|(existing, _)| existing == &name) {
                                    return Err(JsError::runtime("Duplicate RegExp group name"));
                                }
                                names.push((name, capture_count + 1));
                                capture = true;
                            }
                        }
                        Some(first @ ('i' | 'm' | 's' | '-')) => {
                            let mut enabled = true;
                            let mut flags_seen = [false; 3];
                            let mut flag_count = 0;
                            let mut disabled_count = 0;
                            let mut flag = first;
                            prefix.push('?');
                            loop {
                                if flag == '-' && enabled {
                                    enabled = false;
                                    prefix.push('-');
                                } else {
                                    let index = match flag {
                                        'i' => 0,
                                        'm' => 1,
                                        's' => 2,
                                        _ => {
                                            return Err(JsError::runtime(
                                                "Invalid scoped RegExp flags",
                                            ));
                                        }
                                    };
                                    if flags_seen[index] {
                                        return Err(JsError::runtime(
                                            "Duplicate scoped RegExp flag",
                                        ));
                                    }
                                    flags_seen[index] = true;
                                    flag_count += 1;
                                    if !enabled {
                                        disabled_count += 1;
                                    }
                                    match flag {
                                        'i' => {
                                            insensitive = enabled;
                                            input_insensitive |= enabled;
                                        }
                                        'm' => multiline = enabled,
                                        's' => dotall = enabled,
                                        _ => unreachable!(),
                                    }
                                    prefix.push(flag);
                                }
                                flag = chars.next().ok_or_else(|| {
                                    JsError::runtime("Unterminated scoped RegExp flags")
                                })?;
                                if flag == ':' {
                                    break;
                                }
                            }
                            if flag_count == 0 || (!enabled && disabled_count == 0) {
                                return Err(JsError::runtime("Empty scoped RegExp flags"));
                            }
                            prefix.push(':');
                        }
                        _ => return Err(JsError::runtime("Unsupported RegExp group syntax")),
                    }
                }
                let capture = if capture {
                    if groups.iter().any(|group| group.behind) {
                        return Err(JsError::runtime(
                            "Captures inside RegExp lookbehind are not implemented",
                        ));
                    }
                    capture_count += 1;
                    if capture_count > 24 {
                        return Err(JsError::resource("RegExp capture count exceeds 24"));
                    }
                    Some(capture_count)
                } else {
                    None
                };
                groups.push(Group {
                    dotall: old_dotall,
                    multiline: old_multiline,
                    insensitive: old_insensitive,
                    capture,
                    behind,
                    atomic_assertion,
                    assertion,
                });
                output.push_str(&prefix);
                continue;
            }
            if !in_class && c == ')' {
                let group = groups
                    .pop()
                    .ok_or_else(|| JsError::runtime("Unmatched RegExp closing group"))?;
                if group.assertion
                    && (chars.peek().is_some_and(|c| matches!(c, '*' | '+' | '?'))
                        || (chars.peek() == Some(&'{')
                            && chars.clone().nth(1).is_some_and(|c| c.is_ascii_digit())))
                {
                    return Err(JsError::runtime(
                        "Quantified RegExp assertions are not implemented",
                    ));
                }
                dotall = group.dotall;
                multiline = group.multiline;
                insensitive = group.insensitive;
                if let Some(index) = group.capture {
                    closed[index] = true;
                }
                if group.atomic_assertion {
                    output.push(')');
                }
            }
            if !in_class && matches!(c, '*' | '+' | '?') {
                if quantifier_state == 1 && c == '?' {
                    quantifier_state = 2;
                } else if quantifier_state != 0 {
                    return Err(JsError::runtime("Repeated RegExp quantifier"));
                } else {
                    quantifier_state = 1;
                }
            } else if !in_class {
                quantifier_state = 0;
            }
            if !in_class && matches!(c, '*' | '+' | '?') && chars.peek() == Some(&'+') {
                return Err(JsError::runtime(
                    "Possessive RegExp quantifiers are not supported by JavaScript",
                ));
            }
            if c == '[' {
                if in_class {
                    output.push_str("\\[");
                    continue;
                }
                if chars.peek() == Some(&']') {
                    chars.next();
                    output.push_str("(?-u:\\b\\B)");
                    continue;
                }
                let mut empty = chars.clone();
                if empty.next() == Some('^') && empty.next() == Some(']') {
                    chars.next();
                    chars.next();
                    output.push_str("(?s:.)");
                    continue;
                }
                in_class = true;
            }
            if c == ']' {
                if !in_class {
                    if unicode {
                        return Err(JsError::runtime(
                            "Unescaped RegExp closing bracket in u mode",
                        ));
                    }
                    output.push_str("\\]");
                    continue;
                }
                in_class = false;
            }
            if !in_class && matches!(c, '^' | '$') {
                if multiline {
                    advanced = true;
                    output.push_str(if c == '^' {
                        "(?:\\A|(?<=[\\n\\r\\u{2028}\\u{2029}]))"
                    } else {
                        "(?:\\z|(?=[\\n\\r\\u{2028}\\u{2029}]))"
                    });
                } else {
                    output.push_str(if c == '^' { "\\A" } else { "\\z" });
                }
                continue;
            }
            if !in_class && c == '.' && !dotall {
                output.push_str("[^\\n\\r\\u{2028}\\u{2029}]");
            } else {
                output.push(c);
            }
        }
    }
    if in_class {
        return Err(JsError::runtime("Unterminated RegExp character class"));
    }
    if !groups.is_empty() {
        return Err(JsError::runtime("Unterminated RegExp group"));
    }
    if output.len() > MAX_TRANSLATED {
        return Err(JsError::resource("RegExp translation exceeds 64 KiB"));
    }
    Ok(Translation {
        pattern: output,
        names,
        advanced,
        backreferences,
        insensitive: input_insensitive,
        expansion,
    })
}

fn group_name(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Result<String, JsError> {
    let mut name = String::new();
    loop {
        match chars.next() {
            Some('>') if !name.is_empty() => return Ok(name),
            Some(c)
                if name.len() < 128
                    && (c.is_ascii_alphabetic()
                        || matches!(c, '_' | '$')
                        || (!name.is_empty() && c.is_ascii_digit())) =>
            {
                name.push(c)
            }
            _ => {
                return Err(JsError::runtime(
                    "Invalid or unsupported RegExp group name (ASCII identifiers only)",
                ));
            }
        }
    }
}
fn backreference(
    output: &mut String,
    index: usize,
    closed: &[bool; 25],
    groups: &[Group],
) -> Result<(), JsError> {
    if !closed[index]
        || groups
            .iter()
            .any(|group| group.capture == Some(index) || group.behind)
    {
        return Err(JsError::runtime(
            "Forward, self or lookbehind RegExp backreferences are not implemented",
        ));
    }
    // An unmatched JavaScript capture matches empty. A bare VM backreference
    // would fail, so encode the actual conditional behavior explicitly.
    output.push_str(&format!("(?({index})\\{index}|)"));
    Ok(())
}

pub fn utf16_index(text: &str, byte: usize) -> usize {
    text[..byte].encode_utf16().count()
}
pub fn byte_index(text: &str, unit: usize) -> Option<usize> {
    let mut units = 0;
    for (byte, c) in text.char_indices() {
        if units == unit {
            return Some(byte);
        }
        units += c.len_utf16();
        if units > unit {
            return None;
        }
    }
    (units == unit).then_some(text.len())
}
