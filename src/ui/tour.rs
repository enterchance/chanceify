//! The guided tour overlay: dims the window, lights up the part a step is
//! about, and shows a small card with Back / Next. Steps come from
//! `crate::tour` and can be edited in Settings.

use egui::{Align2, CornerRadius, Frame, Id, Margin, Rect, Stroke, pos2, vec2};

use crate::app::App;
use crate::theme;

fn anchor_rect(app: &App, ctx: &egui::Context, anchor: &str) -> Option<Rect> {
    let screen = ctx.content_rect();
    match anchor {
        // Right of the library, so the sidebar is not lit up for this step.
        "top" => {
            let left = if app.settings.sidebar_visible {
                ctx.data(|data| data.get_temp::<Rect>(Id::new("tour-sidebar")))
                    .map_or(screen.min.x + app.settings.sidebar_width, |rect| rect.max.x)
            } else {
                screen.min.x
            };
            Some(Rect::from_min_max(
                pos2(left, screen.min.y),
                pos2(screen.max.x, screen.min.y + theme::TOP_BAR_HEIGHT),
            ))
        }
        "player" => ctx
            .data(|data| data.get_temp::<Rect>(Id::new("tour-player")))
            .or_else(|| {
                Some(Rect::from_min_max(
                    pos2(screen.min.x, screen.max.y - theme::PLAYER_BAR_HEIGHT),
                    screen.max,
                ))
            }),
        "controls" | "seek" | "volume" | "visual" => {
            let id = format!("tour-{anchor}");
            ctx.data(|data| data.get_temp::<Rect>(Id::new(id)))
        }
        "bottom" => Some(Rect::from_min_max(
            pos2(screen.min.x, screen.max.y - theme::PLAYER_BAR_HEIGHT),
            screen.max,
        )),
        // All the way to the top of the screen, so the right-click features
        // up there can be pointed at too.
        "sidebar" if app.settings.sidebar_visible => {
            let rect = ctx
                .data(|data| data.get_temp::<Rect>(Id::new("tour-sidebar")))
                .unwrap_or_else(|| {
                    Rect::from_min_max(
                        pos2(screen.min.x, screen.min.y),
                        pos2(
                            screen.min.x + app.settings.sidebar_width,
                            screen.max.y - theme::PLAYER_BAR_HEIGHT,
                        ),
                    )
                });
            Some(Rect::from_min_max(pos2(rect.min.x, screen.min.y), rect.max))
        }
        "account" => ctx.data(|data| data.get_temp::<Rect>(Id::new("tour-account"))),
        _ => None,
    }
}

pub fn show(app: &mut App, ctx: &egui::Context) {
    // The visualizer step shows the real settings panel instead of a made-up
    // list, so what the tour shows is always what the panel has. The panel is
    // opened for that step and closed again when the tour moves on or ends.
    let on_visual = app
        .tour_step
        .and_then(|index| app.tour.steps.get(index))
        .is_some_and(|step| step.anchor == "visual");
    let opened = ctx
        .data(|data| data.get_temp::<bool>(Id::new("tour-vis-open")))
        .unwrap_or(false);
    if on_visual {
        app.vis_panel = true;
        if !opened {
            ctx.data_mut(|data| data.insert_temp(Id::new("tour-vis-open"), true));
        }
    } else if opened {
        ctx.data_mut(|data| data.insert_temp(Id::new("tour-vis-open"), false));
        app.vis_panel = false;
    }
    let Some(step_index) = app.tour_step else {
        return;
    };
    let total = app.tour.steps.len();
    if step_index >= total {
        app.tour_step = None;
        return;
    }
    let palette = app.palette;
    let step = app.tour.steps[step_index].clone();
    let screen = ctx.content_rect();
    let hole = anchor_rect(app, ctx, &step.anchor).map(|rect| rect.expand(4.0));
    let mut go: Option<isize> = None;
    let mut close = false;
    // Under the card, but menus opened from the lit-up part (a right-click
    // on the library, say) still draw over the dimming.
    // The panel is a window of its own, which the dimming would sit over, so
    // that step is left undimmed.
    if !on_visual {
    egui::Area::new(Id::new("tour-dim"))
        .order(egui::Order::Middle)
        .fixed_pos(screen.min)
        .interactable(false)
        .show(ctx, |ui| {
            // Only painted, never interactive: the player and everything
            // else stays usable while the tour is open.
            let dim = egui::Color32::from_black_alpha(165);
            let painter = ui.painter();
            match hole {
                Some(hole) => {
                    let hole = hole.intersect(screen);
                    painter.rect_filled(
                        Rect::from_min_max(screen.min, pos2(screen.max.x, hole.min.y)),
                        0.0,
                        dim,
                    );
                    painter.rect_filled(
                        Rect::from_min_max(pos2(screen.min.x, hole.max.y), screen.max),
                        0.0,
                        dim,
                    );
                    painter.rect_filled(
                        Rect::from_min_max(
                            pos2(screen.min.x, hole.min.y),
                            pos2(hole.min.x, hole.max.y),
                        ),
                        0.0,
                        dim,
                    );
                    painter.rect_filled(
                        Rect::from_min_max(
                            pos2(hole.max.x, hole.min.y),
                            pos2(screen.max.x, hole.max.y),
                        ),
                        0.0,
                        dim,
                    );
                    painter.rect_stroke(
                        hole,
                        CornerRadius::same(8),
                        Stroke::new(2.0, palette.accent),
                        egui::StrokeKind::Outside,
                    );
                }
                None => {
                    painter.rect_filled(screen, 0.0, dim);
                }
            }
        });
    }
    // The card, narrower than 340 in a window that is.
    let card_width = (screen.width() - 32.0).clamp(180.0, 340.0);
    let pivot_pos = match (&step.anchor[..], hole) {
        // Out of the way of the settings panel: at the top, in the middle.
        ("visual", _) => (pos2(screen.center().x, screen.min.y + 12.0), Align2::CENTER_TOP),
        ("bottom" | "player", Some(hole)) => (pos2(screen.center().x, hole.min.y - 16.0), Align2::CENTER_BOTTOM),
        ("controls" | "seek" | "volume", Some(hole)) => (
            pos2(
                hole.center().x.clamp(
                    screen.min.x + card_width / 2.0 + 12.0,
                    screen.max.x - card_width / 2.0 - 12.0,
                ),
                hole.min.y - 16.0,
            ),
            Align2::CENTER_BOTTOM,
        ),
        ("top", Some(hole)) => (pos2(hole.center().x, hole.max.y + 16.0), Align2::CENTER_TOP),
        ("sidebar", Some(hole)) => (pos2(hole.max.x + 16.0, screen.max.y - theme::PLAYER_BAR_HEIGHT - 24.0), Align2::LEFT_BOTTOM),
        ("account", Some(hole)) => (pos2(hole.max.x, hole.max.y + 16.0), Align2::RIGHT_TOP),
        _ => (screen.center(), Align2::CENTER_CENTER),
    };
    egui::Area::new(Id::new("tour-card"))
        .order(egui::Order::Foreground)
        .pivot(pivot_pos.1)
        .fixed_pos(pivot_pos.0)
        .show(ctx, |ui| {
            Frame::new()
                .fill(palette.panel)
                .stroke(Stroke::new(1.0, palette.outline))
                .corner_radius(CornerRadius::same(theme::RADIUS + 4))
                .inner_margin(Margin::same(18))
                .shadow(egui::epaint::Shadow {
                    offset: [0, 8],
                    blur: 28,
                    spread: 0,
                    color: palette.shadow,
                })
                .show(ui, |ui| {
                    ui.set_width(card_width);
                    theme::text(ui, &step.title, theme::bold(18.0), palette.text);
                    ui.add_space(6.0);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&step.text)
                                .font(theme::regular(13.5))
                                .color(palette.secondary),
                        )
                        .wrap(),
                    );
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        theme::text(
                            ui,
                            &format!("{} / {}", step_index + 1, total),
                            theme::regular(12.0),
                            palette.dim,
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let last = step_index + 1 == total;
                            let label = if last { "Done" } else { "Next" };
                            if theme::pill_button(ui, &palette, label, true).clicked() {
                                go = Some(1);
                            }
                            if step_index > 0 && theme::pill_button(ui, &palette, "Back", false).clicked() {
                                go = Some(-1);
                            }
                            if theme::pill_button(ui, &palette, "Skip", false).clicked() {
                                close = true;
                            }
                        });
                    });
                });
        });
    if !step.menu.is_empty() && !on_visual {
        let click_at = match (&step.anchor[..], hole) {
            ("sidebar", Some(hole)) => pos2(hole.min.x + 70.0, hole.min.y + 130.0),
            (_, Some(hole)) => hole.center(),
            _ => pos2(screen.center().x + card_width / 2.0 + 70.0, screen.center().y - 110.0),
        };
        demo_right_click(ctx, &palette, step_index, &step.menu, click_at);
    }
    if close {
        app.tour_step = None;
    } else if let Some(direction) = go {
        let next = step_index as isize + direction;
        app.tour_step = if next < 0 || next as usize >= total {
            None
        } else {
            Some(next as usize)
        };
    }
}

/// A pretend cursor that right-clicks at `click_at`, opens a pretend menu
/// there and steps down its rows one after another, over and over. It only
/// draws; nothing is clicked, and the real menus are untouched.
fn demo_right_click(
    ctx: &egui::Context,
    palette: &theme::Palette,
    step_index: usize,
    items: &[String],
    click_at: egui::Pos2,
) {
    let now = ctx.input(|input| input.time);
    let key = Id::new("tour-demo-start");
    let started = match ctx.data(|data| data.get_temp::<(usize, f64)>(key)) {
        Some((held, at)) if held == step_index => at,
        _ => {
            ctx.data_mut(|data| data.insert_temp(key, (step_index, now)));
            now
        }
    };
    let t = (now - started) as f32;
    let screen = ctx.content_rect();
    let row_h = 26.0;
    let font = theme::regular(13.0);
    // About 7.3 pixels a letter at this size; close enough for a pretend menu.
    let width = items
        .iter()
        .map(|item| item.chars().count() as f32 * 7.3)
        .fold(90.0_f32, f32::max)
        + 36.0;
    let height = items.len() as f32 * row_h + 12.0;
    let menu = Rect::from_min_size(
        pos2(
            (click_at.x + 6.0).min(screen.right() - width - 8.0).max(screen.left() + 8.0),
            (click_at.y + 6.0).min(screen.bottom() - height - 8.0).max(screen.top() + 8.0),
        ),
        vec2(width, height),
    );
    let row_centre = |row: usize| pos2(menu.left() + 22.0, menu.top() + 6.0 + (row as f32 + 0.5) * row_h);
    let ease = |x: f32| {
        let x = x.clamp(0.0, 1.0);
        x * x * (3.0 - 2.0 * x)
    };
    let arrive = 0.8_f32;
    let opens = arrive + 0.15;
    let slot = 0.9_f32;
    let steps = (((t - opens - 0.35).max(0.0)) / slot) as usize;
    let row = steps % items.len();
    let previous = if steps == 0 || row == 0 {
        None
    } else {
        Some(row - 1)
    };
    let from = previous.map_or(click_at, row_centre);
    let within = ((t - opens - 0.35).max(0.0) % slot) / slot;
    let cursor = if t < arrive {
        let start = click_at + vec2(120.0, 90.0);
        start + (click_at - start) * ease(t / arrive)
    } else if t < opens + 0.35 {
        click_at
    } else {
        let wrapped_from = if row == 0 && steps > 0 { row_centre(items.len() - 1) } else { from };
        wrapped_from + (row_centre(row) - wrapped_from) * ease(within * 3.0)
    };
    egui::Area::new(Id::new("tour-demo"))
        .order(egui::Order::Tooltip)
        .fixed_pos(screen.min)
        .interactable(false)
        .show(ctx, |ui| {
            let painter = ui.painter();
            // The click: two rings spreading out from the point.
            if t >= arrive {
                for ring in 0..2 {
                    let age = ((t - arrive) - ring as f32 * 0.18) / 0.6;
                    if (0.0..1.0).contains(&age) {
                        painter.circle_stroke(
                            click_at,
                            6.0 + age * 26.0,
                            egui::Stroke::new(2.0, palette.accent.gamma_multiply(1.0 - age)),
                        );
                    }
                }
            }
            if t >= opens {
                let fade = ((t - opens) / 0.2).clamp(0.0, 1.0);
                painter.rect_filled(
                    menu,
                    CornerRadius::same(8),
                    palette.panel.gamma_multiply(fade),
                );
                painter.rect_stroke(
                    menu,
                    CornerRadius::same(8),
                    Stroke::new(1.0, palette.outline.gamma_multiply(fade)),
                    egui::StrokeKind::Inside,
                );
                let lit = (t >= opens + 0.35).then_some(row);
                for (index, item) in items.iter().enumerate() {
                    let rect = Rect::from_min_size(
                        pos2(menu.left() + 4.0, menu.top() + 6.0 + index as f32 * row_h),
                        vec2(menu.width() - 8.0, row_h),
                    );
                    if lit == Some(index) {
                        painter.rect_filled(
                            rect,
                            CornerRadius::same(5),
                            palette.accent.gamma_multiply(0.35 * fade),
                        );
                    }
                    painter.text(
                        pos2(rect.left() + 12.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        item,
                        font.clone(),
                        (if lit == Some(index) { palette.text } else { palette.secondary })
                            .gamma_multiply(fade),
                    );
                }
            }
            // The pointer: a white arrow with a dark edge.
            painter.add(egui::Shape::convex_polygon(
                vec![
                    cursor,
                    cursor + vec2(0.0, 18.0),
                    cursor + vec2(5.0, 14.0),
                    cursor + vec2(12.0, 13.0),
                ],
                egui::Color32::WHITE,
                Stroke::new(1.2, egui::Color32::BLACK),
            ));
        });
    ctx.request_repaint();
}
