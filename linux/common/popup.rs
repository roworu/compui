//! the popup as a gtk4 layer-shell overlay, drawn like kuterm's: kuterm's one dark / one light colors,
//! rows one terminal line high, label column in the kind's color, smaller muted descriptions, the
//! selected one's whole description under a divider, a scrollbar thumb when the list is longer
use crate::{App, Cand, Popup, ROWS, Rect, log};
use gtk4::{self as gtk, cairo, gdk, pango, prelude::*};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use std::{cell::RefCell, rc::Rc};
use zbus::blocking::Connection;

pub struct Theme {
    bg: u32,
    border: u32,
    selected: u32,
    text: u32,
    muted: u32,
    scrollbar: u32,
    /// subcommand, suggestion, option, command, folder: ansi magenta, cyan, yellow, green, blue
    kinds: [u32; 5],
}

/// kuterm's bundled dark and light themes
pub const DARK: Theme = Theme { bg: 0x2f343e, border: 0x464b57, selected: 0x282c33, text: 0xdce0e5, muted: 0xa9afbc, scrollbar: 0x4e5666, kinds: [0xc678dd, 0x56b6c2, 0xe5c07b, 0x98c379, 0x61afef] };
pub const LIGHT: Theme = Theme { bg: 0xebebec, border: 0xc9c9ca, selected: 0xfafafa, text: 0x383a42, muted: 0x6a6b70, scrollbar: 0xc4c4c7, kinds: [0xa626a4, 0x0997b3, 0xc18401, 0x50a14f, 0x0184bc] };

impl Theme {
    fn kind(&self, kind: &str) -> u32 {
        match kind {
            "subcommand" => self.kinds[0],
            "suggestion" => self.kinds[1],
            "option" => self.kinds[2],
            "command" => self.kinds[3],
            "folder" => self.kinds[4],
            _ => self.text,
        }
    }
}

// widest label and description, in cells, longer ones end in an ellipsis
const MAX_LABEL_CELLS: usize = 40;
const MAX_DESCRIPTION_CELLS: usize = 32;
const DESCRIPTION_SCALE: f64 = 0.85;
const MAX_DETAIL_LINES: i32 = 3;
const PAD: f64 = 6.0;
const RADIUS: f64 = 6.0;
const FONT: &str = "JetBrains Mono, monospace";
/// room around the box for the shadow, which the compositor doesn't draw for layer surfaces
const SHADOW: (f64, f64, f64, f64) = (14.0, 6.0, 14.0, 26.0); // left, top, right, bottom

/// what the draw function paints, laid out by `show`
#[derive(Default)]
struct View {
    dark: bool,
    /// label, description, label color
    rows: Vec<(String, String, u32)>,
    selected: Option<usize>,
    detail: Option<String>,
    /// thumb offset and height
    thumb: Option<(f64, f64)>,
    font: f64,
    row_h: f64,
    label_w: f64,
    w: f64,
    h: f64,
}

pub struct Gtk {
    win: gtk::Window,
    area: gtk::DrawingArea,
    view: Rc<RefCell<View>>,
    bus: Option<Connection>,
}

fn rgb(cr: &cairo::Context, c: u32) {
    cr.set_source_rgb(((c >> 16) & 0xff) as f64 / 255.0, ((c >> 8) & 0xff) as f64 / 255.0, (c & 0xff) as f64 / 255.0);
}

fn rounded(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    use std::f64::consts::PI;
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -PI / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, PI / 2.0);
    cr.arc(x + r, y + h - r, r, PI / 2.0, PI);
    cr.arc(x + r, y + r, r, PI, 3.0 * PI / 2.0);
    cr.close_path();
}

/// a one line layout cut to `width` with an ellipsis, or wrapped to `lines` when given
fn layout(ctx: &pango::Context, text: &str, px: f64, width: f64, lines: Option<i32>) -> pango::Layout {
    let l = pango::Layout::new(ctx);
    let mut font = pango::FontDescription::from_string(FONT);
    font.set_absolute_size(px * pango::SCALE as f64);
    l.set_font_description(Some(&font));
    l.set_text(text);
    l.set_width((width.max(1.0) * pango::SCALE as f64) as i32);
    l.set_ellipsize(pango::EllipsizeMode::End);
    if let Some(n) = lines {
        l.set_wrap(pango::WrapMode::WordChar);
        l.set_height(-n);
    }
    l
}

fn size(l: &pango::Layout) -> (f64, f64) {
    let (w, h) = l.size();
    (w as f64 / pango::SCALE as f64, h as f64 / pango::SCALE as f64)
}

fn draw(v: &View, cr: &cairo::Context) {
    let t = if v.dark { &DARK } else { &LIGHT };
    let (w, h) = (v.w, v.h);
    // soft shadow, roughly a large drop shadow: stacked translucent rounded rects
    for i in 0..10 {
        let e = i as f64 * 1.5 - 3.0;
        rounded(cr, SHADOW.0 - e, SHADOW.1 + 8.0 - e, w + 2.0 * e, h + 2.0 * e, RADIUS + e.max(0.0));
        cr.set_source_rgba(0.0, 0.0, 0.0, if v.dark { 0.04 } else { 0.02 });
        let _ = cr.fill();
    }
    cr.translate(SHADOW.0, SHADOW.1);
    rounded(cr, 0.0, 0.0, w, h, RADIUS);
    rgb(cr, t.bg);
    let _ = cr.fill_preserve();
    cr.save().ok();
    cr.clip();
    let ctx = pangocairo::functions::create_context(cr);
    let desc_px = v.font * DESCRIPTION_SCALE;
    let top = 4.0;
    for (i, (label, desc, color)) in v.rows.iter().enumerate() {
        let y = top + i as f64 * v.row_h;
        if v.selected == Some(i) {
            rgb(cr, t.selected);
            cr.rectangle(0.0, y, w, v.row_h);
            let _ = cr.fill();
        }
        let l = layout(&ctx, label, v.font, v.label_w, None);
        cr.move_to(PAD, y + (v.row_h - size(&l).1) / 2.0);
        rgb(cr, *color);
        pangocairo::functions::show_layout(cr, &l);
        if !desc.is_empty() {
            let x = PAD + v.label_w + v.row_h * 0.5;
            let l = layout(&ctx, desc, desc_px, w - x - PAD, None);
            cr.move_to(x, y + (v.row_h - size(&l).1) / 2.0);
            rgb(cr, t.muted);
            pangocairo::functions::show_layout(cr, &l);
        }
    }
    if let Some(d) = &v.detail {
        let y = top + v.rows.len() as f64 * v.row_h + 3.0;
        rgb(cr, t.border);
        cr.rectangle(PAD, y, w - 2.0 * PAD, 1.0);
        let _ = cr.fill();
        let l = layout(&ctx, d, desc_px, w - 2.0 * PAD, Some(MAX_DETAIL_LINES));
        cr.move_to(PAD, y + 3.0);
        rgb(cr, t.muted);
        pangocairo::functions::show_layout(cr, &l);
    }
    if let Some((off, len)) = v.thumb {
        rounded(cr, w - 4.0, top + off, 3.0, len, 1.5);
        rgb(cr, t.scrollbar);
        let _ = cr.fill();
    }
    cr.restore().ok();
    rounded(cr, 0.5, 0.5, w - 1.0, h - 1.0, RADIUS - 0.5);
    rgb(cr, t.border);
    cr.set_line_width(1.0);
    let _ = cr.stroke();
}

impl Gtk {
    pub fn new() -> Self {
        let win = gtk::Window::new();
        win.init_layer_shell();
        win.set_namespace(Some("compui"));
        win.set_layer(Layer::Overlay);
        win.set_keyboard_mode(KeyboardMode::None);
        win.set_anchor(Edge::Top, true);
        win.set_anchor(Edge::Left, true);
        win.add_css_class("compui");
        // ignores the mouse in v1, a click would go to the popup instead of the terminal
        win.connect_map(|w| {
            if let Some(s) = w.surface() {
                s.set_input_region(Some(&cairo::Region::create()));
            }
        });
        let css = gtk::CssProvider::new();
        css.load_from_string("window.compui { background: transparent; }");
        gtk::style_context_add_provider_for_display(&WidgetExt::display(&win), &css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
        let view = Rc::new(RefCell::new(View::default()));
        let area = gtk::DrawingArea::new();
        let v = view.clone();
        area.set_draw_func(move |_, cr, _, _| draw(&v.borrow(), cr));
        win.set_child(Some(&area));
        Gtk { win, area, view, bus: Connection::session().ok() }
    }

    fn monitor(&self, output: &str) -> Option<gdk::Monitor> {
        let ms = WidgetExt::display(&self.win).monitors();
        let all: Vec<gdk::Monitor> = (0..ms.n_items()).filter_map(|i| ms.item(i)?.downcast().ok()).collect();
        all.iter().find(|m| m.connector().as_deref() == Some(output)).or(all.first()).cloned()
    }

    /// the desktop's color scheme from the settings portal; dark unless it says light, as kuterm
    fn dark(&self) -> bool {
        let read = |c: &Connection| -> zbus::Result<u32> {
            let m = c.call_method(Some("org.freedesktop.portal.Desktop"), "/org/freedesktop/portal/desktop", Some("org.freedesktop.portal.Settings"), "ReadOne", &("org.freedesktop.appearance", "color-scheme"))?;
            let v: zbus::zvariant::OwnedValue = m.body().deserialize()?;
            Ok(u32::try_from(v)?)
        };
        self.bus.as_ref().and_then(|c| read(c).ok()) != Some(2)
    }
}

impl Popup for Gtk {
    fn show(&mut self, app: &App, cell: Rect, cands: &[Cand], selected: usize, top: usize) {
        let Some(mon) = self.monitor(&app.output) else { return };
        let g = mon.geometry();
        let dark = if self.win.is_visible() { self.view.borrow().dark } else { self.dark() };
        let t = if dark { &DARK } else { &LIGHT };
        let ctx = self.area.pango_context();
        // the font is sized so a char is as wide as the terminal's cell
        let probe = |px: f64| size(&layout(&ctx, "MMMMMMMMMM", px, 1e4, None)).0 / 10.0;
        let font = (cell.w / 0.6).clamp(6.0, 72.0);
        let font = font * cell.w / probe(font).max(1.0);
        let adv = probe(font);
        let shown = &cands[top.min(cands.len())..cands.len().min(top + ROWS)];
        let cells = |s: &str, max: usize| s.chars().count().min(max);
        let label_cells = shown.iter().map(|c| cells(&c.label, MAX_LABEL_CELLS)).max().unwrap_or(0).max(12);
        let desc_cells = shown.iter().filter(|c| !c.desc.is_empty()).map(|c| cells(&c.desc, MAX_DESCRIPTION_CELLS)).max().map_or(0, |n| n + 2);
        let w = (adv * (label_cells + desc_cells) as f64 + 2.0 * PAD).min(g.width() as f64 - 8.0).ceil();
        let row_h = cell.h.round();
        let detail = cands.get(selected).map(|c| c.desc.clone()).filter(|d| !d.is_empty());
        let detail_h = detail.as_ref().map_or(0.0, |d| size(&layout(&ctx, d, font * DESCRIPTION_SCALE, w - 2.0 * PAD, Some(MAX_DETAIL_LINES))).1 + 7.0);
        let h = (row_h * shown.len() as f64 + 8.0 + detail_h).ceil();
        let thumb = (cands.len() > ROWS).then(|| {
            let track = row_h * ROWS as f64;
            let len = (track * ROWS as f64 / cands.len() as f64).max(8.0);
            ((track - len) * top as f64 / (cands.len() - ROWS) as f64, len)
        });
        *self.view.borrow_mut() = View {
            dark,
            rows: shown.iter().map(|c| (c.label.clone(), c.desc.clone(), t.kind(&c.kind))).collect(),
            selected: selected.checked_sub(top).filter(|i| *i < shown.len()),
            detail,
            thumb,
            font,
            row_h,
            label_w: adv * label_cells as f64,
            w,
            h,
        };
        // below the line, above when it doesn't fit there and there is more room above; 4 px off the edges
        let (mx, my, mw, mh) = (g.x() as f64, g.y() as f64, g.width() as f64, g.height() as f64);
        let below = cell.y + cell.h;
        let y = if below + h > my + mh - 4.0 && cell.y - my > my + mh - below { cell.y - h } else { below };
        let x = cell.x.min(mx + mw - w - 4.0).max(mx + 4.0);
        let (sw, sh) = ((w + SHADOW.0 + SHADOW.2) as i32, (h + SHADOW.1 + SHADOW.3) as i32);
        log(&format!("popup at {x},{y} on {} size {w}x{h}", app.output));
        self.win.set_monitor(Some(&mon));
        self.win.set_margin(Edge::Left, (x - mx - SHADOW.0) as i32);
        self.win.set_margin(Edge::Top, (y - my - SHADOW.1) as i32);
        self.area.set_content_width(sw);
        self.area.set_content_height(sh);
        // shrinks to the new content, a gtk window keeps its largest size otherwise
        self.win.set_default_size(sw, sh);
        self.area.queue_draw();
        self.win.set_visible(true);
    }

    fn hide(&mut self) {
        self.win.set_visible(false);
    }
}
