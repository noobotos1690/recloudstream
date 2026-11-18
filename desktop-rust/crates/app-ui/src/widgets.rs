//! Reusable widgets: navigation items, poster cards, custom vector icons.
//!
//! Icons are painted with primitives rather than font glyphs. A media centre that
//! ships without a bundled icon font cannot rely on the system having one, and a
//! missing glyph reads as a blank button. Drawing a home, a magnifier and a
//! bookmark from lines and triangles is a few dozen lines and always renders.

use crate::theme::{placeholder_tint, Palette};
use app_core::MediaItem;
use egui::{Align2, Color32, CornerRadius, FontId, Rect, Response, Sense, Stroke, Ui, Vec2};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Home,
    Browse,
    Search,
    Bookmark,
    Extensions,
    Settings,
    Play,
    Pause,
    Close,
    Refresh,
}

/// The surface panel that list rows and cards sit on. One place to change the
/// radius when the shape language moves.
pub fn card_frame(palette: Palette, corner: u8, margin: i8) -> egui::Frame {
    egui::Frame::default()
        .fill(palette.surface)
        .corner_radius(CornerRadius::same(corner))
        .inner_margin(egui::Margin::same(margin))
}

/// Draws `icon` centred in `rect`.
pub fn draw_icon(ui: &Ui, rect: Rect, icon: Icon, color: Color32) {
    let painter = ui.painter();
    let center = rect.center();
    let size = rect.width().min(rect.height());
    let stroke = Stroke::new(1.7_f32, color);

    match icon {
        Icon::Home => {
            let half = size * 0.34;
            let roof = vec![
                egui::pos2(center.x, center.y - half),
                egui::pos2(center.x + half, center.y),
                egui::pos2(center.x - half, center.y),
            ];
            painter.add(egui::Shape::convex_polygon(roof, color, Stroke::NONE));
            let body = Rect::from_min_max(
                egui::pos2(center.x - half * 0.68, center.y),
                egui::pos2(center.x + half * 0.68, center.y + half),
            );
            painter.rect_filled(body, CornerRadius::same(1), color);
        }
        Icon::Browse => {
            let square = size * 0.26;
            let gap = size * 0.1;
            for row in 0..2 {
                for column in 0..2 {
                    let min = egui::pos2(
                        center.x - square - gap / 2.0 + column as f32 * (square + gap),
                        center.y - square - gap / 2.0 + row as f32 * (square + gap),
                    );
                    painter.rect_filled(
                        Rect::from_min_size(min, Vec2::splat(square)),
                        CornerRadius::same(2),
                        color,
                    );
                }
            }
        }
        Icon::Search => {
            let radius = size * 0.26;
            painter.circle_stroke(
                egui::pos2(center.x - size * 0.06, center.y - size * 0.06),
                radius,
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(center.x + radius * 0.55, center.y + radius * 0.55),
                    egui::pos2(center.x + size * 0.3, center.y + size * 0.3),
                ],
                stroke,
            );
        }
        Icon::Bookmark => {
            let half_w = size * 0.22;
            let half_h = size * 0.3;
            let points = vec![
                egui::pos2(center.x - half_w, center.y - half_h),
                egui::pos2(center.x + half_w, center.y - half_h),
                egui::pos2(center.x + half_w, center.y + half_h),
                egui::pos2(center.x, center.y + half_h * 0.35),
                egui::pos2(center.x - half_w, center.y + half_h),
            ];
            painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
        }
        Icon::Extensions => {
            // A 2x2 block with one tile offset, reading as "pieces that fit".
            let square = size * 0.24;
            let offsets = [
                egui::vec2(-1.0, -1.0),
                egui::vec2(0.35, -1.0),
                egui::vec2(-1.0, 0.35),
                egui::vec2(0.35, 0.35),
            ];
            for (index, offset) in offsets.iter().enumerate() {
                let min = center + *offset * square * 0.95;
                let radius = if index == 3 {
                    CornerRadius::same(4)
                } else {
                    CornerRadius::same(2)
                };
                painter.rect_filled(Rect::from_min_size(min, Vec2::splat(square)), radius, color);
            }
        }
        Icon::Settings => {
            // Sliders, not a gear: trivially drawable and unambiguous.
            for (index, y) in [-1.0f32, 0.0, 1.0].iter().enumerate() {
                let y = center.y + y * size * 0.24;
                painter.line_segment(
                    [
                        egui::pos2(center.x - size * 0.3, y),
                        egui::pos2(center.x + size * 0.3, y),
                    ],
                    stroke,
                );
                let knob_x = center.x + size * 0.3 * [-0.45, 0.3, -0.05][index];
                painter.circle_filled(egui::pos2(knob_x, y), size * 0.075, color);
            }
        }
        Icon::Play => {
            let half = size * 0.28;
            let points = vec![
                egui::pos2(center.x - half * 0.75, center.y - half),
                egui::pos2(center.x + half, center.y),
                egui::pos2(center.x - half * 0.75, center.y + half),
            ];
            painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
        }
        Icon::Pause => {
            let bar = size * 0.12;
            let height = size * 0.5;
            for x in [-0.22f32, 0.22] {
                painter.rect_filled(
                    Rect::from_min_size(
                        egui::pos2(center.x + x * size - bar / 2.0, center.y - height / 2.0),
                        Vec2::new(bar, height),
                    ),
                    CornerRadius::same(1),
                    color,
                );
            }
        }
        Icon::Close => {
            let half = size * 0.22;
            painter.line_segment(
                [
                    egui::pos2(center.x - half, center.y - half),
                    egui::pos2(center.x + half, center.y + half),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(center.x + half, center.y - half),
                    egui::pos2(center.x - half, center.y + half),
                ],
                stroke,
            );
        }
        Icon::Refresh => {
            // Two arcs approximated by short segments.
            let radius = size * 0.28;
            let mut previous: Option<egui::Pos2> = None;
            for step in 0..=16 {
                let angle = -1.1 + step as f32 * 0.28;
                let point = egui::pos2(
                    center.x + radius * angle.cos(),
                    center.y + radius * angle.sin(),
                );
                if let Some(prev) = previous {
                    painter.line_segment([prev, point], stroke);
                }
                previous = Some(point);
            }
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(center.x + radius * 0.55, center.y + radius * 0.95),
                    egui::pos2(center.x + radius * 1.4, center.y + radius * 0.62),
                    egui::pos2(center.x + radius * 0.68, center.y + radius * 0.18),
                ],
                color,
                Stroke::NONE,
            ));
        }
    }
}

/// One navigation entry. Returns the click response.
/// `size` is passed in rather than derived so the bottom bar can divide the
/// window width evenly before entering a horizontal layout.
pub fn nav_item(
    ui: &mut Ui,
    label: &str,
    icon: Icon,
    selected: bool,
    with_label: bool,
    size: Vec2,
    palette: Palette,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());

    if selected || response.hovered() {
        let fill = if selected {
            palette
                .accent
                .linear_multiply(if palette.is_dark { 0.22 } else { 0.14 })
        } else {
            palette.surface_alt
        };
        ui.painter().rect_filled(rect, CornerRadius::same(9), fill);
    }
    if selected {
        // A short accent bar rather than an underline: reads at any width.
        let bar = Rect::from_min_size(
            egui::pos2(rect.left() + 4.0, rect.center().y - 9.0),
            Vec2::new(3.0, 18.0),
        );
        ui.painter()
            .rect_filled(bar, CornerRadius::same(2), palette.accent);
    }

    let color = if selected {
        palette.text
    } else {
        palette.text_dim
    };
    if with_label {
        let icon_rect = Rect::from_center_size(
            egui::pos2(rect.left() + 30.0, rect.center().y),
            Vec2::splat(20.0),
        );
        draw_icon(
            ui,
            icon_rect,
            icon,
            if selected { palette.accent } else { color },
        );
        ui.painter().text(
            egui::pos2(rect.left() + 50.0, rect.center().y),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(14.5),
            color,
        );
    } else {
        let icon_rect = Rect::from_center_size(rect.center(), Vec2::splat(20.0));
        draw_icon(
            ui,
            icon_rect,
            icon,
            if selected { palette.accent } else { color },
        );
    }

    response
}

/// A poster tile: artwork (or a deterministic placeholder), title, resume bar and
/// a bookmark marker.
pub struct CardResult {
    pub response: Response,
}

/// Everything a tile needs beyond the item itself. Grouped so the call sites stay
/// readable and the painter does not take eight positional arguments.
pub struct CardSpec {
    pub width: f32,
    pub poster_height: f32,
    pub cover: Option<egui::TextureHandle>,
    pub progress: Option<f32>,
    pub bookmarked: bool,
    pub palette: Palette,
}

pub fn poster_card(ui: &mut Ui, item: &MediaItem, spec: CardSpec) -> CardResult {
    let CardSpec {
        width,
        poster_height,
        cover,
        progress,
        bookmarked,
        palette,
    } = spec;
    let title_height = 20.0;
    let total = Vec2::new(width, poster_height + title_height);
    let (rect, response) = ui.allocate_exact_size(total, Sense::click());

    let art = Rect::from_min_size(rect.min, Vec2::new(width, poster_height));
    let hovered = response.hovered();
    if hovered {
        ui.painter().rect_filled(
            rect.expand2(Vec2::new(2.0, 2.0)),
            CornerRadius::same(11),
            palette.surface,
        );
    }

    match cover {
        Some(texture) => {
            ui.painter().image(
                texture.id(),
                art,
                Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        None => {
            let (top, bottom) = placeholder_tint(&item.key());
            // Two bands stand in for a gradient without a shader.
            let half = art.height() * 0.55;
            ui.painter().rect_filled(
                Rect::from_min_max(art.min, egui::pos2(art.max.x, art.min.y + half)),
                CornerRadius::same(8),
                top,
            );
            ui.painter().rect_filled(
                Rect::from_min_max(egui::pos2(art.min.x, art.min.y + half), art.max),
                CornerRadius::same(8),
                bottom,
            );
            let initial = item
                .title
                .chars()
                .find(|c| c.is_alphanumeric())
                .map(|c| c.to_uppercase().to_string())
                .unwrap_or_else(|| "?".to_string());
            ui.painter().text(
                art.center(),
                Align2::CENTER_CENTER,
                initial,
                FontId::proportional((poster_height * 0.28).clamp(20.0, 52.0)),
                palette.text.linear_multiply(0.55),
            );
        }
    }

    // Kind badge, top-left.
    if item.kind != app_core::MediaKind::Other {
        let badge = Rect::from_min_size(
            egui::pos2(art.left() + 6.0, art.top() + 6.0),
            Vec2::new(38.0, 16.0),
        );
        ui.painter()
            .rect_filled(badge, CornerRadius::same(4), Color32::from_black_alpha(150));
        ui.painter().text(
            badge.center(),
            Align2::CENTER_CENTER,
            item.kind.label(),
            FontId::proportional(10.0),
            Color32::from_white_alpha(220),
        );
    }

    // Bookmark marker, top-right.
    if bookmarked {
        let rect = Rect::from_center_size(
            egui::pos2(art.right() - 14.0, art.top() + 14.0),
            Vec2::splat(16.0),
        );
        draw_icon(ui, rect, Icon::Bookmark, palette.accent);
    }

    if let Some(fraction) = progress {
        if fraction > 0.01 {
            let track = Rect::from_min_size(
                egui::pos2(art.left() + 4.0, art.bottom() - 6.0),
                Vec2::new(art.width() - 8.0, 3.0),
            );
            ui.painter()
                .rect_filled(track, CornerRadius::same(2), Color32::from_black_alpha(120));
            let filled = Rect::from_min_size(
                track.min,
                Vec2::new(track.width() * fraction.clamp(0.0, 1.0), track.height()),
            );
            ui.painter()
                .rect_filled(filled, CornerRadius::same(2), palette.accent);
        }
    }

    let title = truncate(&item.title, (width / 6.4) as usize);
    ui.painter().text(
        egui::pos2(rect.left() + 2.0, art.bottom() + 4.0),
        Align2::LEFT_TOP,
        title,
        FontId::proportional(12.5),
        if hovered {
            palette.text
        } else {
            palette.text_dim
        },
    );

    CardResult { response }
}

pub fn section_title(ui: &mut Ui, text: &str, palette: Palette) {
    ui.painter().text(
        ui.cursor().min + Vec2::new(0.0, 2.0),
        Align2::LEFT_TOP,
        text,
        FontId::proportional(16.0),
        palette.text,
    );
    ui.add_space(24.0);
}

pub fn muted_text(ui: &mut Ui, text: &str, palette: Palette) -> Response {
    ui.label(egui::RichText::new(text).size(13.0).color(palette.text_dim))
}

pub fn chip(ui: &mut Ui, text: &str, active: bool, palette: Palette) -> Response {
    let padding = Vec2::new(11.0, 5.0);
    let width = text.len() as f32 * 7.2 + padding.x * 2.0;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 26.0), Sense::click());
    let fill = if active {
        palette
            .accent
            .linear_multiply(if palette.is_dark { 0.3 } else { 0.16 })
    } else if response.hovered() {
        palette.surface_alt
    } else {
        palette.surface
    };
    ui.painter().rect_filled(rect, CornerRadius::same(13), fill);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(12.5),
        if active {
            palette.accent
        } else {
            palette.text_dim
        },
    );
    response
}

/// A rectangular button with an icon and optional label, used for transport
/// controls and primary actions.
pub fn icon_button(
    ui: &mut Ui,
    icon: Icon,
    label: Option<&str>,
    size: Vec2,
    selected: bool,
    palette: Palette,
    accent: bool,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let fill = if accent {
        palette.accent
    } else if selected || response.hovered() {
        palette.surface_alt
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        let radius = CornerRadius::same((size.y / 2.0).clamp(4.0, 22.0) as u8);
        ui.painter().rect_filled(rect, radius, fill);
    }
    let color = if accent {
        if palette.is_dark {
            Color32::from_rgb(0x08, 0x0B, 0x12)
        } else {
            Color32::WHITE
        }
    } else {
        palette.text
    };
    match label {
        Some(text) => {
            let icon_rect = Rect::from_center_size(
                egui::pos2(rect.left() + 20.0, rect.center().y),
                Vec2::splat(16.0),
            );
            draw_icon(ui, icon_rect, icon, color);
            ui.painter().text(
                egui::pos2(rect.left() + 34.0, rect.center().y),
                Align2::LEFT_CENTER,
                text,
                FontId::proportional(13.5),
                color,
            );
        }
        None => {
            let icon_rect = Rect::from_center_size(rect.center(), Vec2::splat(size.y * 0.5));
            draw_icon(ui, icon_rect, icon, color);
        }
    }
    response
}

/// Timeline scrubber. Returns the new fraction while dragging.
pub fn seek_bar(ui: &mut Ui, fraction: f32, height: f32, palette: Palette) -> Option<f32> {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), height.max(18.0)),
        Sense::click_and_drag(),
    );
    let track_height = 4.0;
    let track = Rect::from_min_size(
        egui::pos2(rect.left(), rect.center().y - track_height / 2.0),
        Vec2::new(rect.width(), track_height),
    );
    ui.painter()
        .rect_filled(track, CornerRadius::same(2), palette.surface_alt);
    let fraction = fraction.clamp(0.0, 1.0);
    let filled = Rect::from_min_size(
        track.min,
        Vec2::new(track.width() * fraction, track.height()),
    );
    ui.painter()
        .rect_filled(filled, CornerRadius::same(2), palette.accent);
    let knob_x = track.left() + track.width() * fraction;
    ui.painter()
        .circle_filled(egui::pos2(knob_x, rect.center().y), 6.0, palette.accent);

    if response.dragged() || response.clicked() {
        if let Some(pointer) = response.interact_pointer_pos() {
            let raw = (pointer.x - track.left()) / track.width().max(1.0);
            return Some(raw.clamp(0.0, 1.0));
        }
    }
    None
}

pub fn empty_state(
    ui: &mut Ui,
    icon: Icon,
    title: &str,
    body: &str,
    palette: Palette,
    action: Option<&str>,
) -> bool {
    let mut clicked = false;
    ui.add_space(ui.available_height() * 0.18);
    ui.vertical_centered(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(44.0), Sense::hover());
        draw_icon(ui, rect, icon, palette.text_dim);
        ui.add_space(6.0);
        ui.label(egui::RichText::new(title).size(17.0).color(palette.text));
        ui.add_space(2.0);
        ui.label(egui::RichText::new(body).size(13.0).color(palette.text_dim));
        if let Some(label) = action {
            ui.add_space(10.0);
            if ui
                .add(egui::Button::new(
                    egui::RichText::new(label).size(13.5).color(palette.text),
                ))
                .clicked()
            {
                clicked = true;
            }
        }
    });
    clicked
}

pub fn toast(ui: &mut Ui, message: &str, palette: Palette, is_error: bool) -> Response {
    let accent = if is_error {
        palette.danger
    } else {
        palette.success
    };
    let inner = egui::Frame::default()
        .fill(palette.surface)
        .stroke(Stroke::new(1.0_f32, palette.border))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                ui.painter().circle_filled(rect.center(), 4.0, accent);
                ui.label(egui::RichText::new(message).size(12.5).color(palette.text));
            });
        });
    inner.response
}

pub fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// `1:02:03` / `12:34`.
pub fn format_time(seconds: f64) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return "0:00".to_string();
    }
    let total = seconds as u64;
    let (hours, minutes, secs) = (total / 3600, (total % 3600) / 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes}:{secs:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_formatting_covers_hours_and_minutes() {
        assert_eq!(format_time(0.0), "0:00");
        assert_eq!(format_time(9.4), "0:09");
        assert_eq!(format_time(75.0), "1:15");
        assert_eq!(format_time(3725.0), "1:02:05");
        assert_eq!(format_time(f64::NAN), "0:00");
    }

    #[test]
    fn truncation_is_character_safe() {
        assert_eq!(truncate("short", 10), "short");
        let cut = truncate("ünïcodé title that is long", 6);
        assert_eq!(cut.chars().count(), 6);
        assert!(cut.ends_with('…'));
    }
}
