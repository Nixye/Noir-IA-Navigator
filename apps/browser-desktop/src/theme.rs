//! Native shell design system. Web documents retain their own styles.
use eframe::egui::{self, Color32, CornerRadius, FontId, Stroke, TextStyle, Vec2};
pub const BG: Color32 = Color32::from_rgb(14, 18, 20);
pub const PANEL: Color32 = Color32::from_rgb(23, 29, 31);
pub const SURFACE: Color32 = Color32::from_rgb(33, 41, 43);
pub const ACCENT: Color32 = Color32::from_rgb(219, 239, 170);
pub const TEXT: Color32 = Color32::from_rgb(239, 242, 235);
pub const MUTED: Color32 = Color32::from_rgb(149, 160, 157);
pub const LINE: Color32 = Color32::from_rgb(47, 60, 60);
pub const AMBER: Color32 = Color32::from_rgb(232, 190, 132);
pub fn install(ctx: &egui::Context) {
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    let mut v = egui::Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.override_text_color = Some(TEXT);
    v.weak_text_color = Some(MUTED);
    v.extreme_bg_color = BG;
    v.text_edit_bg_color = Some(BG);
    v.code_bg_color = SURFACE;
    v.hyperlink_color = ACCENT;
    v.warn_fg_color = AMBER;
    v.window_corner_radius = CornerRadius::same(18);
    v.menu_corner_radius = CornerRadius::same(12);
    v.window_stroke = Stroke::new(1., LINE);
    v.selection.bg_fill = Color32::from_rgb(62, 75, 52);
    v.selection.stroke = Stroke::new(1., ACCENT);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = CornerRadius::same(9);
        w.bg_stroke = Stroke::new(1., Color32::TRANSPARENT);
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1., LINE);
    v.widgets.inactive.bg_fill = SURFACE;
    v.widgets.inactive.weak_bg_fill = SURFACE;
    v.widgets.hovered.bg_fill = Color32::from_rgb(43, 54, 53);
    v.widgets.hovered.weak_bg_fill = v.widgets.hovered.bg_fill;
    v.widgets.hovered.bg_stroke = Stroke::new(1., ACCENT.gamma_multiply(0.3));
    v.widgets.active.bg_fill = Color32::from_rgb(62, 76, 56);
    v.widgets.active.fg_stroke = Stroke::new(1., ACCENT);
    style.visuals = v;
    style
        .text_styles
        .insert(TextStyle::Body, FontId::proportional(14.));
    style
        .text_styles
        .insert(TextStyle::Button, FontId::proportional(14.));
    style
        .text_styles
        .insert(TextStyle::Small, FontId::proportional(11.));
    style.spacing.item_spacing = Vec2::new(9., 10.);
    style.spacing.button_padding = Vec2::new(12., 9.);
    style.spacing.interact_size.y = 34.;
    style.animation_time = 0.22;
    ctx.set_style_of(egui::Theme::Dark, style);
    ctx.set_theme(egui::Theme::Dark);
}
pub fn glass(color: Color32, opacity: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), opacity)
}
pub fn blend(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0., 1.);
    Color32::from_rgba_unmultiplied(
        (a.r() as f32 + (b.r() as f32 - a.r() as f32) * t) as u8,
        (a.g() as f32 + (b.g() as f32 - a.g() as f32) * t) as u8,
        (a.b() as f32 + (b.b() as f32 - a.b() as f32) * t) as u8,
        (a.a() as f32 + (b.a() as f32 - a.a() as f32) * t) as u8,
    )
}
