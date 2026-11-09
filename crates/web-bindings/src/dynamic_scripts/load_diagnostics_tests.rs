use super::tests::{check, response, session};
use super::*;

#[test]
fn failed_load_produces_diagnostic_without_exposing_requested_source() {
    for src in [
        "https://user:PRIVATE_PASSWORD@cdn.example.test/PRIVATE_PATH.js?token=PRIVATE_QUERY#PRIVATE_FRAGMENT",
        "file:///home/PRIVATE_USER/PRIVATE_FILE.js",
        "data:text/javascript,PRIVATE_SOURCE",
        "./PRIVATE_RELATIVE.js?PRIVATE_QUERY",
    ] {
        let mut s = session();
        s.execute(&format!("var events='';var node=document.createElement('script');node.src='{src}';node.onerror=function(){{events+='error';}};document.head.appendChild(node);")).unwrap();
        let requests = s.take_script_requests();
        let report = s
            .complete_scripts(&[response(requests[0].id, None)])
            .unwrap();
        assert_eq!(report.executed, 0);
        assert_eq!(report.warnings.len(), 1);
        let warning = &report.warnings[0];
        assert!(
            warning.starts_with("JavaScript dynamic script load failed"),
            "{warning}"
        );
        assert!(warning.contains("phase=load"), "{warning}");
        assert!(
            warning.contains(&format!("request={}", requests[0].id)),
            "{warning}"
        );
        assert!(warning.contains("node="), "{warning}");
        assert!(!warning.contains("PRIVATE"), "{warning}");
        assert!(!warning.contains(src), "{warning}");
        assert!(warning.len() <= diagnostics::MAX_WARNING_BYTES);
        check(&mut s, "events==='error';");
    }
}

#[test]
fn failed_load_and_failed_error_handler_are_distinct_and_not_replayed() {
    let mut s = session();
    s.execute("var events=0;var node=document.createElement('script');node.src='missing.js';node.onerror=function(){events++;throw 'ordinary';};document.head.appendChild(node);").unwrap();
    let requests = s.take_script_requests();
    let responses = [response(requests[0].id, None)];
    let report = s.complete_scripts(&responses).unwrap();
    assert_eq!(report.warnings.len(), 2);
    assert!(report.warnings[0].contains("phase=load"));
    assert!(report.warnings[1].contains("error callback"));
    assert!(s.complete_scripts(&responses).is_err());
    check(&mut s, "events===1;");
    assert!(s.take_script_requests().is_empty());
}

#[test]
fn failed_load_diagnostics_share_bounded_report_budget_with_callbacks() {
    let mut s = session();
    s.execute("var events=0;for(var i=0;i<32;i++){var node=document.createElement('script');node.src='missing.js';node.onerror=function(){events++;throw 'ordinary';};document.head.appendChild(node);}").unwrap();
    let requests = s.take_script_requests();
    let responses = requests
        .iter()
        .map(|request| response(request.id, None))
        .collect::<Vec<_>>();
    let report = s.complete_scripts(&responses).unwrap();
    assert_eq!(report.warnings.len(), 32);
    assert_eq!(report.executed, 0);
    assert!(
        report
            .warnings
            .iter()
            .all(|warning| warning.len() <= diagnostics::MAX_WARNING_BYTES)
    );
    check(&mut s, "events===32;");
}

#[test]
fn successful_load_has_no_unavailability_warning_even_if_execution_throws() {
    let mut s = session();
    s.execute("var events='';var node=document.createElement('script');node.src='ok.js';node.onload=function(){events+='load';};document.head.appendChild(node);").unwrap();
    let requests = s.take_script_requests();
    let report = s
        .complete_scripts(&[response(requests[0].id, Some("throw 'ordinary';"))])
        .unwrap();
    assert_eq!(report.warnings.len(), 1);
    assert!(!report.warnings[0].contains("phase=load"));
    check(&mut s, "events==='load';");
}
