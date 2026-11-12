use super::*;
use crate::ErrorKind;
use std::sync::atomic::{AtomicUsize, Ordering};
#[derive(Debug)]
struct Provider {
    calls: Arc<AtomicUsize>,
    geometry: BoundingGeometry,
    client: bool,
    invalid: u8,
}
impl GeometryResolver for Provider {
    fn viewport(&self) -> Viewport {
        Viewport::default()
    }
    fn resolve(&self, document: &Document) -> Result<GeometrySnapshot, JsError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut nodes = vec![NodeGeometry::default(); document.nodes.len()];
        for (id, e) in document.elements() {
            if e.attr("id") == Some("target") {
                nodes[id] = NodeGeometry {
                    client_width: 20.,
                    client_height: 30.,
                    client_left: 1.,
                    client_top: 1.,
                    client_available: self.client,
                    bounding: self.geometry,
                };
            }
        }
        let mut snapshot = GeometrySnapshot {
            nodes,
            document_width: 400.,
            document_height: 600.,
        };
        match self.invalid {
            1 => {
                snapshot.nodes.pop();
            }
            2 => snapshot.document_height = f32::NAN,
            3 => {
                snapshot.nodes[0].bounding = BoundingGeometry::Box(Rect {
                    x: f32::INFINITY,
                    y: 0.,
                    width: 1.,
                    height: 1.,
                })
            }
            4 => snapshot.nodes[0].client_width = -1.,
            _ => {}
        }
        Ok(snapshot)
    }
}
fn session(geometry: BoundingGeometry) -> (DocumentSession, Arc<AtomicUsize>) {
    let mut s = DocumentSession::new(web_html::parse("<div id=target></div>").unwrap());
    let calls = Arc::new(AtomicUsize::new(0));
    s.set_geometry_resolver(Arc::new(Provider {
        calls: calls.clone(),
        geometry,
        client: true,
        invalid: 0,
    }));
    (s, calls)
}
fn principal() -> BoundingGeometry {
    BoundingGeometry::Box(Rect {
        x: 50.,
        y: 100.,
        width: 20.,
        height: 30.,
    })
}
fn origin(s: &mut DocumentSession) {
    s.set_view_context(Some(ViewContext::Origin)).unwrap();
}
#[test]
fn direct_extracted_and_borrowed_method_return_fresh_own_data_snapshots() {
    let (mut s, _) = session(principal());
    origin(&mut s);
    assert_eq!(s.execute("const e=document.getElementById('target');const a=e.getBoundingClientRect();const f=e.getBoundingClientRect;const b=f.call(e);a!==b&&a.x===50&&a.y===100&&a.width===20&&a.height===30&&a.left===50&&a.top===100&&a.right===70&&a.bottom===130&&b.x===a.x&&Object.keys(a).length===8&&Object.hasOwn(a,'x')&&f.name==='getBoundingClientRect'&&f.length===0;").unwrap(),Value::Bool(true));
}
#[test]
fn numeric_snapshot_mutation_is_local_and_never_invokes_prototype_setter() {
    let (mut s, _) = session(principal());
    origin(&mut s);
    assert_eq!(s.execute("let called=0;Object.defineProperty(Object.prototype,'x',{set:function(){called++;},configurable:true});const e=document.getElementById('target');const a=e.getBoundingClientRect();a.x=999;const b=e.getBoundingClientRect();called===0&&a.x===999&&b.x===50;").unwrap(),Value::Bool(true));
}
#[test]
fn unknown_origin_is_not_zero_and_invalid_context_clears_prior_origin() {
    let (mut s, calls) = session(principal());
    assert!(
        s.execute("document.getElementById('target').getBoundingClientRect();")
            .is_err()
    );
    origin(&mut s);
    assert_eq!(
        s.execute("document.getElementById('target').getBoundingClientRect().x;")
            .unwrap(),
        Value::Number(50.)
    );
    assert!(
        s.set_view_context(Some(ViewContext::Sampled {
            scroll_x: f32::NAN,
            scroll_y: 0.,
            clip_width: 10.,
            clip_height: 10.
        }))
        .is_err()
    );
    assert!(
        s.execute("document.getElementById('target').getBoundingClientRect();")
            .is_err()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
#[test]
fn sixty_five_scroll_updates_reuse_document_coordinates_and_old_snapshot() {
    let (mut s, calls) = session(principal());
    origin(&mut s);
    s.execute("var e=document.getElementById('target');var old=e.getBoundingClientRect();")
        .unwrap();
    for scroll in 0..70 {
        s.set_view_context(Some(ViewContext::Sampled {
            scroll_x: 0.,
            scroll_y: scroll as f32,
            clip_width: 100.,
            clip_height: 100.,
        }))
        .unwrap();
        assert_eq!(
            s.execute("e.getBoundingClientRect().top;").unwrap(),
            Value::Number(100. - f64::from(scroll))
        );
    }
    assert_eq!(s.execute("old.top;").unwrap(), Value::Number(100.));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
#[test]
fn sampled_scroll_clamps_against_extent_and_real_clip_not_layout_viewport() {
    let (mut s, _) = session(principal());
    s.set_view_context(Some(ViewContext::Sampled {
        scroll_x: 999.,
        scroll_y: 999.,
        clip_width: 390.,
        clip_height: 550.,
    }))
    .unwrap();
    assert_eq!(
        s.execute(
            "const b=document.getElementById('target').getBoundingClientRect();b.x===40&&b.y===50;"
        )
        .unwrap(),
        Value::Bool(true)
    );
}
#[test]
fn negative_border_coordinates_are_valid_not_clipped() {
    let (mut s, _) = session(BoundingGeometry::Box(Rect {
        x: -20.,
        y: -30.,
        width: 100.,
        height: 90.,
    }));
    origin(&mut s);
    assert_eq!(s.execute("const b=document.getElementById('target').getBoundingClientRect();b.left===-20&&b.top===-30&&b.right===80&&b.bottom===60;").unwrap(),Value::Bool(true));
}
#[test]
fn no_box_zero_does_not_require_known_origin_but_unsupported_is_explicit() {
    let (mut s, _) = session(BoundingGeometry::NoBox);
    assert_eq!(s.execute("const b=document.getElementById('target').getBoundingClientRect();b.x===0&&b.y===0&&b.width===0&&b.height===0;").unwrap(),Value::Bool(true));
    for reason in [
        UnsupportedGeometry::Inline,
        UnsupportedGeometry::TableStructure,
        UnsupportedGeometry::InlineImageDecoration,
        UnsupportedGeometry::AtomicContents,
    ] {
        let (mut s, _) = session(BoundingGeometry::Unsupported(reason));
        origin(&mut s);
        let error = s
            .execute("document.getElementById('target').getBoundingClientRect();")
            .unwrap_err();
        assert!(error.message.contains("not implemented"));
    }
}
#[test]
fn client_unavailable_does_not_block_real_bounding_box() {
    let (mut s, calls) = session(principal());
    origin(&mut s);
    s.set_geometry_resolver(Arc::new(Provider {
        calls,
        geometry: principal(),
        client: false,
        invalid: 0,
    }));
    assert!(
        s.execute("document.getElementById('target').clientWidth;")
            .is_err()
    );
    assert_eq!(
        s.execute("document.getElementById('target').getBoundingClientRect().width;")
            .unwrap(),
        Value::Number(20.)
    );
}
#[test]
fn invalid_provider_counts_extents_rects_and_client_metrics_fail_closed() {
    for invalid in 1..=4 {
        let (mut s, calls) = session(principal());
        origin(&mut s);
        s.set_geometry_resolver(Arc::new(Provider {
            calls,
            geometry: principal(),
            client: true,
            invalid,
        }));
        assert_eq!(
            s.execute("document.getElementById('target').getBoundingClientRect();")
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
    }
}
#[test]
fn element_brand_rejects_document_text_plain_and_unbound_receiver() {
    let (mut s, calls) = session(principal());
    origin(&mut s);
    assert_eq!(s.execute("const e=document.getElementById('target');const f=e.getBoundingClientRect;const t=document.createTextNode('x');let errors=0;try{f.call(document);}catch(x){errors++;}try{f.call(t);}catch(x){errors++;}try{f.call({});}catch(x){errors++;}try{f();}catch(x){errors++;}errors===4&&('getBoundingClientRect' in e)&&!('getBoundingClientRect' in t);").unwrap(),Value::Bool(true));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
#[test]
fn mutation_invalidates_layout_and_sixty_fifth_resolution_is_resource_error() {
    let (mut s, calls) = session(principal());
    origin(&mut s);
    s.execute("var e=document.getElementById('target');")
        .unwrap();
    for n in 0..64 {
        assert_eq!(
            s.execute(&format!(
                "e.setAttribute('data-step','{n}');e.getBoundingClientRect().x;"
            ))
            .unwrap(),
            Value::Number(50.)
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 64);
    assert_eq!(
        s.execute("e.setAttribute('data-step','overflow');e.getBoundingClientRect();")
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
    assert_eq!(calls.load(Ordering::SeqCst), 64);
    assert!(s.scripts_disabled);
}
#[test]
fn resource_rollback_restores_dom_and_preexisting_snapshot_handle() {
    let (mut s, _) = session(principal());
    origin(&mut s);
    s.execute("var e=document.getElementById('target');var snapshot=e.getBoundingClientRect();")
        .unwrap();
    let saved = s.runtime.global_value("snapshot").unwrap();
    assert_eq!(
        s.execute("snapshot.x=999;e.id='changed';while(true){}")
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
    assert!(
        s.document
            .elements()
            .any(|(_, e)| e.attr("id") == Some("target"))
    );
    assert_eq!(s.runtime.global_value("snapshot"), Some(saved));
    assert!(s.scripts_disabled);
}
