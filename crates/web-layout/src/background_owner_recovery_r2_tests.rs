//! Principal-box background ownership, not complete inline fragmentation.
use super::*;
const RED: Color = Color(255, 0, 0, 127);
const BLUE: Color = Color(0, 0, 255, 127);
fn document(source: &str) -> Document {
    web_html::parse(&format!("<style>body{{margin:0}}</style>{source}")).unwrap()
}
fn page(source: &str) -> LayoutResult {
    let d = document(source);
    let styles = web_css::compute(&d).unwrap();
    layout(&d, &styles.styles, &[], Viewport::default()).unwrap()
}
fn colored(p: &LayoutResult, color: Color) -> Vec<Rect> {
    p.items
        .iter()
        .filter_map(|item| match item {
            DisplayItem::Rect {
                rect,
                color: actual,
            } if *actual == color => Some(*rect),
            _ => None,
        })
        .collect()
}
fn texts(p: &LayoutResult) -> Vec<&str> {
    p.items
        .iter()
        .filter_map(|item| match item {
            DisplayItem::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}
fn engine<'a>(document: &'a Document, styles: &'a [Style]) -> Layout<'a> {
    Layout {
        document,
        styles,
        images: &[],
        items: vec![],
        links: vec![],
        link_clips: vec![],
        owned_clips: BTreeMap::new(),
        clip_depth: 0,
        warnings: vec![],
        max_x: 1024.,
        measure_budget: 4_000_000,
        positioned: vec![PositionFrame::default()],
        position_budget: document.nodes.len(),
        height_context: vec![Some(768.)],
        control_count: 0,
        boxes: vec![None; document.nodes.len()],
        unsupported_geometry: vec![None; document.nodes.len()],
        geometry_order: Vec::new(),
        background_owner: None,
    }
}
#[test]
fn principal_background_is_painted_once_with_or_without_direct_text() {
    for content in ["", "Text", "One two three"] {
        let p = page(&format!(
            "<div style='background:rgba(255,0,0,.5);width:120px;height:24px'>{content}</div>"
        ));
        let rects = colored(&p, RED);
        assert_eq!(rects.len(), 1, "{content}");
        assert_eq!((rects[0].width, rects[0].height), (120., 24.));
    }
}
#[test]
fn nested_boxes_restore_owner_for_text_before_and_after_child() {
    let p = page(
        "<div style='background:rgba(255,0,0,.5)'>before<div style='background:rgba(0,0,255,.5)'>inside</div>after</div>",
    );
    assert_eq!(colored(&p, RED).len(), 1);
    assert_eq!(colored(&p, BLUE).len(), 1);
    assert_eq!(texts(&p), ["before", "inside", "after"]);
}
#[test]
fn transparent_inline_children_do_not_repaint_principal_background() {
    let p = page("<div style='background:rgba(255,0,0,.5)'>before<span>middle</span>after</div>");
    assert_eq!(colored(&p, RED).len(), 1);
    assert_eq!(texts(&p), ["before", "middle", "after"]);
}
#[test]
fn inline_span_and_link_keep_their_own_run_backgrounds() {
    let p = page(
        "<div style='background:rgba(255,0,0,.5)'>before<span style='background:rgba(0,0,255,.5)'>Span</span><a href='/target' style='background:rgba(0,0,255,.5)'>Link</a>after</div>",
    );
    assert_eq!(colored(&p, RED).len(), 1);
    assert_eq!(colored(&p, BLUE).len(), 2);
    assert_eq!(p.links.len(), 1);
    assert_eq!(p.links[0].href, "/target");
    assert_eq!(texts(&p), ["before", "Span", "Link", "after"]);
}
#[test]
fn inline_block_paints_its_box_once_and_keeps_inner_inline_background() {
    let p = page(
        "<span style='display:inline-block;background:rgba(255,0,0,.5);width:120px'>direct<span style='background:rgba(0,0,255,.5)'>nested</span>tail</span>",
    );
    assert_eq!(colored(&p, RED).len(), 1);
    assert_eq!(colored(&p, BLUE).len(), 1);
    assert_eq!(texts(&p), ["direct", "nested", "tail"]);
}
#[test]
fn flex_and_grid_anonymous_text_do_not_repaint_container_background() {
    for display in ["flex", "grid"] {
        let p = page(&format!(
            "<div style='display:{display};width:200px;background:rgba(255,0,0,.5)'>Text</div>"
        ));
        assert_eq!(colored(&p, RED).len(), 1, "{display}");
        assert_eq!(texts(&p), ["Text"]);
    }
}
#[test]
fn flex_and_grid_blockified_inline_items_own_their_background() {
    for display in ["flex", "grid"] {
        let d = document(&format!(
            "<div style='display:{display};width:200px'><span id=target style='background:rgba(255,0,0,.5)'>Text</span></div>"
        ));
        let styles = web_css::compute(&d).unwrap();
        let id = d
            .elements()
            .find(|(_, e)| e.attr("id") == Some("target"))
            .unwrap()
            .0;
        assert_eq!(styles.styles[id].display, Display::Inline);
        let p = layout(&d, &styles.styles, &[], Viewport::default()).unwrap();
        assert!(p.boxes[id].is_some());
        assert_eq!(colored(&p, RED).len(), 1, "{display}");
    }
}
#[test]
fn absolute_inline_and_relative_block_own_their_backgrounds() {
    let p = page(
        "<div style='position:relative;width:200px;height:80px;background:rgba(0,0,255,.5)'>direct<span style='position:absolute;left:10px;top:20px;width:80px;height:24px;background:rgba(255,0,0,.5)'>Absolute</span>after</div>",
    );
    assert_eq!(colored(&p, BLUE).len(), 1);
    let red = colored(&p, RED);
    assert_eq!(red.len(), 1);
    assert_eq!(
        (red[0].x, red[0].y, red[0].width, red[0].height),
        (10., 20., 80., 24.)
    );
    assert!(texts(&p).contains(&"Absolute"));
}
#[test]
fn relative_inline_preserves_run_background_without_becoming_principal_box() {
    let p = page(
        "<div><span style='position:relative;left:7px;top:3px;background:rgba(255,0,0,.5)'>Inline</span></div>",
    );
    let red = colored(&p, RED);
    assert_eq!(red.len(), 1);
    assert_eq!((red[0].x, red[0].y), (7., 3.));
    let text = p
        .items
        .iter()
        .find_map(|i| match i {
            DisplayItem::Text { rect, text, .. } if text == "Inline" => Some(*rect),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        (text.x, text.y, text.width, text.height),
        (red[0].x, red[0].y, red[0].width, red[0].height)
    );
}
#[test]
fn wrapping_and_preserved_spaces_do_not_multiply_box_background() {
    for whitespace in ["normal", "pre", "pre-wrap", "pre-line"] {
        let p = page(&format!(
            "<div style='width:35px;white-space:{whitespace};background:rgba(255,0,0,.5)'>AA BB\nCC DD</div>"
        ));
        assert_eq!(colored(&p, RED).len(), 1, "{whitespace}");
        assert!(texts(&p).len() >= 4, "{whitespace}");
    }
}
#[test]
fn clipping_stays_balanced_and_link_geometry_remains_inside_box() {
    let p = page(
        "<div style='width:40px;height:12px;overflow:hidden;background:rgba(255,0,0,.5)'><a href='/target'>Link</a></div>",
    );
    assert_eq!(colored(&p, RED).len(), 1);
    let mut depth = 0usize;
    let mut clips = 0;
    for item in &p.items {
        match item {
            DisplayItem::PushClip { .. } => {
                depth += 1;
                clips += 1;
            }
            DisplayItem::PopClip => {
                assert!(depth > 0);
                depth -= 1;
            }
            _ => {}
        }
    }
    assert!(clips > 0);
    assert_eq!(depth, 0);
    assert_eq!(p.links.len(), 1);
    assert!(p.links[0].rect.y >= 0. && p.links[0].rect.y + p.links[0].rect.height <= 12.);
    assert!(p.links[0].rect.x + p.links[0].rect.width <= 40.);
}
#[test]
fn hidden_principal_does_not_hide_explicitly_visible_inline_background() {
    let p = page(
        "<div style='visibility:hidden;background:rgba(255,0,0,.5)'>hidden<span style='visibility:visible;background:rgba(0,0,255,.5)'>visible</span></div>",
    );
    assert!(colored(&p, RED).is_empty());
    assert_eq!(colored(&p, BLUE).len(), 1);
    assert_eq!(texts(&p), ["visible"]);
}
#[test]
fn computed_text_styles_remain_intact_while_decoration_is_suppressed() {
    let d = document(
        "<div style='background:rgba(255,0,0,.5);font-size:20px;color:#123456'>Text</div>",
    );
    let styles = web_css::compute(&d).unwrap();
    let id = d
        .nodes
        .iter()
        .position(|n| matches!(&n.kind,NodeKind::Text(s) if s=="Text"))
        .unwrap();
    assert_eq!(styles.styles[id].background, RED);
    let p = layout(&d, &styles.styles, &[], Viewport::default()).unwrap();
    assert_eq!(styles.styles[id].background, RED);
    assert_eq!(colored(&p, RED).len(), 1);
    assert!(p.items.iter().any(|i| matches!(i,DisplayItem::Text { text,size,color,.. } if text=="Text" && *size==20. && *color==Color(18,52,86,255))));
}
#[test]
fn background_does_not_change_text_metrics_links_or_principal_boxes() {
    let d = document(
        "<div style='width:120px;text-align:center;background:rgba(255,0,0,.5)'>Text<a href='/target'>Link</a></div>",
    );
    let styles = web_css::compute(&d).unwrap();
    let mut clear = styles.styles.clone();
    for s in &mut clear {
        s.background = Color(0, 0, 0, 0);
    }
    let colored = layout(&d, &styles.styles, &[], Viewport::default()).unwrap();
    let transparent = layout(&d, &clear, &[], Viewport::default()).unwrap();
    let geometry = |p: &LayoutResult| {
        p.items
            .iter()
            .filter(|i| !matches!(i, DisplayItem::Rect { .. }))
            .map(|i| format!("{i:?}"))
            .collect::<Vec<_>>()
    };
    assert_eq!(geometry(&colored), geometry(&transparent));
    assert_eq!(
        format!("{:?}", colored.links),
        format!("{:?}", transparent.links)
    );
    assert_eq!(
        format!("{:?}", colored.boxes),
        format!("{:?}", transparent.boxes)
    );
}
#[test]
fn principal_owner_restores_after_success_and_display_list_limit_error() {
    let d = document("<div id=target style='background:rgba(255,0,0,.5)'>Text</div>");
    let styles = web_css::compute(&d).unwrap();
    let id = d
        .elements()
        .find(|(_, e)| e.attr("id") == Some("target"))
        .unwrap()
        .0;
    let mut e = engine(&d, &styles.styles);
    e.background_owner = Some(0);
    e.block(id, 0., 0., 120., None, false).unwrap();
    assert_eq!(e.background_owner, Some(0));
    e.items = vec![
        DisplayItem::Rect {
            rect: Rect {
                x: 0.,
                y: 0.,
                width: 1.,
                height: 1.
            },
            color: RED
        };
        MAX_ITEMS
    ];
    assert_eq!(
        e.block(id, 0., 0., 120., None, false).unwrap_err(),
        "Display list item limit exceeded"
    );
    assert_eq!(e.background_owner, Some(0));
    assert_eq!(e.items.len(), MAX_ITEMS);
}
#[test]
fn principal_owner_restores_after_nested_text_work_error() {
    let d = document("<div id=target><div>inside</div>after</div>");
    let styles = web_css::compute(&d).unwrap();
    let id = d
        .elements()
        .find(|(_, e)| e.attr("id") == Some("target"))
        .unwrap()
        .0;
    let mut e = engine(&d, &styles.styles);
    e.background_owner = Some(0);
    e.measure_budget = 0;
    assert!(
        e.block(id, 0., 0., 120., None, false)
            .unwrap_err()
            .contains("work limit")
    );
    assert_eq!(e.background_owner, Some(0));
}
