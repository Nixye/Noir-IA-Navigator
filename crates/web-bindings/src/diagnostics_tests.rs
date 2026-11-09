use super::*;

fn session(html: &str) -> DocumentSession {
    let mut session = DocumentSession::new(web_html::parse(html).unwrap());
    session
        .set_document_url("https://example.test/page.html?document-secret#private")
        .unwrap();
    session
}
fn asset(src: &str, source: &str) -> ScriptAsset {
    ScriptAsset {
        src: src.into(),
        source: source.into(),
    }
}

#[test]
fn source_positions_require_valid_utf8_offsets_and_count_js_newlines() {
    let source = "á😀\r\nx\ry\nz\u{2028}w\u{2029}終";
    assert_eq!(source_position(source, 0), Some((1, 1)));
    assert_eq!(source_position(source, "á".len()), Some((1, 2)));
    assert_eq!(source_position(source, "á😀".len()), Some((1, 3)));
    assert_eq!(source_position(source, "á😀\r\n".len()), Some((2, 1)));
    assert_eq!(
        source_position(source, source.find('終').unwrap()),
        Some((6, 1))
    );
    assert_eq!(source_position(source, source.len()), Some((6, 2)));
    assert_eq!(source_position(source, 1), None);
    assert_eq!(source_position(source, source.len() + 1), None);
    assert_eq!(source_position(source, usize::MAX), None);
}

#[test]
fn source_labels_bound_input_output_and_strip_url_secrets() {
    let base = url::Url::parse("https://base-user:base-pass@example.test/dir/page?secret#private")
        .unwrap();
    for src in [
        "https://alice:password@example.test/script.js?token=hidden#private",
        "//alice:password@example.test/script.js?token=hidden#private",
        "/script.js?token=hidden#private",
    ] {
        assert_eq!(
            source_label(src, Some(&base)),
            "https://example.test/script.js"
        );
    }
    assert_eq!(
        source_label("./script.js?token=hidden#private", None),
        "/script.js"
    );
    assert_eq!(
        source_label("data:text/javascript,SECRET", None),
        "<unsupported source URL scheme>"
    );
    assert_eq!(
        source_label(&"😀".repeat(3000), Some(&base)),
        "<oversized source URL>"
    );
    let label = source_label(
        &format!("https://example.test/{}?hidden#private", "a".repeat(1000)),
        None,
    );
    assert!(label.len() <= MAX_SOURCE_LABEL_BYTES);
    assert!(label.ends_with("..."));
    assert!(!label.contains("hidden"));
    assert!(!label.contains("private"));
}

#[test]
fn bounded_unicode_warnings_never_copy_unbounded_fields_or_controls() {
    assert_eq!(bounded_text("á😀終", 7), "á...");
    assert_eq!(bounded_text("a\n\tb\u{202e}c", 32), "a  b c");
    let error = JsError::runtime("😀".repeat(20_000));
    let warning = callback_warning("timer", None, &error);
    assert!(warning.len() <= MAX_WARNING_BYTES);
    assert!(warning.ends_with("..."));
    let origin = ScriptOrigin::capture(0, 4, false, false, Some(&"a".repeat(100_000)), None);
    let warning = origin.warning("execute", &error, None);
    assert!(warning.len() <= MAX_WARNING_BYTES);
    assert!(warning.contains("<oversized source URL>"));
}

#[test]
fn inline_classic_reports_node_phase_and_only_real_parser_position() {
    let source = "var label='á😀';\n@";
    let mut s = session(&format!("<script>{source}</script>"));
    let node = s.query_selector("script").unwrap().unwrap();
    let report = s.run_initial(&[]);
    assert_eq!(report.executed, 0);
    assert_eq!(report.warnings.len(), 1);
    let warning = &report.warnings[0];
    assert!(warning.contains("JavaScript script 1 error"), "{warning}");
    assert!(warning.contains("phase=execute, kind=classic"), "{warning}");
    assert!(
        warning.contains(&format!("node={node}, source=inline")),
        "{warning}"
    );
    assert!(
        warning.contains(&format!(
            "byte={}, line=2, column=1",
            source.find('@').unwrap()
        )),
        "{warning}"
    );
    assert!(!warning.contains("label="), "{warning}");
}

#[test]
fn runtime_json_and_dynamic_errors_do_not_claim_initial_source_positions() {
    for source in [
        "missing();",
        "JSON.parse('{');",
        "eval('@');",
        "new Function('@');",
    ] {
        let mut s = session(&format!("<script>/*PRIVATE_SOURCE*/{source}</script>"));
        let report = s.run_initial(&[]);
        assert_eq!(report.warnings.len(), 1, "{source}");
        let warning = &report.warnings[0];
        assert!(warning.contains("source=inline"), "{warning}");
        assert!(!warning.contains("line="), "{warning}");
        assert!(!warning.contains("column="), "{warning}");
        assert!(!warning.contains("PRIVATE_SOURCE"), "{warning}");
    }
}

#[test]
fn deferred_external_origin_is_frozen_and_sanitized_before_execution() {
    let src = "https://alice:password@example.test/old.js?token=hidden#private";
    let mut s = session(&format!(
        "<script id=queued defer src='{src}'></script><script>document.getElementById('queued').setAttribute('src','new.js');document.getElementById('queued').remove();</script>"
    ));
    let node = s.query_selector("#queued").unwrap().unwrap();
    let report = s.run_initial(&[asset(src, "missing();")]);
    assert_eq!(report.executed, 1);
    assert_eq!(report.warnings.len(), 1);
    let warning = &report.warnings[0];
    assert!(warning.contains("JavaScript script 1 error"), "{warning}");
    assert!(
        warning.contains(&format!(
            "node={node}, source=https://example.test/old.js, schedule=defer"
        )),
        "{warning}"
    );
    for private in [
        "alice",
        "password",
        "token=",
        "hidden",
        "private",
        "new.js",
        "document-secret",
    ] {
        assert!(!warning.contains(private), "{warning}");
    }
}

#[test]
fn external_preparation_errors_retain_origin_without_disclosing_url_secrets() {
    let mut s = session(
        "<script src='https://alice:password@example.test/missing.js?hidden#private'></script>",
    );
    let report = s.run_initial(&[]);
    assert_eq!(report.warnings.len(), 1);
    let warning = &report.warnings[0];
    assert!(warning.contains("phase=preparation"), "{warning}");
    assert!(
        warning.contains("source=https://example.test/missing.js"),
        "{warning}"
    );
    for private in ["alice", "password", "hidden", "private"] {
        assert!(!warning.contains(private), "{warning}");
    }
}

#[test]
fn module_dependency_parser_location_names_the_dependency_not_root() {
    let mut s = session("<script type=module src='./root.mjs?root-secret'></script>");
    let report = s.run_initial(&[
        asset(
            "noir-module:https://example.test/root.mjs?root-secret",
            "import './dep.mjs?dep-secret';",
        ),
        asset("noir-module:https://example.test/dep.mjs?dep-secret", "\n@"),
    ]);
    assert_eq!(report.executed, 0);
    assert_eq!(report.warnings.len(), 1);
    let warning = &report.warnings[0];
    assert!(
        warning.contains("phase=preparation, kind=module"),
        "{warning}"
    );
    assert!(
        warning.contains("source=https://example.test/root.mjs, schedule=defer"),
        "{warning}"
    );
    assert!(
        warning.contains("module source=https://example.test/dep.mjs"),
        "{warning}"
    );
    assert!(
        warning.contains("module byte=1 line=2 column=1"),
        "{warning}"
    );
    assert!(!warning.contains("root-secret"), "{warning}");
    assert!(!warning.contains("dep-secret"), "{warning}");
}

#[test]
fn inline_module_and_module_link_errors_do_not_fabricate_runtime_locations() {
    for html in [
        "<script type=module>missing();</script>",
        "<script type=module>import 'bare-specifier';</script>",
    ] {
        let mut s = session(html);
        let report = s.run_initial(&[]);
        assert_eq!(report.warnings.len(), 1);
        let warning = &report.warnings[0];
        assert!(warning.contains("kind=module"), "{warning}");
        assert!(
            warning.contains("source=inline, schedule=defer"),
            "{warning}"
        );
        assert!(!warning.contains("line="), "{warning}");
        assert!(!warning.contains("column="), "{warning}");
        assert!(!warning.contains("document-secret"), "{warning}");
    }
}

#[test]
fn unsupported_dom_src_identifies_receiver_and_type_without_value_or_coercion() {
    // script.src is now a supported, brokered classic-script property.
    for tag in ["img", "iframe"] {
        let mut s = session("");
        let error = s.execute(&format!(
            "var calls=0;var receiver=document.createElement('{tag}');var value={{toString:function(){{calls++;return 'SECRET_VALUE';}}}};Object.defineProperty(value,'value',{{get:function(){{calls++;return 'SECRET_GETTER';}}}});receiver.src=value;"
        )).unwrap_err();
        assert_eq!(error.message, "Unsupported DOM assignment src");
        assert_eq!(error.offset, None);
        assert_eq!(s.runtime.global_value("calls"), Some(Value::Number(0.)));
        let diagnostic = error.to_string();
        assert!(
            diagnostic.contains(&format!("kind=element tag={tag} value-type=object")),
            "{diagnostic}"
        );
        assert!(diagnostic.contains("node="), "{diagnostic}");
        assert!(!diagnostic.contains("SECRET"), "{diagnostic}");
        assert!(!s.scripts_disabled);
        assert!(
            !s.document
                .nodes
                .iter()
                .any(|node| matches!(&node.kind, NodeKind::Element(e) if e.attr("src").is_some()))
        );
    }
}

#[test]
fn unsupported_dom_get_and_set_keep_js_messages_and_safe_receiver_categories() {
    let mut s = session("<img id=pic>");
    let node = s.query_selector("#pic").unwrap().unwrap();
    let mut host = DomHost {
        document: &mut s.document,
        state: &mut s.state,
    };
    let get = host.get(NODE + node as u64, "src").unwrap_err();
    assert_eq!(get.message, "Unsupported DOM property src");
    assert!(get.to_string().contains("kind=element tag=img"));
    let set = host
        .set(
            NODE + node as u64,
            "src",
            Value::string("https://SECRET/?token=PRIVATE"),
        )
        .unwrap_err();
    assert_eq!(set.message, "Unsupported DOM assignment src");
    assert!(set.to_string().contains("value-type=string"));
    assert!(!set.to_string().contains("SECRET"));
    assert!(!set.to_string().contains("PRIVATE"));
    let text = host
        .document
        .append(0, NodeKind::Text("SECRET_TEXT".into()))
        .unwrap();
    let get = host.get(NODE + text as u64, "src").unwrap_err();
    assert!(get.to_string().contains("kind=text"));
    assert!(!get.to_string().contains("SECRET_TEXT"));
    assert!(
        host.get(u64::MAX, "src")
            .unwrap_err()
            .diagnostic_contexts()
            .is_empty()
    );
}

#[test]
fn dom_diagnostics_bound_tags_and_display_without_changing_property_messages() {
    let mut document = Document::default();
    let node = document
        .append(
            0,
            NodeKind::Element(Element {
                tag: "😀".repeat(1000),
                attributes: BTreeMap::new(),
            }),
        )
        .unwrap();
    let error = unsupported_dom(
        &document,
        node,
        &"😀".repeat(1000),
        Some(&Value::Utf16(std::sync::Arc::from([0xd800]))),
    );
    assert_eq!(
        error.message,
        format!("Unsupported DOM assignment {}", "😀".repeat(1000))
    );
    assert!(callback_warning("timer", None, &error).len() <= MAX_WARNING_BYTES);
    let context = error.diagnostic_contexts().join(" ");
    assert!(context.len() <= 128);
    assert!(context.contains("value-type=string"));
    assert!(context.contains("..."));
}

#[test]
fn timer_diagnostic_identifies_src_receiver_without_fabricating_script_source() {
    let mut s = session(
        "<script>setTimeout(()=>{document.createElement('img').src='PRIVATE_VALUE';},0);</script>",
    );
    let report = s.run_initial(&[]);
    assert_eq!(report.executed, 1);
    assert_eq!(report.warnings.len(), 1);
    let warning = &report.warnings[0];
    assert!(
        warning.contains("phase=timer, source=unavailable"),
        "{warning}"
    );
    assert!(warning.contains("tag=img value-type=string"), "{warning}");
    assert!(!warning.contains("PRIVATE_VALUE"), "{warning}");
    assert!(!warning.contains("line="), "{warning}");
}

#[test]
fn initial_warning_count_and_bytes_stay_bounded() {
    let html = "<script>throw '😀'.repeat(2000);</script>".repeat(64);
    let mut s = session(&html);
    let report = s.run_initial(&[]);
    assert_eq!(report.warnings.len(), 32);
    assert_eq!(report.executed, 0);
    assert!(
        report
            .warnings
            .iter()
            .all(|warning| warning.len() <= MAX_WARNING_BYTES)
    );
}

#[test]
fn secondary_exception_report_keeps_the_original_dom_receiver_context() {
    let mut s = session(
        "<script>var _DumpException=0;try{document.createElement('img').src='PRIVATE_VALUE';}catch(e){_DumpException(e);}</script>",
    );
    let report = s.run_initial(&[]);
    assert_eq!(report.executed, 0);
    assert_eq!(report.warnings.len(), 1);
    let warning = &report.warnings[0];
    assert!(warning.contains("Value is not callable"), "{warning}");
    assert!(warning.contains("previously caught"), "{warning}");
    assert!(
        warning.contains("Unsupported DOM assignment src"),
        "{warning}"
    );
    assert!(warning.contains("tag=img value-type=string"), "{warning}");
    assert!(!warning.contains("PRIVATE_VALUE"), "{warning}");
    assert!(!warning.contains("line="), "{warning}");
}

#[test]
fn caught_dom_messages_preserve_long_properties_and_controls_but_warnings_are_bounded() {
    let mut s = session("");
    s.execute("var property='😀\\n'.repeat(100);var captured;try{document.createElement('img')[property]='PRIVATE_VALUE';}catch(e){captured=e.message;}").unwrap();
    let property = s.runtime.global_value("property").unwrap();
    let property = property.as_str().unwrap();
    assert!(property.contains('\n'));
    assert!((121..=1024).contains(&property.len()));
    assert_eq!(
        s.runtime.global_value("captured"),
        Some(Value::string(format!(
            "Unsupported DOM assignment {property}"
        )))
    );
    let mut s = session(
        "<script>var property='😀\\n'.repeat(100);document.createElement('img')[property]='PRIVATE_VALUE';</script>",
    );
    let report = s.run_initial(&[]);
    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].len() <= MAX_WARNING_BYTES);
    assert!(!report.warnings[0].contains('\n'));
    assert!(!report.warnings[0].contains("PRIVATE_VALUE"));
}

#[test]
fn template_subparser_offsets_are_not_mapped_to_classic_or_module_source() {
    for kind in ["", " type=module"] {
        let mut s = session(&format!(
            "<script{kind}>/*long prefix before nested parser*/var x=`value:${{@}}`;</script>"
        ));
        let report = s.run_initial(&[]);
        assert_eq!(report.warnings.len(), 1);
        let warning = &report.warnings[0];
        assert!(warning.contains("Syntax"), "{warning}");
        assert!(!warning.contains("line="), "{warning}");
        assert!(!warning.contains("column="), "{warning}");
    }
}
