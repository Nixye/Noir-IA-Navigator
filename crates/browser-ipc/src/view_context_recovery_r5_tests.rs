use super::*;
#[test]
fn absent_context_is_unknown_and_origin_is_explicit() {
    assert!(Viewport::default().view_context.is_none());
    let omitted: Viewport = serde_json::from_str(r#"{"width":100,"height":80}"#).unwrap();
    assert!(omitted.view_context.is_none());
    assert_eq!(
        Viewport::default().at_origin().view_context,
        Some(ViewContext::Origin)
    );
}
#[test]
fn sampled_context_roundtrips_and_protocol_is_ten() {
    let viewport = Viewport {
        width: 100.,
        height: 80.,
        view_context: Some(ViewContext::Sampled {
            scroll_x: 10.,
            scroll_y: 30.,
            clip_width: 88.,
            clip_height: 70.,
        }),
    };
    let request = SessionRequest::Reflow { id: 44, viewport };
    let mut data = Vec::new();
    write_message(&mut data, &request).unwrap();
    let decoded: SessionRequest = read_message(&mut data.as_slice()).unwrap();
    let SessionRequest::Reflow {
        viewport: decoded, ..
    } = decoded
    else {
        panic!("reflow")
    };
    assert_eq!(decoded.view_context, viewport.view_context);
    assert_eq!(PROTOCOL_VERSION, 10);
}
#[test]
fn context_validation_rejects_nan_infinity_negatives_and_bad_clip() {
    for value in [f32::NAN, f32::INFINITY, -1., 1_000_001.] {
        assert!(
            validate_view_context(Some(ViewContext::Sampled {
                scroll_x: value,
                scroll_y: 0.,
                clip_width: 1.,
                clip_height: 1.
            }))
            .is_err()
        );
    }
    for value in [f32::NAN, f32::INFINITY, -1., 0., 16385.] {
        assert!(
            validate_view_context(Some(ViewContext::Sampled {
                scroll_x: 0.,
                scroll_y: 0.,
                clip_width: 1.,
                clip_height: value
            }))
            .is_err()
        );
    }
    assert!(validate_view_context(None).is_ok());
    assert!(validate_view_context(Some(ViewContext::Origin)).is_ok());
}
