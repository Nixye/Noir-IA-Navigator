//! Background ownership integration on V13 via R1; authored before validation.
use super::*;
use browser_ipc::{Color, DisplayItem, Rect};
use web_javascript::Value;

fn session(html: &str) -> RenderSession {
    RenderSession::new(&DocumentInput {
        url: "noir:background-ownership-recovery-r2".into(),
        html: html.into(),
        images: vec![],
        scripts: vec![],
        stylesheets: vec![],
        audio_metadata: vec![],
    })
    .unwrap()
}
fn rects(page: &RenderedPage, color: Color) -> Vec<Rect> {
    page.items
        .iter()
        .filter_map(|item| match item {
            DisplayItem::Rect { rect, color: found } if *found == color => Some(*rect),
            _ => None,
        })
        .collect()
}

#[test]
fn recovery_r2_direct_block_background_has_no_text_run_copy_after_classlist_raf() {
    let mut s = session(include_str!(
        "../../../fixtures/background-ownership-recovery-r2.html"
    ));
    let initial = s.render(Viewport::default()).unwrap();
    assert_eq!(initial.title, "R2 red");
    assert!(initial.warnings.is_empty());
    // Three distinct red owners: direct panel, inline-layer parent, positioned box.
    let red = rects(&initial, Color(255, 0, 0, 127));
    assert_eq!(red.len(), 3);
    assert_eq!(
        red.iter().map(|r| (r.width, r.height)).collect::<Vec<_>>(),
        vec![(160., 32.), (180., 32.), (80., 32.)]
    );
    let blue = s.animation_frame(1., Viewport::default()).unwrap();
    assert_eq!(blue.title, "R2 blue");
    assert!(blue.warnings.is_empty());
    assert_eq!(rects(&blue, Color(0, 0, 255, 63)), vec![red[0]]);
    assert_eq!(rects(&blue, Color(255, 0, 0, 127)), red[1..]);
    assert_eq!(
        s.session
            .execute("frames===1&&jobs===1&&conversions===1;")
            .unwrap(),
        Value::Bool(true)
    );
    let idle = s.animation_frame(2., Viewport::default()).unwrap();
    assert_eq!(idle.title, "R2 blue");
    assert_eq!(idle.items, blue.items);
}

#[test]
fn recovery_r2_preserves_inline_layer_geometry_and_positioned_clip() {
    let mut s = session(include_str!(
        "../../../fixtures/background-ownership-recovery-r2.html"
    ));
    let page = s.render(Viewport::default()).unwrap();
    let green = rects(&page, Color(0, 255, 0, 127));
    assert_eq!(green.len(), 1);
    let text = page
        .items
        .iter()
        .find_map(|item| match item {
            DisplayItem::Text {
                rect, text, color, ..
            } if text == "Layer" => {
                assert_eq!(color.3, 0);
                Some(*rect)
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(green[0], text);
    let red = rects(&page, Color(255, 0, 0, 127));
    let positioned: Vec<_> = red.iter().filter(|r| r.width == 80.).collect();
    assert_eq!(positioned.len(), 1);
    let positioned = *positioned[0];
    assert_eq!(positioned.height, 32.);
    // The A/B frame shows a normal-flow clip, then a separate reapplication
    // for positioned descendants. Verify the active intersection at paint,
    // not the total number of equivalent PushClip markers in the whole frame.
    let intersect = |a: Rect, b: Rect| {
        let x = a.x.max(b.x);
        let y = a.y.max(b.y);
        Rect {
            x,
            y,
            width: (a.x + a.width).min(b.x + b.width) - x,
            height: (a.y + a.height).min(b.y + b.height) - y,
        }
    };
    let mut stack: Vec<Rect> = Vec::new();
    let mut painted = 0;
    for item in &page.items {
        match item {
            DisplayItem::PushClip { rect } => stack.push(*rect),
            DisplayItem::PopClip => {
                assert!(stack.pop().is_some(), "PopClip without PushClip");
            }
            DisplayItem::Rect { rect, color }
                if *rect == positioned && *color == Color(255, 0, 0, 127) =>
            {
                painted += 1;
                let mut clips = stack.iter().copied();
                let first = clips
                    .next()
                    .expect("positioned box must paint inside a clip");
                let effective = clips.fold(first, intersect);
                assert_eq!((effective.width, effective.height), (80., 32.));
                assert_eq!(positioned.x, effective.x + 60.);
                assert_eq!(positioned.y, effective.y);
                assert_eq!(
                    intersect(positioned, effective),
                    Rect {
                        x: positioned.x,
                        y: positioned.y,
                        width: 20.,
                        height: 32.,
                    }
                );
            }
            _ => {}
        }
    }
    assert_eq!(painted, 1);
    assert!(stack.is_empty(), "unclosed clip at end of frame");
}

#[test]
fn recovery_r2_box_layout_modes_paint_direct_text_background_once() {
    for display in ["block", "inline-block", "flex", "grid"] {
        let mut s = session(&format!(
            "<div style='display:{display};width:160px;height:32px;background:rgba(255,0,0,0.5);color:transparent'>Direct</div>"
        ));
        let page = s.render(Viewport::default()).unwrap();
        assert!(page.warnings.is_empty(), "{display}: {:?}", page.warnings);
        let red = rects(&page, Color(255, 0, 0, 127));
        assert_eq!(red.len(), 1, "{display}");
        assert_eq!((red[0].width, red[0].height), (160., 32.), "{display}");
        assert!(page.items.iter().any(|item| matches!(item,
            DisplayItem::Text { text, color: Color(_, _, _, 0), .. } if text == "Direct")));
    }
}
