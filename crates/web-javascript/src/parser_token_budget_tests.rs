use super::*;
use crate::ErrorKind;

#[test]
fn compact_token_limit_and_end_marker_have_separate_storage_slots() {
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
        diagnostic.contains(&format!(
            "lexer tokens={} limit={} input-bytes={} scanned-bytes={}",
            MAX_SCRIPT_TOKENS + 1,
            MAX_SCRIPT_TOKENS,
            MAX_SCRIPT_TOKENS + 1,
            MAX_SCRIPT_TOKENS + 1,
        )),
        "{diagnostic}"
    );
}

#[test]
fn token_storage_growth_stays_bounded_including_end_marker() {
    for count in [
        0,
        3,
        4,
        7,
        8,
        16_383,
        16_384,
        32_767,
        32_768,
        MAX_SCRIPT_TOKENS - 1,
        MAX_SCRIPT_TOKENS,
    ] {
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
            close_paren: NO_TOKEN,
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
    assert_eq!(ast.message, "Parser visit budget exceeded");
    assert!(ast.to_string().contains("parser visits=16385 limit=16384"));
    assert!(!ast.to_string().contains("lexer tokens="));
    let input = parse(&" ".repeat(128 * 1024 + 1)).unwrap_err();
    assert_eq!(input.kind, ErrorKind::Resource);
    assert_eq!(input.message, "Script exceeds 128 KiB");
    assert!(!input.to_string().contains("lexer tokens="));
}

#[test]
fn ast_limit_is_unchanged_and_does_not_invent_source_position() {
    assert_eq!(
        parse(&";".repeat(MAX_PARSER_VISITS)).unwrap().len(),
        MAX_PARSER_VISITS
    );
    let error = parse(&";".repeat(MAX_PARSER_VISITS + 1)).unwrap_err();
    assert_eq!(error.offset, None);
    assert_eq!(
        error.diagnostic_contexts(),
        &["parser visits=16385 limit=16384"]
    );
}

#[test]
fn compact_token_layout_and_regex_boxes_fit_below_legacy_storage() {
    // These are the supported Windows/Linux 64-bit builds. Do not silently
    // enlarge all token slots when adding a rare literal payload later.
    if usize::BITS == 64 {
        assert_eq!(std::mem::size_of::<Kind>(), 32);
        assert_eq!(std::mem::size_of::<Token>(), 48);
        assert_eq!(std::mem::size_of::<RegExpLiteral>(), 48);
        let compact = (MAX_SCRIPT_TOKENS + 1) * std::mem::size_of::<Token>()
            + crate::regexp::MAX_REGEXES * std::mem::size_of::<RegExpLiteral>();
        assert_eq!(MAX_SCRIPT_TOKENS, 40_960);
        assert_eq!(compact, 1_969_200);
        assert_eq!(MAX_TOKEN_STORAGE_BYTES, 2_097_216);
        assert!(compact < MAX_TOKEN_STORAGE_BYTES);
    }
}

#[test]
fn punctuation_tokens_use_canonical_static_storage_without_losing_positions() {
    let source = "/*😀*/a+=1;\n(a?.b ?? c) >>>= 2;";
    let tokens = lex(source).unwrap();
    let symbols: Vec<_> = tokens
        .iter()
        .filter_map(|token| {
            if let Kind::Symbol(symbol) = &token.kind {
                assert_eq!(
                    source.get(token.offset..token.offset + symbol.len()),
                    Some(*symbol)
                );
                Some(*symbol)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(symbols, ["+=", ";", "(", "?", ".", "??", ")", ">>>=", ";"]);
    assert!(std::ptr::eq(symbols[1], symbols[8]));
    assert!(
        tokens
            .iter()
            .find(|token| matches!(token.kind, Kind::Symbol("(")))
            .unwrap()
            .newline
    );
    for symbol in "{}[]().,;:?# +-*/%!=<>&|^~"
        .chars()
        .filter(|ch| !ch.is_whitespace())
    {
        let canonical = single_symbol(symbol).unwrap();
        assert_eq!(canonical.chars().collect::<Vec<_>>(), [symbol]);
        assert!(std::ptr::eq(canonical, single_symbol(symbol).unwrap()));
    }
    assert!(single_symbol('😀').is_none());
}

#[test]
fn compact_regex_payloads_preserve_literal_limits_and_disambiguation() {
    let source = "/a/;".repeat(crate::regexp::MAX_REGEXES);
    let tokens = lex(&source).unwrap();
    let regexes = tokens
        .iter()
        .filter(|token| matches!(token.kind, Kind::RegExp(_)))
        .count();
    assert_eq!(regexes, crate::regexp::MAX_REGEXES);
    let error = lex(&(source + "/b/;")).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert_eq!(error.message, "RegExp literal budget exceeded");
    let tokens = lex("if(true)/a/.test('a'); let n=8/2; ({x:1})/2;").unwrap();
    assert_eq!(
        tokens
            .iter()
            .filter(|token| matches!(token.kind, Kind::RegExp(_)))
            .count(),
        1
    );
    assert_eq!(
        tokens
            .iter()
            .filter(|token| matches!(token.kind, Kind::Symbol("/")))
            .count(),
        2
    );
    let Kind::RegExp(literal) = &tokens
        .iter()
        .find(|token| matches!(token.kind, Kind::RegExp(_)))
        .unwrap()
        .kind
    else {
        unreachable!()
    };
    assert_eq!(literal.pattern, "a");
    assert_eq!(literal.flags, "");
}

#[test]
fn compaction_does_not_raise_payload_or_context_push_quotas() {
    for token in ["name ", "'x'", "? ", "(", "{", "0 ", "1n ", "`x` "] {
        let prefix = token.repeat(MAX_COSTLY_TOKENS);
        // Some narrow targets may hit their byte-derived physical slot cap first.
        if MAX_SCRIPT_TOKENS >= MAX_COSTLY_TOKENS {
            assert!(lex(&prefix).is_ok(), "Rejected boundary token {token}");
            let source = format!(";{prefix}{token}");
            let error = match lex(&source) {
                Err(error) => error,
                Ok(_) => panic!("Costly token limit was bypassed"),
            };
            assert_eq!(error.kind, ErrorKind::Resource);
            assert_eq!(error.message, "Script token payload budget exceeded");
            assert!(
                error
                    .to_string()
                    .contains("payload-context-tokens=32769 limit=32768")
            );
        }
    }
}

#[test]
fn payload_classification_covers_owned_values_and_all_context_pushes() {
    for source in ["name", "1", "2n", "'x'", "`x`", "/x/", "(", "{", "?"] {
        let tokens = lex(source).unwrap();
        assert!(costly_token(&tokens[0].kind), "{source}");
        assert!(!costly_token(&tokens[1].kind));
    }
    for source in [";", ")", "}", "[", "]", "=>", ".", ":", "??"] {
        assert!(!costly_token(&lex(source).unwrap()[0].kind), "{source}");
    }
}

#[test]
fn costly_boundary_keeps_syntax_precedence_and_private_free_diagnostics() {
    let prefix = "? ".repeat(MAX_COSTLY_TOKENS);
    for suffix in ["@", "/*", "'unterminated", "`unterminated"] {
        assert_eq!(
            lex(&(prefix.clone() + suffix)).unwrap_err().kind,
            ErrorKind::Syntax
        );
    }
    let source = format!("/* SECRET 😀 */{prefix} PRIVATE_TRAILING");
    let error = lex(&source).unwrap_err();
    assert_eq!(error.message, "Script token payload budget exceeded");
    assert_eq!(error.offset, None);
    assert!(
        error
            .to_string()
            .contains(&format!("input-bytes={}", source.len()))
    );
    assert!(!error.to_string().contains("PRIVATE"));
    assert!(!error.to_string().contains("SECRET"));
    assert!(!error.to_string().contains('😀'));
    assert_eq!(error.diagnostic_contexts().len(), 1);
    assert!(error.diagnostic_contexts()[0].len() <= 128);
}

#[test]
fn dense_program_uses_compact_slots_without_bypassing_parser_work_limit() {
    let dense = format!("{}f(0);", "a.b.c();".repeat(4685));
    assert_eq!(dense.len(), 37_485);
    let tokens = lex(&dense).unwrap();
    assert_eq!(tokens.len() - 1, 37_485);
    assert_eq!(
        tokens
            .iter()
            .filter(|token| costly_token(&token.kind))
            .count(),
        18_743
    );
    assert_eq!(parse(&dense).unwrap().len(), 4_686);
    assert_eq!(
        parse(&";".repeat(MAX_PARSER_VISITS + 1))
            .unwrap_err()
            .message,
        "Parser visit budget exceeded"
    );
}

#[test]
fn maximum_token_capacity_and_regexp_boxes_share_the_old_byte_budget() {
    let prefix = "/x/;".repeat(crate::regexp::MAX_REGEXES);
    let source = format!(
        "{prefix}{}",
        ";".repeat(MAX_SCRIPT_TOKENS - 2 * crate::regexp::MAX_REGEXES)
    );
    let tokens = lex(&source).unwrap();
    assert_eq!(tokens.len(), MAX_SCRIPT_TOKENS + 1);
    assert_eq!(tokens.capacity(), MAX_SCRIPT_TOKENS + 1);
    assert!(
        tokens.capacity() * std::mem::size_of::<Token>() + REGEXP_BOX_BYTES
            <= MAX_TOKEN_STORAGE_BYTES
    );
    let source = format!(
        "{}{prefix}",
        ";".repeat(MAX_SCRIPT_TOKENS - 2 * crate::regexp::MAX_REGEXES)
    );
    assert_eq!(lex(&source).unwrap().len(), MAX_SCRIPT_TOKENS + 1);
}

#[test]
fn dense_fixture_uses_same_lexer_policy_for_scripts_modules_and_function_body() {
    let html = include_str!("../../../fixtures/javascript-compact-lexer.html");
    let source = html
        .split_once("<script>")
        .unwrap()
        .1
        .split_once("</script>")
        .unwrap()
        .0;
    let tokens = lex(source).unwrap();
    assert!(tokens.len() - 1 > MAX_COSTLY_TOKENS);
    assert!(tokens.len() - 1 <= MAX_SCRIPT_TOKENS);
    assert!(
        tokens
            .iter()
            .filter(|token| costly_token(&token.kind))
            .count()
            <= MAX_COSTLY_TOKENS
    );
    assert!(source.len() <= 64 * 1024);
    assert!(parse(source).is_ok());
    parse_module(source).unwrap();
    assert!(parse_dynamic_function("", source).is_ok());
}

#[test]
fn eof_only_growth_does_not_reserve_unused_token_slots() {
    for count in [0, 4, 8, 16, 1024, 16_384, 32_768] {
        let tokens = lex(&";".repeat(count)).unwrap();
        assert_eq!(tokens.capacity(), count + 1);
    }
}
