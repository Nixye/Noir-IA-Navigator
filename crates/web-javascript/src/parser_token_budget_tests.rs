use super::*;
use crate::ErrorKind;

#[test]
fn token_limit_is_unchanged_and_end_marker_does_not_consume_it() {
    let source = ";".repeat(MAX_SCRIPT_TOKENS);
    let tokens = lex(&source).unwrap();
    assert_eq!(tokens.len(), MAX_SCRIPT_TOKENS + 1);
    assert_eq!(tokens.capacity(), MAX_SCRIPT_TOKENS + 1);
    assert!(matches!(tokens.last().unwrap().kind, Kind::End));
    let error = lex(&(source + ";")).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert_eq!(error.message, "Script token budget exceeded");
    assert_eq!(error.offset, None);
    let diagnostic = error.to_string();
    assert!(
        diagnostic.contains("lexer tokens=32769 limit=32768 input-bytes=32769 scanned-bytes=32769"),
        "{diagnostic}"
    );
}

#[test]
fn token_storage_growth_stays_bounded_including_end_marker() {
    for count in [0, 3, 4, 7, 8, 16_383, 16_384, 32_767, 32_768] {
        let tokens = lex(&";".repeat(count)).unwrap();
        assert_eq!(tokens.len(), count + 1);
        assert!(tokens.capacity() <= MAX_SCRIPT_TOKENS + 1);
        assert!(tokens.capacity() <= (count + 1).next_power_of_two().max(4));
    }
    let mut tokens = lex(&";".repeat(MAX_SCRIPT_TOKENS)).unwrap();
    let capacity = tokens.capacity();
    let error = push_token(
        &mut tokens,
        Token {
            kind: Kind::End,
            offset: 0,
            newline: false,
        },
    )
    .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert_eq!(tokens.len(), MAX_SCRIPT_TOKENS + 1);
    assert_eq!(tokens.capacity(), capacity);
}

#[test]
fn trailing_comments_and_syntax_errors_keep_their_preexisting_precedence() {
    let source = ";".repeat(MAX_SCRIPT_TOKENS);
    assert!(lex(&(source.clone() + " /* no extra token */\n")).is_ok());
    for suffix in ["@", "/*", "'unterminated", "`unterminated"] {
        assert_eq!(
            lex(&(source.clone() + suffix)).unwrap_err().kind,
            ErrorKind::Syntax
        );
    }
}

#[test]
fn numeric_lexer_diagnostic_measures_bytes_not_characters_and_omits_source() {
    let prefix = "/* PRIVATE_SOURCE 😀 */\n";
    let source = format!(
        "{prefix}{} PRIVATE_TRAILING",
        ";".repeat(MAX_SCRIPT_TOKENS + 1)
    );
    let error = lex(&source).unwrap_err();
    let diagnostic = error.to_string();
    assert!(
        diagnostic.contains(&format!("input-bytes={}", source.len())),
        "{diagnostic}"
    );
    assert!(
        diagnostic.contains(&format!(
            "scanned-bytes={}",
            prefix.len() + MAX_SCRIPT_TOKENS + 1
        )),
        "{diagnostic}"
    );
    assert!(!diagnostic.contains("PRIVATE"));
    assert!(!diagnostic.contains('😀'));
    assert_eq!(error.diagnostic_contexts().len(), 1);
    assert!(error.diagnostic_contexts()[0].len() <= 128);
}

#[test]
fn token_diagnostics_do_not_replace_ast_or_source_size_budgets() {
    let ast = parse(&";".repeat(16_385)).unwrap_err();
    assert_eq!(ast.kind, ErrorKind::Resource);
    assert_eq!(ast.message, "Syntax tree node budget exceeded");
    assert!(ast.to_string().contains("parser nodes=16385 limit=16384"));
    assert!(!ast.to_string().contains("lexer tokens="));
    let input = parse(&" ".repeat(128 * 1024 + 1)).unwrap_err();
    assert_eq!(input.kind, ErrorKind::Resource);
    assert_eq!(input.message, "Script exceeds 128 KiB");
    assert!(!input.to_string().contains("lexer tokens="));
}

#[test]
fn ast_limit_is_unchanged_and_does_not_invent_source_position() {
    assert_eq!(
        parse(&";".repeat(MAX_SYNTAX_NODES)).unwrap().len(),
        MAX_SYNTAX_NODES
    );
    let error = parse(&";".repeat(MAX_SYNTAX_NODES + 1)).unwrap_err();
    assert_eq!(error.offset, None);
    assert_eq!(
        error.diagnostic_contexts(),
        &["parser nodes=16385 limit=16384"]
    );
}
