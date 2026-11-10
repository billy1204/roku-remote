//! Theme, vector icons and the remote's custom widgets.
use eframe::egui::{self, Color32, CornerRadius, FontId, Pos2, Rect, Response, Sense, Shape, Stroke, Vec2, pos2, vec2};
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// The colors a theme changes.
pub struct Palette {
    pub bg: Color32,
    pub body: Color32,
    pub key: Color32,
    pub key_hover: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub text: Color32,
    pub weak: Color32,
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

const fn palette(c: [u32; 8]) -> Palette {
    Palette {
        bg: rgb(c[0]),
        body: rgb(c[1]),
        key: rgb(c[2]),
        key_hover: rgb(c[3]),
        accent: rgb(c[4]),
        accent_soft: rgb(c[5]),
        text: rgb(c[6]),
        weak: rgb(c[7]),
    }
}

/// Themes by name, the first is the default.
pub const THEMES: [(&str, Palette); 6] = [
    ("Violet", palette([0x0f0d16, 0x1b1726, 0x2a243a, 0x372f4b, 0x7c4dff, 0x4a338f, 0xece8f5, 0x9a93ad])),
    ("Ocean", palette([0x0b1118, 0x14202c, 0x1f3142, 0x2a4157, 0x2f8fff, 0x1f538a, 0xe6f1fa, 0x8fa3b5])),
    ("Forest", palette([0x0c130f, 0x15211a, 0x213327, 0x2c4434, 0x22a95e, 0x1b6a40, 0xe6f4ea, 0x8ea89a])),
    ("Sunset", palette([0x15100c, 0x241a14, 0x36281e, 0x473428, 0xf2672a, 0x8f4223, 0xf7ece4, 0xb09d90])),
    ("Rose", palette([0x150c11, 0x24151d, 0x37212c, 0x482c3a, 0xf0407e, 0x8f2c4c, 0xf8e8ef, 0xb294a3])),
    ("Graphite", palette([0x101112, 0x1b1c1f, 0x2a2b2f, 0x37393e, 0x5f6b7a, 0x3f4651, 0xeceef1, 0x989ca4])),
];

static THEME: AtomicUsize = AtomicUsize::new(0);
static ANIMATE: AtomicBool = AtomicBool::new(true);

pub fn pal() -> &'static Palette {
    &THEMES[THEME.load(Ordering::Relaxed)].1
}

pub fn theme() -> usize {
    THEME.load(Ordering::Relaxed)
}

/// Switch to theme `index` (out of range keeps the current one).
pub fn set_theme(ctx: &egui::Context, index: usize) {
    if index < THEMES.len() {
        THEME.store(index, Ordering::Relaxed);
        install(ctx);
    }
}

pub fn animated() -> bool {
    ANIMATE.load(Ordering::Relaxed)
}

pub fn set_animated(on: bool) {
    ANIMATE.store(on, Ordering::Relaxed);
}

/// 0 to 1 as `on` turns true, eased over `secs`; instant with animations off.
fn ease(ctx: &egui::Context, id: egui::Id, on: bool, secs: f32) -> f32 {
    if animated() {
        ctx.animate_bool_with_time_and_easing(id, on, secs, egui::emath::easing::cubic_out)
    } else {
        on as u8 as f32
    }
}

const PULSE_SECS: f64 = 0.45;

/// How far through its click pulse the widget `id` is, if it's in one.
/// `clicked` starts a new pulse.
fn pulse(ctx: &egui::Context, id: egui::Id, clicked: bool) -> Option<f32> {
    if !animated() {
        return None;
    }
    let id = id.with("pulse");
    let now = ctx.input(|i| i.time);
    if clicked {
        ctx.data_mut(|d| d.insert_temp(id, now));
    }
    let t = (now - ctx.data(|d| d.get_temp::<f64>(id))?) / PULSE_SECS;
    if t >= 1.0 {
        return None;
    }
    ctx.request_repaint();
    Some(t as f32)
}

/// An accent ring spreading out from `rect` and fading.
fn paint_pulse(p: &egui::Painter, rect: Rect, radius: f32, t: f32) {
    let k = egui::emath::easing::cubic_out(t);
    let grow = 9.0 * k;
    let color = pal().accent.gamma_multiply(0.75 * (1.0 - k));
    p.rect_stroke(rect.expand(grow), radius + grow, Stroke::new(2.5 * (1.0 - k) + 0.5, color), egui::StrokeKind::Outside);
}

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
        v.panel_fill = pal().bg;
        v.window_fill = pal().body;
        v.extreme_bg_color = pal().key;
        v.override_text_color = Some(pal().text);
        v.selection.bg_fill = pal().accent_soft;
        v.selection.stroke = Stroke::new(1.0, pal().accent);
        v.window_corner_radius = CornerRadius::same(14);
        v.menu_corner_radius = CornerRadius::same(12);
        v.window_stroke = Stroke::new(1.0, pal().key_hover);
        for (w, fill) in [
            (&mut v.widgets.noninteractive, pal().body),
            (&mut v.widgets.inactive, pal().key),
            (&mut v.widgets.hovered, pal().key_hover),
            (&mut v.widgets.active, pal().accent),
            (&mut v.widgets.open, pal().key_hover),
        ] {
            w.bg_fill = fill;
            w.weak_bg_fill = fill;
            w.corner_radius = CornerRadius::same(10);
            w.bg_stroke = Stroke::NONE;
            w.expansion = 0.0;
        }
        v.widgets.noninteractive.fg_stroke.color = pal().weak;
        v.widgets.inactive.fg_stroke.color = pal().text;
        v.widgets.hovered.fg_stroke.color = pal().text;
        v.widgets.active.fg_stroke.color = Color32::WHITE;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, pal().accent_soft);
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
    PlayOnly,
    Pause,
    Fwd,
    VolDown,
    VolUp,
    Mute,
    Refresh,
    Send,
    Appearance,
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
        Icon::PlayOnly => {
            let h = s * 0.62;
            tri(p, [c + vec2(-s * 0.45, -h), c + vec2(-s * 0.45, h), c + vec2(s * 0.62, 0.0)], color);
        }
        Icon::Pause => {
            for x in [-s * 0.26, s * 0.26] {
                p.rect_filled(Rect::from_center_size(c + vec2(x, 0.0), vec2(s * 0.24, s * 1.15)), 1.5, color);
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
            tri(p, [c + vec2(-s * 0.6, -s * 0.12), c + vec2(-s * 0.05, 0.0), c + vec2(-s * 0.6, s * 0.12)], pal().body);
        }
        Icon::Appearance => {
            // A circle, half filled: light and dark.
            let r = s * 0.62;
            line(arc(c, r, 0.0, TAU));
            p.add(Shape::convex_polygon(arc(c, r, -FRAC_PI_2, FRAC_PI_2), color, Stroke::NONE));
        }
        Icon::Chevron(a) => {
            let (d, n) = (vec2(a.cos(), a.sin()), vec2(-a.sin(), a.cos()));
            let k = s * 0.42;
            line(vec![c - d * k * 0.5 + n * k, c + d * k * 0.5, c - d * k * 0.5 - n * k]);
        }
    }
}

/// A key on the remote: rounded (or round, when `radius` is half its height),
/// lit while pressed, while `lit`, and on hover. With animations on, the
/// colors fade, the key sinks while held and a ring spreads when clicked.
pub fn key(ui: &mut egui::Ui, size: Vec2, radius: f32, lit: bool, draw: impl FnOnce(&egui::Painter, Rect, Color32)) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let ctx = ui.ctx();
        let pressed = resp.is_pointer_button_down_on() || lit;
        let hover = ease(ctx, resp.id.with("hover"), resp.hovered(), 0.15);
        let down = ease(ctx, resp.id.with("down"), pressed, 0.1);
        let fill = pal().key.lerp_to_gamma(pal().key_hover, hover).lerp_to_gamma(pal().accent, down);
        let (face, sink) = if animated() { (rect.shrink(1.5 * down), 1.5 * down) } else { (rect, 0.0) };
        let face = face.translate(vec2(0.0, sink));
        let p = ui.painter();
        p.rect_filled(rect.translate(vec2(0.0, 2.0)), radius, Color32::from_black_alpha(70));
        p.rect_filled(face, radius, fill);
        if let Some(t) = pulse(ctx, resp.id, resp.clicked()) {
            paint_pulse(p, rect, radius, t);
        }
        draw(p, face, pal().text.lerp_to_gamma(Color32::WHITE, down));
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
    p.circle_filled(c, r, pal().key);
    let ring_mid = (r + ri) / 2.0;
    let ring_w = r - ri - 6.0;
    let dirs = [("Right", 0.0), ("Down", FRAC_PI_2), ("Left", PI), ("Up", -FRAC_PI_2)];
    let ctx = ui.ctx();
    for (name, a) in dirs {
        let on = held == Some(name) || lit == Some(name);
        let id = resp.id.with(name);
        let h = ease(ctx, id.with("hover"), hover == Some(name), 0.15);
        let d = ease(ctx, id.with("down"), on, 0.1);
        if h > 0.0 || d > 0.0 {
            let fill = pal().key.lerp_to_gamma(pal().key_hover, h).lerp_to_gamma(pal().accent, d);
            p.add(Shape::line(arc(c, ring_mid, a - FRAC_PI_4 + 0.05, a + FRAC_PI_4 - 0.05), Stroke::new(ring_w, fill)));
        }
        let dir = vec2(a.cos(), a.sin());
        // The chevron nudges the way it points while pressed.
        let nudge = if animated() { 4.0 * d } else { 0.0 };
        let at = c + (ring_mid + nudge) * dir;
        let color = pal().text.lerp_to_gamma(Color32::WHITE, d);
        paint_icon(p, Rect::from_center_size(at, Vec2::splat(r * 0.32)), Icon::Chevron(a), color);
    }
    // Diagonal seams between the four directions.
    for k in 0..4 {
        let a = FRAC_PI_4 + k as f32 * FRAC_PI_2;
        let d = vec2(a.cos(), a.sin());
        p.line_segment([c + d * (ri + 2.0), c + d * (r - 2.0)], Stroke::new(3.0, pal().body));
    }
    let ok_on = held == Some("Select") || lit == Some("Select");
    let h = ease(ctx, resp.id.with("ok-hover"), hover == Some("Select"), 0.15);
    let d = ease(ctx, resp.id.with("ok-down"), ok_on, 0.1);
    let ok_r = if animated() { ri - 4.0 - 2.0 * d } else { ri - 4.0 };
    p.circle_filled(c, ri + 1.0, pal().body);
    p.circle_filled(c, ok_r, pal().key.lerp_to_gamma(pal().key_hover, h).lerp_to_gamma(pal().accent, d));
    p.text(c, egui::Align2::CENTER_CENTER, "OK", FontId::proportional(r * 0.2), pal().text.lerp_to_gamma(Color32::WHITE, d));
    let clicked = if resp.clicked() { resp.interact_pointer_pos().and_then(zone) } else { None };
    // The ring spreads from the whole pad for a direction, from OK for OK.
    let ring = resp.id.with(if clicked == Some("Select") { "ok" } else { "ring" });
    for (part, size) in [("ok", (ri - 4.0) * 2.0), ("ring", diameter)] {
        let id = resp.id.with(part);
        if let Some(t) = pulse(ctx, id, clicked.is_some() && id == ring) {
            paint_pulse(p, Rect::from_center_size(c, Vec2::splat(size)), size / 2.0, t);
        }
    }
    if hover.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    clicked
}

/// An on/off switch whose knob slides across.
pub fn toggle(ui: &mut egui::Ui, on: &mut bool) -> Response {
    let size = vec2(44.0, 24.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *on, ""));
    if ui.is_rect_visible(rect) {
        let t = ease(ui.ctx(), resp.id, *on, 0.18);
        let r = size.y / 2.0;
        let track = pal().key_hover.lerp_to_gamma(pal().accent, t);
        let p = ui.painter();
        p.rect_filled(rect, r, track);
        let x = egui::lerp(rect.left() + r..=rect.right() - r, t);
        let knob = if resp.hovered() { r - 2.0 } else { r - 3.0 };
        p.circle_filled(pos2(x, rect.center().y + 1.0), knob, Color32::from_black_alpha(60));
        p.circle_filled(pos2(x, rect.center().y), knob, Color32::WHITE);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A round sample of a theme: its key color ringed by its accent, with a
/// ring around it when it's the current one.
pub fn swatch(ui: &mut egui::Ui, theme: &Palette, selected: bool) -> Response {
    let d = 34.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(d, d), Sense::click());
    if ui.is_rect_visible(rect) {
        let ctx = ui.ctx();
        let sel = ease(ctx, resp.id.with("sel"), selected, 0.2);
        let hover = ease(ctx, resp.id.with("hover"), resp.hovered(), 0.15);
        let c = rect.center();
        let r = d / 2.0 - 4.0 + 1.5 * hover;
        let p = ui.painter();
        p.circle_filled(c, r, theme.accent);
        p.circle_filled(c + vec2(r * 0.25, r * 0.25), r * 0.45, theme.key);
        if sel > 0.0 {
            p.circle_stroke(c, r + 3.0, Stroke::new(2.0, pal().text.gamma_multiply(sel)));
        }
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}
