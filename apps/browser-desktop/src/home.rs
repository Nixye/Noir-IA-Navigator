//! Native launch page with useful shortcuts and a procedural glass sculpture.
use crate::{
    icons::{self, Icon},
    theme::{self, ACCENT, LINE, MUTED, PANEL, SURFACE, TEXT},
};
use browser_config::Favorite;
use eframe::egui::{self, Align2, Color32, FontId, Rect, RichText, Sense, Shape, Stroke, Ui, Vec2};
use std::time::Duration;
#[derive(Default)]
pub struct Home {
    pub query: String,
    pub focus_query: bool,
}
pub enum Action {
    Navigate(String),
    AddFavorite,
    EditFavorite(u64),
    RemoveFavorite(u64),
    OpenFile,
    OpenAi,
    OpenSettings,
    NewWorkspace,
}
impl Home {
    pub fn show(
        &mut self,
        ui: &mut Ui,
        favorites: &[Favorite],
        workspace: &str,
        reduced: bool,
        opacity: u8,
    ) -> Option<Action> {
        let mut action = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let available = ui.available_width();
                let width = (available - 80.).clamp(260., 1100.);
                let columns = if width >= 900. {
                    4
                } else if width >= 640. {
                    3
                } else {
                    2
                };
                let rows = favorites.len().max(1).div_ceil(columns);
                let stacked_actions = width < 620.;
                let height = (680. + rows as f32 * 112. + if stacked_actions { 140. } else { 0. })
                    .max(ui.available_height());
                let (canvas, _) =
                    ui.allocate_exact_size(Vec2::new(available, height), Sense::hover());
                let painter = ui.painter_at(canvas);
                let left = canvas.left() + (available - width) / 2.;
                let top = canvas.top() + 45.;
                let time = if reduced {
                    0.
                } else {
                    ui.input(|i| i.time) as f32
                };
                let hero = Rect::from_min_size(egui::pos2(left, top), Vec2::new(width, 310.));
                if width > 700. {
                    sculpture(
                        ui,
                        Rect::from_min_size(
                            egui::pos2(hero.right() - 320., hero.top() - 10.),
                            Vec2::new(330., 300.),
                        ),
                        time,
                    );
                }
                let pill = Rect::from_min_size(
                    hero.min,
                    Vec2::new(
                        (workspace.chars().count() as f32 * 7. + 56.).clamp(100., 240.),
                        26.,
                    ),
                );
                painter.rect_filled(pill, 13., Color32::from_white_alpha(6));
                painter.rect_stroke(
                    pill,
                    13.,
                    Stroke::new(1., Color32::from_white_alpha(16)),
                    egui::StrokeKind::Inside,
                );
                painter.circle_filled(pill.left_center() + Vec2::new(15., 0.), 3., ACCENT);
                painter
                    .with_clip_rect(Rect::from_min_max(
                        pill.min + Vec2::new(26., 0.),
                        pill.max - Vec2::new(6., 0.),
                    ))
                    .text(
                        pill.left_center() + Vec2::new(28., 0.),
                        Align2::LEFT_CENTER,
                        fit(workspace, 30),
                        FontId::proportional(11.),
                        TEXT,
                    );
                ui.interact(pill, ui.scope_id().with("workspace-label"), Sense::hover())
                    .on_hover_text(workspace);
                let title = if width > 700. {
                    51.
                } else if width > 400. {
                    38.
                } else {
                    32.
                };
                painter.text(
                    hero.min + Vec2::new(0., 49.),
                    Align2::LEFT_TOP,
                    "Um novo ponto",
                    FontId::proportional(title),
                    TEXT,
                );
                painter.text(
                    hero.min + Vec2::new(0., 49. + title * 1.1),
                    Align2::LEFT_TOP,
                    "de partida.",
                    FontId::proportional(title),
                    ACCENT,
                );
                painter.text(
                    hero.min + Vec2::new(2., 49. + title * 2.35),
                    Align2::LEFT_TOP,
                    fit(
                        "O mundo está a uma boa ideia de distância.",
                        (width / 7.).floor() as usize,
                    ),
                    FontId::proportional(14.),
                    MUTED,
                );
                let search_width = if width > 700. {
                    (width - 310.).clamp(420., 740.)
                } else {
                    width
                };
                let search = Rect::from_min_size(
                    hero.min + Vec2::new(0., 222.),
                    Vec2::new(search_width, 61.),
                );
                let focus = ui.scope_id().with("search");
                let t = ui.ctx().animate_bool_with_time(
                    focus.with("focus"),
                    ui.memory(|m| m.has_focus(focus)) || ui.rect_contains_pointer(search),
                    if reduced { 0. } else { 0.24 },
                );
                painter.rect_filled(search.expand(3. * t), 18., ACCENT.gamma_multiply(0.035 * t));
                painter.rect_filled(
                    search,
                    16.,
                    theme::glass(SURFACE, opacity.saturating_add(40)),
                );
                painter.rect_stroke(
                    search,
                    16.,
                    Stroke::new(
                        1.,
                        theme::blend(
                            Color32::from_white_alpha(30),
                            ACCENT.gamma_multiply(0.65),
                            t,
                        ),
                    ),
                    egui::StrokeKind::Inside,
                );
                icons::paint(
                    ui,
                    Icon::Search,
                    Rect::from_center_size(search.min + Vec2::new(27., 30.), Vec2::splat(20.)),
                    MUTED,
                );
                let edit = ui.put(
                    Rect::from_min_max(
                        search.min + Vec2::new(53., 18.),
                        search.max - Vec2::new(65., 13.),
                    ),
                    egui::TextEdit::singleline(&mut self.query)
                        .id(focus)
                        .hint_text("Pesquisar no Google ou abrir um endereço")
                        .font(FontId::proportional(14.))
                        .frame(egui::Frame::NONE),
                );
                if self.focus_query {
                    edit.request_focus();
                    if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), edit.id) {
                        state
                            .cursor
                            .set_char_range(Some(egui::text::CCursorRange::two(
                                egui::text::CCursor::new(0),
                                egui::text::CCursor::new(self.query.chars().count()),
                            )));
                        state.store(ui.ctx(), edit.id);
                    }
                    self.focus_query = false;
                }
                if edit.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    && !self.query.trim().is_empty()
                {
                    action = Some(Action::Navigate(self.query.clone()));
                }
                let go = Rect::from_center_size(
                    search.right_center() - Vec2::new(28., 0.),
                    Vec2::splat(36.),
                );
                let response = ui.put(go, egui::Button::new("").fill(ACCENT).corner_radius(11));
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::Role::Button, true, "Pesquisar")
                });
                icons::paint(
                    ui,
                    Icon::Forward,
                    go.shrink(8.),
                    Color32::from_rgb(30, 35, 26),
                );
                if response.on_hover_text("Pesquisar").clicked() && !self.query.trim().is_empty() {
                    action = Some(Action::Navigate(self.query.clone()));
                }
                painter.text(
                    search.left_bottom() + Vec2::new(2., 12.),
                    Align2::LEFT_TOP,
                    "Pesquisa Google",
                    FontId::proportional(10.),
                    MUTED.gamma_multiply(0.7),
                );
                painter.text(
                    search.right_bottom() + Vec2::new(-2., 12.),
                    Align2::RIGHT_TOP,
                    "Ctrl + L",
                    FontId::proportional(10.),
                    MUTED.gamma_multiply(0.7),
                );
                let favorites_y = top + 365.;
                painter.text(
                    egui::pos2(left, favorites_y),
                    Align2::LEFT_CENTER,
                    "Seus favoritos",
                    FontId::proportional(19.),
                    TEXT,
                );
                let add = Rect::from_min_size(
                    egui::pos2(left + width - 132., favorites_y - 16.),
                    Vec2::new(132., 32.),
                );
                let add_response = ui.put(
                    add,
                    egui::Button::new(RichText::new("Adicionar site").size(12.)).frame(false),
                );
                icons::paint(
                    ui,
                    Icon::Plus,
                    Rect::from_center_size(add.left_center() + Vec2::new(7., 0.), Vec2::splat(12.)),
                    MUTED,
                );
                if add_response.clicked() {
                    action = Some(Action::AddFavorite);
                }
                let gap = 14.;
                let card_width = (width - gap * (columns - 1) as f32) / columns as f32;
                if favorites.is_empty() {
                    let rect = Rect::from_min_size(
                        egui::pos2(left, favorites_y + 32.),
                        Vec2::new(width, 92.),
                    );
                    painter.rect_stroke(rect, 15., Stroke::new(1., LINE), egui::StrokeKind::Inside);
                    painter.text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        "Adicione os sites que fazem parte do seu dia.",
                        FontId::proportional(14.),
                        MUTED,
                    );
                    if ui
                        .interact(rect, ui.scope_id().with("empty-favorites"), Sense::click())
                        .clicked()
                    {
                        action = Some(Action::AddFavorite);
                    }
                }
                for (i, favorite) in favorites.iter().enumerate() {
                    let rect = Rect::from_min_size(
                        egui::pos2(
                            left + (i % columns) as f32 * (card_width + gap),
                            favorites_y + 32. + (i / columns) as f32 * 112.,
                        ),
                        Vec2::new(card_width, 96.),
                    );
                    let response = ui.interact(
                        rect,
                        ui.scope_id().with(("favorite", favorite.id)),
                        Sense::click(),
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::Role::Button, true, &favorite.title)
                    });
                    let hover = ui.ctx().animate_bool_with_time(
                        response.id.with("hover"),
                        response.hovered() || response.has_focus(),
                        if reduced { 0. } else { 0.22 },
                    );
                    let visual = rect.translate(Vec2::new(0., -2. * hover));
                    painter.rect_filled(
                        visual,
                        14.,
                        theme::blend(
                            theme::glass(PANEL, opacity.saturating_add(12)),
                            theme::glass(SURFACE, opacity.saturating_add(40)),
                            hover,
                        ),
                    );
                    painter.rect_stroke(
                        visual,
                        14.,
                        Stroke::new(
                            1.,
                            theme::blend(
                                Color32::from_white_alpha(16),
                                ACCENT.gamma_multiply(0.35),
                                hover,
                            ),
                        ),
                        egui::StrokeKind::Inside,
                    );
                    let badge =
                        Rect::from_min_size(visual.min + Vec2::new(17., 17.), Vec2::splat(32.));
                    let color =
                        Color32::from_rgb(favorite.color[0], favorite.color[1], favorite.color[2]);
                    painter.rect_filled(badge, 10., color.gamma_multiply(0.1));
                    site_icon(ui, &favorite.url, &favorite.title, badge.shrink(6.), color);
                    painter.text(
                        visual.min + Vec2::new(61., 20.),
                        Align2::LEFT_TOP,
                        fit(&favorite.title, ((card_width - 78.) / 7.) as usize),
                        FontId::proportional(13.),
                        TEXT,
                    );
                    painter.text(
                        visual.min + Vec2::new(17., 66.),
                        Align2::LEFT_TOP,
                        fit(&domain(&favorite.url), ((card_width - 48.) / 6.) as usize),
                        FontId::proportional(11.),
                        MUTED,
                    );
                    icons::paint(
                        ui,
                        Icon::Forward,
                        Rect::from_center_size(
                            visual.right_bottom() - Vec2::new(21., 23.),
                            Vec2::splat(13.),
                        ),
                        MUTED.gamma_multiply(0.5 + hover * 0.4),
                    );
                    if response.clicked() {
                        action = Some(Action::Navigate(favorite.url.clone()));
                    }
                    response.context_menu(|ui| {
                        if ui.button("Editar favorito").clicked() {
                            action = Some(Action::EditFavorite(favorite.id));
                            ui.close();
                        }
                        if ui.button("Remover favorito").clicked() {
                            action = Some(Action::RemoveFavorite(favorite.id));
                            ui.close();
                        }
                    });
                    response
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(&favorite.url);
                }
                let actions_y = favorites_y + 32. + rows as f32 * 112. + 26.;
                let actions_width = if stacked_actions {
                    width
                } else {
                    (width - gap) / 2.
                };
                for (i, (icon, title, body, link)) in [
                    (
                        Icon::Spark,
                        "Uma ideia pode ir mais longe.",
                        "Converse com o assistente e encontre seu próximo passo.",
                        "Abrir assistente",
                    ),
                    (
                        Icon::Workspace,
                        "Um espaço para cada contexto.",
                        "Separe suas abas e mantenha o que importa por perto.",
                        "Criar grupo",
                    ),
                ]
                .iter()
                .enumerate()
                {
                    let rect = Rect::from_min_size(
                        egui::pos2(
                            left + if stacked_actions {
                                0.
                            } else {
                                i as f32 * (actions_width + gap)
                            },
                            actions_y + if stacked_actions { i as f32 * 140. } else { 0. },
                        ),
                        Vec2::new(actions_width, 126.),
                    );
                    let response =
                        ui.interact(rect, ui.scope_id().with(("home-action", i)), Sense::click());
                    response
                        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, *link));
                    let hover = ui.ctx().animate_bool_with_time(
                        response.id.with("hover"),
                        response.hovered(),
                        if reduced { 0. } else { 0.24 },
                    );
                    painter.rect_filled(
                        rect,
                        16.,
                        theme::blend(
                            Color32::from_white_alpha(3),
                            Color32::from_white_alpha(8),
                            hover,
                        ),
                    );
                    painter.rect_stroke(
                        rect,
                        16.,
                        Stroke::new(1., Color32::from_white_alpha(14)),
                        egui::StrokeKind::Inside,
                    );
                    icons::paint(
                        ui,
                        *icon,
                        Rect::from_min_size(rect.min + Vec2::new(20., 19.), Vec2::splat(21.)),
                        if i == 0 { ACCENT } else { MUTED },
                    );
                    painter.text(
                        rect.min + Vec2::new(54., 22.),
                        Align2::LEFT_TOP,
                        fit(title, ((actions_width - 75.) / 7.) as usize),
                        FontId::proportional(14.),
                        TEXT,
                    );
                    if actions_width > 350. {
                        painter.text(
                            rect.min + Vec2::new(20., 56.),
                            Align2::LEFT_TOP,
                            body,
                            FontId::proportional(12.),
                            MUTED,
                        );
                    }
                    painter.text(
                        rect.min + Vec2::new(20., 92.),
                        Align2::LEFT_TOP,
                        link,
                        FontId::proportional(12.),
                        if i == 0 { ACCENT } else { TEXT },
                    );
                    icons::paint(
                        ui,
                        Icon::Forward,
                        Rect::from_center_size(
                            rect.right_bottom() - Vec2::new(25., 27.),
                            Vec2::splat(14.),
                        ),
                        MUTED,
                    );
                    if response.clicked() {
                        action = Some(if i == 0 {
                            Action::OpenAi
                        } else {
                            Action::NewWorkspace
                        });
                    }
                }
                let footer = Rect::from_min_size(
                    egui::pos2(
                        left,
                        actions_y + 149. + if stacked_actions { 140. } else { 0. },
                    ),
                    Vec2::new(width, 35.),
                );
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(footer)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    |ui| {
                        if width >= 340. {
                            ui.label(
                                RichText::new("NOIR")
                                    .size(10.)
                                    .color(MUTED.gamma_multiply(0.6)),
                            );
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Personalizar").size(11.).color(MUTED),
                                    )
                                    .frame(false),
                                )
                                .clicked()
                            {
                                action = Some(Action::OpenSettings);
                            }
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Abrir arquivo").size(11.).color(MUTED),
                                    )
                                    .frame(false),
                                )
                                .clicked()
                            {
                                action = Some(Action::OpenFile);
                            }
                        });
                    },
                );
                if !reduced {
                    ui.ctx().request_repaint_after(Duration::from_millis(16));
                }
            });
        action
    }
}
fn fit(text: &str, max: usize) -> String {
    if text.chars().count() > max {
        format!(
            "{}…",
            text.chars().take(max.saturating_sub(1)).collect::<String>()
        )
    } else {
        text.into()
    }
}
fn domain(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| {
            u.host_str()
                .map(|h| h.trim_start_matches("www.").to_owned())
        })
        .unwrap_or_else(|| "Seu navegador".into())
}
fn site_icon(ui: &Ui, url: &str, title: &str, rect: Rect, color: Color32) {
    let host = url::Url::parse(url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(str::to_owned));
    if host
        .as_deref()
        .is_some_and(|host| host == "youtube.com" || host.ends_with(".youtube.com"))
    {
        ui.painter()
            .rect_filled(rect, 5., Color32::from_rgb(242, 68, 71));
        let c = rect.center();
        ui.painter().add(Shape::convex_polygon(
            vec![
                c + Vec2::new(-3., -5.),
                c + Vec2::new(5., 0.),
                c + Vec2::new(-3., 5.),
            ],
            Color32::WHITE,
            Stroke::NONE,
        ));
    } else if url.starts_with("noir:") {
        icons::paint(ui, Icon::Diamond, rect, ACCENT);
    } else {
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            title
                .chars()
                .next()
                .unwrap_or('N')
                .to_uppercase()
                .to_string(),
            FontId::proportional(20.),
            color,
        );
    }
}
fn sculpture(ui: &Ui, rect: Rect, time: f32) {
    let p = ui.painter_at(rect);
    let center = rect.center() + Vec2::new(0., 5.);
    for glow in (0..14).rev() {
        p.circle_filled(
            center,
            30. + glow as f32 * 8.,
            Color32::from_rgba_unmultiplied(173, 195, 125, 1),
        );
    }
    for band in 0..39 {
        let depth = band as f32 / 38.;
        let points = (0..=100)
            .map(|step| {
                let t = step as f32 / 100.;
                let x = (t - 0.5) * 250.;
                let y = (t * 5.2 + depth * 1.9 + time * 0.055).sin() * 62. + (depth - 0.5) * 128.;
                center + Vec2::new(x * 0.94 + y * 0.36, y * 0.94 - x * 0.36)
            })
            .collect::<Vec<_>>();
        let color = theme::blend(
            Color32::from_rgba_unmultiplied(134, 166, 172, 15),
            Color32::from_rgba_unmultiplied(218, 239, 162, 110),
            depth,
        );
        p.add(Shape::line(
            points,
            Stroke::new(if band == 38 { 1.3 } else { 0.65 }, color),
        ));
    }
}
