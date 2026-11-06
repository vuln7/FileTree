//! Shapes the tree. Each subtree is summarised bottom-up as a fan: how wide it spreads and
//! how far it reaches. A parent shares out its own fan so those can never overlap, and
//! lengthens each limb only as far as its share requires. The tree grows gently with depth
//! rather than doubling at every level, in time linear in the nodes that actually changed.

use crate::tree::{Kind, NodeId, Tree, rand01};
use std::f32::consts::PI;

pub const FLOWER_RADIUS: f32 = 19.0;
const PEDICEL: f32 = 28.0;
const BUD_RADIUS: f32 = 22.0;
const TRUNK: f32 = 160.0;
const SAPLING: f32 = 44.0;
const LIMB: f32 = 62.0;
const SPRIG_STEM: f32 = 38.0;
const GAP: f32 = 9.0;

/// Past this radius a subtree stops getting bigger at the same rate and starts getting
/// finer. Zooming in reveals it at full size.
const CROWN_LIMIT: f32 = 340.0;
const CROWN_POWER: f32 = 0.72;

/// The least share of a fan any one child gets when space is tight, in radians.
const MIN_SHARE: f32 = 0.2;

/// Upper bound on stem extension, far past anything a real folder needs.
const MAX_EXTENSION: f32 = 1.0e6;

pub fn relayout(tree: &mut Tree) {
    let root = tree.root;
    measure(tree, root, 0);
    let node = tree.get_mut(root);
    node.theta = 0.0;
    node.a_theta.target = 0.0;
}

fn fan_angle(depth: u32, count: usize, kind: Kind, largest_child: f32) -> f32 {
    let base = match kind {
        _ if depth == 0 => 150.0,
        // Sprigs open wide, so large folders make round crowns rather than tall poles.
        Kind::Sprig => 172.0,
        _ => 124.0,
    };
    // A lone child or small pair of twigs should not splay wide open.
    let narrow = match count {
        1 => 0.0,
        2 if largest_child < 150.0 => 62.0,
        3 if largest_child < 150.0 => 96.0,
        _ => base,
    };
    f32::min(base, narrow) * PI / 180.0
}

fn measure(tree: &mut Tree, id: NodeId, depth: u32) {
    let node = tree.get(id);
    if !node.dirty {
        return;
    }
    let kind = node.kind;
    let seed = node.seed;
    let open = node.expanded && !node.children.is_empty();

    if !open {
        let (length, margin) = match kind {
            Kind::File(_) => (PEDICEL + rand01(seed, 1) * 8.0, FLOWER_RADIUS + GAP),
            Kind::Sprig => (SPRIG_STEM + rand01(seed, 2) * 22.0, BUD_RADIUS + GAP),
            _ if depth == 0 => (SAPLING, BUD_RADIUS + GAP),
            _ => (LIMB * 0.8 + rand01(seed, 2) * 22.0, BUD_RADIUS + GAP),
        };
        let n = tree.get_mut(id);
        n.length = length;
        n.ext_center = 0.0;
        n.ext_radius = margin;
        n.wedge = 0.0;
        n.reach = 0.0;
        n.margin = margin;
        n.subscale = 1.0;
        n.weight = 1;
        n.dirty = false;
        return;
    }

    let children = std::mem::take(&mut tree.get_mut(id).children);
    let mut weight = 1u32;
    for &child in &children {
        measure(tree, child, depth + 1);
        weight = weight.saturating_add(tree.get(child).weight);
    }

    // Each child's subtree is a fan seen from this tip. All that has to fit around this
    // tip is the angle each fan takes up, and a longer stem makes a fan look narrower.
    // Claiming only that angle, rather than a whole circle, is what keeps the tree from
    // doubling in size with every level of depth.
    struct Fan {
        base: f32,
        reach: f32,
        wedge: f32,
        margin: f32,
    }
    let fans: Vec<Fan> = children
        .iter()
        .map(|&c| {
            let n = tree.get(c);
            Fan { base: base_length(n.kind, n.seed), reach: n.reach, wedge: n.wedge, margin: n.margin }
        })
        .collect();
    let seen = |f: &Fan, length: f32| -> f32 {
        let (sin, cos) = f.wedge.sin_cos();
        let corner = (f.reach * sin).atan2(length + f.reach * cos);
        // A fan wider than its tangent point looks widest at the tangent, not its edge.
        let spread = if f.reach < length && cos < -f.reach / length {
            (f.reach / length).asin()
        } else {
            corner
        };
        spread + (f.margin / length).clamp(0.0, 0.999).asin()
    };

    let largest = fans.iter().fold(0.0f32, |m, f| m.max(f.reach + f.margin));
    let fan = fan_angle(depth, children.len(), kind, largest);

    // Treat the fan as a budget. Everyone gets a fair minimum, the rest goes to whoever
    // carries the most, and each stem then grows only as long as its own share requires.
    let shortest: Vec<f32> = fans.iter().map(|f| f.base.max(f.margin * 1.04)).collect();
    let crowded = children.len() >= 6
        && fans.iter().zip(&shortest).map(|(f, l)| 2.0 * seen(f, *l)).sum::<f32>() > fan;
    let takes = |f: &Fan, length: f32| -> f32 {
        let whole = 2.0 * seen(f, length);
        if crowded && f.reach == 0.0 { whole / 3.0 } else { whole }
    };
    let wants: Vec<f32> = fans.iter().zip(&shortest).map(|(f, l)| takes(f, *l)).collect();
    let mut budget: Vec<f32> = wants.clone();
    if children.len() > 1 && wants.iter().sum::<f32>() > fan {
        // Leave most of the fan to hand out by size, so an open limb among many closed
        // siblings gets the room it needs.
        let floor = (0.3 * fan / children.len() as f32).min(MIN_SHARE);
        budget = wants.iter().map(|w| w.min(floor)).collect();
        let shares: Vec<f32> = fans.iter().map(|f| (f.reach + f.margin).powf(1.5)).collect();
        // Hand out what's left by size. Anyone who reaches what they want drops out, and
        // what they didn't need goes round again.
        for _ in 0..4 {
            let spare = fan - budget.iter().sum::<f32>();
            if spare <= 0.0001 {
                break;
            }
            let hungry: f32 = (0..fans.len()).filter(|i| budget[*i] < wants[*i]).map(|i| shares[i]).sum();
            if hungry <= 0.0 {
                break;
            }
            for i in 0..fans.len() {
                if budget[i] < wants[i] {
                    budget[i] = (budget[i] + spare * shares[i] / hungry).min(wants[i]);
                }
            }
        }
    }
    let lengths: Vec<f32> = fans
        .iter()
        .enumerate()
        .map(|(i, f)| {
            if children.len() <= 1 || takes(f, shortest[i]) <= budget[i] {
                return shortest[i];
            }
            let (mut lo, mut hi) = (shortest[i], shortest[i] * 2.0);
            while takes(f, hi) > budget[i] && hi < MAX_EXTENSION {
                lo = hi;
                hi *= 2.0;
            }
            for _ in 0..18 {
                let mid = 0.5 * (lo + hi);
                if takes(f, mid) > budget[i] {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            hi
        })
        .collect();

    let total: f32 = fans.iter().zip(&lengths).map(|(f, l)| takes(f, *l)).sum();
    let slack = if children.len() > 1 { ((fan - total) / (children.len() - 1) as f32).max(0.0) } else { 0.0 };
    let mut cursor = -(total + slack * (children.len().saturating_sub(1)) as f32) * 0.5;

    let own_length = {
        let n = tree.get(id);
        if depth == 0 {
            // A sapling with a short stem, whose trunk rises as the tree fills out.
            (SAPLING + (weight as f32).sqrt() * 10.0).min(TRUNK)
        } else {
            base_length(n.kind, n.seed)
        }
    };

    let (mut wedge, mut reach, mut margin) = (0.0f32, 0.0f32, 0.0f32);
    let mut row = 0;
    for (i, &child) in children.iter().enumerate() {
        let f = &fans[i];
        let half = takes(f, lengths[i]) * 0.5;
        let staggered = crowded && f.reach == 0.0;
        let length = if staggered { lengths[i] + (row % 3) as f32 * f.margin * 2.2 } else { lengths[i] };
        if staggered {
            row += 1;
        }
        cursor += half;
        let mut theta = cursor;
        cursor += half + slack;
        let n = tree.get_mut(child);
        if children.len() == 1 {
            // A single child leans a little instead of continuing dead straight.
            theta = (rand01(n.seed, 3) - 0.5) * 0.7;
        }
        n.theta = theta;
        n.length = length;
        n.a_theta.target = theta;
        n.a_length.target = length;
        wedge = wedge.max(theta.abs() + seen(f, length));
        reach = reach.max(length + f.reach);
        margin = margin.max(f.margin);
    }

    // Past a certain spread, everything beyond this limb is drawn finer, as on a real tree.
    // That keeps the whole tree in proportion however deep it goes; zooming in shows any
    // part at full size.
    let crown = reach + margin;
    let subscale = if crown > CROWN_LIMIT { (CROWN_LIMIT / crown).powf(CROWN_POWER) } else { 1.0 };

    let n = tree.get_mut(id);
    n.children = children;
    n.length = own_length;
    n.a_length.target = own_length;
    n.wedge = wedge;
    n.reach = reach * subscale;
    n.margin = margin * subscale;
    // Everything this limb carries lies within this circle around its tip.
    n.ext_center = 0.0;
    n.ext_radius = (reach + margin) * subscale;
    // A limb is as thick as what it visibly carries.
    n.weight = (1.0 + (weight.saturating_sub(1)) as f32 * subscale * subscale) as u32;
    n.subscale = subscale;
    n.dirty = false;
}

fn base_length(kind: Kind, seed: u32) -> f32 {
    match kind {
        Kind::File(_) => PEDICEL + rand01(seed, 1) * 8.0,
        Kind::Sprig => SPRIG_STEM + rand01(seed, 2) * 14.0,
        Kind::Dir | Kind::Places => LIMB + rand01(seed, 2) * 34.0,
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::Entry;
    use crate::tree::Load;
    use std::path::PathBuf;

    fn entry(name: String, is_dir: bool) -> Entry {
        Entry { name: name.into(), is_dir, hidden: false, size: 1, modified: 0 }
    }

    fn resolve(tree: &Tree, id: NodeId, base: [f32; 2], heading: f32, out: &mut Vec<(NodeId, [f32; 2])>) {
        walk(tree, id, base, heading, 1.0, out, &mut 1.0f32);
    }

    fn walk(
        tree: &Tree,
        id: NodeId,
        base: [f32; 2],
        heading: f32,
        scale: f32,
        out: &mut Vec<(NodeId, [f32; 2])>,
        smallest: &mut f32,
    ) {
        let node = tree.get(id);
        let h = heading + node.theta;
        let tip = [base[0] + h.sin() * node.length * scale, base[1] + h.cos() * node.length * scale];
        out.push((id, tip));
        *smallest = smallest.min(scale);
        for &child in &node.children {
            walk(tree, child, tip, h, scale * node.subscale, out, smallest);
        }
    }

    fn smallest_scale(tree: &Tree) -> f32 {
        let mut smallest = 1.0;
        let mut out = Vec::new();
        walk(tree, tree.root, [0.0, 0.0], 0.0, 1.0, &mut out, &mut smallest);
        smallest
    }

    fn build(files: usize, dirs: usize, depth: usize) -> Tree {
        let mut tree = Tree::new(Some(PathBuf::from("/root")));
        fill(&mut tree, 0, files, dirs, depth);
        tree
    }

    fn fill(tree: &mut Tree, id: NodeId, files: usize, dirs: usize, depth: usize) {
        let mut entries: Vec<Entry> = (0..files).map(|i| entry(format!("f{i:05}.txt"), false)).collect();
        if depth > 0 {
            entries.extend((0..dirs).map(|i| entry(format!("d{i:05}"), true)));
        }
        tree.apply_listing(id, &entries, None);
        tree.get_mut(id).expanded = true;
        tree.get_mut(id).load = Load::Loaded;
        if depth > 0 {
            let mut kids = Vec::new();
            tree.logical_children(id, &mut kids);
            for kid in kids {
                if tree.get(kid).is_dir() {
                    fill(tree, kid, files, dirs, depth - 1);
                }
            }
        }
    }

    fn assert_flowers_apart(tree: &Tree) {
        let mut placed = Vec::new();
        resolve(tree, tree.root, [0.0, 0.0], 0.0, &mut placed);
        let flowers: Vec<[f32; 2]> =
            placed.iter().filter(|(id, _)| tree.get(*id).is_file()).map(|(_, p)| *p).collect();
        assert!(!flowers.is_empty());
        let mut worst = f32::MAX;
        let mut grid: rustc_hash::FxHashMap<(i32, i32), Vec<[f32; 2]>> = Default::default();
        let shrink = smallest_scale(tree);
        let cell = FLOWER_RADIUS * 4.0 * shrink;
        for p in &flowers {
            let key = ((p[0] / cell).floor() as i32, (p[1] / cell).floor() as i32);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    if let Some(others) = grid.get(&(key.0 + dx, key.1 + dy)) {
                        for q in others {
                            let d = ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)).sqrt();
                            worst = worst.min(d);
                        }
                    }
                }
            }
            grid.entry(key).or_default().push(*p);
        }
        assert!(worst >= FLOWER_RADIUS * 1.6 * shrink, "flowers overlap: nearest pair is {worst}, scale {shrink}");
    }

    #[test]
    fn going_deeper_grows_the_tree_gently() {
        // A chain of folders, each with a few closed siblings, like walking down a real disk.
        let mut tree = Tree::new(Some(PathBuf::from("/root")));
        let mut cur = tree.root;
        for _ in 0..12 {
            let entries: Vec<Entry> = (0..6).map(|i| entry(format!("d{i}"), true)).collect();
            tree.apply_listing(cur, &entries, None);
            tree.get_mut(cur).expanded = true;
            let mut kids = Vec::new();
            tree.logical_children(cur, &mut kids);
            cur = kids[3];
        }
        tree.get_mut(cur).expanded = false;
        relayout(&mut tree);
        let reach = tree.get(tree.root).reach;
        // Doubling per level would be about 300,000 here.
        assert!(reach < 4000.0, "a 12-level walk reaches {reach}");
    }

    #[test]
    fn a_lone_child_does_not_become_a_long_pole() {
        let mut tree = build(0, 1, 2);
        relayout(&mut tree);
        let child = tree.get(tree.root).children[0];
        assert!(tree.get(child).length < 200.0, "stem is {}", tree.get(child).length);
    }

    #[test]
    fn small_tree_has_no_overlapping_flowers() {
        let mut tree = build(5, 3, 2);
        relayout(&mut tree);
        assert_flowers_apart(&tree);
    }

    #[test]
    fn huge_flat_folder_has_no_overlapping_flowers() {
        let mut tree = build(5000, 0, 0);
        relayout(&mut tree);
        assert_flowers_apart(&tree);
    }

    #[test]
    fn wide_and_deep_tree_has_no_overlapping_flowers() {
        let mut tree = build(20, 6, 3);
        relayout(&mut tree);
        assert_flowers_apart(&tree);
    }

    #[test]
    fn every_child_stays_within_its_parents_circle() {
        let mut tree = build(12, 4, 2);
        relayout(&mut tree);
        let root = tree.get(tree.root);
        let center = root.length + root.ext_center;
        let radius = root.ext_radius;
        let mut placed = Vec::new();
        resolve(&tree, tree.root, [0.0, 0.0], 0.0, &mut placed);
        for (_, p) in placed {
            let d = (p[0] * p[0] + (p[1] - center).powi(2)).sqrt();
            assert!(d <= radius + 1.0, "{d} > {radius}");
        }
    }

    #[test]
    fn relayout_only_touches_what_changed() {
        let mut tree = build(6, 3, 1);
        relayout(&mut tree);
        let mut kids = Vec::new();
        tree.logical_children(tree.root, &mut kids);
        let dirs: Vec<NodeId> = kids.iter().copied().filter(|k| tree.get(*k).is_dir()).collect();
        let keep = tree.get(dirs[1]).ext_radius;
        tree.mark_dirty(dirs[0]);
        tree.get_mut(dirs[1]).ext_radius = keep; // unchanged and not dirty
        assert!(!tree.get(dirs[1]).dirty);
        assert!(tree.get(tree.root).dirty);
        relayout(&mut tree);
        assert_eq!(tree.get(dirs[1]).ext_radius, keep);
        assert!(!tree.get(tree.root).dirty);
    }

    #[test]
    fn layout_of_large_tree_is_fast() {
        let mut tree = build(10, 5, 4);
        let count = tree.nodes.len();
        let start = std::time::Instant::now();
        relayout(&mut tree);
        let took = start.elapsed();
        assert!(count > 3000, "{count}");
        // Generous bound so debug builds on slow CI still pass.
        assert!(took.as_millis() < 1500, "{count} nodes took {took:?}");
    }
}
