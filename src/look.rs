//! Theme, vector icons and the remote's custom widgets.
use eframe::egui::{self, Color32, CornerRadius, FontId, Pos2, Rect, Response, Sense, Shape, Stroke, Vec2, pos2, vec2};
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

pub const BG: Color32 = Color32::from_rgb(0x0f, 0x0d, 0x16);
pub const BODY: Color32 = Color32::from_rgb(0x1b, 0x17, 0x26);
pub const KEY: Color32 = Color32::from_rgb(0x2a, 0x24, 0x3a);
pub const KEY_HOVER: Color32 = Color32::from_rgb(0x37, 0x2f, 0x4b);
pub const ACCENT: Color32 = Color32::from_rgb(0x7c, 0x4d, 0xff);
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(0x4a, 0x33, 0x8f);
pub const TEXT: Color32 = Color32::from_rgb(0xec, 0xe8, 0xf5);
pub const WEAK: Color32 = Color32::from_rgb(0x9a, 0x93, 0xad);
pub const GOOD: Color32 = Color32::from_rgb(0x4c, 0xd9, 0x8a);
pub const WARN: Color32 = Color32::from_rgb(0xff, 0xb4, 0x54);
pub const POWER: Color32 = Color32::from_rgb(0xff, 0x5c, 0x6c);

pub fn install(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        use egui::TextStyle as T;
        style.text_styles.insert(T::Body, FontId::proportional(15.0));
        style.text_styles.insert(T::Button, FontId::proportional(15.0));
        style.text_styles.insert(T::Heading, FontId::proportional(21.0));
        style.text_styles.insert(T::Small, FontId::proportional(12.5));
        style.spacing.item_spacing = vec2(10.0, 10.0);
        style.spacing.button_padding = vec2(12.0, 7.0);
        style.spacing.interact_size.y = 34.0;

        let v = &mut style.visuals;
        v.panel_fill = BG;
        v.window_fill = BODY;
        v.extreme_bg_color = KEY;
        v.override_text_color = Some(TEXT);
        v.selection.bg_fill = ACCENT_SOFT;
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        v.window_corner_radius = CornerRadius::same(14);
        v.menu_corner_radius = CornerRadius::same(12);
        v.window_stroke = Stroke::new(1.0, KEY_HOVER);
        for (w, fill) in [
            (&mut v.widgets.noninteractive, BODY),
            (&mut v.widgets.inactive, KEY),
            (&mut v.widgets.hovered, KEY_HOVER),
            (&mut v.widgets.active, ACCENT),
            (&mut v.widgets.open, KEY_HOVER),
        ] {
            w.bg_fill = fill;
            w.weak_bg_fill = fill;
            w.corner_radius = CornerRadius::same(10);
            w.bg_stroke = Stroke::NONE;
            w.expansion = 0.0;
        }
        v.widgets.noninteractive.fg_stroke.color = WEAK;
        v.widgets.inactive.fg_stroke.color = TEXT;
        v.widgets.hovered.fg_stroke.color = TEXT;
        v.widgets.active.fg_stroke.color = Color32::WHITE;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT_SOFT);
    });
}

#[derive(Clone, Copy, PartialEq)]
pub enum Icon {
    Power,
    Home,
    Back,
    Options,
    Replay,
    Rev,
    Play,
    Fwd,
    VolDown,
    VolUp,
    Mute,
    Refresh,
    Send,
    Chevron(f32),
}

fn arc(c: Pos2, r: f32, from: f32, to: f32) -> Vec<Pos2> {
    let n = ((to - from).abs() * r / 2.0).ceil().max(8.0) as usize;
    (0..=n).map(|i| from + (to - from) * i as f32 / n as f32).map(|a| c + r * vec2(a.cos(), a.sin())).collect()
}

fn tri(p: &egui::Painter, pts: [Pos2; 3], color: Color32) {
    p.add(Shape::convex_polygon(pts.to_vec(), color, Stroke::NONE));
}

/// Paint `icon` centered in `rect`, scaled to its smaller side.
pub fn paint_icon(p: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) / 2.0;
    let w = (s * 0.14).max(1.6);
    let stroke = Stroke::new(w, color);
    let line = |pts: Vec<Pos2>| {
        p.add(Shape::line(pts, stroke));
    };
    match icon {
        Icon::Power => {
            line(arc(c + vec2(0.0, s * 0.08), s * 0.62, -FRAC_PI_2 + 0.75, -FRAC_PI_2 - 0.75 + TAU));
            line(vec![c + vec2(0.0, -s * 0.78), c + vec2(0.0, -s * 0.05)]);
        }
        Icon::Home => {
            let (l, r, t, b) = (c.x - s * 0.62, c.x + s * 0.62, c.y - s * 0.7, c.y + s * 0.62);
            let eave = c.y - s * 0.08;
            line(vec![pos2(l - s * 0.12, eave + s * 0.1), pos2(c.x, t), pos2(r + s * 0.12, eave + s * 0.1)]);
            line(vec![pos2(l, eave), pos2(l, b), pos2(r, b), pos2(r, eave)]);
            line(vec![pos2(c.x - s * 0.18, b), pos2(c.x - s * 0.18, c.y + s * 0.2), pos2(c.x + s * 0.18, c.y + s * 0.2), pos2(c.x + s * 0.18, b)]);
        }
        Icon::Back => {
            // U-turn arrow pointing left.
            let mut pts = vec![c + vec2(-s * 0.55, -s * 0.3)];
            pts.extend(arc(c + vec2(s * 0.12, s * 0.12), s * 0.42, -FRAC_PI_2, FRAC_PI_2));
            pts.push(c + vec2(-s * 0.3, s * 0.54));
            line(pts);
            line(vec![c + vec2(-s * 0.25, -s * 0.62), c + vec2(-s * 0.58, -s * 0.3), c + vec2(-s * 0.25, s * 0.02)]);
        }
        Icon::Options => {
            for k in 0..3 {
                let a = k as f32 * PI / 3.0 + FRAC_PI_2;
                let d = s * 0.62 * vec2(a.cos(), a.sin());
                line(vec![c - d, c + d]);
            }
        }
        Icon::Replay | Icon::Refresh => {
            let r = s * 0.58;
            let (from, to) = (-FRAC_PI_2 - 0.2, -FRAC_PI_2 + TAU - 1.1);
            line(arc(c, r, from, to));
            let end = c + r * vec2(to.cos(), to.sin());
            let tangent = vec2(-to.sin(), to.cos());
            let normal = vec2(to.cos(), to.sin());
            let h = s * 0.3;
            tri(p, [end + tangent * h, end - normal * h * 0.8, end + normal * h * 0.8], color);
            if icon == Icon::Replay {
                // "Instant replay": a small play mark inside the loop.
                let k = s * 0.22;
                tri(p, [c + vec2(-k * 0.7, -k), c + vec2(-k * 0.7, k), c + vec2(k, 0.0)], color);
            }
        }
        Icon::Rev | Icon::Fwd => {
            let dir = if icon == Icon::Fwd { 1.0 } else { -1.0 };
            let h = s * 0.42;
            for off in [-0.36, 0.36] {
                let x = c.x + off * s;
                tri(p, [pos2(x - dir * h * 0.8, c.y - h), pos2(x - dir * h * 0.8, c.y + h), pos2(x + dir * h * 0.8, c.y)], color);
            }
        }
        Icon::Play => {
            let h = s * 0.5;
            tri(p, [c + vec2(-s * 0.62, -h), c + vec2(-s * 0.62, h), c + vec2(s * 0.1, 0.0)], color);
            for x in [s * 0.3, s * 0.6] {
                p.rect_filled(Rect::from_center_size(c + vec2(x, 0.0), vec2(s * 0.16, h * 2.0)), 1.0, color);
            }
        }
        Icon::VolDown | Icon::VolUp | Icon::Mute => {
            let o = c + vec2(-s * 0.3, 0.0);
            p.add(Shape::convex_polygon(
                vec![
                    o + vec2(-s * 0.42, -s * 0.2),
                    o + vec2(-s * 0.18, -s * 0.2),
                    o + vec2(s * 0.2, -s * 0.55),
                    o + vec2(s * 0.2, s * 0.55),
                    o + vec2(-s * 0.18, s * 0.2),
                    o + vec2(-s * 0.42, s * 0.2),
                ],
                color,
                Stroke::NONE,
            ));
            let m = c + vec2(s * 0.48, 0.0);
            let k = s * 0.24;
            match icon {
                Icon::VolDown => line(vec![m - vec2(k, 0.0), m + vec2(k, 0.0)]),
                Icon::VolUp => {
                    line(vec![m - vec2(k, 0.0), m + vec2(k, 0.0)]);
                    line(vec![m - vec2(0.0, k), m + vec2(0.0, k)]);
                }
                _ => {
                    line(vec![m - vec2(k, k), m + vec2(k, k)]);
                    line(vec![m + vec2(-k, k), m + vec2(k, -k)]);
                }
            }
        }
        Icon::Send => {
            tri(p, [c + vec2(-s * 0.6, -s * 0.55), c + vec2(s * 0.65, 0.0), c + vec2(-s * 0.6, s * 0.55)], color);
            tri(p, [c + vec2(-s * 0.6, -s * 0.12), c + vec2(-s * 0.05, 0.0), c + vec2(-s * 0.6, s * 0.12)], BODY);
        }
        Icon::Chevron(a) => {
            let (d, n) = (vec2(a.cos(), a.sin()), vec2(-a.sin(), a.cos()));
            let k = s * 0.42;
            line(vec![c - d * k * 0.5 + n * k, c + d * k * 0.5, c - d * k * 0.5 - n * k]);
        }
    }
}

/// A key on the remote: rounded (or round, when `radius` is half its height),
/// lit while pressed, while `lit`, and on hover.
pub fn key(ui: &mut egui::Ui, size: Vec2, radius: f32, lit: bool, draw: impl FnOnce(&egui::Painter, Rect, Color32)) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let pressed = resp.is_pointer_button_down_on() || lit;
        let fill = if pressed {
            ACCENT
        } else if resp.hovered() {
            KEY_HOVER
        } else {
            KEY
        };
        let p = ui.painter();
        p.rect_filled(rect.translate(vec2(0.0, 2.0)), radius, Color32::from_black_alpha(70));
        p.rect_filled(rect, radius, fill);
        draw(p, rect, if pressed { Color32::WHITE } else { TEXT });
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn icon_key(ui: &mut egui::Ui, size: Vec2, icon: Icon, lit: bool, tint: Option<Color32>) -> Response {
    let radius = size.y / 2.0;
    key(ui, size, radius, lit, |p, rect, color| {
        let color = if lit { color } else { tint.unwrap_or(color) };
        paint_icon(p, Rect::from_center_size(rect.center(), Vec2::splat(size.y * 0.52)), icon, color);
    })
}

pub fn text_key(ui: &mut egui::Ui, size: Vec2, text: &str, lit: bool) -> Response {
    key(ui, size, size.y / 2.0, lit, |p, rect, color| {
        p.text(rect.center(), egui::Align2::CENTER_CENTER, text, FontId::proportional(13.0), color);
    })
}

/// Round directional pad. Returns the key clicked, if any.
pub fn dpad(ui: &mut egui::Ui, diameter: f32, lit: Option<&str>) -> Option<&'static str> {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(diameter), Sense::click());
    let c = rect.center();
    let r = diameter / 2.0;
    let ri = r * 0.4;
    let zone = |pos: Pos2| -> Option<&'static str> {
        let d = pos - c;
        let len = d.length();
        if len <= ri {
            Some("Select")
        } else if len <= r {
            Some(if d.x.abs() > d.y.abs() {
                if d.x > 0.0 { "Right" } else { "Left" }
            } else if d.y > 0.0 {
                "Down"
            } else {
                "Up"
            })
        } else {
            None
        }
    };
    let hover = resp.hover_pos().and_then(zone);
    let held = if resp.is_pointer_button_down_on() { hover } else { None };
    if !ui.is_rect_visible(rect) {
        return None;
    }
    let p = ui.painter();
    p.circle_filled(c + vec2(0.0, 3.0), r, Color32::from_black_alpha(80));
    p.circle_filled(c, r, KEY);
    let ring_mid = (r + ri) / 2.0;
    let ring_w = r - ri - 6.0;
    let dirs = [("Right", 0.0), ("Down", FRAC_PI_2), ("Left", PI), ("Up", -FRAC_PI_2)];
    for (name, a) in dirs {
        let on = held == Some(name) || lit == Some(name);
        let fill = if on {
            Some(ACCENT)
        } else if hover == Some(name) {
            Some(KEY_HOVER)
        } else {
            None
        };
        if let Some(fill) = fill {
            p.add(Shape::line(arc(c, ring_mid, a - FRAC_PI_4 + 0.05, a + FRAC_PI_4 - 0.05), Stroke::new(ring_w, fill)));
        }
        let at = c + ring_mid * vec2(a.cos(), a.sin());
        let color = if on { Color32::WHITE } else { TEXT };
        paint_icon(p, Rect::from_center_size(at, Vec2::splat(r * 0.32)), Icon::Chevron(a), color);
    }
    // Diagonal seams between the four directions.
    for k in 0..4 {
        let a = FRAC_PI_4 + k as f32 * FRAC_PI_2;
        let d = vec2(a.cos(), a.sin());
        p.line_segment([c + d * (ri + 2.0), c + d * (r - 2.0)], Stroke::new(3.0, BODY));
    }
    let ok_on = held == Some("Select") || lit == Some("Select");
    p.circle_filled(c, ri + 1.0, BODY);
    p.circle_filled(
        c,
        ri - 4.0,
        if ok_on {
            ACCENT
        } else if hover == Some("Select") {
            KEY_HOVER
        } else {
            KEY
        },
    );
    p.text(c, egui::Align2::CENTER_CENTER, "OK", FontId::proportional(r * 0.2), if ok_on { Color32::WHITE } else { TEXT });
    if hover.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if resp.clicked() { resp.interact_pointer_pos().and_then(zone) } else { None }
}
