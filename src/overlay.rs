//! Everything drawn above the tree and panel: context menu, dialogs, toasts, Quick Look,
//! and the flowers that follow the pointer during a drag.

use crate::actions::{Cmd, DialogKind};
use crate::app::{Action, App};
use crate::fs::Preview;
use crate::gpu::{FlowerInst, RectInst, rgb};
use crate::palette as p;
use crate::scene::{draw_text, fit_label};
use crate::text::{Face, Text};
use crate::tree::NONE;
use crate::ui::{Icon, draw_icon, format_size, line, rect, soft_rect, with_alpha, wrap};

const NO_HALO: [f32; 4] = [0.0; 4];
const QUICK: usize = 6;
const QUICK_CLIP: usize = 7;
const OVER: usize = 8;
const TOP: usize = 9;

impl App {
    /// Eases a control's hover amount toward whether it's hovered, and reports the amount.
    pub fn hov(&mut self, action: Action) -> f32 {
        let target = if self.hover_action == action { 1.0 } else { 0.0 };
        let dt = self.acts.ui_dt.max(1.0 / 240.0);
        let entry = self.acts.hover.entry(action).or_insert(0.0);
        let rate = if self.reduced_motion { 1.0 } else { 1.0 - (-dt * 20.0f32).exp() };
        *entry += (target - *entry) * rate;
        if (*entry - target).abs() < 0.01 {
            *entry = target;
        } else {
            self.redraw = true;
        }
        let value = *entry;
        if value == 0.0 && target == 0.0 {
            self.acts.hover.remove(&action);
        }
        value
    }

    /// Fades and shifts what a layer has gained since a mark, for things sliding into view.
    pub fn settle_layer(&mut self, layer: usize, rects: usize, glyphs: usize, alpha: f32, dx: f32, dy: f32) {
        let l = &mut self.draw.ui[layer];
        for f in &mut l.flowers {
            f.pos = [f.pos[0] + dx, f.pos[1] + dy];
            f.radius *= 0.85 + 0.15 * alpha;
        }
        if let Some((r, _)) = &mut l.image_rect {
            *r = [r[0] + dx, r[1] + dy, r[2] + dx, r[3] + dy];
        }
        for r in &mut l.rects[rects..] {
            let ring_or_box = r.params[2] > 1.5;
            r.a = [r.a[0] + dx, r.a[1] + dy];
            if !ring_or_box {
                r.b = [r.b[0] + dx, r.b[1] + dy];
            }
            r.color[3] *= alpha;
        }
        for g in &mut l.glyphs[glyphs..] {
            g.pos = [g.pos[0] + dx, g.pos[1] + dy];
            g.color[3] *= alpha;
            g.halo[3] *= alpha;
        }
    }

    /// The panel's contents ease into place whenever the selection changes.
    pub fn settle_panel(&mut self) {
        let t = self.acts.panel_in.clamp(0.0, 1.0);
        if t >= 1.0 {
            return;
        }
        let e = 1.0 - (1.0 - t).powi(3);
        let dy = (1.0 - e) * 10.0 * self.scale;
        self.settle_layer(1, 2, 0, e, 0.0, dy);
        self.settle_layer(2, 0, 0, e, 0.0, dy);
        self.settle_layer(3, 0, 0, e, 0.0, dy);
    }

    pub fn build_overlays(&mut self) {
        self.build_quick_look();
        self.build_toasts();
        self.build_drag_ghost();
        self.build_dialog();
        self.build_menu();
        if self.acts.film_cursor {
            self.build_film_cursor();
        }
    }

    fn build_film_cursor(&mut self) {
        let s = self.scale;
        let [x, y] = self.pointer;
        let r = if self.pressed_at.is_some() { 9.0 } else { 11.0 } * s;
        let l = self.layer(TOP + 1);
        l.rects.push(soft_rect([x - r + 1.0 * s, y - r + 3.0 * s], [x + r + 1.0 * s, y + r + 3.0 * s], r, 4.0 * s, with_alpha(rgb(p::SHADOW), 0.5)));
        l.rects.push(rect([x - r, y - r], [x + r, y + r], r, rgb(p::INK)));
        let inner = r - 2.0 * s;
        l.rects.push(rect([x - inner, y - inner], [x + inner, y + inner], inner, rgb(p::CREAM)));
    }

    // ------------------------------------------------------------ menu

    fn build_menu(&mut self) {
        let Some(menu) = &self.acts.menu else { return };
        let s = self.scale;
        let (mut x, mut y, open, cursor) = (menu.x, menu.y, menu.open.value.clamp(0.0, 1.0), menu.cursor);
        let rows: Vec<(Option<Cmd>, String, bool, bool)> =
            menu.items.iter().map(|i| (i.cmd, i.label.clone(), i.enabled, i.danger)).collect();

        let size = 13.5 * s;
        let row_h = 32.0 * s;
        let gap_h = 9.0 * s;
        let mut width = 190.0 * s;
        for (cmd, label, _, _) in &rows {
            let keys = cmd.map_or("", Cmd::shortcut);
            let need = self.text.width(label, Face::Ui, size) + self.text.width(keys, Face::Ui, 12.0 * s) + 56.0 * s;
            width = width.max(need);
        }
        let height: f32 = rows.iter().map(|r| if r.0.is_some() { row_h } else { gap_h }).sum::<f32>() + 12.0 * s;
        x = x.min(self.width - width - 8.0 * s).max(8.0 * s);
        y = y.min(self.height - height - 8.0 * s).max(self.toolbar_height());

        self.zones.push(crate::app::Zone { rect: [0.0, 0.0, self.width, self.height], action: Action::MenuClose });
        let from_rects = self.draw.ui[TOP].rects.len();
        let from_glyphs = self.draw.ui[TOP].glyphs.len();
        {
            let l = &mut self.draw.ui[TOP];
            l.rects.push(soft_rect([x + 2.0 * s, y + 6.0 * s], [x + width + 2.0 * s, y + height + 6.0 * s], 4.0 * s, 7.0 * s, with_alpha(rgb(p::SHADOW), 0.5)));
            l.rects.push(rect([x, y], [x + width, y + height], 4.0 * s, rgb(p::CREAM)));
        }
        let mut ry = y + 6.0 * s;
        for (i, (cmd, label, enabled, danger)) in rows.iter().enumerate() {
            let Some(cmd) = cmd else {
                let mid = ry + gap_h * 0.5;
                self.draw.ui[TOP].rects.push(rect([x + 12.0 * s, mid - 0.5 * s], [x + width - 12.0 * s, mid + 0.5 * s], 0.0, with_alpha(rgb(p::INK), 0.18)));
                ry += gap_h;
                continue;
            };
            let r = [x + 5.0 * s, ry, x + width - 5.0 * s, ry + row_h];
            let action = Action::MenuItem(i);
            let amount = if *enabled { self.hov(action) } else { 0.0 };
            let lit = amount.max(if cursor == Some(i) { 1.0 } else { 0.0 });
            let ink = rgb(p::INK);
            if lit > 0.0 {
                let fill = if *danger { rgb(p::DANGER) } else { ink };
                self.draw.ui[TOP].rects.push(rect([r[0], r[1]], [r[2], r[3]], 3.0 * s, with_alpha(fill, lit)));
            }
            let idle = if !enabled {
                with_alpha(ink, 0.35)
            } else if *danger {
                rgb(p::DANGER_INK)
            } else {
                ink
            };
            let text = crate::scene::mix_rgb(idle, rgb(p::CREAM), lit);
            let base = ry + row_h * 0.5 + size * 0.36;
            let out = &mut self.draw.ui[TOP].glyphs;
            draw_text(&mut self.text, out, label, Face::Ui, size, x + 15.0 * s, base, text, NO_HALO);
            let keys = cmd.shortcut();
            if !keys.is_empty() {
                let w = self.text.width(keys, Face::Ui, 12.0 * s);
                let hint = crate::scene::mix_rgb(with_alpha(rgb(p::INK_SOFT), if *enabled { 1.0 } else { 0.4 }), rgb(p::CREAM), lit);
                let out = &mut self.draw.ui[TOP].glyphs;
                draw_text(&mut self.text, out, keys, Face::Ui, 12.0 * s, x + width - 15.0 * s - w, base, hint, NO_HALO);
            }
            if *enabled {
                self.zones.push(crate::app::Zone { rect: r, action });
            }
            ry += row_h;
        }
        self.settle_layer(TOP, from_rects, from_glyphs, open, 0.0, (1.0 - open) * -8.0 * s);
    }

    // ------------------------------------------------------------ dialogs

    fn build_dialog(&mut self) {
        let Some(dialog) = &self.acts.dialog else { return };
        let s = self.scale;
        let open = dialog.open.value.clamp(0.0, 1.0);
        let (title, body, confirm, field) = (dialog.title.clone(), dialog.body.clone(), dialog.confirm.clone(), dialog.field.clone());
        let destructive = matches!(dialog.kind, DialogKind::Confirm(_));
        let problem = self.dialog_problem();

        let width = (430.0 * s).min(self.width - 40.0 * s);
        let x = (self.width - width) * 0.5;
        let pad = 22.0 * s;
        let room = width - pad * 2.0;
        let body_lines = wrap(&mut self.text, &body, Face::Ui, 14.0 * s, room);
        let mut height = 72.0 * s + 56.0 * s;
        height += body_lines.len() as f32 * 21.0 * s;
        if field.is_some() {
            height += 62.0 * s;
        }
        if !body_lines.is_empty() {
            height += 10.0 * s;
        }
        let y = ((self.height - height) * 0.42).max(20.0 * s);

        self.zones.push(crate::app::Zone { rect: [0.0, 0.0, self.width, self.height], action: Action::DialogCancel });
        self.zones.push(crate::app::Zone { rect: [x, y, x + width, y + height], action: Action::Nothing });

        let rects_mark = self.draw.ui[OVER].rects.len();
        let glyphs_mark = self.draw.ui[OVER].glyphs.len();
        {
            let l = &mut self.draw.ui[OVER];
            l.rects.push(rect([0.0, 0.0], [self.width, self.height], 0.0, with_alpha(rgb(p::SHADOW), 0.42)));
            l.rects.push(soft_rect([x + 3.0 * s, y + 8.0 * s], [x + width + 3.0 * s, y + height + 8.0 * s], 5.0 * s, 10.0 * s, with_alpha(rgb(p::SHADOW), 0.5)));
            l.rects.push(rect([x, y], [x + width, y + height], 5.0 * s, rgb(p::CREAM)));
        }
        let mut cy = y + 24.0 * s;
        let out = &mut self.draw.ui[OVER].glyphs;
        draw_text(&mut self.text, out, &title, Face::Display, 26.0 * s, x + pad, cy + 22.0 * s, rgb(p::INK), NO_HALO);
        cy += 44.0 * s;
        for text in &body_lines {
            let out = &mut self.draw.ui[OVER].glyphs;
            draw_text(&mut self.text, out, text, Face::Ui, 14.0 * s, x + pad, cy + 14.0 * s, rgb(p::INK_SOFT), NO_HALO);
            cy += 21.0 * s;
        }
        if !body_lines.is_empty() {
            cy += 10.0 * s;
        }
        if let Some(field) = &field {
            let h = 38.0 * s;
            let size = 15.0 * s;
            let (bx0, bx1) = (x + pad, x + width - pad);
            let ring = if problem.is_some() { rgb(p::DANGER) } else { rgb(p::GROUND) };
            {
                let l = &mut self.draw.ui[OVER];
                l.rects.push(rect([bx0 - 2.0 * s, cy - 2.0 * s], [bx1 + 2.0 * s, cy + h + 2.0 * s], 4.0 * s, ring));
                l.rects.push(rect([bx0, cy], [bx1, cy + h], 3.0 * s, rgb(0xffffff)));
            }
            // The text is drawn in pieces so the selection can have its own colour behind it.
            let text_x = bx0 + 10.0 * s;
            let base = cy + h * 0.5 + size * 0.36;
            let upto = |text: &mut Text, n: usize| -> f32 {
                let piece: String = field.chars[..n].iter().collect();
                text.width(&piece, Face::Ui, size)
            };
            let (a, b) = (field.anchor.min(field.caret), field.anchor.max(field.caret));
            let (ax, bx, cx) = (upto(&mut self.text, a), upto(&mut self.text, b), upto(&mut self.text, field.caret));
            // Keep the caret in view for names longer than the box.
            let room = bx1 - bx0 - 20.0 * s;
            let shift = (cx - room).max(0.0);
            self.draw.ui[OVER].rects.push(rect([text_x + ax - shift, cy + 7.0 * s], [text_x + bx - shift, cy + h - 7.0 * s], 1.5 * s, with_alpha(rgb(p::GROUND), 0.28)));
            let shown = field.text();
            let out = &mut self.draw.ui[OVER].glyphs;
            draw_text(&mut self.text, out, &shown, Face::Ui, size, text_x - shift, base, rgb(p::INK), NO_HALO);
            let caret = text_x + cx - shift;
            self.draw.ui[OVER].rects.push(rect([caret, cy + 8.0 * s], [caret + 1.6 * s, cy + h - 8.0 * s], 0.0, rgb(p::INK)));
            cy += h + 8.0 * s;
            if let Some(problem) = problem {
                let shown = fit_label(&mut self.text, problem, Face::Ui, 12.5 * s, room);
                let out = &mut self.draw.ui[OVER].glyphs;
                draw_text(&mut self.text, out, &shown, Face::Ui, 12.5 * s, bx0, cy + 10.0 * s, rgb(p::DANGER_INK), NO_HALO);
            }
            cy += 16.0 * s;
        }
        // Buttons along the bottom right.
        let by = y + height - 54.0 * s;
        let confirm_w = self.text.width(&confirm, Face::UiBold, 13.5 * s) + 30.0 * s;
        let cancel_w = self.text.width("Cancel", Face::UiBold, 13.5 * s) + 30.0 * s;
        let cx1 = x + width - pad;
        let ok = [cx1 - confirm_w, by, cx1, by + 36.0 * s];
        let no = [ok[0] - 8.0 * s - cancel_w, by, ok[0] - 8.0 * s, by + 36.0 * s];
        let ok_hover = self.hov(Action::DialogOk);
        let no_hover = self.hov(Action::DialogCancelButton);
        let base_fill = if destructive { rgb(p::DANGER) } else { rgb(p::INK) };
        let hot = if destructive { rgb(p::DANGER_HOVER) } else { rgb(p::BAR_HOVER) };
        let fill = crate::scene::mix_rgb(base_fill, hot, ok_hover);
        let blocked = problem.is_some();
        {
            let l = &mut self.draw.ui[OVER];
            l.rects.push(rect([ok[0], ok[1]], [ok[2], ok[3]], 3.0 * s, with_alpha(fill, if blocked { 0.4 } else { 1.0 })));
            l.rects.push(rect([no[0], no[1]], [no[2], no[3]], 3.0 * s, rgb(p::INK)));
            let t = 1.4 * s;
            l.rects.push(rect([no[0] + t, no[1] + t], [no[2] - t, no[3] - t], 2.0 * s, crate::scene::mix_rgb(rgb(p::CREAM), rgb(p::PANEL_HOVER), no_hover)));
        }
        let base = by + 18.0 * s + 13.5 * s * 0.36;
        let out = &mut self.draw.ui[OVER].glyphs;
        draw_text(&mut self.text, out, &confirm, Face::UiBold, 13.5 * s, ok[0] + 15.0 * s, base, rgb(p::CREAM), NO_HALO);
        let out = &mut self.draw.ui[OVER].glyphs;
        draw_text(&mut self.text, out, "Cancel", Face::UiBold, 13.5 * s, no[0] + 15.0 * s, base, rgb(p::INK), NO_HALO);
        if !blocked {
            self.zones.push(crate::app::Zone { rect: ok, action: Action::DialogOk });
        }
        self.zones.push(crate::app::Zone { rect: no, action: Action::DialogCancelButton });
        let _ = cy;
        self.settle_layer(OVER, rects_mark, glyphs_mark, open, 0.0, (1.0 - open) * 14.0 * s);
    }

    // ------------------------------------------------------------ toasts

    fn build_toasts(&mut self) {
        let s = self.scale;
        let stage = self.stage();
        let working = self.acts.working;
        let mut y = self.height - 52.0 * s;
        if let Some(progress) = working {
            let width = 260.0 * s;
            let x = (stage[0] + stage[2] - width) * 0.5;
            let h = 40.0 * s;
            let top = y - h;
            let label = if progress > 0.0 { format!("Working… {:.0}%", progress * 100.0) } else { "Working…".to_string() };
            let l = &mut self.draw.ui[TOP];
            l.rects.push(soft_rect([x + 1.0 * s, top + 4.0 * s], [x + width + 1.0 * s, y + 4.0 * s], 4.0 * s, 5.0 * s, with_alpha(rgb(p::SHADOW), 0.45)));
            l.rects.push(rect([x, top], [x + width, y], 4.0 * s, rgb(p::INK)));
            l.rects.push(rect([x, y - 3.0 * s], [x + width * progress.clamp(0.0, 1.0), y], 1.5 * s, rgb(p::SPECIES[5][0])));
            let out = &mut self.draw.ui[TOP].glyphs;
            draw_text(&mut self.text, out, &label, Face::Ui, 13.5 * s, x + 14.0 * s, top + h * 0.5 + 4.8 * s, rgb(p::CREAM), NO_HALO);
            y = top - 8.0 * s;
        }
        for i in 0..self.acts.toasts.len() {
            let (text, has_undo, error, shown) = {
                let t = &self.acts.toasts[i];
                (t.text.clone(), t.undo.is_some(), t.error, t.shown.value.clamp(0.0, 1.0))
            };
            let size = 13.5 * s;
            let undo_w = if has_undo { self.text.width("Undo", Face::UiBold, size) + 30.0 * s } else { 0.0 };
            let max_text = (stage[2] - stage[0] - 120.0 * s).min(560.0 * s);
            let label = fit_label(&mut self.text, &text, Face::Ui, size, max_text);
            let width = self.text.width(&label, Face::Ui, size) + 32.0 * s + undo_w;
            let x = (stage[0] + stage[2] - width) * 0.5;
            let h = 40.0 * s;
            let top = y - h;
            let rects_mark = self.draw.ui[TOP].rects.len();
            let glyphs_mark = self.draw.ui[TOP].glyphs.len();
            {
                let fill = if error { rgb(p::DANGER_INK) } else { rgb(p::INK) };
                let l = &mut self.draw.ui[TOP];
                l.rects.push(soft_rect([x + 1.0 * s, top + 4.0 * s], [x + width + 1.0 * s, y + 4.0 * s], 4.0 * s, 5.0 * s, with_alpha(rgb(p::SHADOW), 0.45)));
                l.rects.push(rect([x, top], [x + width, y], 4.0 * s, fill));
            }
            let out = &mut self.draw.ui[TOP].glyphs;
            draw_text(&mut self.text, out, &label, Face::Ui, size, x + 16.0 * s, top + h * 0.5 + size * 0.36, rgb(p::CREAM), NO_HALO);
            if has_undo {
                let r = [x + width - undo_w, top, x + width, y];
                let action = Action::ToastUndo(i);
                let lit = self.hov(action);
                self.draw.ui[TOP].rects.push(rect([r[0] + 4.0 * s, r[1] + 5.0 * s], [r[2] - 5.0 * s, r[3] - 5.0 * s], 3.0 * s, with_alpha(rgb(p::CREAM), 0.12 + lit * 0.18)));
                let out = &mut self.draw.ui[TOP].glyphs;
                draw_text(&mut self.text, out, "Undo", Face::UiBold, size, r[0] + 15.0 * s, top + h * 0.5 + size * 0.36, rgb(p::CREAM), NO_HALO);
                self.zones.push(crate::app::Zone { rect: r, action });
            }
            self.settle_layer(TOP, rects_mark, glyphs_mark, shown, 0.0, (1.0 - shown) * 16.0 * s);
            y = top - 8.0 * s;
        }
    }

    // ------------------------------------------------------------ drag

    fn build_drag_ghost(&mut self) {
        let Some(drag) = &self.acts.drag else { return };
        let s = self.scale;
        let [px, py] = self.pointer;
        let flowers = drag.flowers.clone();
        let count = drag.items.len();
        let target = drag.target;
        for (i, species) in flowers.iter().enumerate() {
            let shift = i as f32 * 9.0 * s;
            self.draw.ui[TOP].flowers.push(FlowerInst {
                pos: [px + 14.0 * s + shift, py + 12.0 * s - shift * 0.5],
                radius: 20.0 * s,
                rotation: i as f32 * 0.8,
                bloom: 1.0,
                species: *species,
                seed: i as f32 * 3.0,
                lift: 1.0,
                dim: 0.0,
            });
        }
        let what = if count == 1 { String::new() } else { format!("{count} items ") };
        let label = if target != NONE {
            let copies = self.drop_copies(target);
            let name = self.tree.get(target).name.to_string();
            format!("{}{what}to {name}", if copies { "Copy " } else { "Move " })
        } else {
            format!("{what}Drop on a folder").trim_start().to_string()
        };
        let size = 12.5 * s;
        let label = fit_label(&mut self.text, &label, Face::Ui, size, 300.0 * s);
        let width = self.text.width(&label, Face::Ui, size) + 20.0 * s;
        let x = px + 14.0 * s;
        let y = py + 38.0 * s;
        let fill = if target != NONE { rgb(p::INK) } else { with_alpha(rgb(p::INK), 0.72) };
        self.draw.ui[TOP].rects.push(rect([x, y], [x + width, y + 26.0 * s], 3.0 * s, fill));
        let out = &mut self.draw.ui[TOP].glyphs;
        draw_text(&mut self.text, out, &label, Face::Ui, size, x + 10.0 * s, y + 13.0 * s + size * 0.36, rgb(p::CREAM), NO_HALO);
        // The folder being aimed at gets a paper ring, so it's clear what the drop will hit.
        if target != NONE {
            let tip = self.tree.get(target).tip;
            let z = self.camera.zoom();
            let center = [self.camera.ox.value + tip[0] * z, self.camera.oy.value - tip[1] * z];
            let pulse = 1.0 + (self.time * 7.0).sin() * 0.06;
            self.draw.ui[0].rects.push(RectInst {
                a: center,
                b: [22.0 * s * pulse, 0.0],
                params: [0.0, 0.0, 2.0, 2.0 * s],
                color: rgb(p::CREAM),
            });
        }
    }

    // ------------------------------------------------------------ Quick Look

    fn build_quick_look(&mut self) {
        let Some(quick) = &self.acts.quick else { return };
        let (open, zoom, pan, scroll) = (quick.open.value.clamp(0.0, 1.0), quick.zoom, quick.pan, quick.scroll.value);
        let id = self.selected;
        if !self.tree.is_valid(id) {
            return;
        }
        let s = self.scale;
        let (w, h) = (self.width, self.height);
        let margin = 34.0 * s;
        let (x0, y0, x1, y1) = (margin, margin, w - margin, h - margin);
        let header = 62.0 * s;
        let name = self.tree.get(id).name.to_string();
        let species = self.tree.get(id).species();
        let size = self.tree.get(id).size;

        self.zones.push(crate::app::Zone { rect: [0.0, 0.0, w, h], action: Action::QuickClose });
        self.zones.push(crate::app::Zone { rect: [x0, y0, x1, y1], action: Action::QuickContent });

        let rects_mark = self.draw.ui[QUICK].rects.len();
        let glyphs_mark = self.draw.ui[QUICK].glyphs.len();
        {
            let l = &mut self.draw.ui[QUICK];
            l.rects.push(rect([0.0, 0.0], [w, h], 0.0, with_alpha(rgb(p::SHADOW), 0.72 * open)));
            l.rects.push(soft_rect([x0 + 4.0 * s, y0 + 10.0 * s], [x1 + 4.0 * s, y1 + 10.0 * s], 6.0 * s, 14.0 * s, with_alpha(rgb(p::SHADOW), 0.55 * open)));
            l.rects.push(rect([x0, y0], [x1, y1], 6.0 * s, rgb(p::CREAM)));
        }
        let room = x1 - x0 - 400.0 * s;
        let title = fit_label(&mut self.text, &name, Face::Display, 26.0 * s, room);
        let out = &mut self.draw.ui[QUICK].glyphs;
        draw_text(&mut self.text, out, &title, Face::Display, 26.0 * s, x0 + 24.0 * s, y0 + 36.0 * s, rgb(p::INK), NO_HALO);
        let sub = format!("{}  ·  {}", species.describe(), format_size(size));
        let out = &mut self.draw.ui[QUICK].glyphs;
        draw_text(&mut self.text, out, &sub, Face::Ui, 13.0 * s, x0 + 24.0 * s, y0 + 54.0 * s, rgb(p::INK_SOFT), NO_HALO);

        // Controls on the right of the header.
        let mut cx = x1 - 14.0 * s;
        for (icon, action) in [(Icon::Close, Action::QuickClose2), (Icon::Forward, Action::QuickNext), (Icon::Back, Action::QuickPrev)] {
            let size = 36.0 * s;
            let r = [cx - size, y0 + 13.0 * s, cx, y0 + 13.0 * s + size];
            let lit = self.hov(action);
            let l = &mut self.draw.ui[QUICK];
            if lit > 0.0 {
                l.rects.push(rect([r[0], r[1]], [r[2], r[3]], 3.0 * s, with_alpha(rgb(p::PANEL_HOVER), lit)));
            }
            draw_icon(l, icon, [(r[0] + r[2]) * 0.5, (r[1] + r[3]) * 0.5], s, rgb(p::INK));
            self.zones.push(crate::app::Zone { rect: r, action });
            cx -= size + 2.0 * s;
        }
        let open_w = self.text.width("Open", Face::UiBold, 13.5 * s) + 30.0 * s;
        let r = [cx - open_w - 8.0 * s, y0 + 13.0 * s, cx - 8.0 * s, y0 + 49.0 * s];
        let action = Action::Cmd(Cmd::Open);
        let lit = self.hov(action);
        self.draw.ui[QUICK].rects.push(rect([r[0], r[1]], [r[2], r[3]], 3.0 * s, crate::scene::mix_rgb(rgb(p::INK), rgb(p::BAR_HOVER), lit)));
        let out = &mut self.draw.ui[QUICK].glyphs;
        draw_text(&mut self.text, out, "Open", Face::UiBold, 13.5 * s, r[0] + 15.0 * s, y0 + 31.0 * s + 13.5 * s * 0.36, rgb(p::CREAM), NO_HALO);
        self.zones.push(crate::app::Zone { rect: r, action });

        // The contents.
        let (cx0, cy0, cx1, cy1) = (x0 + 14.0 * s, y0 + header, x1 - 14.0 * s, y1 - 14.0 * s);
        self.draw.ui[QUICK].rects.push(rect([cx0, cy0], [cx1, cy1], 4.0 * s, rgb(p::PREVIEW_BG)));
        self.draw.ui[QUICK_CLIP].clip = Some([cx0, cy0, cx1, cy1]);
        let (cw, ch) = (cx1 - cx0, cy1 - cy0);
        let center = [(cx0 + cx1) * 0.5, (cy0 + cy1) * 0.5];

        enum View {
            Image(f32, f32),
            Text(Vec<String>, bool),
            Listing(Vec<(String, u64)>, usize),
            Audio,
            Other(Option<String>),
        }
        let view = match self.preview.as_ref().map(|l| &l.preview) {
            Some(Preview::Image { width, height, .. }) => View::Image(*width as f32, *height as f32),
            Some(Preview::Text { lines, truncated }) => View::Text(lines.clone(), *truncated),
            Some(Preview::Listing { entries, total }) => View::Listing(entries.clone(), *total),
            Some(Preview::Audio) => View::Audio,
            Some(Preview::Unavailable { reason }) => View::Other(Some(reason.clone())),
            Some(_) => View::Other(Some("No preview for this kind of file. Press Enter to open it.".to_string())),
            // Still on its way, so there's nothing to say about it yet.
            None => View::Other(None),
        };
        match view {
            View::Image(iw, ih) => {
                let fit = (cw / iw).min(ch / ih).min(3.0) * zoom;
                let (dw, dh) = (iw * fit, ih * fit);
                let (ix, iy) = (center[0] + pan[0], center[1] + pan[1]);
                self.draw.ui[QUICK_CLIP].image_rect = Some(([ix - dw * 0.5, iy - dh * 0.5, ix + dw * 0.5, iy + dh * 0.5], 2.0 * s));
            }
            View::Text(lines, truncated) => {
                let text_size = 14.0 * s;
                let line_h = 21.0 * s;
                let content = lines.len() as f32 * line_h + 28.0 * s;
                let max = (content - ch).max(0.0);
                if let Some(q) = &mut self.acts.quick {
                    q.scroll.target = q.scroll.target.min(max);
                }
                let scroll = scroll.min(max).max(0.0);
                let first = (scroll / line_h) as usize;
                let count = (ch / line_h) as usize + 2;
                let code = crate::ui::is_code(species);
                let gutter = self.text.width(&format!("{}", lines.len()), Face::Mono, 12.0 * s) + 24.0 * s;
                if lines.is_empty() {
                    let out = &mut self.draw.ui[QUICK_CLIP].glyphs;
                    draw_text(&mut self.text, out, "This file is empty.", Face::Ui, 15.0 * s, cx0 + 20.0 * s, cy0 + 34.0 * s, rgb(p::INK_SOFT), NO_HALO);
                }
                for (i, text) in lines.iter().enumerate().skip(first).take(count) {
                    let y = cy0 + 14.0 * s + i as f32 * line_h - scroll + text_size;
                    let number = format!("{}", i + 1);
                    let nw = self.text.width(&number, Face::Mono, 12.0 * s);
                    let out = &mut self.draw.ui[QUICK_CLIP].glyphs;
                    draw_text(&mut self.text, out, &number, Face::Mono, 12.0 * s, cx0 + gutter - 12.0 * s - nw, y, with_alpha(rgb(p::INK_SOFT), 0.7), NO_HALO);
                    let shown: String = text.chars().take(((cw - gutter) / (text_size * 0.6)) as usize + 2).collect();
                    crate::ui::draw_code(&mut self.text, &mut self.draw.ui[QUICK_CLIP].glyphs, &shown, text_size, cx0 + gutter, y, code);
                }
                if truncated {
                    let y = cy0 + 14.0 * s + lines.len() as f32 * line_h - scroll + text_size;
                    let out = &mut self.draw.ui[QUICK_CLIP].glyphs;
                    draw_text(&mut self.text, out, "Showing the start of this file.", Face::Ui, 13.0 * s, cx0 + gutter, y, rgb(p::INK_SOFT), NO_HALO);
                }
            }
            View::Listing(entries, total) => {
                let line_h = 28.0 * s;
                let content = entries.len() as f32 * line_h + 40.0 * s;
                let max = (content - ch).max(0.0);
                if let Some(q) = &mut self.acts.quick {
                    q.scroll.target = q.scroll.target.min(max);
                }
                let scroll = scroll.min(max).max(0.0);
                let first = (scroll / line_h) as usize;
                let count = (ch / line_h) as usize + 2;
                let head = format!("{total} items in this archive");
                let out = &mut self.draw.ui[QUICK_CLIP].glyphs;
                draw_text(&mut self.text, out, &head, Face::UiBold, 14.0 * s, cx0 + 20.0 * s, cy0 + 26.0 * s - scroll, rgb(p::INK), NO_HALO);
                for (i, (entry, bytes)) in entries.iter().enumerate().skip(first).take(count) {
                    let y = cy0 + 54.0 * s + i as f32 * line_h - scroll;
                    let shown = fit_label(&mut self.text, entry, Face::Ui, 14.0 * s, cw - 140.0 * s);
                    let out = &mut self.draw.ui[QUICK_CLIP].glyphs;
                    draw_text(&mut self.text, out, &shown, Face::Ui, 14.0 * s, cx0 + 20.0 * s, y, rgb(p::INK), NO_HALO);
                    let size_text = format_size(*bytes);
                    let sw = self.text.width(&size_text, Face::Ui, 12.5 * s);
                    let out = &mut self.draw.ui[QUICK_CLIP].glyphs;
                    draw_text(&mut self.text, out, &size_text, Face::Ui, 12.5 * s, cx1 - 20.0 * s - sw, y, rgb(p::INK_SOFT), NO_HALO);
                }
            }
            View::Audio => {
                self.draw.ui[QUICK_CLIP].rects.push(rect([cx0, cy0], [cx1, cy1], 4.0 * s, rgb(p::GROUND)));
                self.draw.ui[QUICK_CLIP].flowers.push(FlowerInst {
                    pos: [center[0], center[1] - 60.0 * s],
                    radius: 110.0 * s,
                    rotation: 0.3,
                    bloom: 1.0,
                    species: species.index() as f32,
                    seed: 4.0,
                    lift: 1.0,
                    dim: 0.0,
                });
                let width = (cw * 0.6).min(620.0 * s);
                self.draw_audio_player(QUICK_CLIP, center[0] - width * 0.5, center[1] + 90.0 * s, width, rgb(p::CREAM), with_alpha(rgb(p::CREAM), 0.25));
            }
            View::Other(reason) => {
                self.draw.ui[QUICK_CLIP].rects.push(rect([cx0, cy0], [cx1, cy1], 4.0 * s, rgb(p::GROUND)));
                self.draw.ui[QUICK_CLIP].flowers.push(FlowerInst {
                    pos: [center[0], center[1] - 40.0 * s],
                    radius: 110.0 * s,
                    rotation: 0.3,
                    bloom: 1.0,
                    species: species.index() as f32,
                    seed: 4.0,
                    lift: 1.0,
                    dim: 0.0,
                });
                if let Some(text) = reason {
                    let tw = self.text.width(&text, Face::Ui, 15.0 * s);
                    let out = &mut self.draw.ui[QUICK_CLIP].glyphs;
                    draw_text(&mut self.text, out, &text, Face::Ui, 15.0 * s, center[0] - tw * 0.5, center[1] + 110.0 * s, rgb(p::CREAM), NO_HALO);
                }
            }
        }
        // Slide and fade the whole sheet in, except its backdrop, which already fades itself.
        let dy = (1.0 - open) * 22.0 * s;
        self.settle_layer(QUICK, rects_mark + 1, glyphs_mark, open, 0.0, dy);
        self.settle_layer(QUICK_CLIP, 0, 0, open, 0.0, dy);
    }

    /// Play button, progress bar, and times, used in the panel and in Quick Look.
    pub fn draw_audio_player(&mut self, layer: usize, x: f32, y: f32, width: f32, ink: [f32; 4], track: [f32; 4]) {
        let s = self.scale;
        let this_path = self.tree.is_valid(self.selected).then(|| self.tree.path_of(self.selected)).flatten();
        let player = self.acts.audio.as_ref().filter(|a| Some(a.path.as_path()) == this_path.as_deref());
        let (playing, position, duration) =
            player.map_or((false, 0.0, 0.0), |a| (a.is_playing(), a.position(), a.duration));
        if playing {
            self.redraw = true;
        }
        let d = 42.0 * s;
        let r = [x, y, x + d, y + d];
        let lit = self.hov(Action::AudioToggle);
        let center = [x + d * 0.5, y + d * 0.5];
        let l = &mut self.draw.ui[layer];
        l.rects.push(rect([r[0], r[1]], [r[2], r[3]], d * 0.5, crate::scene::mix_rgb(ink, track, lit * 0.3)));
        let fg = if ink[0] > 0.5 { rgb(p::INK) } else { rgb(p::CREAM) };
        if playing {
            l.rects.push(line([center[0] - 4.0 * s, center[1] - 6.0 * s], [center[0] - 4.0 * s, center[1] + 6.0 * s], 1.7 * s, fg));
            l.rects.push(line([center[0] + 4.0 * s, center[1] - 6.0 * s], [center[0] + 4.0 * s, center[1] + 6.0 * s], 1.7 * s, fg));
        } else {
            let (a, b, c) = (
                [center[0] - 4.0 * s, center[1] - 7.0 * s],
                [center[0] - 4.0 * s, center[1] + 7.0 * s],
                [center[0] + 7.0 * s, center[1]],
            );
            l.rects.push(line(a, b, 1.5 * s, fg));
            l.rects.push(line(b, c, 1.5 * s, fg));
            l.rects.push(line(c, a, 1.5 * s, fg));
            l.rects.push(line([center[0] - 2.0 * s, center[1] - 3.0 * s], [center[0] - 2.0 * s, center[1] + 3.0 * s], 2.2 * s, fg));
        }
        self.zones.push(crate::app::Zone { rect: r, action: Action::AudioToggle });
        let bar = [x + d + 14.0 * s, y + d * 0.5 - 2.5 * s, x + width - 76.0 * s, y + d * 0.5 + 2.5 * s];
        let fraction = if duration > 0.0 { (position / duration).clamp(0.0, 1.0) } else { 0.0 };
        let l = &mut self.draw.ui[layer];
        l.rects.push(rect([bar[0], bar[1]], [bar[2], bar[3]], 2.5 * s, track));
        let filled = bar[0] + (bar[2] - bar[0]) * fraction;
        l.rects.push(rect([bar[0], bar[1]], [filled.max(bar[0] + 5.0 * s), bar[3]], 2.5 * s, ink));
        l.rects.push(rect([filled - 6.0 * s, (bar[1] + bar[3]) * 0.5 - 6.0 * s], [filled + 6.0 * s, (bar[1] + bar[3]) * 0.5 + 6.0 * s], 6.0 * s, ink));
        let hit = [bar[0], y, bar[2], y + d];
        self.acts.audio_bar = hit;
        self.zones.push(crate::app::Zone { rect: hit, action: Action::AudioSeek });
        let clock = |t: f32| format!("{}:{:02}", (t / 60.0) as u32, (t as u32) % 60);
        let time = if duration > 0.0 { format!("{} / {}", clock(position), clock(duration)) } else { clock(position) };
        let out = &mut self.draw.ui[layer].glyphs;
        draw_text(&mut self.text, out, &time, Face::Ui, 12.0 * s, bar[2] + 12.0 * s, y + d * 0.5 + 4.2 * s, ink, NO_HALO);
    }
}
