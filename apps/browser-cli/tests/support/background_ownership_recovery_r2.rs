//! CPU alpha oracles through a retained real worker; no network or GUI.
use browser_ipc::{Color, DisplayItem, DocumentInput, Rect, RenderedPage, Viewport};
use browser_process::RendererSession;

fn document(html: &str) -> DocumentInput {
    DocumentInput {
        url: "noir:background-ownership-recovery-r2".into(),
        html: html.into(),
        images: vec![],
        scripts: vec![],
        stylesheets: vec![],
        audio_metadata: vec![],
    }
}
fn pixel(surface: &web_paint::Surface, x: f32, y: f32) -> [u8; 3] {
    let offset = ((y as usize * surface.width as usize) + x as usize) * 3;
    surface.rgb[offset..offset + 3].try_into().unwrap()
}
fn owner(page: &RenderedPage, color: Color, width: f32) -> Rect {
    let boxes: Vec<_> = page
        .items
        .iter()
        .filter_map(|item| match item {
            DisplayItem::Rect { rect, color: found }
                if *found == color && rect.width == width && rect.height == 32. =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .collect();
    assert_eq!(boxes.len(), 1);
    boxes[0]
}
fn pixels(page: &RenderedPage, blue: bool) {
    assert_eq!(page.background, Color(255, 255, 255, 255));
    assert!(page.warnings.is_empty(), "{:?}", page.warnings);
    let surface = web_paint::rasterize(page, &[], 320, 240).unwrap();
    let color = if blue {
        Color(0, 0, 255, 63)
    } else {
        Color(255, 0, 0, 127)
    };
    let panel = owner(page, color, 160.);
    // Alpha .5 truncates to 127, .25 to 63. Integer blending over white
    // yields red [255,128,128] or blue [192,192,255], once per box.
    let expected = if blue {
        [192, 192, 255]
    } else {
        [255, 128, 128]
    };
    let direct = page
        .items
        .iter()
        .find_map(|item| match item {
            DisplayItem::Text {
                rect, text, color, ..
            } if text == "Direct" => {
                assert_eq!(color.3, 0);
                Some(*rect)
            }
            _ => None,
        })
        .unwrap();
    assert!(direct.width < panel.width);
    // Compare a region beneath the formerly duplicated run with the box remainder.
    for dy in 2..20 {
        for dx in 2..20 {
            assert_eq!(
                pixel(&surface, direct.x + dx as f32, direct.y + dy as f32),
                expected
            );
            assert_eq!(
                pixel(&surface, panel.x + 120. + dx as f32, panel.y + dy as f32),
                expected
            );
        }
    }
    let parent = owner(page, Color(255, 0, 0, 127), 180.);
    let inline = page
        .items
        .iter()
        .find_map(|item| match item {
            DisplayItem::Rect {
                rect,
                color: Color(0, 255, 0, 127),
            } => Some(*rect),
            _ => None,
        })
        .unwrap();
    // Legitimate green inline layer over red: truncation occurs at EACH layer.
    assert_eq!(
        pixel(&surface, inline.x + 2., inline.y + 2.),
        [128, 191, 64]
    );
    assert_eq!(
        pixel(&surface, parent.x + 160., parent.y + 2.),
        [255, 128, 128]
    );
    let positioned = owner(page, Color(255, 0, 0, 127), 80.);
    assert_eq!(
        pixel(&surface, positioned.x + 2., positioned.y + 2.),
        [255, 128, 128]
    );
    // Only the first 20px of the positioned child lie inside its 80px parent clip.
    assert_eq!(
        pixel(&surface, positioned.x + 30., positioned.y + 2.),
        [255, 255, 255]
    );
}

#[test]
fn recovery_r2_worker_alpha_regions_update_once_with_retained_classlist_raf() {
    let viewport = Viewport {
        width: 320.,
        height: 240.,
        view_context: None,
    };
    let (mut session, initial) = RendererSession::spawn(
        super::executable(),
        document(include_str!(
            "../../../../fixtures/background-ownership-recovery-r2.html"
        )),
        viewport,
    )
    .unwrap();
    let pid = initial.metrics.worker_pid;
    assert!(pid > 0);
    assert_ne!(pid, std::process::id());
    assert_eq!(initial.title, "R2 red");
    assert!(initial.animation_frame_pending);
    pixels(&initial, false);
    let reflow = session.render(viewport, None).unwrap();
    assert_eq!(reflow.metrics.worker_pid, pid);
    assert_eq!(reflow.title, "R2 red");
    pixels(&reflow, false);
    let blue = session.animation_frame(1., viewport).unwrap();
    assert_eq!(blue.title, "R2 blue");
    assert_eq!(blue.metrics.worker_pid, pid);
    assert!(!blue.animation_frame_pending);
    pixels(&blue, true);
    let idle = session.animation_frame(2., viewport).unwrap();
    assert_eq!(idle.metrics.worker_pid, pid);
    assert_eq!(idle.title, "R2 blue");
    assert_eq!(idle.items, blue.items);
    pixels(&idle, true);
}

#[test]
fn recovery_r2_same_color_inline_layer_remains_two_legitimate_compositions() {
    let html = "<div style='width:180px;height:32px;background:rgba(255,0,0,0.5);color:transparent'><span style='background:rgba(255,0,0,0.5)'>Layer</span></div>";
    let (_, page) = RendererSession::spawn(
        super::executable(),
        document(html),
        Viewport {
            width: 320.,
            height: 240.,
            view_context: None,
        },
    )
    .unwrap();
    assert!(page.warnings.is_empty());
    let parent = owner(&page, Color(255, 0, 0, 127), 180.);
    let text = page
        .items
        .iter()
        .find_map(|item| match item {
            DisplayItem::Text { rect, text, .. } if text == "Layer" => Some(*rect),
            _ => None,
        })
        .unwrap();
    let red: Vec<_> = page
        .items
        .iter()
        .filter_map(|item| match item {
            DisplayItem::Rect {
                rect,
                color: Color(255, 0, 0, 127),
            } => Some(*rect),
            _ => None,
        })
        .collect();
    assert_eq!(red, vec![parent, text]);
    let surface = web_paint::rasterize(&page, &[], 320, 240).unwrap();
    assert_eq!(pixel(&surface, text.x + 2., text.y + 2.), [255, 64, 64]);
    assert_eq!(
        pixel(&surface, parent.x + 160., parent.y + 2.),
        [255, 128, 128]
    );
}
