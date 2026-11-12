//! Principal-box coordinates, deliberately independent from paint bounds.
use super::*;
fn page(source: &str) -> (Document, LayoutResult) {
    let d = web_html::parse(&format!("<style>body{{margin:0}}</style>{source}")).unwrap();
    let styles = web_css::compute(&d).unwrap();
    let p = layout(&d, &styles.styles, &[], Viewport::default()).unwrap();
    (d, p)
}
fn id(d: &Document, name: &str) -> NodeId {
    d.elements()
        .find(|(_, e)| e.attr("id") == Some(name))
        .unwrap()
        .0
}
fn metrics(d: &Document, p: &LayoutResult, name: &str) -> BoxMetrics {
    let n = id(d, name);
    assert_eq!(p.unsupported_geometry[n], None);
    p.boxes[n].unwrap()
}
fn near(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.02, "{actual} != {expected}");
}
#[test]
fn fractional_border_box_excludes_margin_and_keeps_padding() {
    let (d, p) = page(
        "<div id=x style='width:40.5px;height:20.25px;margin:3.5px;padding:2.25px;border:1.5px solid red'></div>",
    );
    let m = metrics(&d, &p, "x");
    near(m.x, 3.5);
    near(m.y, 3.5);
    near(m.width, 48.);
    near(m.height, 27.75);
}
#[test]
fn overflow_and_absolute_descendants_never_expand_principal_box() {
    let (d, p) = page(
        "<div id=x style='position:relative;width:40px;height:20px'><div style='width:300px;height:100px'></div><div style='position:absolute;left:500px;top:400px;width:200px;height:100px'></div></div>",
    );
    let m = metrics(&d, &p, "x");
    near(m.width, 40.);
    near(m.height, 20.);
    near(m.x, 0.);
    near(m.y, 0.);
}
#[test]
fn clip_and_hidden_visibility_do_not_erase_border_box() {
    let (d, p) = page(
        "<div style='width:10px;height:10px;overflow:hidden'><div id=x style='visibility:hidden;position:relative;left:40px;top:30px;width:50px;height:60px'></div></div>",
    );
    let m = metrics(&d, &p, "x");
    near(m.x, 40.);
    near(m.y, 30.);
    near(m.width, 50.);
    near(m.height, 60.);
}
#[test]
fn nested_relative_translation_moves_each_box_once() {
    let (d, p) = page(
        "<div id=a style='position:relative;left:10px;top:20px;width:100px'><div id=b style='position:relative;left:3px;top:4px;width:30px;height:10px'><img id=c style='width:5px;height:6px;visibility:hidden'></div></div>",
    );
    let a = metrics(&d, &p, "a");
    let b = metrics(&d, &p, "b");
    let c = metrics(&d, &p, "c");
    near(a.x, 10.);
    near(a.y, 20.);
    near(b.x, 13.);
    near(b.y, 24.);
    near(c.x, 13.);
    near(c.y, 24.);
}
#[test]
fn hidden_replaced_inline_participates_in_center_line_alignment() {
    let (d, p) = page(
        "<div style='width:100px;text-align:center'><img id=x style='width:20px;height:10px;visibility:hidden'></div>",
    );
    let m = metrics(&d, &p, "x");
    near(m.x, 40.);
    near(m.width, 20.);
    assert!(
        !p.items
            .iter()
            .any(|i| matches!(i, DisplayItem::Image { .. }))
    );
}
#[test]
fn inline_block_parent_does_not_take_its_inner_text_alignment() {
    let (d, p) = page(
        "<div style='width:200px;text-align:center'><div id=x style='display:inline-block;width:100px;text-align:right'><img id=i style='width:20px;height:10px'></div></div>",
    );
    let m = metrics(&d, &p, "x");
    let i = metrics(&d, &p, "i");
    near(m.x, 50.);
    near(i.x, 130.);
}
#[test]
fn flex_end_alignment_and_stretch_are_final() {
    let (d, p) = page(
        "<div style='display:flex;width:200px;height:100px;align-items:flex-end'><div id=x style='width:30px;height:20px'></div></div><div style='display:flex;width:200px;height:80px;align-items:stretch'><div id=y style='width:30px'></div></div>",
    );
    let x = metrics(&d, &p, "x");
    let y = metrics(&d, &p, "y");
    near(x.y, 80.);
    near(x.height, 20.);
    near(y.y, 100.);
    near(y.height, 80.);
}
#[test]
fn grid_end_alignment_and_column_flex_distribution_move_geometry() {
    let (d, p) = page(
        "<div style='display:grid;grid-template-columns:50px 50px;align-items:end'><div style='height:40px'></div><div id=x style='height:10px'></div></div><div style='display:flex;flex-direction:column;height:100px;justify-content:flex-end'><div id=y style='height:20px'></div></div>",
    );
    near(metrics(&d, &p, "x").y, 30.);
    near(metrics(&d, &p, "y").y, 120.);
}
#[test]
fn supported_control_geometry_matches_painted_control_and_hidden_survives() {
    let (d, p) = page(
        "<input id=x style='margin:3px;width:20px;height:10px;padding:2px;border:1px solid red'><input id=y style='visibility:hidden;width:20px;height:10px'>",
    );
    let x = metrics(&d, &p, "x");
    let y = metrics(&d, &p, "y");
    let rect = p
        .items
        .iter()
        .find_map(|i| match i {
            DisplayItem::Control { rect, .. } => Some(rect),
            _ => None,
        })
        .unwrap();
    near(x.x, rect.x);
    near(x.y, rect.y);
    near(x.width, rect.width);
    near(x.height, rect.height);
    assert!(y.width > 0.);
}
#[test]
fn no_box_is_distinct_from_unsupported_geometry() {
    let (d, p) = page(
        "<div style='display:none'><span id=none>x</span></div><input id=hidden type=hidden><span id=inline>x</span><button><span id=atomic>x</span><span id=atomicnone style='display:none'>x</span></button><table><tbody id=group><tr id=row><td id=cell>x</td></tr></tbody></table>",
    );
    for name in ["none", "hidden", "atomicnone"] {
        let n = id(&d, name);
        assert!(p.boxes[n].is_none());
        assert_eq!(p.unsupported_geometry[n], None);
    }
    assert_eq!(
        p.unsupported_geometry[id(&d, "inline")],
        Some(UnsupportedGeometry::Inline)
    );
    assert_eq!(
        p.unsupported_geometry[id(&d, "atomic")],
        Some(UnsupportedGeometry::AtomicContents)
    );
    for name in ["group", "row"] {
        assert_eq!(
            p.unsupported_geometry[id(&d, name)],
            Some(UnsupportedGeometry::TableStructure)
        );
    }
    assert!(p.boxes[id(&d, "cell")].is_some());
}
#[test]
fn inline_image_decoration_is_explicitly_unsupported_but_block_image_has_box() {
    let (d, p) = page(
        "<img id=plain style='width:10px;height:5px'><img id=decorated style='width:10px;height:5px;padding:2px'><img id=block style='display:block;width:10px;height:5px;padding:2px;border:1px solid red'>",
    );
    near(metrics(&d, &p, "plain").width, 10.);
    assert_eq!(
        p.unsupported_geometry[id(&d, "decorated")],
        Some(UnsupportedGeometry::InlineImageDecoration)
    );
    near(metrics(&d, &p, "block").width, 16.);
    near(metrics(&d, &p, "block").height, 11.);
}
#[test]
fn geometry_allocation_rejects_dom_over_limit_before_traversal() {
    let mut d = web_html::parse("").unwrap();
    d.nodes = vec![d.nodes[0].clone(); web_dom::MAX_NODES + 1];
    let styles = vec![Style::default(); d.nodes.len()];
    assert!(layout(&d, &styles, &[], Viewport::default()).is_err());
}

#[test]
fn positioned_right_bottom_use_parent_box_not_overflow_union() {
    let (d, p) = page(
        "<div id=parent style='position:relative;width:40px;height:20px'><div style='width:300px;height:100px'></div><div id=child style='position:absolute;right:0;bottom:0;width:5px;height:6px;background:#123456'></div></div>",
    );
    let parent = metrics(&d, &p, "parent");
    let child = metrics(&d, &p, "child");
    near(parent.width, 40.);
    near(parent.height, 20.);
    near(child.x, 35.);
    near(child.y, 14.);
    near(child.width, 5.);
    near(child.height, 6.);
    let painted = p
        .items
        .iter()
        .find_map(|item| match item {
            DisplayItem::Rect { rect, color } if *color == Color(18, 52, 86, 255) => Some(rect),
            _ => None,
        })
        .expect("child background is painted");
    near(painted.x, 35.);
    near(painted.y, 14.);
    near(painted.width, 5.);
    near(painted.height, 6.);
}
