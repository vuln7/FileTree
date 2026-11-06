//! Turns the tree into paper for this frame: walks it from the root, carries each branch's
//! motion down to its children, and skips whole limbs that are off screen.

use crate::app::{App, Visible};
use crate::gpu::{BranchInst, FlowerInst, GlyphInst, LONG_SEGMENTS, SHORT_SEGMENTS, rgb};
use crate::layout::FLOWER_RADIUS;
use crate::palette as p;
use crate::text::{Face, Text};
use crate::tree::{Kind, Load, NONE, NodeId, rand01};

/// How far below the soil line the trunk starts.
pub const ROOT_SINK: f32 = 34.0;

pub struct LabelCand {
    pub id: NodeId,
    pub x: f32,
    pub y: f32,
    pub gap_x: f32,
    pub gap_y: f32,
    /// A point part-way back down the limb, for a folder name to sit beside.
    pub back: [f32; 2],
    /// Sides to try first: 0 right, 1 left, 2 above, 3 below. The corners follow.
    pub order: [u8; 4],
    pub priority: f32,
    pub target: f32,
}

struct Walk {
    zoom: f32,
    ox: f32,
    oy: f32,
    margin: [f32; 4],
    time: f32,
    wind: f32,
    scale: f32,
    dt: f32,
    needs_more: bool,
    selected: NodeId,
    hover: NodeId,
    focus: NodeId,
    query: Vec<String>,
}

impl App {
    pub fn build_scene(&mut self, dt: f32) -> bool {
        self.draw.clear();
        self.visible.clear();
        self.label_cands.clear();
        self.stem_marks.clear();
        let z = self.camera.zoom();
        self.draw.time = self.time;
        self.draw.scale = self.scale;
        self.draw.world_origin = [self.camera.ox.value, self.camera.oy.value];
        self.draw.zoom = z;
        self.draw.wind = self.wind.value;

        // Dim everything outside the folder holding the selection, so focus reads instantly.
        let mut focus = self.selected;
        if focus != NONE && self.tree.is_valid(focus) {
            if self.tree.get(focus).is_file() {
                focus = self.tree.logical_parent(focus);
            }
        } else {
            focus = NONE;
        }
        if focus == self.tree.root {
            focus = NONE;
        }
        let query = if self.search_active && !self.looks_like_path() {
            self.search_text.to_lowercase().split_whitespace().map(str::to_owned).collect()
        } else {
            Vec::new()
        };

        let reach = 80.0 * self.scale;
        let mut walk = Walk {
            zoom: z,
            ox: self.camera.ox.value,
            oy: self.camera.oy.value,
            margin: [-reach, -reach, self.width + reach, self.height + reach],
            time: self.time,
            wind: self.wind.value * (1.5 * self.scale / z).min(1.0),
            scale: self.scale,
            dt,
            needs_more: false,
            selected: self.selected,
            hover: self.hover,
            focus,
            query,
        };

        let root = self.tree.root;
        let root_on_path = self.selected != NONE && self.selected != root;
        self.walk_node(&mut walk, root, [0.0, -ROOT_SINK], 0.0, 0.0, f32::INFINITY, 0, focus == NONE, root_on_path, 1.0);

        // Falling petals.
        for particle in &self.particles {
            let fade = particle.life.clamp(0.0, 1.0);
            self.draw.top_flowers.push(FlowerInst {
                pos: [walk.ox + particle.pos[0] * z, walk.oy - particle.pos[1] * z],
                radius: particle.size * z * (0.4 + 0.6 * fade),
                rotation: particle.rotation,
                bloom: 1.0,
                species: p::PETAL,
                seed: particle.species,
                lift: 0.3,
                dim: 0.0,
            });
        }

        self.place_labels(&mut walk);
        walk.needs_more
    }

    #[allow(clippy::too_many_arguments)]
    fn walk_node(
        &mut self,
        w: &mut Walk,
        id: NodeId,
        base: [f32; 2],
        parent_heading: f32,
        parent_end_tangent: f32,
        parent_end_width: f32,
        depth: u32,
        in_focus: bool,
        ancestor_of_selection: bool,
        scale: f32,
    ) {
        let (kind, seed, expanded, load, weight) = {
            let n = self.tree.get(id);
            (n.kind, n.seed, n.expanded, n.load, n.weight)
        };
        let is_file = matches!(kind, Kind::File(_));
        let in_focus = in_focus || id == w.focus;

        let n = self.tree.get(id);
        let grow = n.grow.value.clamp(0.0, 1.2);
        let length = n.a_length.value * grow.max(0.0) * scale;
        let inner_scale = scale * n.subscale;
        let bloom = n.bloom.value;
        let lift = n.lift.value;
        let selected = id == w.selected;
        let on_path = ancestor_of_selection && self.tree.is_ancestor_or_self(id, w.selected);

        // The breeze is strongest out on thin twigs.
        let flex = if is_file { 1.0 } else { (1.0 / (1.0 + weight as f32 * 0.05)).max(0.12) };
        let sway = (w.time * (0.6 + rand01(seed, 11) * 0.5) + rand01(seed, 12) * 6.28).sin() * 0.032 * flex * w.wind;
        let heading = parent_heading + n.a_theta.value + sway;
        let (sh, ch) = heading.sin_cos();
        let tip = [base[0] + sh * length, base[1] + ch * length];
        // Limbs arrive at a slight angle to their own line, so no branch looks ruled.
        let lean = if is_file { 0.25 } else { 0.5 };
        let mut deferred: Option<(u8, BranchInst)> = None;
        let end_tangent = heading + (rand01(seed, 5) - 0.5) * lean;

        // Thickness follows the amount of tree it carries, as real limbs do.
        let thick = limb_thickness(kind, weight) * scale;
        let w_start = thick.min(parent_end_width);
        // A limb narrows to the size of its largest branch, so the main line flows through.
        let w_end = if is_file {
            thick * 0.55
        } else {
            let largest = self.tree.get(id).children.iter().fold(0.0f32, |m, c| {
                let child = self.tree.get(*c);
                if child.is_file() { m } else { m.max(limb_thickness(child.kind, child.weight)) }
            });
            (largest * inner_scale).clamp(thick * 0.62, thick)
        };

        {
            let node = self.tree.get_mut(id);
            node.base = base;
            node.tip = tip;
            node.heading = heading;
            node.width = thick;
        }

        // Skip limbs entirely outside the window, along with everything they carry.
        let (center_off, radius) = {
            let n = self.tree.get(id);
            (n.ext_center, n.ext_radius)
        };
        let (center_off, radius) = (center_off * scale, radius * scale);
        let center = [tip[0] + sh * center_off, tip[1] + ch * center_off];
        let sx = w.ox + center[0] * w.zoom;
        let sy = w.oy - center[1] * w.zoom;
        let sr = radius * w.zoom + w_start * w.zoom;
        let base_sx = w.ox + base[0] * w.zoom;
        let base_sy = w.oy - base[1] * w.zoom;
        let base_visible = base_sx > w.margin[0]
            && base_sx < w.margin[2]
            && base_sy > w.margin[1]
            && base_sy < w.margin[3];
        if !base_visible
            && (sx + sr < w.margin[0] || sx - sr > w.margin[2] || sy + sr < w.margin[1] || sy - sr > w.margin[3])
        {
            return;
        }

        // How far back this sits from the current focus or search.
        let matches = !w.query.is_empty() && {
            let lower = self.tree.get(id).name.to_lowercase();
            w.query.iter().all(|q| lower.contains(q.as_str()))
        };
        // A raised flower is one that's been picked out, which never recedes.
        let picked = self.tree.get(id).lift.target >= 1.0;
        let dim_target = if !w.query.is_empty() {
            if matches || selected { 0.0 } else { 0.62 }
        } else if in_focus || on_path || picked {
            0.0
        } else {
            0.32
        };
        let dim = {
            let node = self.tree.get_mut(id);
            let delta = dim_target - node.dim;
            if delta.abs() > 0.004 {
                node.dim += delta * (1.0 - (-w.dt * 11.0).exp());
                w.needs_more = true;
            } else {
                node.dim = dim_target;
            }
            node.dim
        };

        let tip_s = [w.ox + tip[0] * w.zoom, w.oy - tip[1] * w.zoom];
        let base_s = [base_sx, base_sy];

        // ---- the strip of paper
        if length > 0.01 && grow > 0.001 {
            let k = length * 0.45;
            let (psh, pch) = parent_end_tangent.sin_cos();
            let (esh, ech) = end_tangent.sin_cos();
            let c1 = [base[0] + psh * k, base[1] + pch * k];
            let c2 = [tip[0] - esh * k, tip[1] - ech * k];
            let to_s = |q: [f32; 2]| [w.ox + q[0] * w.zoom, w.oy - q[1] * w.zoom];
            // Out in a fine crown the flowers should carry the colour, not their stems.
            // Zoom in and they're paper again.
            let fine = FLOWER_RADIUS * scale * w.zoom < 5.0 * w.scale
                && (is_file || matches!(kind, Kind::Sprig));
            let (min0, min1) = if fine {
                (1.1, 0.8)
            } else if is_file {
                (1.9, 1.3)
            } else {
                (2.2, 1.8)
            };
            let width0 = (w_start * w.zoom).max(min0 * w.scale);
            let width1 = (w_end * w.zoom).max(min1 * w.scale);
            let screen_len = length * w.zoom;
            let paper = mix_rgb(rgb(p::BRANCH), rgb(p::BRANCH_DIM), (dim * 1.4).min(1.0));
            let color = if fine { mix_rgb(paper, rgb(p::GROUND), 0.25) } else { paper };
            let inst = |segments: u32, color: [f32; 4]| BranchInst {
                p0: base_s,
                c1: to_s(c1),
                c2: to_s(c2),
                p1: tip_s,
                w: [width0, width1, grow.min(1.0), rand01(seed, 6)],
                misc: [segments as f32, lift, dim, 0.0],
                color,
            };
            // Pushed after the children, so within a layer a parent lies over its branches.
            if on_path && !is_file || (on_path && selected) {
                deferred = Some((3, inst(LONG_SEGMENTS, rgb(p::PATH))));
            } else if fine && screen_len < 70.0 * w.scale {
                if screen_len > 1.0 {
                    deferred = Some((0, inst(SHORT_SEGMENTS, color)));
                }
            } else if screen_len > 70.0 * w.scale {
                deferred = Some((2, inst(LONG_SEGMENTS, color)));
            } else if screen_len > 1.5 {
                deferred = Some((1, inst(SHORT_SEGMENTS, color)));
            }

            // Labels keep off file stems as well as limbs.
            if is_file && FLOWER_RADIUS * scale * w.zoom > 4.0 * w.scale && screen_len > 16.0 * w.scale {
                let samples = ((screen_len / (11.0 * w.scale)) as usize).clamp(2, 24);
                for s in 1..samples {
                    let q = bezier(base, c1, c2, tip, s as f32 / samples as f32);
                    self.stem_marks.push([w.ox + q[0] * w.zoom, w.oy - q[1] * w.zoom]);
                }
            }

            // Leaves on limbs and on the larger twigs.
            if !is_file && screen_len > 34.0 * w.scale && (!matches!(kind, Kind::Sprig) || scale > 0.6) {
                self.add_leaves(w, id, base, c1, c2, tip, length, grow, dim, scale);
            }

            // Pointer targets along the body of the branch.
            if !is_file && screen_len > 24.0 * w.scale {
                let r = (thick * w.zoom * 0.5 + 6.0 * w.scale).max(9.0 * w.scale);
                let samples = ((screen_len / (r * 1.4)) as usize).clamp(2, 28);
                for s in 1..samples {
                    let t = s as f32 / samples as f32;
                    let q = bezier(base, c1, c2, tip, t);
                    self.visible.push(Visible {
                        id: if matches!(kind, Kind::Sprig) { self.nearest_dir(id) } else { id },
                        x: w.ox + q[0] * w.zoom,
                        y: w.oy - q[1] * w.zoom,
                        r,
                        rank: 3,
                    });
                }
            }
        }

        // ---- flower, or bud at the tip of a folder
        if is_file {
            let n = self.tree.get(id);
            let species = n.species().index() as f32;
            let scale_up = 1.0 + lift * 0.28;
            let radius = FLOWER_RADIUS * scale * w.zoom * scale_up * grow.min(1.0);
            // A flower too small to draw as petals becomes a generous dot, so a crown of
            // thousands reads as a mass of colour rather than as bare stems.
            let dot_at = 5.0 * w.scale;
            let drawn = if radius < dot_at { radius * (1.0 + (1.0 - radius / dot_at) * 1.1) } else { radius };
            let inst = FlowerInst {
                pos: tip_s,
                radius: drawn.max(1.8 * w.scale),
                rotation: rand01(seed, 7) * 6.283,
                bloom: bloom.clamp(0.0, 1.0),
                species,
                seed: rand01(seed, 8) * 40.0,
                lift: lift.min(0.8),
                dim: if self.acts.cut_nodes.contains(&id) || self.acts.drag.as_ref().is_some_and(|d| d.ids.contains(&id)) {
                    dim.max(0.6)
                } else {
                    dim
                },
            };
            if selected || id == w.hover || lift > 0.5 {
                self.draw.top_flowers.push(inst);
            } else {
                self.draw.flowers.push(inst);
            }
            if bloom > 0.15 {
                self.visible.push(Visible {
                    id,
                    x: tip_s[0],
                    y: tip_s[1],
                    r: (radius * 1.15).max(6.0 * w.scale),
                    rank: 0,
                });
            }
            let priority = if selected {
                1000.0
            } else if matches {
                600.0
            } else {
                40.0 + if in_focus { 90.0 } else { 0.0 } + radius / w.scale
            };
            let big_enough = radius > 8.5 * w.scale || selected || id == w.hover || matches;
            // A label waits until the stem carrying it has arrived.
            if big_enough && grow > 0.92 {
                self.label_cands.push(LabelCand {
                    id,
                    x: tip_s[0],
                    y: tip_s[1],
                    gap_x: radius + 8.0 * w.scale,
                    gap_y: radius * 0.83 + 7.0 * w.scale,
                    back: [0.0, 0.0],
                    order: if sh >= 0.0 { [0, 1, 2, 3] } else { [1, 0, 2, 3] },
                    priority,
                    target: 1.0 - dim * 0.75,
                });
            } else {
                self.fade_label(id, w);
            }
        } else if !matches!(kind, Kind::Sprig) {
            // A closed bud sits on the tip until the folder opens.
            let openness = bloom.clamp(0.0, 1.0);
            let bud_scale = (1.0 - openness) * grow.min(1.0);
            let pulse = if load == Load::Loading && expanded {
                w.needs_more = true;
                1.0 + (w.time * 6.0).sin() * 0.08
            } else {
                1.0
            };
            let failed = load == Load::Failed;
            let empty = expanded && load == Load::Loaded && self.tree.get(id).children.is_empty();
            let bud_size = 15.0 * scale * pulse * (1.0 + lift * 0.14);
            if bud_scale > 0.01 || empty || failed {
                let size = bud_size * w.zoom * bud_scale.max(if empty || failed { 0.7 } else { 0.0 });
                // A closed disk is a little blossom. A closed folder is a bud.
                let drive = self.is_drive(id);
                let forward = size * if drive { 0.5 } else { 0.86 };
                let inst = FlowerInst {
                    pos: [tip_s[0] + sh * forward, tip_s[1] - ch * forward],
                    radius: size.max(1.5 * w.scale),
                    rotation: if drive { rand01(seed, 7) * 6.283 } else { heading - std::f32::consts::FRAC_PI_2 },
                    bloom: if drive { grow.clamp(0.0, 1.0) } else { 1.0 },
                    species: if drive { p::BLOSSOM } else { p::BUD },
                    seed: rand01(seed, 9) * 40.0,
                    lift,
                    dim: if failed { 0.75 } else { dim },
                };
                self.draw.flowers.push(inst);
            }
            let r = (24.0 * w.zoom).max(11.0 * w.scale);
            self.visible.push(Visible { id, x: tip_s[0], y: tip_s[1], r, rank: 1 });
            let screen_size = thick * w.zoom;
            let priority = if selected {
                1100.0
            } else if on_path {
                800.0
            } else if matches {
                700.0
            } else {
                200.0 + screen_size.min(40.0) * 2.0 - depth as f32 * 3.0 + if in_focus { 150.0 } else { 0.0 }
            };
            let is_root = self.tree.get(id).parent == NONE;
            let half = thick * w.zoom * 0.5;
            let bud_showing = bud_scale.max(if empty || failed { 0.7 } else { 0.0 });
            let (x, y, gap_x, gap_y, order) = if is_root {
                // Beside the trunk, where it never fights the fork above.
                let mid = [(tip_s[0] + base_s[0]) * 0.5, (tip_s[1] + base_s[1]) * 0.5];
                (mid[0], mid[1], half + 9.0 * w.scale, 6.0 * w.scale, [0, 1, 2, 3])
            } else if bud_showing > 0.3 {
                // A closed bud carries its name above it, clear of the whole bud.
                let reach = 15.0 * scale * w.zoom * bud_showing;
                let at = [tip_s[0] + sh * reach, tip_s[1] - ch * reach];
                (at[0], at[1], (reach + 6.0 * w.scale).max(9.0 * w.scale), (reach + 2.0 * w.scale).max(7.0 * w.scale), [2, 0, 1, 3])
            } else {
                // An open fork carries its name to the side, clear of the limbs leaving it.
                let order = if sh >= 0.0 { [0, 1, 3, 2] } else { [1, 0, 3, 2] };
                (tip_s[0], tip_s[1], half + 8.0 * w.scale, half + 3.0 * w.scale, order)
            };
            // A name appears once its limb has arrived, the same as for flowers.
            if grow > 0.92 {
                let along = (length * w.zoom * 0.4).min(50.0 * w.scale);
                self.label_cands.push(LabelCand {
                    id,
                    x,
                    y,
                    gap_x,
                    gap_y,
                    back: [-sh * along, ch * along],
                    order,
                    priority,
                    target: 1.0 - dim * 0.6,
                });
            } else {
                self.fade_label(id, w);
            }
        }

        // ---- a far-off group of files is drawn as a cluster of real blossoms
        let cluster_r = radius * w.zoom * 0.6;
        if matches!(kind, Kind::Sprig) && !on_path && cluster_r < 26.0 * w.scale {
            self.draw_cluster(w, id, tip_s, [sh, -ch], cluster_r, dim, grow);
            if let Some((tier, inst)) = deferred {
                match tier {
                    0 => self.draw.branches_fine.push(inst),
                    1 => self.draw.branches_short.push(inst),
                    2 => self.draw.branches_long.push(inst),
                    _ => self.draw.path_branches.push(inst),
                }
            }
            return;
        }

        // ---- onward along the tree
        let child_count = self.tree.get(id).children.len();
        for index in 0..child_count {
            let child = self.tree.get(id).children[index];
            self.walk_node(w, child, tip, heading, end_tangent, w_end, depth + 1, in_focus, on_path, inner_scale);
        }

        if let Some((tier, inst)) = deferred {
            match tier {
                0 => self.draw.branches_fine.push(inst),
                1 => self.draw.branches_short.push(inst),
                2 => self.draw.branches_long.push(inst),
                _ => self.draw.path_branches.push(inst),
            }
        }
    }

    /// Seven blossoms, sampled from the files in this group so its colours are honest,
    /// packed the way seeds sit in a flower head.
    #[allow(clippy::too_many_arguments)]
    fn draw_cluster(&mut self, w: &Walk, id: NodeId, tip: [f32; 2], dir: [f32; 2], r: f32, dim: f32, grow: f32) {
        const SHOWN: usize = 7;
        let mut samples: Vec<NodeId> = Vec::with_capacity(SHOWN);
        self.sample_files(id, SHOWN, &mut samples);
        if samples.is_empty() {
            return;
        }
        let r = r.max(4.0 * w.scale);
        let center = [tip[0] + dir[0] * r * 0.72, tip[1] + dir[1] * r * 0.72];
        let each = (r * 0.46).max(2.4 * w.scale) * grow.min(1.0);
        let count = samples.len() as f32;
        for (i, file) in samples.iter().enumerate() {
            let n = self.tree.get(*file);
            let angle = i as f32 * 2.399963 + rand01(n.seed, 3);
            let dist = r * ((i as f32 + 0.5) / count).sqrt();
            self.draw.flowers.push(FlowerInst {
                pos: [center[0] + angle.cos() * dist, center[1] + angle.sin() * dist],
                radius: each,
                rotation: rand01(n.seed, 7) * 6.283,
                bloom: if n.is_file() { n.bloom.value.clamp(0.0, 1.0) } else { 1.0 },
                species: if n.is_file() { n.species().index() as f32 } else { p::BUD },
                seed: rand01(n.seed, 8) * 40.0,
                lift: 0.0,
                dim,
            });
        }
        self.visible.push(Visible { id, x: center[0], y: center[1], r: (r * 1.15).max(8.0 * w.scale), rank: 2 });
    }

    fn sample_files(&self, id: NodeId, limit: usize, out: &mut Vec<NodeId>) {
        for &child in &self.tree.get(id).children {
            if out.len() >= limit {
                return;
            }
            let node = self.tree.get(child);
            if node.is_sprig() {
                self.sample_files(child, limit, out);
            } else if !node.dying {
                out.push(child);
            }
        }
    }

    fn nearest_dir(&self, mut id: NodeId) -> NodeId {
        while self.tree.get(id).is_sprig() {
            id = self.tree.get(id).parent;
        }
        id
    }

    fn fade_label(&mut self, id: NodeId, w: &mut Walk) {
        let node = self.tree.get_mut(id);
        if node.label_alpha > 0.0 {
            node.label_alpha = (node.label_alpha - w.dt * 7.0).max(0.0);
            w.needs_more = true;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn add_leaves(
        &mut self,
        w: &Walk,
        id: NodeId,
        base: [f32; 2],
        c1: [f32; 2],
        c2: [f32; 2],
        tip: [f32; 2],
        length: f32,
        grow: f32,
        dim: f32,
        scale: f32,
    ) {
        let seed = self.tree.get(id).seed;
        let count = ((length / (58.0 * scale.max(0.05))) as u32).clamp(1, 6);
        // Where several limbs leave one fork, leaves at the base just clump together.
        let parent = self.tree.get(id).parent;
        let bare_until = if parent != NONE {
            match self.tree.get(parent).children.len() {
                0..=2 => 0.0,
                3..=5 => 0.28,
                _ => 0.45,
            }
        } else {
            0.0
        };
        for i in 0..count {
            let t = (i as f32 + 0.35 + rand01(seed, 20 + i) * 0.4) / count as f32;
            // The trunk stays bare where it leaves the soil.
            if t > grow || parent == NONE || t < bare_until {
                continue;
            }
            let at = bezier(base, c1, c2, tip, t);
            let side = if (seed.wrapping_add(i)) & 1 == 0 { 1.0 } else { -1.0 };
            let heading = self.tree.get(id).heading;
            let angle = heading + side * (0.75 + rand01(seed, 30 + i) * 0.5);
            // The leaf blade is centred on its own midpoint, so walk out half its length.
            let size = (11.0 + rand01(seed, 40 + i) * 7.0) * grow.min(1.0) * scale;
            let (sa, ca) = angle.sin_cos();
            // Start from the edge of the branch, so a blade never lies across a thick limb.
            let out = size + self.tree.get(id).width * 0.42;
            let center = [at[0] + sa * out, at[1] + ca * out];
            let radius = size * w.zoom;
            if radius < 2.0 * w.scale {
                continue;
            }
            // Labels would rather not sit on a leaf, though a name matters more than a leaf does.
            let spot = [w.ox + center[0] * w.zoom, w.oy - center[1] * w.zoom];
            self.stem_marks.push(spot);
            self.draw.leaves.push(FlowerInst {
                pos: spot,
                radius,
                rotation: angle - std::f32::consts::FRAC_PI_2,
                bloom: grow.clamp(0.0, 1.0),
                species: p::LEAF,
                seed: rand01(seed, 50 + i) * 40.0,
                lift: 0.0,
                dim,
            });
        }
    }

    /// Shows the labels. Where they go is decided once, when the tree comes to rest or
    /// something changes what deserves a name. While anything moves, each label simply
    /// stays on its side, so names never hunt during growth or shift in the breeze.
    /// Hovering names a flower without disturbing anything else.
    fn place_labels(&mut self, w: &mut Walk) {
        let moving = self.layout_moving || self.dragging;
        let solve = !moving && (self.was_moving || self.labels_dirty);
        self.was_moving = moving;
        if solve {
            self.labels_dirty = false;
        }

        let mut cands = std::mem::take(&mut self.label_cands);
        cands.sort_unstable_by(|a, b| b.priority.total_cmp(&a.priority));
        let s = w.scale;
        let stage = self.stage();

        // A coarse occupancy grid over the window: checking a label against thousands of
        // obstacles stays a handful of cell lookups.
        let cell = 5.0 * s;
        let cols = (self.width / cell) as usize + 2;
        let rows = (self.height / cell) as usize + 2;
        let mut grid = std::mem::take(&mut self.label_grid);
        let span = |r: [f32; 4]| -> (usize, usize, usize, usize) {
            let to = |v: f32, max: usize| ((v / cell).max(0.0) as usize).min(max - 1);
            (to(r[0], cols), to(r[1], rows), to(r[2], cols), to(r[3], rows))
        };
        const SOFT: u8 = 1; // limbs and stems: a folder's name may cross one if it must
        const HARD: u8 = 2; // flowers, labels, and controls: never
        const HIDDEN: u8 = 254; // the last solve found no room for this label
        let mark = |grid: &mut Vec<u8>, r: [f32; 4], bit: u8| {
            let (x0, y0, x1, y1) = span(r);
            for y in y0..=y1 {
                for cell in &mut grid[y * cols + x0..=y * cols + x1] {
                    *cell |= bit;
                }
            }
        };
        let free = |grid: &[u8], r: [f32; 4], mask: u8| -> bool {
            let (x0, y0, x1, y1) = span(r);
            (y0..=y1).all(|y| grid[y * cols + x0..=y * cols + x1].iter().all(|c| c & mask == 0))
        };

        let mut flowers: Vec<[f32; 2]> = Vec::new();
        let mut tips: Vec<[f32; 2]> = Vec::new();
        if solve {
            self.label_boxes.clear();
            grid.clear();
            grid.resize(cols * rows, 0);
            for v in &self.visible {
                let (half, bit) = match v.rank {
                    0 if v.r > 7.0 * s => {
                        flowers.push([v.x, v.y]);
                        (v.r * 0.72, HARD)
                    }
                    1 => {
                        tips.push([v.x, v.y]);
                        // As wide as the bud that may be sitting here.
                        ((v.r * 0.6).max(5.0 * s), HARD)
                    }
                    2 => (v.r * 0.8, HARD),
                    3 => ((v.r - 5.0 * s).max(2.0 * s), SOFT),
                    _ => continue,
                };
                mark(&mut grid, [v.x - half, v.y - half, v.x + half, v.y + half], bit);
            }
            for q in &self.stem_marks {
                let half = 2.0 * s;
                mark(&mut grid, [q[0] - half, q[1] - half, q[0] + half, q[1] + half], SOFT);
            }
            // Nothing runs under the interface: the status tab, the corner buttons, and so on.
            for zone in &self.zones {
                mark(&mut grid, zone.rect, HARD);
            }
            if flowers.len() > 300 {
                flowers.clear();
            }
        }

        let mut placed = 0;
        let mut unplaced = false;
        const MAX_LABELS: usize = 260;

        for cand in &cands {
            let (selected, hovered) = (cand.id == w.selected, cand.id == w.hover);
            let (name, is_file, count_hint, load, kind, previous) = {
                let n = self.tree.get(cand.id);
                (n.name.to_string(), n.is_file(), n.file_count + n.dir_count, n.load, n.kind, n.label_slot)
            };
            let on_path = self.tree.is_valid(w.selected) && self.tree.is_ancestor_or_self(cand.id, w.selected);
            let (face, size) = if selected || hovered || !is_file {
                (Face::UiBold, if selected { 15.0 } else { 13.5 } * s)
            } else {
                (Face::Ui, 12.0 * s)
            };
            let shown = self.drive_info.get(&name).filter(|_| self.is_drive(cand.id)).map(|d| d.display.clone());
            let text = fit_label(&mut self.text, shown.as_deref().unwrap_or(&name), face, size, 230.0 * s);
            let width = self.text.width(&text, face, size);
            let show_count = !is_file && load == Load::Unloaded && kind == Kind::Dir && count_hint > 0;
            let total = width + if show_count { 28.0 * s } else { 0.0 };
            let height = size * 1.25;
            let at = |slot: u8| -> (f32, f32) {
                match slot {
                    0 => (cand.x + cand.gap_x, cand.y - height * 0.5),
                    1 => (cand.x - cand.gap_x - total, cand.y - height * 0.5),
                    2 => (cand.x - total * 0.5, cand.y - cand.gap_y - height),
                    3 => (cand.x - total * 0.5, cand.y + cand.gap_y),
                    8 => (cand.x - total * 0.5, cand.y - cand.gap_y - height * 2.0),
                    9 => (cand.x - total * 0.5, cand.y + cand.gap_y + height),
                    10 => (cand.x + cand.back[0] + cand.gap_x, cand.y + cand.back[1] - height * 0.5),
                    11 => (cand.x + cand.back[0] - cand.gap_x - total, cand.y + cand.back[1] - height * 0.5),
                    12 => (cand.x + cand.back[0] * 2.0 + cand.gap_x, cand.y + cand.back[1] * 2.0 - height * 0.5),
                    13 => (cand.x + cand.back[0] * 2.0 - cand.gap_x - total, cand.y + cand.back[1] * 2.0 - height * 0.5),
                    14 => (cand.x - total * 0.5, cand.y - cand.gap_y - height * 3.0),
                    15 => (cand.x - total * 0.5, cand.y + cand.gap_y + height * 2.0),
                    4 => (cand.x + cand.gap_x * 0.6, cand.y - cand.gap_y - height),
                    5 => (cand.x - cand.gap_x * 0.6 - total, cand.y - cand.gap_y - height),
                    6 => (cand.x + cand.gap_x * 0.6, cand.y + cand.gap_y),
                    _ => (cand.x - cand.gap_x * 0.6 - total, cand.y + cand.gap_y),
                }
            };
            let box_of = |pos: (f32, f32)| [pos.0 - 3.0 * s, pos.1 - 1.0 * s, pos.0 + total + 3.0 * s, pos.1 + height + 1.0 * s];
            let on_stage = |r: [f32; 4]| r[0] >= stage[0] && r[2] <= stage[2] && r[1] >= stage[1] && r[3] <= stage[3];

            // A file's name must plainly belong to its own flower. A name level with its
            // flower is attached by alignment, so it's allowed closer to a neighbour.
            let own_nearest = |r: [f32; 4], slot: u8| -> bool {
                let dist = |x: f32, y: f32| {
                    let dx = (r[0] - x).max(x - r[2]).max(0.0);
                    let dy = (r[1] - y).max(y - r[3]).max(0.0);
                    (dx * dx + dy * dy).sqrt()
                };
                // Beside the incoming limb, a folder's name belongs to that point of the limb.
                let own = match slot {
                    10 | 11 => dist(cand.x + cand.back[0], cand.y + cand.back[1]),
                    12 | 13 => dist(cand.x + cand.back[0] * 2.0, cand.y + cand.back[1] * 2.0),
                    _ => dist(cand.x, cand.y),
                };
                let allowance = if slot <= 1 { 0.7 } else { 0.85 };
                let neighbours = if is_file { &flowers } else { &tips };
                !neighbours.iter().any(|f| {
                    let other = (f[0] - cand.x).abs() + (f[1] - cand.y).abs() > 0.5;
                    other && dist(f[0], f[1]) < own * allowance
                })
            };
            // Rows further out hang from a little stem, which has to have a clear run.
            let threaded = |slot: u8| is_file && matches!(slot, 8 | 9 | 14 | 15);
            let stem_run = |slot: u8, r: [f32; 4]| -> [f32; 4] {
                let clearance = 6.0 * s;
                if matches!(slot, 8 | 14) {
                    [cand.x - 1.0, r[3], cand.x + 1.0, cand.y - cand.gap_y + 2.0 * s - clearance]
                } else {
                    [cand.x - 1.0, cand.y + cand.gap_y - 2.0 * s + clearance, cand.x + 1.0, r[1]]
                }
            };

            let important = selected || hovered;
            let mut chosen = None;
            if solve {
                if placed < MAX_LABELS {
                    // Folder names matter more than keeping clear of limbs, so a folder with no
                    // clean spot tries again, allowed to cross a limb. Its halo keeps it legible.
                    let masks: &[u8] = if is_file { &[SOFT | HARD] } else { &[SOFT | HARD, HARD] };
                    let further: &[u8] = if is_file { &[8, 14, 9, 15] } else { &[10, 11, 12, 13, 4, 5, 6, 7, 8, 9] };
                    'search: for &mask in masks {
                        for slot in cand.order.iter().chain(further).copied() {
                            let r = box_of(at(slot));
                            let clear = if threaded(slot) {
                                free(&grid, stem_run(slot, r), HARD)
                            } else {
                                own_nearest(r, slot)
                            };
                            if on_stage(r) && free(&grid, r, mask) && clear {
                                chosen = Some(slot);
                                break 'search;
                            }
                        }
                    }
                }
                // The selection is always named, whatever it takes.
                if chosen.is_none() && selected {
                    chosen = Some(cand.order[0]);
                }
                self.tree.get_mut(cand.id).label_slot = chosen.unwrap_or(HIDDEN);
            } else if previous <= 15 {
                if on_stage(box_of(at(previous))) {
                    chosen = Some(previous);
                }
            } else if previous != HIDDEN {
                chosen = Some(cand.order[0]);
                if !moving {
                    unplaced = true;
                }
            }
            // A hovered flower is always named, in the first spot clear of the settled labels.
            if chosen.is_none() && selected {
                chosen = Some(cand.order[0]);
            }
            let mut slot = chosen.unwrap_or(if previous <= 15 { previous } else { cand.order[0] });
            if chosen.is_none() && hovered {
                let options = [cand.order[0], cand.order[1], cand.order[2], cand.order[3], 8, 14, 9, 15];
                let clear = |o: u8, avoid_stems: bool| -> bool {
                    let r = box_of(at(o));
                    let overlaps = |b: &[f32; 4]| r[0] < b[2] && b[0] < r[2] && r[1] < b[3] && b[1] < r[3];
                    let on_flower = self.visible.iter().any(|v| {
                        v.rank == 0
                            && v.id != cand.id
                            && overlaps(&[v.x - v.r * 0.8, v.y - v.r * 0.8, v.x + v.r * 0.8, v.y + v.r * 0.8])
                    });
                    let on_stem = avoid_stems
                        && self.stem_marks.iter().any(|q| q[0] > r[0] && q[0] < r[2] && q[1] > r[1] && q[1] < r[3]);
                    on_stage(r) && !on_flower && !on_stem && !self.label_boxes.iter().any(overlaps)
                };
                if let Some(found) = options
                    .iter()
                    .copied()
                    .find(|o| clear(*o, true))
                    .or_else(|| options.iter().copied().find(|o| clear(*o, false)))
                {
                    slot = found;
                }
                chosen = Some(slot);
            }
            let (mut x0, mut y0) = at(slot);
            if important {
                // What's being pointed at or selected stays inside the stage.
                x0 = x0.clamp(stage[0] + 4.0 * s, (stage[2] - total - 4.0 * s).max(stage[0] + 4.0 * s));
                y0 = y0.clamp(stage[1] + 4.0 * s, (stage[3] - height - 4.0 * s).max(stage[1] + 4.0 * s));
            }
            let (x0_target, y0_target) = (x0, y0);

            let node = self.tree.get_mut(cand.id);
            // A label that has to move glides there, and one just appearing starts in place.
            let aim = [x0 - cand.x, y0 - cand.y];
            if node.label_alpha < 0.05 {
                node.label_off = aim;
            } else {
                let ease = 1.0 - (-w.dt * 12.0).exp();
                node.label_off[0] += (aim[0] - node.label_off[0]) * ease;
                node.label_off[1] += (aim[1] - node.label_off[1]) * ease;
                if (aim[0] - node.label_off[0]).abs() + (aim[1] - node.label_off[1]).abs() > 0.3 {
                    w.needs_more = true;
                } else {
                    node.label_off = aim;
                }
            }
            let (x0, y0) = (cand.x + node.label_off[0], cand.y + node.label_off[1]);
            let goal = if chosen.is_some() { cand.target } else { 0.0 };
            let delta = goal - node.label_alpha;
            if delta.abs() > 0.01 {
                node.label_alpha += delta.signum() * delta.abs().min(w.dt * 7.5);
                w.needs_more = true;
            } else {
                node.label_alpha = goal;
            }
            let alpha = node.label_alpha;
            if solve && chosen.is_some() {
                let placed_box = box_of((x0_target, y0_target));
                mark(&mut grid, placed_box, HARD);
                self.label_boxes.push(placed_box);
                if threaded(slot) {
                    mark(&mut grid, stem_run(slot, placed_box), HARD);
                }
                placed += 1;
            }
            if alpha <= 0.01 {
                continue;
            }
            let color = if selected || (on_path && !is_file) { rgb(p::PATH) } else { rgb(p::CREAM) };
            let color = [color[0], color[1], color[2], alpha];
            let halo = rgb(p::GROUND_DEEP);
            let halo = [halo[0], halo[1], halo[2], alpha];
            let baseline = y0 + height * 0.5 + size * 0.36;
            if threaded(slot) && !important && alpha > 0.2 {
                // Too far from its flower to read on its own, the name gets a little stem:
                // cut from the same paper as the others, and curved and tapered like them.
                let r = box_of((x0_target, y0_target));
                let upward = matches!(slot, 8 | 14);
                let start = [cand.x, if upward { cand.y - cand.gap_y + 2.0 * s } else { cand.y + cand.gap_y - 2.0 * s }];
                let end = [cand.x, if upward { r[3] + 1.0 * s } else { r[1] - 1.0 * s }];
                let seed = self.tree.get(cand.id).seed;
                let sway = (rand01(seed, 60) - 0.5) * 9.0 * s;
                let dim = self.tree.get(cand.id).dim;
                let paper = mix_rgb(rgb(p::BRANCH), rgb(p::BRANCH_DIM), (dim * 1.4).min(1.0));
                let rise = end[1] - start[1];
                self.draw.branches_short.push(BranchInst {
                    p0: start,
                    c1: [start[0] + sway, start[1] + rise * 0.35],
                    c2: [end[0] + sway * 0.6, end[1] - rise * 0.3],
                    p1: end,
                    w: [1.9 * s * alpha, 1.0 * s * alpha, 1.0, rand01(seed, 61)],
                    misc: [SHORT_SEGMENTS as f32, 0.0, dim, 0.0],
                    color: paper,
                });
            }
            draw_text(&mut self.text, &mut self.draw.label_halos, &text, face, size, x0, baseline, [0.0; 4], halo);
            draw_text(&mut self.text, &mut self.draw.labels, &text, face, size, x0, baseline, color, [0.0; 4]);

            // A closed folder says how much waits inside, once that's known.
            if show_count {
                let note = format!("{count_hint}");
                let color = [color[0], color[1], color[2], alpha * 0.8];
                let nx = x0 + width + 5.0 * s;
                draw_text(&mut self.text, &mut self.draw.label_halos, &note, Face::Ui, 11.0 * s, nx, baseline, [0.0; 4], halo);
                draw_text(&mut self.text, &mut self.draw.labels, &note, Face::Ui, 11.0 * s, nx, baseline, color, [0.0; 4]);
            }
        }
        if unplaced {
            self.labels_dirty = true;
            w.needs_more = true;
        }
        self.label_cands = cands;
        self.label_grid = grid;
    }
}

fn limb_thickness(kind: Kind, weight: u32) -> f32 {
    match kind {
        Kind::File(_) => 3.2,
        Kind::Sprig => 1.8 + (weight as f32).sqrt() * 1.0,
        _ => (4.0 + (weight as f32).sqrt() * 2.1).min(60.0),
    }
}

#[inline]
fn bezier(p0: [f32; 2], c1: [f32; 2], c2: [f32; 2], p1: [f32; 2], t: f32) -> [f32; 2] {
    let u = 1.0 - t;
    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    [a * p0[0] + b * c1[0] + c * c2[0] + d * p1[0], a * p0[1] + b * c1[1] + c * c2[1] + d * p1[1]]
}

pub fn mix_rgb(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t, a[3] + (b[3] - a[3]) * t]
}

/// Shortens text to fit, keeping the end of a filename because that's where it differs.
pub fn fit_label(text: &mut Text, name: &str, face: Face, size: f32, max: f32) -> String {
    if text.width(name, face, size) <= max {
        return name.to_string();
    }
    let chars: Vec<char> = name.chars().collect();
    let (mut lo, mut hi) = (1usize, chars.len());
    let render = |keep: usize| -> String {
        let tail = (keep / 3).max(1).min(keep);
        let head = keep - tail;
        let mut out: String = chars[..head].iter().collect();
        out.push('…');
        out.extend(chars[chars.len() - tail..].iter());
        out
    };
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if text.width(&render(mid), face, size) <= max {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    render(lo)
}

#[allow(clippy::too_many_arguments)]
pub fn draw_text(
    text: &mut Text,
    out: &mut Vec<GlyphInst>,
    string: &str,
    face: Face,
    size: f32,
    x: f32,
    baseline: f32,
    color: [f32; 4],
    halo: [f32; 4],
) -> f32 {
    let (glyphs, width): (Vec<_>, f32) = {
        let run = text.run(string, face, size);
        (run.glyphs.to_vec(), run.width)
    };
    let baseline = baseline.round();
    for g in glyphs {
        let gx = x + g.x;
        let whole = gx.floor();
        let sub = ((gx - whole) * 4.0).floor() as u8 & 3;
        let Some(atlas) = text.glyph(g.font, g.id, face, size, sub) else { continue };
        out.push(GlyphInst {
            pos: [whole + atlas.left as f32, baseline - g.y - atlas.top as f32],
            size: [atlas.width as f32, atlas.height as f32],
            uv: [atlas.uv[0] as f32, atlas.uv[1] as f32, atlas.uv[2] as f32, atlas.uv[3] as f32],
            color,
            halo,
        });
    }
    width
}
