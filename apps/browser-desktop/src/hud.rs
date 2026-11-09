//! Original static architecture constellation for the local start page.
//! The diagram contains no measured telemetry or simulated connection state.
use eframe::egui::{self, Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, Vec2};
use std::f32::consts::{PI, TAU};

const CYAN: Color32 = Color32::from_rgb(124, 233, 239);
const AMBER: Color32 = Color32::from_rgb(229, 180, 103);

pub fn paint(painter: &Painter, rect: Rect) {
    if rect.width() < 1.0 || rect.height() < 1.0 {
        return;
    }
    let painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    let scale = (rect.width() / 300.0).min(rect.height() / 440.0);
    let origin = rect.center() - Vec2::new(150.0, 220.0) * scale;
    let point = |x: f32, y: f32| origin + Vec2::new(x, y) * scale;
    let thin = |alpha| Stroke::new(0.7 * scale, CYAN.gamma_multiply(alpha));
    let line = |a: (f32, f32), b: (f32, f32), stroke: Stroke| {
        painter.line_segment([point(a.0, a.1), point(b.0, b.1)], stroke);
    };
    let label = |x, y, anchor, text: &str, color, size| {
        painter.text(
            point(x, y),
            anchor,
            text,
            FontId::monospace(size * scale),
            color,
        );
    };

    // A sparse registration grid grounds the composition without competing
    // with document text. All positions are stable between frames.
    for x in (22..300).step_by(32) {
        line((x as f32, 55.0), (x as f32, 386.0), thin(0.045));
    }
    for y in (66..390).step_by(32) {
        line((14.0, y as f32), (286.0, y as f32), thin(0.045));
    }
    for &(x, y) in &[(22.0, 66.0), (278.0, 66.0), (22.0, 386.0), (278.0, 386.0)] {
        line((x - 2.0, y), (x + 2.0, y), thin(0.35));
        line((x, y - 2.0), (x, y + 2.0), thin(0.35));
    }

    // Topographic contours: a deliberately abstract field, not a geographic map.
    for contour in 0..9 {
        let vertices = (0..=48)
            .map(|sample| {
                let x = 4.0 + sample as f32 * 6.1;
                let wave = ((x / 65.0) + contour as f32 * 0.17).sin() * 10.0;
                let ridge = (-((x - 178.0) / 73.0).powi(2)).exp() * 34.0;
                point(x, 330.0 + contour as f32 * 6.5 + wave - ridge)
            })
            .collect();
        painter.add(Shape::line(vertices, thin(0.055)));
    }

    // Header rails and a small registration diamond.
    line((16.0, 27.0), (39.0, 27.0), thin(0.6));
    painter.add(Shape::convex_polygon(
        vec![
            point(19.0, 17.0),
            point(22.0, 20.0),
            point(19.0, 23.0),
            point(16.0, 20.0),
        ],
        CYAN.gamma_multiply(0.8),
        Stroke::NONE,
    ));
    label(
        31.0,
        19.0,
        Align2::LEFT_CENTER,
        "NOIR / LOCAL NODE",
        CYAN.gamma_multiply(0.85),
        9.0,
    );
    label(
        16.0,
        42.0,
        Align2::LEFT_CENTER,
        "ARQUITETURA DE DOCUMENTOS",
        CYAN.gamma_multiply(0.3),
        7.5,
    );
    line((230.0, 27.0), (282.0, 27.0), thin(0.25));
    line((282.0, 27.0), (282.0, 45.0), thin(0.25));

    let center = point(151.0, 214.0);
    let nodes = [
        (55.0, 111.0),
        (235.0, 142.0),
        (245.0, 291.0),
        (82.0, 325.0),
        (35.0, 231.0),
    ];
    let links = [(0, 1), (1, 2), (2, 3), (3, 4), (4, 0), (0, 2), (1, 3)];
    for (from, to) in links {
        let a = nodes[from];
        let b = nodes[to];
        line(a, b, thin(if from == 0 && to == 2 { 0.19 } else { 0.12 }));
    }
    for &(x, y) in &nodes[..3] {
        line((x, y), (151.0, 214.0), thin(0.12));
    }

    // Fragmented scanning rings, compass ticks and interrupted radial spokes.
    for &(radius, from, to, alpha) in &[
        (82.0, -1.42, -0.58, 0.14),
        (82.0, -0.35, 0.48, 0.33),
        (82.0, 0.78, 2.17, 0.13),
        (82.0, 2.47, 3.84, 0.22),
        (68.0, -1.66, -0.14, 0.58),
        (68.0, 0.16, 1.14, 0.25),
        (68.0, 1.52, 2.81, 0.45),
        (68.0, 3.2, 4.17, 0.19),
        (55.0, -0.76, 0.73, 0.19),
        (55.0, 1.13, 2.46, 0.15),
        (55.0, 2.85, 4.85, 0.3),
    ] {
        arc(&painter, center, radius * scale, from, to, thin(alpha));
    }
    for tick in 0..48 {
        let angle = tick as f32 / 48.0 * TAU - PI / 2.0;
        let major = tick % 6 == 0;
        let inner = if major { 87.0 } else { 90.0 };
        let outer = 93.0;
        line(
            (151.0 + angle.cos() * inner, 214.0 + angle.sin() * inner),
            (151.0 + angle.cos() * outer, 214.0 + angle.sin() * outer),
            thin(if major { 0.42 } else { 0.16 }),
        );
    }
    for &(x, y, letter) in &[
        (151.0, 110.0, "N"),
        (255.0, 214.0, "E"),
        (151.0, 318.0, "S"),
        (47.0, 214.0, "W"),
    ] {
        label(
            x,
            y,
            Align2::CENTER_CENTER,
            letter,
            CYAN.gamma_multiply(0.26),
            7.0,
        );
    }
    painter.circle_filled(center, 41.0 * scale, Color32::from_rgb(7, 21, 29));
    painter.circle_stroke(center, 41.0 * scale, thin(0.3));
    label(
        151.0,
        208.0,
        Align2::CENTER_CENTER,
        "N O I R",
        CYAN.gamma_multiply(0.9),
        14.0,
    );
    label(
        151.0,
        226.0,
        Align2::CENTER_CENTER,
        "MOTOR LOCAL",
        CYAN.gamma_multiply(0.38),
        7.0,
    );
    line((133.0, 238.0), (169.0, 238.0), thin(0.25));

    for (index, &(x, y)) in nodes.iter().enumerate() {
        let color = if index == 3 { AMBER } else { CYAN };
        let node_center = point(x, y);
        painter.circle_filled(node_center, 6.3 * scale, Color32::from_rgb(6, 20, 28));
        painter.circle_stroke(
            node_center,
            6.3 * scale,
            Stroke::new(0.8 * scale, color.gamma_multiply(0.55)),
        );
        painter.circle_filled(node_center, 1.7 * scale, color.gamma_multiply(0.85));
        if index == 0 || index == 3 {
            arc(
                &painter,
                node_center,
                11.0 * scale,
                -1.7,
                0.55,
                Stroke::new(0.6 * scale, color.gamma_multiply(0.25)),
            );
        }
    }
    label(
        55.0,
        89.0,
        Align2::CENTER_CENTER,
        "DOCUMENTO",
        CYAN.gamma_multiply(0.55),
        8.0,
    );
    label(
        247.0,
        123.0,
        Align2::CENTER_CENTER,
        "RENDER",
        CYAN.gamma_multiply(0.55),
        8.0,
    );
    label(
        245.0,
        312.0,
        Align2::CENTER_CENTER,
        "IPC",
        CYAN.gamma_multiply(0.55),
        8.0,
    );
    label(
        82.0,
        348.0,
        Align2::CENTER_CENTER,
        "IA / OFFLINE",
        AMBER.gamma_multiply(0.58),
        8.0,
    );
    label(
        24.0,
        249.0,
        Align2::LEFT_CENTER,
        "DOM",
        CYAN.gamma_multiply(0.4),
        7.0,
    );

    // Bottom ruler has no pseudo-measurements or invented operational counters.
    for tick in 0..39 {
        let x = 16.0 + tick as f32 * 7.0;
        let height = if tick % 5 == 0 { 6.0 } else { 2.5 };
        line(
            (x, 402.0),
            (x, 402.0 + height),
            thin(if tick % 5 == 0 { 0.38 } else { 0.13 }),
        );
    }
    label(
        16.0,
        423.0,
        Align2::LEFT_CENTER,
        "DADOS LOCAIS / CONTEXTO EXPLÍCITO",
        CYAN.gamma_multiply(0.35),
        7.0,
    );
    line((282.0, 417.0), (282.0, 428.0), thin(0.35));
    line((272.0, 428.0), (282.0, 428.0), thin(0.35));
}

fn arc(painter: &egui::Painter, center: Pos2, radius: f32, start: f32, end: f32, stroke: Stroke) {
    let segments = ((end - start).abs() * 12.0).ceil().max(2.0) as usize;
    let vertices = (0..=segments)
        .map(|step| {
            let angle = start + (end - start) * step as f32 / segments as f32;
            center + Vec2::angled(angle) * radius
        })
        .collect();
    painter.add(Shape::line(vertices, stroke));
}
