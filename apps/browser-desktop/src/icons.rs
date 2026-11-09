//! Resolution-independent shell icons. Navigation never depends on font glyphs.
use eframe::egui::{self, Color32, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2};

#[derive(Debug, Clone, Copy)]
pub enum Icon {
    Back,
    Forward,
    Reload,
    Home,
    Diamond,
    File,
    Inspect,
    Spark,
    Search,
    Plus,
    Close,
    Star,
    Settings,
    Workspace,
    Globe,
    Pin,
    ChevronDown,
    Minimize,
    Maximize,
}

/// A keyboard-focusable button with an accessible name and hover tooltip.
pub fn button(ui: &mut Ui, icon: Icon, label: &str, enabled: bool) -> Response {
    let response = ui.add_enabled(
        enabled,
        egui::Button::new("")
            .min_size(Vec2::splat(32.0))
            .frame(false),
    );
    let enabled = enabled && ui.is_enabled();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    if ui.is_rect_visible(response.rect) {
        let visuals = ui.style().interact(&response);
        let hover = ui.ctx().animate_bool_with_time(
            response.id.with("hover"),
            response.hovered() || response.has_focus(),
            ui.style().animation_time,
        );
        if enabled && hover > 0.001 {
            ui.painter().rect_filled(
                response.rect,
                9.0,
                visuals.weak_bg_fill.gamma_multiply(hover),
            );
        }
        if response.has_focus() {
            ui.painter().rect_stroke(
                response.rect.shrink(0.5),
                9.0,
                ui.visuals().selection.stroke,
                egui::StrokeKind::Inside,
            );
        }
        let color = if enabled {
            visuals.fg_stroke.color
        } else {
            ui.visuals().text_color().gamma_multiply(0.35)
        };
        paint(
            ui,
            icon,
            Rect::from_center_size(response.rect.center(), Vec2::splat(20.0)),
            color,
        );
    }
    response.on_hover_text(label)
}

/// Noninteractive mark for product branding or section labels.
pub fn mark(ui: &mut Ui, icon: Icon, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    if ui.is_rect_visible(rect) {
        paint(ui, icon, rect, color);
    }
}

pub fn paint(ui: &Ui, icon: Icon, rect: Rect, color: Color32) {
    let scale = rect.width().min(rect.height()) / 20.0;
    let center = rect.center();
    let point =
        |x: f32, y: f32| -> Pos2 { center + Vec2::new((x - 10.0) * scale, (y - 10.0) * scale) };
    let stroke = Stroke::new(1.65 * scale, color);
    let path = |vertices: &[(f32, f32)]| {
        ui.painter().add(Shape::line(
            vertices.iter().map(|&(x, y)| point(x, y)).collect(),
            stroke,
        ));
    };
    match icon {
        Icon::ChevronDown => path(&[(5., 8.), (10., 13.), (15., 8.)]),
        Icon::Pin => {
            path(&[
                (7., 3.),
                (14., 3.),
                (13., 8.),
                (16., 12.),
                (5., 12.),
                (8., 8.),
                (7., 3.),
            ]);
            path(&[(10.5, 12.), (10.5, 18.)]);
        }
        Icon::Minimize => path(&[(4., 11.), (16., 11.)]),
        Icon::Maximize => path(&[(5., 5.), (15., 5.), (15., 15.), (5., 15.), (5., 5.)]),
        Icon::Search => {
            ui.painter()
                .circle_stroke(point(8.5, 8.5), 5.5 * scale, stroke);
            path(&[(12.5, 12.5), (17., 17.)]);
        }
        Icon::Plus => {
            path(&[(10., 4.), (10., 16.)]);
            path(&[(4., 10.), (16., 10.)]);
        }
        Icon::Close => {
            path(&[(5., 5.), (15., 15.)]);
            path(&[(5., 15.), (15., 5.)]);
        }
        Icon::Star => {
            let vertices = (0..=10)
                .map(|i| {
                    let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.;
                    let r = if i % 2 == 0 { 8. } else { 3.7 };
                    point(10. + a.cos() * r, 10. + a.sin() * r)
                })
                .collect();
            ui.painter().add(Shape::line(vertices, stroke));
        }
        Icon::Settings => {
            ui.painter()
                .circle_stroke(point(10., 10.), 3. * scale, stroke);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.;
                let d = Vec2::new(a.cos(), a.sin());
                ui.painter().line_segment(
                    [
                        point(10., 10.) + d * (6. * scale),
                        point(10., 10.) + d * (8. * scale),
                    ],
                    stroke,
                );
            }
            ui.painter()
                .circle_stroke(point(10., 10.), 6. * scale, stroke);
        }
        Icon::Workspace => {
            for (x, y) in [(3., 3.), (11., 3.), (3., 11.), (11., 11.)] {
                ui.painter().rect_stroke(
                    Rect::from_min_size(point(x, y), Vec2::splat(6. * scale)),
                    1.5,
                    stroke,
                    egui::StrokeKind::Inside,
                );
            }
        }
        Icon::Globe => {
            ui.painter()
                .circle_stroke(point(10., 10.), 7.5 * scale, stroke);
            path(&[(2.5, 10.), (17.5, 10.)]);
            path(&[(10., 2.5), (6.5, 7.), (6.5, 13.), (10., 17.5)]);
            path(&[(10., 2.5), (13.5, 7.), (13.5, 13.), (10., 17.5)]);
        }
        Icon::Back => {
            path(&[(11.5, 4.0), (5.5, 10.0), (11.5, 16.0)]);
            path(&[(5.5, 10.0), (17.0, 10.0)]);
        }
        Icon::Forward => {
            path(&[(8.5, 4.0), (14.5, 10.0), (8.5, 16.0)]);
            path(&[(3.0, 10.0), (14.5, 10.0)]);
        }
        Icon::Reload => {
            let points = (0..=24)
                .map(|step| {
                    let angle = -0.55 - (step as f32 / 24.0) * 5.25;
                    point(10.0 + angle.cos() * 6.7, 10.0 + angle.sin() * 6.7)
                })
                .collect();
            ui.painter().add(Shape::line(points, stroke));
            path(&[(15.7, 2.7), (15.7, 6.5), (11.9, 6.5)]);
        }
        Icon::Home => {
            path(&[(2.5, 9.5), (10.0, 3.0), (17.5, 9.5)]);
            path(&[
                (5.0, 8.0),
                (5.0, 17.0),
                (8.0, 17.0),
                (8.0, 12.0),
                (12.0, 12.0),
                (12.0, 17.0),
                (15.0, 17.0),
                (15.0, 8.0),
            ]);
        }
        Icon::Diamond => {
            path(&[(4., 16.), (4., 4.), (16., 16.), (16., 4.)]);
            ui.painter()
                .circle_filled(point(4., 2.), scale * 1.1, color.gamma_multiply(0.55));
            ui.painter()
                .circle_filled(point(16., 18.), scale * 1.1, color.gamma_multiply(0.55));
        }
        Icon::File => {
            path(&[
                (5.0, 2.0),
                (12.0, 2.0),
                (16.0, 6.0),
                (16.0, 18.0),
                (5.0, 18.0),
                (5.0, 2.0),
            ]);
            path(&[(12.0, 2.0), (12.0, 6.0), (16.0, 6.0)]);
            path(&[(8.0, 10.0), (13.0, 10.0)]);
            path(&[(8.0, 13.5), (13.0, 13.5)]);
        }
        Icon::Inspect => {
            path(&[(8.0, 5.0), (5.5, 5.0), (5.5, 15.0), (8.0, 15.0)]);
            path(&[(12.0, 5.0), (14.5, 5.0), (14.5, 15.0), (12.0, 15.0)]);
            path(&[(10.0, 2.0), (10.0, 18.0)]);
        }
        Icon::Spark => {
            path(&[
                (10.0, 2.0),
                (12.1, 7.9),
                (18.0, 10.0),
                (12.1, 12.1),
                (10.0, 18.0),
                (7.9, 12.1),
                (2.0, 10.0),
                (7.9, 7.9),
                (10.0, 2.0),
            ]);
        }
    }
}
