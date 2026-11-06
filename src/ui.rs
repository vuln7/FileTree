//! The working shell around the tree: toolbar, panel, status line. Familiar controls,
//! cut from the same paper as the tree.

use crate::app::{Action, App, ListItem, SortBy, Zone, inside};
use crate::fs::Preview;
use crate::gpu::{FlowerInst, RectInst, UiLayer, rgb};
use crate::palette as p;
use crate::scene::{draw_text, fit_label, mix_rgb};
use crate::text::Face;
use crate::tree::{Kind, Load, NONE, Species};

const NO_HALO: [f32; 4] = [0.0; 4];

pub fn rect(a: [f32; 2], b: [f32; 2], radius: f32, color: [f32; 4]) -> RectInst {
    RectInst { a, b, params: [radius, 0.0, 0.0, 0.0], color }
}

pub fn soft_rect(a: [f32; 2], b: [f32; 2], radius: f32, blur: f32, color: [f32; 4]) -> RectInst {
    RectInst { a, b, params: [radius, blur, 0.0, 0.0], color }
}

/// A big sheet of paper, with the slightly uneven edge of a scissor cut.
fn cut_rect(a: [f32; 2], b: [f32; 2], wander: f32, color: [f32; 4]) -> RectInst {
    RectInst { a, b, params: [0.0, 0.0, 0.0, wander], color }
}

pub fn line(a: [f32; 2], b: [f32; 2], radius: f32, color: [f32; 4]) -> RectInst {
    RectInst { a, b, params: [radius, 0.0, 1.0, 0.0], color }
}

pub fn ring(center: [f32; 2], radius: f32, thickness: f32, color: [f32; 4]) -> RectInst {
    RectInst { a: center, b: [radius, 0.0], params: [0.0, 0.0, 2.0, thickness], color }
}

pub fn with_alpha(mut c: [f32; 4], a: f32) -> [f32; 4] {
    c[3] = a;
    c
}

#[derive(Clone, Copy)]
pub enum Icon {
    Back,
    Forward,
    Up,
    Search,
    Close,
    Fit,
    Eye,
    EyeOff,
    Calm,
    Lively,
    Reset,
    Rename,
    Copy,
    Cut,
    Trash,
    More,
    NewFolder,
    Paste,
}

/// Icons are drawn as line work, all on one 20-unit grid with one stroke weight.
pub fn draw_icon(layer: &mut UiLayer, icon: Icon, c: [f32; 2], s: f32, color: [f32; 4]) {
    let w = 0.95 * s;
    let pt = |x: f32, y: f32| [c[0] + x * s, c[1] + y * s];
    let mut seg = |a: [f32; 2], b: [f32; 2]| layer.rects.push(line(a, b, w, color));
    match icon {
        Icon::Back => {
            seg(pt(3.0, -6.0), pt(-3.0, 0.0));
            seg(pt(-3.0, 0.0), pt(3.0, 6.0));
        }
        Icon::Forward => {
            seg(pt(-3.0, -6.0), pt(3.0, 0.0));
            seg(pt(3.0, 0.0), pt(-3.0, 6.0));
        }
        Icon::Up => {
            seg(pt(0.0, 7.0), pt(0.0, -7.0));
            seg(pt(-5.5, -1.5), pt(0.0, -7.0));
            seg(pt(5.5, -1.5), pt(0.0, -7.0));
        }
        Icon::Close => {
            seg(pt(-5.0, -5.0), pt(5.0, 5.0));
            seg(pt(5.0, -5.0), pt(-5.0, 5.0));
        }
        Icon::Search => {
            seg(pt(3.6, 3.6), pt(7.5, 7.5));
            layer.rects.push(ring(pt(-1.0, -1.0), 5.8 * s, w, color));
        }
        Icon::Fit => {
            for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                seg(pt(7.0 * x, 7.0 * y), pt(3.0 * x, 7.0 * y));
                seg(pt(7.0 * x, 7.0 * y), pt(7.0 * x, 3.0 * y));
            }
        }
        Icon::Eye | Icon::EyeOff => {
            seg(pt(-8.0, 0.0), pt(-3.0, -4.5));
            seg(pt(-3.0, -4.5), pt(3.0, -4.5));
            seg(pt(3.0, -4.5), pt(8.0, 0.0));
            seg(pt(8.0, 0.0), pt(3.0, 4.5));
            seg(pt(3.0, 4.5), pt(-3.0, 4.5));
            seg(pt(-3.0, 4.5), pt(-8.0, 0.0));
            if matches!(icon, Icon::EyeOff) {
                seg(pt(-6.5, -7.0), pt(6.5, 7.0));
            }
            layer.rects.push(ring(pt(0.0, 0.0), 1.7 * s, w, color));
        }
        Icon::Lively => {
            // A wavy stem: motion on.
            seg(pt(-7.0, 2.0), pt(-3.5, -2.0));
            seg(pt(-3.5, -2.0), pt(0.0, 2.0));
            seg(pt(0.0, 2.0), pt(3.5, -2.0));
            seg(pt(3.5, -2.0), pt(7.0, 2.0));
        }
        Icon::Reset => {
            // A bare sprout: back to the start.
            seg(pt(0.0, 7.5), pt(0.0, -1.0));
            seg(pt(0.0, 2.0), pt(-5.5, -3.5));
            seg(pt(0.0, -1.0), pt(5.5, -6.5));
        }
        Icon::Rename => {
            // A pencil.
            let points = [(-6.5, 6.5), (-5.5, 2.5), (3.5, -6.5), (6.5, -3.5), (-2.5, 5.5), (-6.5, 6.5)];
            for pair in points.windows(2) {
                seg(pt(pair[0].0, pair[0].1), pt(pair[1].0, pair[1].1));
            }
        }
        Icon::Copy => {
            for pair in [(-2.5, -3.0), (6.5, -3.0), (6.5, 7.0), (-2.5, 7.0), (-2.5, -3.0)].windows(2) {
                seg(pt(pair[0].0, pair[0].1), pt(pair[1].0, pair[1].1));
            }
            for pair in [(-6.5, 3.0), (-6.5, -7.0), (2.5, -7.0)].windows(2) {
                seg(pt(pair[0].0, pair[0].1), pt(pair[1].0, pair[1].1));
            }
        }
        Icon::Cut => {
            seg(pt(-2.6, 2.6), pt(5.0, -7.0));
            seg(pt(2.6, 2.6), pt(-5.0, -7.0));
            layer.rects.push(ring(pt(-4.2, 5.0), 2.6 * s, w, color));
            layer.rects.push(ring(pt(4.2, 5.0), 2.6 * s, w, color));
        }
        Icon::Trash => {
            seg(pt(-7.0, -4.0), pt(7.0, -4.0));
            for pair in [(-2.5, -4.0), (-2.5, -7.0), (2.5, -7.0), (2.5, -4.0)].windows(2) {
                seg(pt(pair[0].0, pair[0].1), pt(pair[1].0, pair[1].1));
            }
            for pair in [(-5.5, -4.0), (-4.5, 7.0), (4.5, 7.0), (5.5, -4.0)].windows(2) {
                seg(pt(pair[0].0, pair[0].1), pt(pair[1].0, pair[1].1));
            }
        }
        Icon::More => {
            for x in [-6.0, 0.0, 6.0] {
                layer.rects.push(line(pt(x, 0.0), pt(x, 0.0), 1.45 * s, color));
            }
        }
        Icon::NewFolder => {
            for pair in [(-7.5, 5.5), (-7.5, -5.5), (-2.5, -5.5), (-0.5, -3.0), (7.5, -3.0), (7.5, 5.5), (-7.5, 5.5)].windows(2) {
                seg(pt(pair[0].0, pair[0].1), pt(pair[1].0, pair[1].1));
            }
            seg(pt(0.0, -0.5), pt(0.0, 3.5));
            seg(pt(-2.0, 1.5), pt(2.0, 1.5));
        }
        Icon::Paste => {
            for pair in [(-2.5, -5.0), (-6.0, -5.0), (-6.0, 7.5), (6.0, 7.5), (6.0, -5.0), (2.5, -5.0)].windows(2) {
                seg(pt(pair[0].0, pair[0].1), pt(pair[1].0, pair[1].1));
            }
            for pair in [(-2.5, -3.5), (-2.5, -7.5), (2.5, -7.5), (2.5, -3.5), (-2.5, -3.5)].windows(2) {
                seg(pt(pair[0].0, pair[0].1), pt(pair[1].0, pair[1].1));
            }
        }
        Icon::Calm => {
            // A straight stem: reduced motion.
            seg(pt(-7.0, 0.0), pt(7.0, 0.0));
        }
    }
}

pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["KB", "MB", "GB", "TB", "PB"];
    if bytes == 1 {
        return "1 byte".to_string();
    }
    if bytes < 1024 {
        return format!("{bytes} bytes");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if value >= 100.0 { format!("{value:.0} {}", UNITS[unit]) } else { format!("{value:.1} {}", UNITS[unit]) }
}

/// Recent dates read as relative, older ones as a calendar date.
pub fn format_date(secs: i64, now: i64) -> String {
    if secs <= 0 {
        return "Unknown".to_string();
    }
    let ago = now - secs;
    if (0..60).contains(&ago) {
        return "Just now".to_string();
    }
    if (0..3600).contains(&ago) {
        let m = ago / 60;
        return format!("{m} min ago");
    }
    if (0..86_400).contains(&ago) {
        let h = ago / 3600;
        return if h == 1 { "1 hour ago".to_string() } else { format!("{h} hours ago") };
    }
    if (0..7 * 86_400).contains(&ago) {
        let d = ago / 86_400;
        return if d == 1 { "Yesterday".to_string() } else { format!("{d} days ago") };
    }
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!("{d} {} {y}", MONTHS[(m - 1) as usize])
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

const ROW_H: f32 = 32.0;

impl App {
    pub fn layer(&mut self, index: usize) -> &mut UiLayer {
        while self.draw.ui.len() <= index {
            self.draw.ui.push(UiLayer::default());
        }
        &mut self.draw.ui[index]
    }

    fn zone(&mut self, rect: [f32; 4], action: Action) {
        self.zones.push(Zone { rect, action });
    }

    pub fn build_ui(&mut self) {
        self.zones.clear();
        self.scroll_track = None;
        self.refresh_list();
        for i in 0..10 {
            self.layer(i);
        }
        self.build_status();
        self.build_marks();
        self.build_branch_handle();
        self.build_panel();
        self.settle_panel();
        self.build_toolbar();
        self.build_tooltip();
        self.build_overlays();
    }

    fn refresh_list(&mut self) {
        if !self.list_dirty {
            return;
        }
        self.list_dirty = false;
        let now = now_secs();
        let mut list = std::mem::take(&mut self.list);
        list.clear();
        if self.search_active && !self.search_text.is_empty() {
            let root = self.tree.root_path.clone();
            for hit in &self.search_hits {
                let name = hit.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let parent = hit.path.parent().map(|parent| match &root {
                    Some(r) => parent.strip_prefix(r).unwrap_or(parent).display().to_string(),
                    None => parent.display().to_string(),
                });
                list.push(ListItem {
                    node: NONE,
                    path: Some(hit.path.clone()),
                    species: if hit.is_dir { p::BUD } else { Species::from_name(&name).index() as f32 },
                    name: name.into_boxed_str(),
                    detail: parent.filter(|d| !d.is_empty()).unwrap_or_else(|| "Top of this tree".to_string()),
                    is_dir: hit.is_dir,
                    size: 0,
                    modified: 0,
                });
            }
        } else if self.tree.is_valid(self.selected) && !self.tree.get(self.selected).is_file() {
            let mut ids = Vec::new();
            self.tree.logical_children(self.selected, &mut ids);
            for id in ids {
                let n = self.tree.get(id);
                if n.dying {
                    continue;
                }
                let drive = self.is_drive(id);
                let detail = if drive {
                    self.drive_info.get(&*n.name).map(drive_space).unwrap_or_default()
                } else if n.is_dir() {
                    match self.sort {
                        SortBy::Modified => format_date(n.modified, now),
                        _ => String::new(),
                    }
                } else {
                    match self.sort {
                        SortBy::Modified => format_date(n.modified, now),
                        _ => format_size(n.size),
                    }
                };
                list.push(ListItem {
                    node: id,
                    path: None,
                    name: match self.drive_info.get(&*n.name).filter(|_| drive) {
                        Some(info) => info.display.clone().into_boxed_str(),
                        None => n.name.clone(),
                    },
                    detail,
                    is_dir: n.is_dir(),
                    species: if drive {
                        p::BLOSSOM
                    } else if n.is_dir() {
                        p::BUD
                    } else {
                        n.species().index() as f32
                    },
                    size: n.size,
                    modified: n.modified,
                });
            }
            let sort = self.sort;
            list.sort_by(|a, b| {
                b.is_dir.cmp(&a.is_dir).then_with(|| match sort {
                    SortBy::Name => crate::fs::natural_cmp(&a.name, &b.name),
                    SortBy::Modified => b.modified.cmp(&a.modified),
                    SortBy::Size => b.size.cmp(&a.size).then_with(|| crate::fs::natural_cmp(&a.name, &b.name)),
                })
            });
        }
        self.list = list;
    }

    // ------------------------------------------------------------ toolbar

    fn build_toolbar(&mut self) {
        let s = self.scale;
        let h = self.toolbar_height();
        let w = self.width;
        let layer = 4;
        let bar = rgb(p::BAR);
        let cream = rgb(p::CREAM);
        {
            let l = self.layer(layer);
            l.rects.push(soft_rect([-20.0, -20.0], [w + 20.0, h + 3.0 * s], 0.0, 6.0 * s, with_alpha(rgb(p::SHADOW), 0.55)));
            l.rects.push(cut_rect([-12.0, -12.0], [w + 12.0, h], 1.5 * s, bar));
        }
        self.zone([0.0, 0.0, w, h], Action::Nothing);

        let mut x = 10.0 * s;
        let size = 36.0 * s;
        let y = (h - size) * 0.5;
        let (can_back, can_forward) = (self.can_go_back(), self.can_go_forward());
        let can_up = self.tree.root_path.is_some() || self.selected != self.tree.root;
        for (icon, action, enabled) in
            [(Icon::Back, Action::Back, can_back), (Icon::Forward, Action::Forward, can_forward), (Icon::Up, Action::Up, can_up)]
        {
            let r = [x, y, x + size, y + size];
            let lit = if enabled { self.hov(action) } else { 0.0 };
            let color = with_alpha(cream, if enabled { 1.0 } else { 0.4 });
            let l = self.layer(layer);
            if lit > 0.0 {
                l.rects.push(rect([r[0], r[1]], [r[2], r[3]], 3.0 * s, with_alpha(rgb(p::BAR_HOVER), lit)));
            }
            draw_icon(l, icon, [x + size * 0.5, y + size * 0.5], s, color);
            if enabled {
                self.zone(r, action);
            }
            x += size + 2.0 * s;
        }

        // Search on the right.
        let search_w = (300.0 * s).min(w * 0.3);
        let search_x1 = w - 14.0 * s;
        let search_x0 = search_x1 - search_w;
        let search_h = 34.0 * s;
        let search_y = (h - search_h) * 0.5;
        let active = self.search_active;
        let ink = rgb(p::INK);
        {
            let l = self.layer(layer);
            l.rects.push(rect([search_x0, search_y], [search_x1, search_y + search_h], 3.0 * s, cream));
            if active {
                let focus = rgb(p::FOCUS_RING);
                l.rects.push(RectInst {
                    a: [search_x0 - 2.0 * s, search_y - 2.0 * s],
                    b: [search_x1 + 2.0 * s, search_y + search_h + 2.0 * s],
                    params: [4.0 * s, 0.0, 0.0, 0.0],
                    color: focus,
                });
                l.rects.push(rect([search_x0, search_y], [search_x1, search_y + search_h], 3.0 * s, cream));
            }
            draw_icon(l, Icon::Search, [search_x0 + 18.0 * s, search_y + search_h * 0.5], s * 0.86, ink);
        }
        self.zone([search_x0, search_y, search_x1, search_y + search_h], Action::SearchField);
        let text_x = search_x0 + 34.0 * s;
        let baseline = search_y + search_h * 0.5 + 5.0 * s;
        let text_size = 14.0 * s;
        let text_room = search_x1 - text_x - 32.0 * s;
        let query = self.search_text.clone();
        if query.is_empty() {
            let placeholder = rgb(p::INK_SOFT);
            let label = if active { "Type a name or a path" } else { "Search or enter a path" };
            let label = fit_label(&mut self.text, label, Face::Ui, text_size, text_room);
            let text = &mut self.text;
            let out = &mut self.draw.ui[layer].glyphs;
            draw_text(text, out, &label, Face::Ui, text_size, text_x, baseline, placeholder, NO_HALO);
            if active {
                self.draw.ui[layer].rects.push(rect(
                    [text_x - 1.0 * s, search_y + 8.0 * s],
                    [text_x + 0.5 * s, search_y + search_h - 8.0 * s],
                    0.0,
                    ink,
                ));
            }
        } else {
            // Keep the end of what's being typed in view.
            let chars: Vec<char> = query.chars().collect();
            let mut shown = query.clone();
            let mut start = 0;
            while self.text.width(&shown, Face::Ui, text_size) > text_room && start < chars.len() {
                start += 1;
                shown = chars[start..].iter().collect();
            }
            let text = &mut self.text;
            let out = &mut self.draw.ui[layer].glyphs;
            let width = draw_text(text, out, &shown, Face::Ui, text_size, text_x, baseline, ink, NO_HALO);
            if active {
                self.draw.ui[layer].rects.push(rect(
                    [text_x + width + 1.0 * s, search_y + 8.0 * s],
                    [text_x + width + 2.5 * s, search_y + search_h - 8.0 * s],
                    0.0,
                    ink,
                ));
            }
            let close = [search_x1 - 30.0 * s, search_y, search_x1, search_y + search_h];
            let l = self.layer(layer);
            draw_icon(l, Icon::Close, [close[0] + 15.0 * s, search_y + search_h * 0.5], s * 0.8, ink);
            self.zone(close, Action::ClearSearch);
        }

        // Breadcrumb in the space between. The last crumbs get priority when it's tight.
        let crumb_x0 = x + 10.0 * s;
        let crumb_x1 = search_x0 - 16.0 * s;
        let crumbs = self.crumbs();
        let size_text = 14.0 * s;
        let gap = 6.0 * s;
        let sep_w = self.text.width("/", Face::Ui, size_text);
        let widths: Vec<f32> = crumbs
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let face = if i + 1 == crumbs.len() { Face::UiBold } else { Face::Ui };
                self.text.width(&c.0, face, size_text)
            })
            .collect();
        let mut start = 0;
        let mut total: f32 = widths.iter().sum::<f32>() + (sep_w + gap * 2.0) * crumbs.len().saturating_sub(1) as f32;
        let ellipsis = self.text.width("…", Face::Ui, size_text) + sep_w + gap * 2.0;
        while total + if start > 0 { ellipsis } else { 0.0 } > crumb_x1 - crumb_x0 && start + 1 < crumbs.len() {
            total -= widths[start] + sep_w + gap * 2.0;
            start += 1;
        }
        let mut cx = crumb_x0;
        let baseline = h * 0.5 + 5.0 * s;
        let muted = mix_rgb(cream, bar, 0.28);
        if start > 0 {
            let text = &mut self.text;
            let out = &mut self.draw.ui[layer].glyphs;
            let mut adv = draw_text(text, out, "…", Face::Ui, size_text, cx, baseline, muted, NO_HALO);
            adv += gap;
            cx += adv;
            let text = &mut self.text;
            let out = &mut self.draw.ui[layer].glyphs;
            draw_text(text, out, "/", Face::Ui, size_text, cx, baseline, muted, NO_HALO);
            cx += sep_w + gap;
        }
        for (i, crumb) in crumbs.iter().enumerate().skip(start) {
            let last = i + 1 == crumbs.len();
            let face = if last { Face::UiBold } else { Face::Ui };
            let width = widths[i].min((crumb_x1 - cx).max(0.0));
            let shown = if widths[i] > width { fit_label(&mut self.text, &crumb.0, face, size_text, width) } else { crumb.0.clone() };
            let r = [cx - 4.0 * s, 8.0 * s, cx + width + 4.0 * s, h - 8.0 * s];
            let action = Action::Crumb(i);
            let lit = self.hov(action);
            let l = self.layer(layer);
            if lit > 0.0 {
                l.rects.push(rect([r[0], r[1]], [r[2], r[3]], 3.0 * s, with_alpha(rgb(p::BAR_HOVER), lit)));
            }
            let color = if last { cream } else { mix_rgb(cream, bar, 0.14) };
            let text = &mut self.text;
            let out = &mut self.draw.ui[layer].glyphs;
            draw_text(text, out, &shown, face, size_text, cx, baseline, color, NO_HALO);
            self.zone(r, action);
            cx += width + gap;
            if !last {
                let text = &mut self.text;
                let out = &mut self.draw.ui[layer].glyphs;
                draw_text(text, out, "/", Face::Ui, size_text, cx, baseline, muted, NO_HALO);
                cx += sep_w + gap;
            }
        }
    }

    // ------------------------------------------------------------ status line and tree controls

    fn build_status(&mut self) {
        let s = self.scale;
        let layer = 0;
        let stage = self.stage();
        let cream = rgb(p::CREAM);

        let note = self.status_note.as_ref().map(|n| n.0.clone());
        let line = match note {
            Some(n) => n,
            None => self.status_line(),
        };
        let room = stage[2] - 190.0 * s;
        // In a tight window the folder's contents matter more than the reminder.
        let line = match line.strip_suffix(" · watching for changes") {
            Some(short) if self.text.width(&line, Face::Ui, 12.5 * s) > room => short.to_string(),
            _ => line,
        };
        let line = fit_label(&mut self.text, &line, Face::Ui, 12.5 * s, room);
        let width = self.text.width(&line, Face::Ui, 12.5 * s);
        let tab = [8.0 * s, self.height - 36.0 * s, 8.0 * s + width + 20.0 * s, self.height - 6.0 * s];
        {
            let l = self.layer(layer);
            l.rects.push(soft_rect([tab[0] + 1.0 * s, tab[1] + 3.0 * s], [tab[2] + 1.0 * s, tab[3] + 3.0 * s], 3.0 * s, 3.0 * s, with_alpha(rgb(p::SHADOW), 0.35)));
            l.rects.push(rect([tab[0], tab[1]], [tab[2], tab[3]], 3.0 * s, with_alpha(rgb(p::BAR), 1.0)));
        }
        self.zone(tab, Action::Nothing);
        let text = &mut self.text;
        let out = &mut self.draw.ui[layer].glyphs;
        let baseline = tab[1] + (tab[3] - tab[1]) * 0.5 + 4.4 * s;
        draw_text(text, out, &line, Face::Ui, 12.5 * s, 18.0 * s, baseline, cream, NO_HALO);

        // Small tree controls at the far end of the status line.
        let size = 30.0 * s;
        let y = self.height - size - 6.0 * s;
        let mut x = stage[2] - size - 10.0 * s;
        let show_hidden = self.tree.show_hidden;
        let calm = self.reduced_motion;
        for (icon, action) in [
            (Icon::Fit, Action::Fit),
            (Icon::Reset, Action::Reset),
            (if show_hidden { Icon::Eye } else { Icon::EyeOff }, Action::ToggleHidden),
            (if calm { Icon::Calm } else { Icon::Lively }, Action::ToggleMotion),
        ] {
            let r = [x, y, x + size, y + size];
            let lit = self.hov(action);
            let l = self.layer(layer);
            l.rects.push(rect(
                [r[0], r[1]],
                [r[2], r[3]],
                3.0 * s,
                with_alpha(mix_rgb(rgb(p::BAR), rgb(p::BAR_HOVER), lit), 0.85 + lit * 0.15),
            ));
            draw_icon(l, icon, [x + size * 0.5, y + size * 0.5], s * 0.82, cream);
            self.zone(r, action);
            x -= size + 4.0 * s;
        }
    }

    /// Resting on an open branch shows a small handle on the limb that closes it.
    fn build_branch_handle(&mut self) {
        let target = match self.hover_action {
            Action::CollapseBranch(id) => id,
            _ => self.hover,
        };
        if !self.tree.is_valid(target) || target == self.tree.root {
            return;
        }
        let node = self.tree.get(target);
        if !node.is_dir() || !node.expanded || node.children.is_empty() {
            return;
        }
        let s = self.scale;
        let z = self.camera.zoom();
        let (tip, base, heading) = (node.tip, node.base, node.heading);
        let (sh, ch) = heading.sin_cos();
        let limb = (((tip[0] - base[0]).powi(2) + (tip[1] - base[1]).powi(2)).sqrt()) * z;
        let back = (limb * 0.5).min(26.0 * s);
        let x = self.camera.ox.value + tip[0] * z - sh * back;
        let y = self.camera.oy.value - tip[1] * z + ch * back;
        let stage = self.stage();
        if x < stage[0] || x > stage[2] || y < stage[1] || y > stage[3] {
            return;
        }
        let action = Action::CollapseBranch(target);
        let lit = self.hov(action);
        // The handle grows out of the limb when the pointer arrives.
        let dt = self.acts.ui_dt;
        let appeared = if self.acts.handle_for == target {
            let next = if self.reduced_motion { 1.0 } else { (self.acts.handle_in + dt * 9.0).min(1.0) };
            if next < 1.0 {
                self.redraw = true;
            }
            next
        } else {
            self.acts.handle_for = target;
            self.redraw = true;
            0.0
        };
        self.acts.handle_in = appeared;
        let back_out = 1.0 + 1.7 * (appeared - 1.0).powi(3) + 0.7 * (appeared - 1.0).powi(2);
        let r = 11.0 * s * back_out.max(0.01);
        let l = self.layer(0);
        l.rects.push(soft_rect([x - r + 1.0 * s, y - r + 3.0 * s], [x + r + 1.0 * s, y + r + 3.0 * s], r, 3.0 * s, with_alpha(rgb(p::SHADOW), 0.45)));
        l.rects.push(rect([x - r, y - r], [x + r, y + r], r, mix_rgb(rgb(p::CREAM), rgb(p::PANEL_HOVER), lit)));
        l.rects.push(line([x - r * 0.45, y], [x + r * 0.45, y], 1.0 * s, rgb(p::INK)));
        let reach = 14.0 * s;
        self.zone([x - reach, y - reach, x + reach, y + reach], action);
    }

    fn status_line(&self) -> String {
        let id = if self.tree.is_valid(self.selected) { self.selected } else { self.tree.root };
        let dir = if self.tree.get(id).is_file() { self.tree.logical_parent(id) } else { id };
        if dir == NONE {
            return String::new();
        }
        let n = self.tree.get(dir);
        match n.load {
            Load::Loading => format!("Reading {}…", n.name),
            Load::Failed => "This branch couldn't be read".to_string(),
            _ => {
                let (folders, files) = (n.dir_count, n.file_count);
                if n.kind == Kind::Places {
                    return format!("{}: {}", n.name, plural(folders, "disk", "disks"));
                }
                let name = self
                    .drive_info
                    .get(&*n.name)
                    .filter(|_| self.is_drive(dir))
                    .map_or(n.name.to_string(), |d| d.display.clone());
                format!("{name}: {}, {} · watching for changes", plural(folders, "folder", "folders"), plural(files, "file", "files"))
            }
        }
    }

    // ------------------------------------------------------------ panel

    fn build_panel(&mut self) {
        let s = self.scale;
        let layer = 1;
        let x1 = self.width;
        let x0 = x1 - self.panel_width();
        let y0 = self.toolbar_height();
        let y1 = self.height;
        {
            let l = self.layer(layer);
            l.rects.push(soft_rect([x0 - 1.0 * s, y0 - 20.0], [x1 + 20.0, y1 + 20.0], 0.0, 7.0 * s, with_alpha(rgb(p::SHADOW), 0.5)));
            l.rects.push(cut_rect([x0, y0 - 12.0], [x1 + 12.0, y1 + 12.0], 1.5 * s, rgb(p::CREAM)));
        }
        self.zone([x0, y0, x1, y1], Action::PanelScroll);

        if self.search_active && !self.search_text.is_empty() && !self.looks_like_path() {
            self.panel_search(x0, y0, x1, y1);
            return;
        }
        if self.search_active && self.looks_like_path() {
            self.panel_message(x0, y0, x1, "Go to this path", "Press Enter to grow a branch to it.");
            return;
        }
        if self.acts.bin.is_some() {
            self.panel_bin(x0, y0, x1, y1);
            return;
        }
        let id = self.selected;
        if !self.tree.is_valid(id) {
            self.panel_message(x0, y0, x1, "Nothing selected", "Click a flower or a branch to see it here.");
            return;
        }
        if self.acts.marked.len() > 1 && self.tree.get(id).is_file() {
            self.panel_many(x0, y0, x1, y1);
        } else if self.tree.get(id).is_file() {
            self.panel_file(x0, y0, x1, y1);
        } else {
            self.panel_folder(x0, y0, x1, y1);
        }
    }

    fn panel_message(&mut self, x0: f32, y0: f32, x1: f32, title: &str, body: &str) {
        let s = self.scale;
        let pad = 20.0 * s;
        let ink = rgb(p::INK);
        let mut y = y0 + 34.0 * s;
        let title = fit_label(&mut self.text, title, Face::Display, 28.0 * s, x1 - x0 - pad * 2.0);
        let out = &mut self.draw.ui[1].glyphs;
        draw_text(&mut self.text, out, &title, Face::Display, 28.0 * s, x0 + pad, y + 18.0 * s, ink, NO_HALO);
        y += 40.0 * s;
        for line in wrap(&mut self.text, body, Face::Ui, 14.0 * s, x1 - x0 - pad * 2.0) {
            let out = &mut self.draw.ui[1].glyphs;
            draw_text(&mut self.text, out, &line, Face::Ui, 14.0 * s, x0 + pad, y + 10.0 * s, rgb(p::INK_SOFT), NO_HALO);
            y += 20.0 * s;
        }
    }

    fn panel_title(&mut self, x0: f32, x1: f32, mut y: f32, title: &str) -> f32 {
        let s = self.scale;
        let pad = 20.0 * s;
        let ink = rgb(p::INK);
        let room = x1 - x0 - pad * 2.0;
        // Titles shrink a step before they wrap, and wrap before they truncate.
        let mut size = 30.0 * s;
        if self.text.width(title, Face::Display, size) > room {
            size = 24.0 * s;
        }
        let lines = wrap(&mut self.text, title, Face::Display, size, room);
        let line_h = size * 1.08;
        let shown = lines.len().min(3);
        for (i, line) in lines.iter().take(3).enumerate() {
            let line = if i == 2 && lines.len() > 3 {
                fit_label(&mut self.text, &format!("{line}{}", lines[3]), Face::Display, size, room)
            } else {
                line.clone()
            };
            let out = &mut self.draw.ui[1].glyphs;
            draw_text(&mut self.text, out, &line, Face::Display, size, x0 + pad, y + size * 0.82, ink, NO_HALO);
            y += line_h;
        }
        let _ = shown;
        y + 2.0 * s
    }

    fn panel_button(&mut self, x: f32, y: f32, label: &str, action: Action, primary: bool) -> f32 {
        let s = self.scale;
        let h = 36.0 * s;
        let size = 13.5 * s;
        let width = self.text.width(label, Face::UiBold, size) + 28.0 * s;
        let r = [x, y, x + width, y + h];
        let lit = self.hov(action);
        let ink = rgb(p::INK);
        let l = &mut self.draw.ui[1];
        if primary {
            l.rects.push(rect([r[0], r[1]], [r[2], r[3]], 3.0 * s, mix_rgb(ink, rgb(p::BAR_HOVER), lit)));
        } else {
            let fill = mix_rgb(rgb(p::CREAM), rgb(p::PANEL_HOVER), lit);
            l.rects.push(rect([r[0], r[1]], [r[2], r[3]], 3.0 * s, ink));
            let t = 1.4 * s;
            l.rects.push(rect([r[0] + t, r[1] + t], [r[2] - t, r[3] - t], 2.0 * s, fill));
        }
        let color = if primary { rgb(p::CREAM) } else { ink };
        let out = &mut self.draw.ui[1].glyphs;
        draw_text(&mut self.text, out, label, Face::UiBold, size, x + 14.0 * s, y + h * 0.5 + size * 0.36, color, NO_HALO);
        self.zone(r, action);
        width
    }

    /// A row of small square buttons, each drawn in the panel's own ink.
    fn icon_buttons(&mut self, mut x: f32, y: f32, buttons: &[(Icon, crate::actions::Cmd)]) {
        let s = self.scale;
        let size = 34.0 * s;
        let top = y + 1.0 * s;
        for (icon, cmd) in buttons {
            let r = [x, top, x + size, top + size];
            let action = Action::Cmd(*cmd);
            let lit = self.hov(action);
            let danger = *cmd == crate::actions::Cmd::Delete;
            let wash = if danger { rgb(p::DANGER) } else { rgb(p::PANEL_HOVER) };
            let idle = rgb(p::INK);
            let ink = if danger { mix_rgb(idle, rgb(p::CREAM), lit) } else { idle };
            let l = &mut self.draw.ui[1];
            if lit > 0.0 {
                l.rects.push(rect([r[0], r[1]], [r[2], r[3]], 3.0 * s, with_alpha(wash, lit)));
            }
            draw_icon(l, *icon, [x + size * 0.5, top + size * 0.5], s * 0.9, ink);
            self.zone(r, action);
            x += size + 2.0 * s;
        }
    }

    /// One line of facts: a quiet label and its value.
    fn fact(&mut self, x: f32, x1: f32, y: f32, label: &str, value: &str) {
        let s = self.scale;
        let out = &mut self.draw.ui[1].glyphs;
        draw_text(&mut self.text, out, label, Face::Ui, 13.0 * s, x, y + 10.0 * s, rgb(p::INK_SOFT), NO_HALO);
        let vx = x + 82.0 * s;
        let shown = fit_label(&mut self.text, value, Face::Ui, 13.0 * s, x1 - vx);
        let out = &mut self.draw.ui[1].glyphs;
        draw_text(&mut self.text, out, &shown, Face::Ui, 13.0 * s, vx, y + 10.0 * s, rgb(p::INK), NO_HALO);
    }

    fn panel_file(&mut self, x0: f32, y0: f32, x1: f32, y1: f32) {
        use crate::actions::Cmd;
        let s = self.scale;
        let pad = 20.0 * s;
        let id = self.selected;
        let (name, size, modified, species) = {
            let n = self.tree.get(id);
            (n.name.to_string(), n.size, n.modified, n.species())
        };
        let parent = self.tree.logical_parent(id);
        let parent_name = if parent != NONE { self.tree.get(parent).name.to_string() } else { String::new() };
        let location = if parent != NONE { self.tree.path_of(parent) } else { None };
        let ink = rgb(p::INK);
        let soft = rgb(p::INK_SOFT);
        let mut y = y0 + 18.0 * s;
        let (px0, px1) = (x0 + pad, x1 - pad);

        enum Shown {
            Picture(f32, f32),
            Words(Vec<String>, bool),
            Archive(Vec<(String, u64)>, usize),
            Sound,
            Flower(Option<String>),
        }
        let shown = match self.preview.as_ref().map(|l| &l.preview) {
            Some(Preview::Image { width, height, .. }) => Shown::Picture(*width as f32, *height as f32),
            Some(Preview::Text { lines, truncated }) => Shown::Words(lines.clone(), *truncated),
            Some(Preview::Listing { entries, total }) => Shown::Archive(entries.iter().take(7).cloned().collect(), *total),
            Some(Preview::Audio) => Shown::Sound,
            Some(Preview::Unavailable { reason }) => Shown::Flower(Some(reason.clone())),
            _ => Shown::Flower(None),
        };

        // The picture of the file comes first, unless it's words, which get the rest of the panel.
        let card = 210.0 * s;
        let card_rect = [px0, y, px1, y + card];
        let mid = [(px0 + px1) * 0.5, y + card * 0.5];
        let flower = |app: &mut App, center: [f32; 2], radius: f32| {
            let node = app.tree.get(id);
            let (bloom, seed, rotation) = (
                node.bloom.value.clamp(0.0, 1.0),
                crate::tree::rand01(node.seed, 8) * 40.0,
                crate::tree::rand01(node.seed, 7) * 6.283,
            );
            app.draw.ui[2].flowers.push(FlowerInst {
                pos: center,
                radius,
                rotation,
                bloom,
                species: species.index() as f32,
                seed,
                lift: 0.8,
                dim: 0.0,
            });
        };
        match &shown {
            Shown::Words(..) => {}
            Shown::Picture(iw, ih) => {
                let fit = ((px1 - px0) / iw).min(card / ih);
                let (w2, h2) = (iw * fit, ih * fit);
                self.draw.ui[1].rects.push(rect([card_rect[0], card_rect[1]], [card_rect[2], card_rect[3]], 3.0 * s, rgb(p::PREVIEW_BG)));
                self.draw.ui[2].image_rect = Some(([mid[0] - w2 * 0.5, mid[1] - h2 * 0.5, mid[0] + w2 * 0.5, mid[1] + h2 * 0.5], 3.0 * s));
            }
            Shown::Archive(entries, total) => {
                self.draw.ui[1].rects.push(rect([card_rect[0], card_rect[1]], [card_rect[2], card_rect[3]], 3.0 * s, rgb(p::PREVIEW_BG)));
                let mut ly = y + 24.0 * s;
                let inner = px1 - px0 - 24.0 * s;
                for (entry, bytes) in entries {
                    let size_text = format_size(*bytes);
                    let sw = self.text.width(&size_text, Face::Ui, 11.5 * s);
                    let shown = fit_label(&mut self.text, entry, Face::Ui, 12.5 * s, inner - sw - 8.0 * s);
                    let out = &mut self.draw.ui[1].glyphs;
                    draw_text(&mut self.text, out, &shown, Face::Ui, 12.5 * s, px0 + 12.0 * s, ly, ink, NO_HALO);
                    let out = &mut self.draw.ui[1].glyphs;
                    draw_text(&mut self.text, out, &size_text, Face::Ui, 11.5 * s, px1 - 12.0 * s - sw, ly, soft, NO_HALO);
                    ly += 22.0 * s;
                }
                if *total > entries.len() {
                    let more = format!("and {} more", total - entries.len());
                    let out = &mut self.draw.ui[1].glyphs;
                    draw_text(&mut self.text, out, &more, Face::Ui, 12.5 * s, px0 + 12.0 * s, ly + 2.0 * s, soft, NO_HALO);
                }
            }
            Shown::Sound => {
                self.draw.ui[1].rects.push(rect([card_rect[0], card_rect[1]], [card_rect[2], card_rect[3]], 3.0 * s, rgb(p::GROUND)));
                flower(self, [mid[0], y + 76.0 * s], 56.0 * s);
            }
            Shown::Flower(_) => {
                self.draw.ui[1].rects.push(rect([card_rect[0], card_rect[1]], [card_rect[2], card_rect[3]], 3.0 * s, rgb(p::GROUND)));
                flower(self, mid, 74.0 * s);
            }
        }
        if !matches!(shown, Shown::Words(..)) {
            self.zone(card_rect, Action::Cmd(Cmd::QuickLook));
            if matches!(shown, Shown::Sound) {
                // The player sits on top of the card, so its controls take the clicks.
                self.draw_audio_player(1, px0 + 14.0 * s, y + card - 58.0 * s, px1 - px0 - 28.0 * s, rgb(p::CREAM), with_alpha(rgb(p::CREAM), 0.28));
            }
            y += card + 16.0 * s;
        }

        y = self.panel_title(x0, x1, y, &name);
        let kind = describe_kind(&name, species);
        let sub = if parent_name.is_empty() { kind } else { format!("{kind} in {parent_name}") };
        let sub = fit_label(&mut self.text, &sub, Face::Ui, 13.0 * s, px1 - px0);
        let out = &mut self.draw.ui[1].glyphs;
        draw_text(&mut self.text, out, &sub, Face::Ui, 13.0 * s, px0, y + 11.0 * s, soft, NO_HALO);
        y += 30.0 * s;

        let advance = self.panel_button(px0, y, "Open", Action::Cmd(Cmd::Open), true);
        self.icon_buttons(
            px0 + advance + 8.0 * s,
            y,
            &[
                (Icon::Rename, Cmd::Rename),
                (Icon::Copy, Cmd::Copy),
                (Icon::Cut, Cmd::Cut),
                (Icon::Trash, Cmd::Delete),
                (Icon::More, Cmd::More),
            ],
        );
        y += 50.0 * s;

        let now = now_secs();
        let mut rows = vec![("Size", format_size(size))];
        if self.image_size.0 > 0 {
            rows.push(("Dimensions", format!("{} × {}", self.image_size.0, self.image_size.1)));
        }
        rows.push(("Modified", format_date(modified, now)));
        if let Some((_, created, _)) = &self.acts.facts {
            rows.push(("Created", format_date(*created, now)));
        }
        if let Some(location) = &location {
            rows.push(("Location", location.display().to_string()));
        }
        for (label, value) in rows {
            self.fact(px0, px1, y, label, &value);
            y += 21.0 * s;
        }
        y += 12.0 * s;

        match shown {
            Shown::Words(lines, truncated) => {
                self.panel_text_preview(px0, y, px1, y1 - 18.0 * s, &lines, truncated, species);
            }
            Shown::Flower(Some(reason)) => {
                for line in wrap(&mut self.text, &reason, Face::Ui, 13.0 * s, px1 - px0) {
                    let out = &mut self.draw.ui[1].glyphs;
                    draw_text(&mut self.text, out, &line, Face::Ui, 13.0 * s, px0, y + 10.0 * s, soft, NO_HALO);
                    y += 18.0 * s;
                }
            }
            _ => {}
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn panel_text_preview(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, lines: &[String], truncated: bool, species: Species) {
        let s = self.scale;
        if y1 - y0 < 40.0 * s {
            return;
        }
        let size = 12.0 * s;
        let line_h = 17.0 * s;
        let needed = lines.len().max(1) as f32 * line_h + if truncated { line_h * 1.4 } else { 0.0 } + 18.0 * s;
        let y1 = y1.min(y0 + needed);
        self.draw.ui[1].rects.push(rect([x0, y0], [x1, y1], 3.0 * s, rgb(p::PREVIEW_BG)));
        self.zone([x0, y0, x1, y1], Action::Cmd(crate::actions::Cmd::QuickLook));
        if lines.is_empty() {
            let out = &mut self.draw.ui[1].glyphs;
            draw_text(&mut self.text, out, "This file is empty.", Face::Ui, 13.0 * s, x0 + 10.0 * s, y0 + 22.0 * s, rgb(p::INK_SOFT), NO_HALO);
            return;
        }
        let content = lines.len() as f32 * line_h + if truncated { line_h * 1.4 } else { 0.0 };
        let view = y1 - y0 - 16.0 * s;
        let max_scroll = (content - view).max(0.0);
        // The panel's scroll drives the preview while a file is showing.
        self.panel_scroll.target = self.panel_scroll.target.min(max_scroll);
        let scroll = self.panel_scroll.value.min(max_scroll).max(0.0);
        let first = (scroll / line_h) as usize;
        let count = (view / line_h) as usize + 2;
        let l = &mut self.draw.ui[3];
        l.clip = Some([x0, y0 + 4.0 * s, x1, y1 - 4.0 * s]);
        let room = x1 - x0 - 20.0 * s;
        let code = is_code(species);
        for (i, text) in lines.iter().enumerate().skip(first).take(count) {
            let y = y0 + 8.0 * s + i as f32 * line_h - scroll + size;
            let shown = fit_end(&mut self.text, text, Face::Mono, size, room);
            draw_code(&mut self.text, &mut self.draw.ui[3].glyphs, &shown, size, x0 + 10.0 * s, y, code);
        }
        self.scrollbar(x1 - 9.0 * s, y0 + 4.0 * s, x1 - 3.0 * s, y1 - 4.0 * s, content, scroll, max_scroll);
        if truncated {
            let y = y0 + 8.0 * s + lines.len() as f32 * line_h - scroll + size;
            let out = &mut self.draw.ui[3].glyphs;
            draw_text(&mut self.text, out, "Showing the start of this file.", Face::Ui, 12.0 * s, x0 + 10.0 * s, y + 4.0 * s, rgb(p::INK_SOFT), NO_HALO);
        }
    }

    /// Several things selected at once: what they add up to, and what can be done to them.
    fn panel_many(&mut self, x0: f32, y0: f32, x1: f32, y1: f32) {
        use crate::actions::Cmd;
        let s = self.scale;
        let pad = 20.0 * s;
        let mut y = y0 + 20.0 * s;
        let marked = self.acts.marked.clone();
        let (mut folders, mut files, mut bytes) = (0u32, 0u32, 0u64);
        for id in &marked {
            let n = self.tree.get(*id);
            if n.is_file() {
                files += 1;
                bytes += n.size;
            } else {
                folders += 1;
            }
        }
        y = self.panel_title(x0, x1, y, &format!("{} items", marked.len()));
        let sub = match (folders, files) {
            (0, f) => format!("{}, {}", plural(f, "file", "files"), format_size(bytes)),
            (d, 0) => plural(d, "folder", "folders"),
            (d, f) => format!("{}, {}, {}", plural(f, "file", "files"), plural(d, "folder", "folders"), format_size(bytes)),
        };
        let out = &mut self.draw.ui[1].glyphs;
        draw_text(&mut self.text, out, &sub, Face::Ui, 13.0 * s, x0 + pad, y + 11.0 * s, rgb(p::INK_SOFT), NO_HALO);
        y += 30.0 * s;
        self.many_actions(x0 + pad, y);
        y += 50.0 * s;
        for id in &marked {
            if y > y1 - 40.0 * s {
                let more = "and more";
                let out = &mut self.draw.ui[1].glyphs;
                draw_text(&mut self.text, out, more, Face::Ui, 13.0 * s, x0 + pad, y + 10.0 * s, rgb(p::INK_SOFT), NO_HALO);
                break;
            }
            let name = self.tree.get(*id).name.to_string();
            let shown = fit_label(&mut self.text, &name, Face::Ui, 13.5 * s, x1 - x0 - pad * 2.0);
            let out = &mut self.draw.ui[1].glyphs;
            draw_text(&mut self.text, out, &shown, Face::Ui, 13.5 * s, x0 + pad, y + 12.0 * s, rgb(p::INK), NO_HALO);
            y += 25.0 * s;
        }
        let _ = Cmd::Copy;
    }

    fn many_actions(&mut self, x: f32, y: f32) {
        use crate::actions::Cmd;
        let s = self.scale;
        let mut bx = x;
        bx += self.panel_button(bx, y, "Copy", Action::Cmd(Cmd::Copy), true) + 6.0 * s;
        bx += self.panel_button(bx, y, "Cut", Action::Cmd(Cmd::Cut), false) + 8.0 * s;
        self.icon_buttons(bx, y, &[(Icon::Trash, Cmd::Delete), (Icon::More, Cmd::More)]);
    }

    fn panel_folder(&mut self, x0: f32, y0: f32, x1: f32, y1: f32) {
        use crate::actions::Cmd;
        let s = self.scale;
        let pad = 20.0 * s;
        let id = self.selected;
        let (name, folders, files, load, error, kind) = {
            let n = self.tree.get(id);
            (n.name.to_string(), n.dir_count, n.file_count, n.load, n.error.clone(), n.kind)
        };
        let soft = rgb(p::INK_SOFT);
        let mut y = y0 + 20.0 * s;
        let is_drive = self.is_drive(id);
        let drive = is_drive.then(|| self.drive_info.get(&name).cloned()).flatten();
        let title = drive.as_ref().map_or(name.clone(), |d| d.display.clone());
        y = self.panel_title(x0, x1, y, &title);
        let sub = match load {
            _ if drive.is_some() && matches!(load, Load::Unloaded | Load::Loading) => {
                drive.as_ref().map(drive_space).unwrap_or_default()
            }
            Load::Loaded if kind == Kind::Places => plural(folders, "disk", "disks"),
            Load::Loading => "Reading…".to_string(),
            Load::Failed => "Couldn't read this folder".to_string(),
            Load::Unloaded => "Closed branch".to_string(),
            Load::Loaded => format!("{}, {}", plural(folders, "folder", "folders"), plural(files, "file", "files")),
        };
        let out = &mut self.draw.ui[1].glyphs;
        draw_text(&mut self.text, out, &sub, Face::Ui, 13.0 * s, x0 + pad, y + 11.0 * s, soft, NO_HALO);
        y += 22.0 * s;
        // How much is in here, worked out in the background.
        if self.counts_totals(id) {
            let line = match &self.acts.totals {
                Some((_, bytes, count, _)) => format!("{} across {} in all", format_size(*bytes), plural(*count as u32, "file", "files")),
                None => "Adding up what's inside…".to_string(),
            };
            let line = fit_label(&mut self.text, &line, Face::Ui, 13.0 * s, x1 - x0 - pad * 2.0);
            let out = &mut self.draw.ui[1].glyphs;
            draw_text(&mut self.text, out, &line, Face::Ui, 13.0 * s, x0 + pad, y + 11.0 * s, soft, NO_HALO);
            y += 22.0 * s;
        }
        y += 8.0 * s;

        if kind == Kind::Places {
            self.quick_access(x0, x1, &mut y);
        } else if self.acts.marked.is_empty() {
            // Line-work icons have air around them, so a row that starts with one sits a little left.
            let mut bx = x0 + pad - 8.0 * s;
            if id != self.tree.root {
                let label = if self.tree.get(id).expanded { "Collapse" } else { "Open branch" };
                bx = x0 + pad + self.panel_button(x0 + pad, y, label, Action::ToggleSelected, false) + 8.0 * s;
            }
            let buttons: &[(Icon, Cmd)] = if is_drive || id == self.tree.root {
                &[(Icon::NewFolder, Cmd::NewFolder), (Icon::Paste, Cmd::Paste), (Icon::More, Cmd::More)]
            } else {
                &[(Icon::NewFolder, Cmd::NewFolder), (Icon::Copy, Cmd::Copy), (Icon::Paste, Cmd::Paste), (Icon::Trash, Cmd::Delete), (Icon::More, Cmd::More)]
            };
            self.icon_buttons(bx, y, buttons);
            y += 50.0 * s;
        } else {
            // Rows are being picked out of this list, so the list has to stay in view.
            let count = self.acts.marked.len();
            let label = if count == 1 { "1 selected".to_string() } else { format!("{count} selected") };
            let out = &mut self.draw.ui[1].glyphs;
            draw_text(&mut self.text, out, &label, Face::UiBold, 13.5 * s, x0 + pad, y + 13.0 * s, rgb(p::INK), NO_HALO);
            y += 26.0 * s;
            self.many_actions(x0 + pad, y);
            y += 50.0 * s;
        }

        if let Some(err) = error {
            for line in wrap(&mut self.text, &err, Face::Ui, 14.0 * s, x1 - x0 - pad * 2.0) {
                let out = &mut self.draw.ui[1].glyphs;
                draw_text(&mut self.text, out, &line, Face::Ui, 14.0 * s, x0 + pad, y + 12.0 * s, rgb(p::INK), NO_HALO);
                y += 20.0 * s;
            }
            return;
        }
        if load == Load::Loaded && self.list.is_empty() {
            let hidden_only = folders + files > 0;
            let message = if hidden_only {
                "Everything in here is hidden. Use the eye button below the tree to show it."
            } else {
                "This folder is empty. Press Ctrl+Shift+N to make a folder here, or drop files in."
            };
            for line in wrap(&mut self.text, message, Face::Ui, 14.0 * s, x1 - x0 - pad * 2.0) {
                let out = &mut self.draw.ui[1].glyphs;
                draw_text(&mut self.text, out, &line, Face::Ui, 14.0 * s, x0 + pad, y + 12.0 * s, rgb(p::INK), NO_HALO);
                y += 20.0 * s;
            }
            return;
        }
        if self.list.is_empty() {
            return;
        }

        // Sort tabs. Disks are simply listed in order.
        let tabs = [("Name", SortBy::Name), ("Modified", SortBy::Modified), ("Size", SortBy::Size)];
        let mut tx = x0 + pad;
        let tab_h = 28.0 * s;
        if kind == Kind::Places {
            self.panel_rows(x0, y, x1, y1);
            return;
        }
        for (label, by) in tabs {
            let on = self.sort == by;
            let face = if on { Face::UiBold } else { Face::Ui };
            let width = self.text.width(label, face, 13.0 * s) + 16.0 * s;
            let r = [tx, y, tx + width, y + tab_h];
            let action = Action::Sort(by);
            let lit = self.hov(action);
            let l = &mut self.draw.ui[1];
            if on {
                l.rects.push(rect([r[0], r[1]], [r[2], r[3]], 3.0 * s, rgb(p::INK)));
            } else if lit > 0.0 {
                l.rects.push(rect([r[0], r[1]], [r[2], r[3]], 3.0 * s, with_alpha(rgb(p::PANEL_HOVER), lit)));
            }
            let color = if on { rgb(p::CREAM) } else { rgb(p::INK) };
            let out = &mut self.draw.ui[1].glyphs;
            draw_text(&mut self.text, out, label, face, 13.0 * s, tx + 8.0 * s, y + tab_h * 0.5 + 4.6 * s, color, NO_HALO);
            self.zone(r, action);
            tx += width + 3.0 * s;
        }
        y += tab_h + 8.0 * s;
        self.panel_rows(x0, y, x1, y1);
    }

    /// A paper ring around each thing that's picked out, so several can be told apart at once.
    fn build_marks(&mut self) {
        if self.acts.marked.is_empty() {
            return;
        }
        let s = self.scale;
        let marked: rustc_hash::FxHashSet<_> = self.acts.marked.iter().copied().collect();
        let stage = self.stage();
        let mut rings = Vec::new();
        for v in &self.visible {
            if v.rank <= 1 && marked.contains(&v.id) && inside(stage, v.x, v.y) {
                rings.push(ring([v.x, v.y], v.r * 1.02 + 1.0 * s, 1.3 * s, rgb(p::PATH)));
            }
        }
        self.draw.ui[0].rects.extend(rings);
    }

    /// What's in the Recycle Bin, and a way to bring each thing back.
    fn panel_bin(&mut self, x0: f32, y0: f32, x1: f32, y1: f32) {
        let s = self.scale;
        let pad = 20.0 * s;
        let mut y = y0 + 20.0 * s;
        y = self.panel_title(x0, x1, y, "Recycle Bin");
        let items = self.acts.bin.clone().unwrap_or_default();
        let sub = if items.is_empty() { "Nothing in here".to_string() } else { plural(items.len() as u32, "item", "items") };
        let out = &mut self.draw.ui[1].glyphs;
        draw_text(&mut self.text, out, &sub, Face::Ui, 13.0 * s, x0 + pad, y + 11.0 * s, rgb(p::INK_SOFT), NO_HALO);
        y += 30.0 * s;
        self.panel_button(x0 + pad, y, "Back", Action::BinClose, false);
        y += 50.0 * s;
        if items.is_empty() {
            return;
        }
        let row_h = 52.0 * s;
        let view = (y1 - y).max(0.0);
        let content = items.len() as f32 * row_h + 8.0 * s;
        let max_scroll = (content - view).max(0.0);
        self.panel_scroll.target = self.panel_scroll.target.clamp(0.0, max_scroll);
        let scroll = self.panel_scroll.value.clamp(0.0, max_scroll);
        let first = (scroll / row_h) as usize;
        let count = (view / row_h) as usize + 2;
        self.draw.ui[3].clip = Some([x0, y, x1, y1]);
        let now = now_secs();
        let restore_w = self.text.width("Restore", Face::UiBold, 12.5 * s) + 20.0 * s;
        for (i, item) in items.iter().enumerate().skip(first).take(count) {
            let top = y + i as f32 * row_h - scroll;
            let (tx, tr) = (x0 + pad, x1 - pad - restore_w - 8.0 * s);
            let name = fit_label(&mut self.text, &item.name, Face::Ui, 13.5 * s, tr - tx);
            let out = &mut self.draw.ui[3].glyphs;
            draw_text(&mut self.text, out, &name, Face::Ui, 13.5 * s, tx, top + 20.0 * s, rgb(p::INK), NO_HALO);
            let from = item.was_at.parent().map_or_else(String::new, |p| p.display().to_string());
            let detail = format!("{} · {from}", format_date(item.deleted, now));
            let detail = fit_label(&mut self.text, &detail, Face::Ui, 11.5 * s, tr - tx);
            let out = &mut self.draw.ui[3].glyphs;
            draw_text(&mut self.text, out, &detail, Face::Ui, 11.5 * s, tx, top + 37.0 * s, rgb(p::INK_SOFT), NO_HALO);
            let r = [x1 - pad - restore_w, top + 12.0 * s, x1 - pad, top + 40.0 * s];
            let action = Action::BinRestore(i);
            let lit = self.hov(action);
            let l = &mut self.draw.ui[3];
            l.rects.push(rect([r[0], r[1]], [r[2], r[3]], 3.0 * s, mix_rgb(rgb(p::INK), rgb(p::BAR_HOVER), lit)));
            let out = &mut self.draw.ui[3].glyphs;
            draw_text(&mut self.text, out, "Restore", Face::UiBold, 12.5 * s, r[0] + 10.0 * s, top + 26.0 * s + 4.4 * s, rgb(p::CREAM), NO_HALO);
            let clipped = [r[0], r[1].max(y), r[2], r[3].min(y1)];
            if clipped[3] > clipped[1] {
                self.zone(clipped, action);
            }
        }
        self.scrollbar(x1 - 10.0 * s, y, x1 - 2.0 * s, y1, content, scroll, max_scroll);
    }

    /// The folders people go to most, one click from the very top.
    fn quick_access(&mut self, x0: f32, x1: f32, y: &mut f32) {
        let s = self.scale;
        let pad = 20.0 * s;
        if self.acts.places.is_empty() {
            self.acts.places = crate::platform::quick_places();
        }
        let mut places: Vec<&'static str> = self.acts.places.iter().map(|p| p.0).collect();
        let bin_at = places.len();
        places.push("Recycle Bin");
        let gap = 6.0 * s;
        let width = (x1 - x0 - pad * 2.0 - gap) * 0.5;
        let height = 38.0 * s;
        for (i, name) in places.iter().enumerate() {
            let cx = x0 + pad + (i % 2) as f32 * (width + gap);
            let cy = *y + (i / 2) as f32 * (height + gap);
            let r = [cx, cy, cx + width, cy + height];
            let action = if i == bin_at { Action::ShowBin } else { Action::Place(i) };
            let lit = self.hov(action);
            let l = &mut self.draw.ui[1];
            l.rects.push(rect([r[0], r[1]], [r[2], r[3]], 3.0 * s, mix_rgb(rgb(p::PREVIEW_BG), rgb(p::PANEL_HOVER), lit)));
            let chip = 12.0 * s;
            let (ix, iy) = (cx + 19.0 * s, cy + height * 0.5);
            l.rects.push(rect([ix - chip, iy - chip], [ix + chip, iy + chip], 3.0 * s, rgb(p::GROUND)));
            if i == bin_at {
                draw_icon(l, Icon::Trash, [ix, iy], s * 0.82, rgb(p::CREAM));
            } else {
                l.flowers.push(FlowerInst {
                    pos: [ix, iy],
                    radius: 10.0 * s,
                    rotation: std::f32::consts::FRAC_PI_2,
                    bloom: 1.0,
                    species: p::BUD,
                    seed: i as f32,
                    lift: 0.0,
                    dim: 0.0,
                });
            }
            let out = &mut self.draw.ui[1].glyphs;
            draw_text(&mut self.text, out, name, Face::Ui, 13.5 * s, ix + 19.0 * s, iy + 4.8 * s, rgb(p::INK), NO_HALO);
            self.zone(r, action);
        }
        *y += places.len().div_ceil(2) as f32 * (height + gap) + 10.0 * s;
    }


    fn panel_search(&mut self, x0: f32, y0: f32, x1: f32, y1: f32) {
        let s = self.scale;
        let pad = 20.0 * s;
        let soft = rgb(p::INK_SOFT);
        let mut y = y0 + 20.0 * s;
        let count = self.search_hits.len();
        y = self.panel_title(x0, x1, y, "Search");
        let scope = self.tree.get(self.tree.root).name.to_string();
        let sub = if self.search_running {
            format!("Looking through {scope}… {count} so far")
        } else if count == 0 {
            format!("Nothing in {scope} matches that name")
        } else if self.search_capped {
            format!("First {count} matches in {scope}")
        } else {
            format!("{} in {scope}", plural(count as u32, "match", "matches"))
        };
        let sub = fit_label(&mut self.text, &sub, Face::Ui, 13.0 * s, x1 - x0 - pad * 2.0);
        let out = &mut self.draw.ui[1].glyphs;
        draw_text(&mut self.text, out, &sub, Face::Ui, 13.0 * s, x0 + pad, y + 11.0 * s, soft, NO_HALO);
        y += 30.0 * s;
        if !self.list.is_empty() {
            self.panel_rows(x0, y, x1, y1);
        }
    }

    fn panel_rows(&mut self, x0: f32, y0: f32, x1: f32, y1: f32) {
        let s = self.scale;
        let row_h = ROW_H * s;
        let two_lines = self.search_active;
        let row_h = if two_lines { row_h * 1.4 } else { row_h };
        let view = (y1 - y0).max(0.0);
        let content = self.list.len() as f32 * row_h + 8.0 * s;
        let max_scroll = (content - view).max(0.0);
        self.panel_scroll.target = self.panel_scroll.target.clamp(0.0, max_scroll);
        if let Some(row) = self.selected_row {
            let top = row as f32 * row_h;
            if top < self.panel_scroll.target {
                self.panel_scroll.target = top;
            } else if top + row_h > self.panel_scroll.target + view {
                self.panel_scroll.target = top + row_h - view;
            }
        }
        let scroll = self.panel_scroll.value.clamp(0.0, max_scroll);
        let first = (scroll / row_h) as usize;
        let count = (view / row_h) as usize + 2;
        {
            let l = &mut self.draw.ui[3];
            l.clip = Some([x0, y0, x1, y1]);
        }
        let ink = rgb(p::INK);
        let soft = rgb(p::INK_SOFT);
        let selected_node = self.selected;
        let [px, py] = self.pointer;
        for i in first..(first + count).min(self.list.len()) {
            let top = y0 + i as f32 * row_h - scroll;
            let r = [x0 + 8.0 * s, top, x1 - 12.0 * s, top + row_h];
            let (name, detail, species, node) = {
                let item = &self.list[i];
                (item.name.clone(), item.detail.clone(), item.species, item.node)
            };
            let hovered = inside(r, px, py) && py >= y0;
            let is_selected = self.selected_row == Some(i)
                || (node != NONE && node == selected_node)
                || (node != NONE && self.acts.marked.contains(&node));
            // Rows that could take the drop stand out as the pointer carries something past.
            let aiming = self.acts.drag.as_ref().is_some_and(|d| d.target != NONE && d.target == node);
            let lit = if hovered { 1.0 } else { 0.0f32 };
            let l = &mut self.draw.ui[3];
            if is_selected || aiming {
                l.rects.push(rect([r[0], r[1]], [r[2], r[3]], 3.0 * s, rgb(p::INK)));
            } else if lit > 0.0 {
                l.rects.push(rect([r[0], r[1]], [r[2], r[3]], 3.0 * s, rgb(p::PANEL_HOVER)));
            }
            let (row_ink, row_soft) = if is_selected || aiming { (rgb(p::CREAM), rgb(p::CREAM)) } else { (ink, soft) };
            let icon_x = r[0] + 16.0 * s;
            let cy = (r[1] + r[3]) * 0.5;
            let bud = species == p::BUD;
            // Each flower sits on a chip of the tree's own ground, where its colours were cut for.
            let chip = 12.5 * s;
            l.rects.push(rect([icon_x - chip, cy - chip], [icon_x + chip, cy + chip], 3.0 * s, rgb(p::GROUND)));
            l.flowers.push(FlowerInst {
                pos: [icon_x, cy],
                radius: if bud { 10.5 * s } else { 10.0 * s },
                rotation: if bud { std::f32::consts::FRAC_PI_2 } else { 0.3 },
                bloom: 1.0,
                species,
                seed: i as f32,
                lift: 0.0,
                dim: 0.0,
            });
            let text_x = icon_x + 21.0 * s;
            let right = r[2] - 8.0 * s;
            let size = 13.5 * s;
            if two_lines {
                let shown = fit_label(&mut self.text, &name, Face::Ui, size, right - text_x);
                let out = &mut self.draw.ui[3].glyphs;
                draw_text(&mut self.text, out, &shown, Face::Ui, size, text_x, cy - 2.0 * s, row_ink, NO_HALO);
                let shown = fit_label(&mut self.text, &detail, Face::Ui, 11.5 * s, right - text_x);
                let out = &mut self.draw.ui[3].glyphs;
                draw_text(&mut self.text, out, &shown, Face::Ui, 11.5 * s, text_x, cy + 13.0 * s, row_soft, NO_HALO);
            } else {
                let detail_w = if detail.is_empty() { 0.0 } else { self.text.width(&detail, Face::Ui, 12.0 * s) + 10.0 * s };
                let shown = fit_label(&mut self.text, &name, Face::Ui, size, right - text_x - detail_w);
                let out = &mut self.draw.ui[3].glyphs;
                draw_text(&mut self.text, out, &shown, Face::Ui, size, text_x, cy + 4.6 * s, row_ink, NO_HALO);
                if !detail.is_empty() {
                    let color = row_soft;
                    let out = &mut self.draw.ui[3].glyphs;
                    draw_text(&mut self.text, out, &detail, Face::Ui, 12.0 * s, right - detail_w + 10.0 * s, cy + 4.2 * s, color, NO_HALO);
                }
            }
            // Only rows inside the visible list can take clicks.
            let clipped = [r[0], r[1].max(y0), r[2], r[3].min(y1)];
            if clipped[3] > clipped[1] {
                self.zone(clipped, Action::Row(i));
            }
        }
        self.scrollbar(x1 - 10.0 * s, y0, x1 - 2.0 * s, y1, content, scroll, max_scroll);
    }

    /// A scrollbar in the panel's own ink.
    #[allow(clippy::too_many_arguments)]
    fn scrollbar(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, content: f32, scroll: f32, max: f32) {
        if max <= 0.0 {
            return;
        }
        let s = self.scale;
        let view = y1 - y0;
        let thumb = (view * view / content).max(28.0 * s).min(view);
        let top = y0 + (view - thumb) * (scroll / max);
        let track = [x0 - 2.0 * s, y0, x1 + 2.0 * s, y1];
        let active = self.scrolling || (self.pointer_inside && inside(track, self.pointer[0], self.pointer[1]));
        let cx = (x0 + x1) * 0.5;
        let half = if active { 3.0 * s } else { 2.0 * s };
        let l = &mut self.draw.ui[3];
        l.rects.push(rect(
            [cx - half, top + 2.0 * s],
            [cx + half, top + thumb - 2.0 * s],
            half,
            with_alpha(rgb(p::INK), if active { 0.75 } else { 0.42 }),
        ));
        self.zone(track, Action::PanelScroll);
        self.scroll_track = Some((track, max));
    }

    /// Names whichever icon button the pointer is resting on.
    fn build_tooltip(&mut self) {
        if !self.pointer_inside {
            return;
        }
        if self.acts.menu.is_some() || self.acts.dialog.is_some() || self.acts.quick.is_some() || self.acts.drag.is_some() {
            return;
        }
        let owned;
        let label = match self.hover_action {
            Action::Cmd(cmd) if cmd != crate::actions::Cmd::Open => {
                owned = if cmd.shortcut().is_empty() {
                    cmd.label().to_string()
                } else {
                    format!("{} ({})", cmd.label(), cmd.shortcut())
                };
                owned.as_str()
            }
            Action::Back => "Back (Alt+Left)",
            Action::Forward => "Forward (Alt+Right)",
            Action::Up => "Up one level (Backspace)",
            Action::Fit => "Fit the whole tree (Home)",
            Action::Reset => "Collapse everything (Ctrl+R)",
            Action::CollapseBranch(_) => "Collapse this branch",
            Action::ToggleHidden if self.tree.show_hidden => "Hide hidden items (Ctrl+H)",
            Action::ToggleHidden => "Show hidden items (Ctrl+H)",
            Action::ToggleMotion if self.reduced_motion => "Turn full motion on",
            Action::ToggleMotion => "Reduce motion",
            _ => return,
        };
        let s = self.scale;
        let size = 12.5 * s;
        let width = self.text.width(label, Face::Ui, size) + 18.0 * s;
        let height = 26.0 * s;
        let [px, py] = self.pointer;
        let limit = self.width;
        let x0 = (px - width * 0.5).clamp(6.0 * s, (limit - width - 6.0 * s).max(6.0 * s));
        let y0 = if py < self.toolbar_height() { self.toolbar_height() + 6.0 * s } else { py - height - 22.0 * s };
        let (a, b) = ([x0, y0], [x0 + width, y0 + height]);
        let l = self.layer(5);
        l.rects.push(soft_rect([a[0] + 1.0 * s, a[1] + 3.0 * s], [b[0] + 1.0 * s, b[1] + 3.0 * s], 3.0 * s, 3.0 * s, with_alpha(rgb(p::SHADOW), 0.45)));
        l.rects.push(rect(a, b, 3.0 * s, rgb(p::INK)));
        let out = &mut self.draw.ui[5].glyphs;
        draw_text(&mut self.text, out, label, Face::Ui, size, x0 + 9.0 * s, y0 + height * 0.5 + 4.4 * s, rgb(p::CREAM), NO_HALO);
    }
}

/// Whether this kind of file gets colour on its keywords and strings.
pub fn is_code(species: Species) -> bool {
    matches!(species, Species::Star | Species::Quince)
}

const KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "case", "catch", "class", "const", "continue", "def", "default", "do", "elif",
    "else", "enum", "export", "extends", "false", "finally", "fn", "for", "from", "func", "function", "if", "impl",
    "import", "in", "interface", "let", "match", "mod", "mut", "new", "nil", "None", "not", "null", "or", "package",
    "private", "pub", "public", "return", "self", "static", "struct", "super", "switch", "this", "throw", "trait",
    "true", "True", "False", "try", "type", "use", "var", "void", "while", "with", "yield",
];

/// Draws one line of a file. Source code is split into runs and coloured: this is a tint for
/// reading at a glance, so a run that starts on one line and ends on another isn't tracked.
pub fn draw_code(text: &mut crate::text::Text, out: &mut Vec<crate::gpu::GlyphInst>, line: &str, size: f32, x: f32, y: f32, code: bool) {
    let ink = rgb(p::INK);
    if !code {
        draw_text(text, out, line, Face::Mono, size, x, y, ink, NO_HALO);
        return;
    }
    let chars: Vec<char> = line.chars().collect();
    let starts_run = |c: char| c.is_alphanumeric() || matches!(c, '_' | '"' | '\'' | '`' | '/' | '#' | '-');
    let mut at = 0;
    let mut cx = x;
    while at < chars.len() {
        let ch = chars[at];
        let start = at;
        let only_space_before = chars[..at].iter().all(|c| c.is_whitespace());
        let color = if (ch == '/' && chars.get(at + 1) == Some(&'/'))
            || (ch == '#' && only_space_before)
            || (ch == '-' && chars.get(at + 1) == Some(&'-') && only_space_before)
        {
            at = chars.len();
            rgb(p::CODE_COMMENT)
        } else if matches!(ch, '"' | '\'' | '`') {
            at += 1;
            while at < chars.len() && chars[at] != ch {
                at += if chars[at] == '\\' { 2 } else { 1 };
            }
            at = (at + 1).min(chars.len());
            rgb(p::CODE_STRING)
        } else if ch.is_ascii_digit() {
            while at < chars.len() && (chars[at].is_ascii_alphanumeric() || matches!(chars[at], '.' | '_')) {
                at += 1;
            }
            rgb(p::CODE_NUMBER)
        } else if ch.is_alphabetic() || ch == '_' {
            while at < chars.len() && (chars[at].is_alphanumeric() || chars[at] == '_') {
                at += 1;
            }
            let word: String = chars[start..at].iter().collect();
            if KEYWORDS.contains(&word.as_str()) { rgb(p::CODE_KEYWORD) } else { ink }
        } else {
            at += 1;
            while at < chars.len() && !starts_run(chars[at]) {
                at += 1;
            }
            ink
        };
        let run: String = chars[start..at.min(chars.len())].iter().collect();
        cx += draw_text(text, out, &run, Face::Mono, size, cx, y, color, NO_HALO);
    }
}

/// How full a disk is, for the panel.
fn drive_space(info: &crate::platform::DriveInfo) -> String {
    if info.total == 0 {
        return String::new();
    }
    format!("{} free of {}", format_size(info.free), format_size(info.total))
}

pub fn describe_kind(name: &str, species: Species) -> String {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && ext.len() <= 8 => {
            format!("{} {}", ext.to_ascii_uppercase(), species.describe().to_lowercase())
        }
        _ => species.describe().to_string(),
    }
}

pub fn plural(n: u32, one: &str, many: &str) -> String {
    if n == 1 { format!("1 {one}") } else { format!("{n} {many}") }
}

/// Breaks text across lines at spaces, or anywhere if a single word is too long.
pub fn wrap(text: &mut crate::text::Text, string: &str, face: Face, size: f32, max: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in string.split_inclusive(|c: char| c == ' ' || c == '-' || c == '_' || c == '.') {
        let attempt = format!("{current}{word}");
        if text.width(attempt.trim_end(), face, size) <= max {
            current = attempt;
            continue;
        }
        if !current.is_empty() {
            lines.push(current.trim_end().to_string());
            current = String::new();
        }
        if text.width(word.trim_end(), face, size) <= max {
            current = word.to_string();
        } else {
            for ch in word.chars() {
                let attempt = format!("{current}{ch}");
                if text.width(&attempt, face, size) > max && !current.is_empty() {
                    lines.push(std::mem::take(&mut current));
                }
                current.push(ch);
            }
        }
    }
    if !current.trim().is_empty() {
        lines.push(current.trim_end().to_string());
    }
    lines
}

/// Cuts a line at the right edge without measuring all of a very long line.
pub fn fit_end(text: &mut crate::text::Text, line: &str, face: Face, size: f32, max: f32) -> String {
    let approx = (max / (size * 0.6)) as usize + 2;
    let cut: String = line.chars().take(approx).collect();
    if text.width(&cut, face, size) <= max {
        return cut;
    }
    let mut chars: Vec<char> = cut.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let attempt: String = chars.iter().collect();
        if text.width(&attempt, face, size) <= max {
            return attempt;
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_naturally() {
        assert_eq!(format_size(12), "12 bytes");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(format_size(900 * 1024 * 1024), "900 MB");
    }

    #[test]
    fn dates_read_naturally() {
        let now = 1_800_000_000;
        assert_eq!(format_date(now - 5, now), "Just now");
        assert_eq!(format_date(now - 120, now), "2 min ago");
        assert_eq!(format_date(now - 3600, now), "1 hour ago");
        assert_eq!(format_date(now - 86_400, now), "Yesterday");
        assert_eq!(format_date(0, now), "Unknown");
        assert_eq!(format_date(1_000_000_000, now), "9 Sep 2001");
    }
}
