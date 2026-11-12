use super::*;
use browser_ipc::ViewContext;
use web_javascript::Value;
fn document() -> DocumentInput {
    DocumentInput{url:"noir:r5-context".into(),html:"<div id=box style='width:20px;height:20px'></div><div id=tall style='height:1000px'></div>".into(),images:vec![],scripts:vec![],stylesheets:vec![],audio_metadata:vec![]}
}
fn viewport() -> Viewport {
    Viewport {
        width: 320.,
        height: 240.,
        view_context: Some(ViewContext::Origin),
    }
}
#[test]
fn invalid_direct_callback_context_fails_before_execution_and_clears_origin() {
    let mut s = RenderSession::new_with_viewport(&document(), viewport()).unwrap();
    s.session.execute("var count=0;var e=document.getElementById('box');e.addEventListener('click',function(){count++;});var saved=e.getBoundingClientRect();").unwrap();
    let bad = Viewport {
        view_context: Some(ViewContext::Sampled {
            scroll_x: f32::INFINITY,
            scroll_y: 0.,
            clip_width: 320.,
            clip_height: 240.,
        }),
        ..viewport()
    };
    let node = s.session.query_selector("#box").unwrap().unwrap() as u64;
    s.dispatch_event(node, "click", viewport()).unwrap();
    assert_eq!(s.session.execute("count;").unwrap(), Value::Number(1.));
    assert!(s.dispatch_event(node, "click", bad).is_err());
    assert_eq!(s.session.execute("count;").unwrap(), Value::Number(1.));
    assert!(s.session.execute("e.getBoundingClientRect();").is_err());
}
#[test]
fn new_document_invalid_context_is_rejected_before_initial_scripts() {
    let mut d = document();
    d.html.push_str("<script>var initialRan=true;</script>");
    let mut valid = RenderSession::new_with_viewport(&d, viewport()).unwrap();
    assert_eq!(
        valid.session.execute("initialRan;").unwrap(),
        Value::Bool(true)
    );
    let bad = Viewport {
        view_context: Some(ViewContext::Sampled {
            scroll_x: 0.,
            scroll_y: 0.,
            clip_width: 0.,
            clip_height: 240.,
        }),
        ..viewport()
    };
    assert!(
        RenderSession::new_with_viewport(&d, bad)
            .err()
            .unwrap()
            .contains("context")
    );
}
#[test]
fn shrink_reconciles_sample_against_new_extents_without_changing_snapshot() {
    let mut s = RenderSession::new_with_viewport(&document(), viewport()).unwrap();
    s.session
        .execute("var e=document.getElementById('box');var saved=e.getBoundingClientRect();")
        .unwrap();
    s.update_geometry(Viewport {
        view_context: Some(ViewContext::Sampled {
            scroll_x: 0.,
            scroll_y: 400.,
            clip_width: 320.,
            clip_height: 240.,
        }),
        ..viewport()
    })
    .unwrap();
    assert_eq!(
        s.session
            .execute("e.getBoundingClientRect().top===saved.top-400;")
            .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(s.session.execute("document.getElementById('tall').style.height='10px';e.getBoundingClientRect().top===saved.top;").unwrap(),Value::Bool(true));
}
