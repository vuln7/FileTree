//! The app's state and behaviour: what's selected, what's growing, and how input maps to
//! actions. It knows nothing about windows, so scripted captures and tests drive the same
//! code the real window does.

use crate::actions::{Actions, Cmd};
use crate::fs::{Disk, FsEvent, Preview, SearchHit, Waker};
use crate::gpu::DrawList;
use crate::layout;
use crate::text::Text;
use crate::tree::{Kind, Load, NONE, NodeId, Spring, Tree};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Space,
    Escape,
    Backspace,
    Home,
    PageUp,
    PageDown,
    AltLeft,
    AltRight,
    AltUp,
    Find,
    Fit,
    Paste,
    CopyPath,
    ToggleHidden,
    DeleteWord,
    Reset,
    End,
    Delete,
    SelectAll,
    Rename,
    CopyFiles,
    CutFiles,
    Duplicate,
    NewFile,
    NewFolder,
    Undo,
}

/// What the pointer should look like. The host turns this into the real thing.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Cursor {
    #[default]
    Default,
    Pointer,
    Text,
    Grabbing,
    Copy,
    NotAllowed,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SortBy {
    Name,
    Modified,
    Size,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Action {
    Back,
    Forward,
    Up,
    Crumb(usize),
    SearchField,
    ClearSearch,
    ToggleHidden,
    Fit,
    Reset,
    ToggleSelected,
    CollapseBranch(NodeId),
    Sort(SortBy),
    Row(usize),
    PanelScroll,
    ToggleMotion,
    Cmd(Cmd),
    MenuItem(usize),
    MenuClose,
    DialogOk,
    DialogCancel,
    DialogCancelButton,
    ToastUndo(usize),
    QuickClose,
    QuickClose2,
    QuickContent,
    QuickNext,
    QuickPrev,
    AudioToggle,
    AudioSeek,
    Place(usize),
    ShowBin,
    BinClose,
    BinRestore(usize),
    Nothing,
}

pub struct Zone {
    pub rect: [f32; 4],
    pub action: Action,
}

pub struct Camera {
    pub ox: Spring,
    pub oy: Spring,
    pub log_zoom: Spring,
}

impl Camera {
    pub fn zoom(&self) -> f32 {
        self.log_zoom.value.exp()
    }
}

pub struct Particle {
    pub pos: [f32; 2],
    pub vel: [f32; 2],
    pub rotation: f32,
    pub spin: f32,
    pub life: f32,
    pub species: f32,
    pub size: f32,
}

pub struct Visible {
    pub id: NodeId,
    pub x: f32,
    pub y: f32,
    pub r: f32,
    /// Flowers win over branch tips, and tips over the body of a branch.
    pub rank: u8,
}

pub struct ListItem {
    pub node: NodeId,
    pub path: Option<PathBuf>,
    pub name: Box<str>,
    pub detail: String,
    pub is_dir: bool,
    pub species: f32,
    pub size: u64,
    pub modified: i64,
}

pub struct LoadedPreview {
    pub preview: Preview,
}

pub struct App {
    pub tree: Tree,
    pub disk: Disk,
    pub text: Text,
    pub camera: Camera,
    pub draw: DrawList,

    pub width: f32,
    pub height: f32,
    pub scale: f32,
    pub time: f32,
    pub wind: Spring,
    pub idle_for: f32,
    pub focused: bool,
    pub reduced_motion: bool,
    pub redraw: bool,
    /// Camera keeps the whole tree framed until the visitor moves it themselves.
    pub follow: bool,
    /// What the camera is framing, kept in frame while the tree reshapes around it.
    pub camera_target: NodeId,

    pub pointer: [f32; 2],
    pub pointer_inside: bool,
    pub pressed_at: Option<[f32; 2]>,
    pub dragging: bool,
    pub last_click: (f32, NodeId),
    pub last_row_click: (f32, usize),
    pub hover: NodeId,
    pub hover_action: Action,
    pub zones: Vec<Zone>,
    pub visible: Vec<Visible>,
    pub selected: NodeId,
    pub particles: Vec<Particle>,
    pub collapsing: Vec<NodeId>,
    pub dying: Vec<NodeId>,
    pub pending_nav: Option<PathBuf>,
    pub history: Vec<PathBuf>,
    pub history_at: usize,
    pub status_note: Option<(String, f32)>,

    pub search_active: bool,
    pub search_text: String,
    pub search_generation: u64,
    pub search_due: Option<f32>,
    pub search_hits: Vec<SearchHit>,
    pub search_running: bool,
    pub search_capped: bool,

    pub list: Vec<ListItem>,
    pub list_dirty: bool,
    pub sort: SortBy,
    pub panel_scroll: Spring,
    pub text_scroll: Spring,
    pub selected_row: Option<usize>,
    pub preview: Option<LoadedPreview>,
    /// A decoded picture waiting to go to the GPU.
    pub image_pending: Option<(u32, u32, Vec<u8>)>,
    pub image_clear: bool,
    pub image_size: (u32, u32),
    pub label_grid: Vec<u8>,
    /// Boxes of the labels placed at the last solve, for a hovered name to avoid.
    pub label_boxes: Vec<[f32; 4]>,
    /// True while the tree's shape or the camera is moving (the breeze doesn't count).
    pub layout_moving: bool,
    pub was_moving: bool,
    pub labels_dirty: bool,
    pub stem_marks: Vec<[f32; 2]>,
    /// The panel's scrollbar this frame: track rectangle and how far it can scroll.
    pub scroll_track: Option<([f32; 4], f32)>,
    pub scrolling: bool,
    pub label_cands: Vec<crate::scene::LabelCand>,
    pub pending_copy: Option<String>,
    pub frame_count: u64,
    /// True when the only thing moving is the breeze, which doesn't need full frame rate.
    pub only_breeze: bool,
    /// Names and free space of the disks, as they arrive.
    pub drive_info: rustc_hash::FxHashMap<String, crate::platform::DriveInfo>,
    /// Everything to do with changing files: selection, clipboard, menus, dialogs.
    pub acts: Actions,
    pub cursor: Cursor,
    /// Real seconds since the start. Animation time pauses when nothing moves; this doesn't.
    pub wall: f32,
    /// The control the current press began on, so a release elsewhere doesn't trigger anything.
    pub press_zone: Option<Action>,
}

pub const TOOLBAR_H: f32 = 52.0;
pub const PANEL_W: f32 = 340.0;
const MIN_ZOOM_LN: f32 = -9.0;
const MAX_ZOOM_LN: f32 = 6.5;
/// How large a small tree is drawn: big enough to fill the stage without looking blown up.
const FIT_ZOOM: f32 = 1.45;

impl App {
    pub fn new(start: Option<PathBuf>, waker: Waker, width: f32, height: f32, scale: f32) -> App {
        let tree = Tree::new(start);
        let disk = Disk::new(waker);
        let mut app = App {
            tree,
            disk,
            text: Text::new(),
            camera: Camera {
                ox: Spring::new(width * 0.5),
                oy: Spring::new(height * 0.8),
                log_zoom: Spring::new(0.0),
            },
            draw: DrawList::default(),
            width,
            height,
            scale,
            time: 0.0,
            wind: Spring::new(0.0),
            idle_for: 0.0,
            focused: true,
            reduced_motion: false,
            redraw: true,
            follow: true,
            camera_target: NONE,
            pointer: [-1000.0, -1000.0],
            pointer_inside: false,
            pressed_at: None,
            dragging: false,
            last_click: (-10.0, NONE),
            last_row_click: (-10.0, usize::MAX),
            hover: NONE,
            hover_action: Action::Nothing,
            zones: Vec::new(),
            visible: Vec::new(),
            selected: NONE,
            particles: Vec::new(),
            collapsing: Vec::new(),
            dying: Vec::new(),
            pending_nav: None,
            history: Vec::new(),
            history_at: 0,
            status_note: None,
            search_active: false,
            search_text: String::new(),
            search_generation: 0,
            search_due: None,
            search_hits: Vec::new(),
            search_running: false,
            search_capped: false,
            list: Vec::new(),
            list_dirty: true,
            sort: SortBy::Name,
            panel_scroll: Spring::new(0.0),
            text_scroll: Spring::new(0.0),
            selected_row: None,
            preview: None,
            image_pending: None,
            image_clear: false,
            image_size: (0, 0),
            label_grid: Vec::new(),
            label_boxes: Vec::new(),
            layout_moving: false,
            was_moving: false,
            labels_dirty: true,
            stem_marks: Vec::new(),
            scroll_track: None,
            scrolling: false,
            label_cands: Vec::new(),
            pending_copy: None,
            frame_count: 0,
            only_breeze: false,
            drive_info: Default::default(),
            acts: Actions::default(),
            cursor: Cursor::Default,
            wall: 0.0,
            press_zone: None,
        };
        let root = app.tree.root;
        app.tree.get_mut(root).grow.target = 1.0;
        app.expand(root);
        app.selected = root;
        app.history.push(app.tree.path_of(root).unwrap_or_default());
        app.wake_wind();
        app
    }

    pub fn panel_width(&self) -> f32 {
        PANEL_W * self.scale
    }

    pub fn toolbar_height(&self) -> f32 {
        TOOLBAR_H * self.scale
    }

    /// The part of the window the tree can use.
    pub fn stage(&self) -> [f32; 4] {
        [0.0, self.toolbar_height(), (self.width - self.panel_width()).max(1.0), self.height]
    }

    pub fn wake_wind(&mut self) {
        self.idle_for = 0.0;
        self.wind.target = if self.reduced_motion || !self.focused { 0.0 } else { 1.0 };
        self.redraw = true;
    }

    // ------------------------------------------------------------ tree actions

    pub fn expand(&mut self, id: NodeId) {
        if !self.tree.is_valid(id) || self.tree.get(id).is_file() {
            return;
        }
        self.collapsing.retain(|c| *c != id);
        let node = self.tree.get_mut(id);
        node.expanded = true;
        node.bloom.target = 1.0;
        let load = node.load;
        if matches!(load, Load::Unloaded | Load::Failed) {
            self.request_list(id);
        }
        self.tree.mark_dirty(id);
        self.after_tree_change();
    }

    pub fn collapse(&mut self, id: NodeId) {
        if !self.tree.is_valid(id) || !self.tree.get(id).is_dir() || id == self.tree.root {
            return;
        }
        let node = self.tree.get_mut(id);
        node.expanded = false;
        node.bloom.target = 0.0;
        // A selection inside this branch moves up to the branch.
        if self.selected != NONE && self.selected != id && self.tree.is_ancestor_or_self(id, self.selected) {
            self.select(id, true);
        }
        if self.tree.is_valid(self.camera_target) && self.tree.is_ancestor_or_self(id, self.camera_target) {
            self.camera_target = id;
        }
        self.unwatch_inside(id);
        let kids = self.tree.get(id).children.clone();
        for kid in kids {
            self.retract(kid);
        }
        self.collapsing.push(id);
        self.tree.mark_dirty(id);
        self.after_tree_change();
    }

    /// Stops watching a folder and every open folder inside it.
    fn unwatch_inside(&mut self, id: NodeId) {
        let node = self.tree.get(id);
        if node.kind == Kind::Dir && node.load != Load::Unloaded
            && let Some(path) = self.tree.path_of(id)
        {
            self.disk.unwatch(path);
        }
        for child in self.tree.get(id).children.clone() {
            if !self.tree.get(child).is_file() {
                self.unwatch_inside(child);
            }
        }
    }

    /// Closes every branch, back to the very start.
    pub fn reset_tree(&mut self) {
        self.clear_search();
        self.pending_nav = None;
        let root = self.tree.root;
        self.select(root, true);
        let mut kids = Vec::new();
        self.tree.logical_children(root, &mut kids);
        for kid in kids {
            if self.tree.get(kid).expanded {
                self.collapse(kid);
            }
        }
        self.follow = true;
        self.camera_target = NONE;
        self.fit_all(false);
        self.wake_wind();
    }

    /// Opens or closes a branch from a control, keeping the camera sensible.
    pub fn toggle_branch(&mut self, id: NodeId) {
        if !self.tree.is_valid(id) || !self.tree.get(id).is_dir() || id == self.tree.root {
            return;
        }
        if self.tree.get(id).expanded {
            self.collapse(id);
            self.settle_camera(id);
        } else {
            self.select(id, true);
            self.expand(id);
            self.pending_nav = self.tree.path_of(id);
        }
    }

    fn retract(&mut self, id: NodeId) {
        let node = self.tree.get_mut(id);
        node.delay = 0.0;
        node.grow.target = 0.0;
        node.bloom.target = 0.0;
    }

    pub fn toggle(&mut self, id: NodeId) {
        if self.tree.get(id).expanded {
            self.collapse(id);
        } else {
            self.expand(id);
        }
    }

    fn request_list(&mut self, id: NodeId) {
        let node = self.tree.get_mut(id);
        node.load = Load::Loading;
        if node.kind == Kind::Places {
            let entries = crate::fs::list_places();
            for entry in &entries {
                self.disk.drive_info(entry.name.to_string());
            }
            self.apply_listing(id, &entries, None);
            return;
        }
        if let Some(path) = self.tree.path_of(id) {
            self.disk.watch(path.clone());
            self.disk.list(path);
        }
    }

    pub fn apply_listing(&mut self, dir: NodeId, entries: &[crate::fs::Entry], error: Option<&str>) {
        let (added, removed) = self.tree.apply_listing(dir, entries, error);
        if error.is_none() {
            self.tree.get_mut(dir).bloom.target = if self.tree.get(dir).expanded { 1.0 } else { 0.0 };
        }
        // Anything marked dying during the listing leaves through the shed animation.
        if removed > 0 {
            let mut kids = Vec::new();
            self.tree.logical_children(dir, &mut kids);
            for kid in kids {
                if self.tree.get(kid).dying && !self.dying.contains(&kid) {
                    self.dying.push(kid);
                    self.shed_petals(kid);
                }
            }
        }
        layout::relayout(&mut self.tree);
        if added > 0 {
            self.awaken(dir);
        }
        self.after_layout();
        self.list_dirty = true;
        self.redraw = true;
        self.advance_nav();
        self.settle_focus();
    }

    fn shed_petals(&mut self, id: NodeId) {
        let node = self.tree.get(id);
        if !node.is_file() || self.reduced_motion {
            return;
        }
        let species = node.species().index() as f32;
        let tip = node.tip;
        let seed = node.seed;
        for i in 0..7 {
            let r = crate::tree::rand01(seed, 40 + i);
            let a = r * std::f32::consts::TAU;
            self.particles.push(Particle {
                pos: [tip[0] + a.cos() * 8.0, tip[1] + a.sin() * 8.0],
                vel: [a.cos() * 30.0, 40.0 + r * 30.0],
                rotation: a,
                spin: (r - 0.5) * 5.0,
                life: 1.0,
                species,
                size: 9.0 + r * 4.0,
            });
        }
    }

    /// Starts everything newly created under a folder growing, one after another.
    fn awaken(&mut self, dir: NodeId) {
        let snap = self.reduced_motion;
        let count = self.tree.get(dir).children.len().max(1);
        let step = (0.5 / count as f32).min(0.04);
        let kids = self.tree.get(dir).children.clone();
        for (i, kid) in kids.iter().enumerate() {
            self.awaken_node(*kid, 0.05 + i as f32 * step, snap);
        }
    }

    fn awaken_node(&mut self, id: NodeId, delay: f32, snap: bool) {
        let node = self.tree.get_mut(id);
        if node.grow.target != 0.0 || node.dying {
            return;
        }
        let (theta, length) = (node.a_theta.target, node.a_length.target);
        node.a_theta.snap(theta);
        node.a_length.snap(length);
        node.grow.target = 1.0;
        node.delay = delay;
        if snap {
            node.grow.snap(1.0);
            node.delay = 0.0;
        }
        match node.kind {
            Kind::File(_) => {
                node.bloom.target = 1.0;
                if snap {
                    node.bloom.snap(1.0);
                }
            }
            Kind::Sprig => {
                let kids = node.children.clone();
                let step = (0.3 / kids.len().max(1) as f32).min(0.03);
                for (i, kid) in kids.iter().enumerate() {
                    self.awaken_node(*kid, delay + 0.08 + i as f32 * step, snap);
                }
            }
            _ => {}
        }
    }

    pub fn after_tree_change(&mut self) {
        layout::relayout(&mut self.tree);
        self.after_layout();
        self.list_dirty = true;
        self.wake_wind();
    }

    fn after_layout(&mut self) {
        if self.follow {
            self.fit_all(false);
        } else if self.tree.is_valid(self.camera_target) {
            let target = self.camera_target;
            self.frame_node(target);
        }
    }

    // ------------------------------------------------------------ camera

    /// Bounds of a subtree at its resting layout, relative to the world origin.
    fn rest_bounds(&self, id: NodeId) -> [f32; 4] {
        let mut bounds = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        let (base, heading, scale) = self.rest_base(id);
        self.bounds_walk(id, base, heading, scale, &mut bounds);
        bounds
    }

    /// Where a node's base and heading are once everything has settled.
    fn rest_base(&self, id: NodeId) -> ([f32; 2], f32, f32) {
        let mut chain = Vec::new();
        let mut cur = self.tree.get(id).parent;
        while cur != NONE {
            chain.push(cur);
            cur = self.tree.get(cur).parent;
        }
        let (mut base, mut heading, mut scale) = ([0.0, -crate::scene::ROOT_SINK], 0.0, 1.0);
        for &n in chain.iter().rev() {
            let node = self.tree.get(n);
            heading += node.a_theta.target;
            let length = rest_length(node) * scale;
            base[0] += heading.sin() * length;
            base[1] += heading.cos() * length;
            scale *= node.subscale;
        }
        (base, heading, scale)
    }

    fn bounds_walk(&self, id: NodeId, base: [f32; 2], heading: f32, scale: f32, bounds: &mut [f32; 4]) {
        let node = self.tree.get(id);
        if node.grow.target == 0.0 && node.parent != NONE {
            return;
        }
        let h = heading + node.a_theta.target;
        let length = rest_length(node) * scale;
        let tip = [base[0] + h.sin() * length, base[1] + h.cos() * length];
        let r = if node.is_file() { layout::FLOWER_RADIUS * 1.2 } else { 24.0 } * scale;
        bounds[0] = bounds[0].min(tip[0] - r).min(base[0]);
        bounds[1] = bounds[1].min(tip[1] - r).min(base[1]);
        bounds[2] = bounds[2].max(tip[0] + r).max(base[0]);
        bounds[3] = bounds[3].max(tip[1] + r).max(base[1]);
        if node.expanded {
            for &child in &node.children {
                self.bounds_walk(child, tip, h, scale * node.subscale, bounds);
            }
        }
    }

    /// Frames all of the tree in the stage.
    pub fn fit_all(&mut self, instant: bool) {
        let bounds = self.rest_bounds(self.tree.root);
        // Leave the soil visible beneath the trunk.
        let bounds = [bounds[0], bounds[1].min(-34.0), bounds[2], bounds[3]];
        self.frame_bounds(bounds, instant, FIT_ZOOM, true);
    }

    /// Frames one branch, keeping enough context to see where it hangs from.
    pub fn frame_node(&mut self, id: NodeId) {
        self.camera_target = id;
        let bounds = self.rest_bounds(id);
        let (_, _, scale) = self.rest_base(id);
        // A fine crown may need a closer look to read at full size.
        let inner = self.tree.get(id).subscale * scale;
        self.frame_bounds(bounds, false, 1.6 / inner.max(0.02), false);
    }

    fn frame_bounds(&mut self, b: [f32; 4], instant: bool, max_zoom: f32, on_ground: bool) {
        if b[0] > b[2] {
            return;
        }
        let stage = self.stage();
        let s = self.scale;
        let pad_top = 46.0 * s;
        let pad_bottom = 28.0 * s;
        // The limit applies before positioning, so the view lands where it was aimed.
        let ln = self.zoom_to_fit(b, max_zoom).ln().clamp(MIN_ZOOM_LN, MAX_ZOOM_LN + s.ln());
        let zoom = ln.exp();
        let center_x = (b[0] + b[2]) * 0.5;
        let center_y = (b[1] + b[3]) * 0.5;
        let ox = (stage[0] + stage[2]) * 0.5 - center_x * zoom;
        let oy = if on_ground {
            // Stand on the bottom edge however much room is left above.
            stage[3] - pad_bottom + b[1] * zoom
        } else {
            (stage[1] + pad_top + stage[3] - pad_bottom) * 0.5 + center_y * zoom
        };
        self.camera.ox.target = ox;
        self.camera.oy.target = oy;
        self.camera.log_zoom.target = ln;
        if instant || self.reduced_motion {
            self.camera.ox.snap(ox);
            self.camera.oy.snap(oy);
            self.camera.log_zoom.snap(ln);
        }
        self.redraw = true;
    }

    pub fn pan_by(&mut self, dx: f32, dy: f32) {
        self.follow = false;
        self.camera_target = NONE;
        for (spring, d) in [(&mut self.camera.ox, dx), (&mut self.camera.oy, dy)] {
            spring.value += d;
            spring.target += d;
        }
        self.redraw = true;
    }

    pub fn zoom_at(&mut self, x: f32, y: f32, amount: f32) {
        self.follow = false;
        self.camera_target = NONE;
        let cam = &mut self.camera;
        let zoom = cam.log_zoom.target.exp();
        let ln = (cam.log_zoom.target + amount).clamp(MIN_ZOOM_LN, MAX_ZOOM_LN + self.scale.ln());
        let next = ln.exp();
        let ratio = next / zoom;
        cam.ox.target = x - (x - cam.ox.target) * ratio;
        cam.oy.target = y - (y - cam.oy.target) * ratio;
        cam.log_zoom.target = ln;
        if self.reduced_motion {
            cam.ox.snap(cam.ox.target);
            cam.oy.snap(cam.oy.target);
            cam.log_zoom.snap(ln);
        }
        self.redraw = true;
        self.wake_wind();
    }

    // ------------------------------------------------------------ selection and navigation

    pub fn select(&mut self, id: NodeId, remember: bool) {
        if !self.tree.is_valid(id) {
            return;
        }
        if self.selected != id {
            self.selected = id;
            self.labels_dirty = true;
            self.list_dirty = true;
            self.panel_scroll.snap(0.0);
            self.text_scroll.snap(0.0);
            self.selected_row = None;
            // Stepping through Quick Look holds the last file until the next is ready, so the
            // sheet never flashes empty in between.
            let stepping = self.acts.quick.is_some() && self.tree.get(id).is_file();
            self.acts.quick_stale = stepping;
            if !stepping {
                self.preview = None;
                self.image_clear = true;
                self.image_size = (0, 0);
            }
            if self.tree.get(id).is_file()
                && let Some(path) = self.tree.path_of(id)
            {
                if self.acts.quick.is_some() {
                    self.disk.preview_large(path);
                } else {
                    self.disk.preview(path);
                }
            }
            let previous = std::mem::replace(&mut self.acts.lifted, id);
            if self.tree.is_valid(previous) {
                let rest = self.rest_lift(previous);
                self.tree.get_mut(previous).lift.target = rest;
            }
            let reduced = self.reduced_motion;
            let node = self.tree.get_mut(id);
            node.lift.target = 1.0;
            // A little kick, so being chosen has some life to it.
            if !reduced {
                node.lift.velocity += 7.0;
            }
            self.on_selection_changed(id);
        }
        if remember {
            let path = self.tree.path_of(id).unwrap_or_default();
            self.push_history(path);
        }
        self.wake_wind();
    }

    fn push_history(&mut self, path: PathBuf) {
        if self.history.get(self.history_at).is_some_and(|p| *p == path) {
            return;
        }
        self.history.truncate(self.history_at + 1);
        self.history.push(path);
        self.history_at = self.history.len() - 1;
        if self.history.len() > 200 {
            self.history.remove(0);
            self.history_at -= 1;
        }
    }

    pub fn can_go_back(&self) -> bool {
        self.history_at > 0 && !self.history.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        self.history_at + 1 < self.history.len()
    }

    pub fn go_back(&mut self) {
        if self.can_go_back() {
            self.history_at -= 1;
            let path = self.history[self.history_at].clone();
            self.navigate_to(path, false);
        }
    }

    pub fn go_forward(&mut self) {
        if self.can_go_forward() {
            self.history_at += 1;
            let path = self.history[self.history_at].clone();
            self.navigate_to(path, false);
        }
    }

    /// Moves up a level, growing a new trunk beneath the tree when already at the root.
    pub fn go_up(&mut self) {
        let parent = if self.selected == NONE { NONE } else { self.tree.logical_parent(self.selected) };
        if parent != NONE {
            self.select(parent, true);
            self.frame_if_lost(parent);
        } else {
            self.reroot_up();
        }
    }

    pub fn reroot_up(&mut self) -> bool {
        let Some(new_root) = self.tree.reroot_up() else { return false };
        self.tree.get_mut(new_root).grow.target = 1.0;
        self.tree.get_mut(new_root).bloom.target = 1.0;
        self.tree.get_mut(new_root).a_length.snap(0.0);
        self.tree.get_mut(new_root).load = Load::Unloaded;
        self.select(new_root, true);
        self.follow = true;
        self.request_list(new_root);
        self.after_tree_change();
        true
    }

    /// Goes to any path, growing every branch needed along the way.
    pub fn navigate_to(&mut self, path: PathBuf, remember: bool) {
        self.pending_nav = Some(path.clone());
        if remember {
            self.push_history(path);
        }
        self.advance_nav();
    }

    fn advance_nav(&mut self) {
        let Some(target) = self.pending_nav.clone() else { return };
        // Grow a trunk beneath the tree until the target is inside it.
        if let Some(root) = self.tree.root_path.clone()
            && !target.starts_with(&root)
        {
            if !self.reroot_up() {
                self.pending_nav = None;
            }
            return;
        }
        let root = self.tree.root;
        if self.tree.get(root).load != Load::Loaded {
            if self.tree.get(root).load == Load::Unloaded {
                self.expand(root);
            }
            return;
        }
        let mut cur = root;
        let mut built = self.tree.root_path.clone().unwrap_or_default();
        let rest: PathBuf = match self.tree.root_path.as_ref() {
            Some(r) => target.strip_prefix(r).map(Path::to_path_buf).unwrap_or_default(),
            None => target.clone(),
        };
        for comp in rest.components() {
            built.push(comp);
            if matches!(comp, std::path::Component::RootDir) {
                continue;
            }
            let Some(next) = self.tree.find_path(&built) else {
                // It exists but isn't showing, so it's hidden: show hidden items and carry on
                // once the folders have been read again.
                if !self.tree.show_hidden && std::fs::symlink_metadata(&built).is_ok() {
                    self.toggle_hidden();
                    return;
                }
                // Reached the end of what's loaded and still missing: it doesn't exist here.
                self.pending_nav = None;
                self.note(format!("Couldn't find {}", target.display()));
                self.select(cur, false);
                return;
            };
            if self.tree.get(next).is_dir() && built != target {
                if !self.tree.get(next).expanded {
                    self.expand(next);
                }
                if self.tree.get(next).load != Load::Loaded {
                    return;
                }
            }
            cur = next;
        }
        if self.tree.get(cur).is_dir() {
            if !self.tree.get(cur).expanded {
                self.expand(cur);
            }
            if matches!(self.tree.get(cur).load, Load::Unloaded | Load::Loading) {
                self.select(cur, false);
                return;
            }
        }
        self.pending_nav = None;
        self.select(cur, false);
        self.settle_camera(cur);
    }

    /// Keeps the whole tree in view while its flowers can still be read at that size.
    /// Past that, the camera moves in on the branch being opened.
    fn settle_camera(&mut self, id: NodeId) {
        let (_, _, scale) = self.rest_base(id);
        let inner = scale * self.tree.get(id).subscale;
        let bounds = self.rest_bounds(self.tree.root);
        // A big folder is shown whole, but going to one file inside it moves in close.
        let crown = self.tree.get(id).is_dir()
            && self.tree.get(id).children.iter().any(|c| self.tree.get(*c).is_sprig());
        let readable = crown || self.zoom_to_fit(bounds, FIT_ZOOM) * layout::FLOWER_RADIUS * inner >= 6.0 * self.scale;
        if readable {
            self.follow = true;
            self.camera_target = NONE;
            self.fit_all(false);
        } else {
            self.follow = false;
            self.frame_node(id);
        }
    }

    fn zoom_to_fit(&self, b: [f32; 4], max_zoom: f32) -> f32 {
        let stage = self.stage();
        let s = self.scale;
        let w = (stage[2] - stage[0] - (192.0 * s).min((stage[2] - stage[0]) * 0.22)).max(40.0);
        let h = (stage[3] - stage[1] - 74.0 * s).max(40.0);
        (w / (b[2] - b[0]).max(1.0)).min(h / (b[3] - b[1]).max(1.0)).min(max_zoom * s)
    }

    fn frame_if_lost(&mut self, id: NodeId) {
        if self.follow {
            return;
        }
        let node = self.tree.get(id);
        let z = self.camera.zoom();
        let x = self.camera.ox.value + node.tip[0] * z;
        let y = self.camera.oy.value - node.tip[1] * z;
        let stage = self.stage();
        if x < stage[0] || x > stage[2] || y < stage[1] || y > stage[3] {
            self.frame_node(id);
        }
    }

    pub fn open_selected(&mut self) {
        if self.selected == NONE {
            return;
        }
        if let Some(path) = self.tree.path_of(self.selected) {
            self.open_path(&path);
        }
    }

    pub fn open_path(&mut self, path: &Path) {
        if let Err(err) = opener::open(path) {
            self.note(format!("Couldn't open this: {err}"));
        }
    }

    pub fn reveal_selected(&mut self) {
        if self.selected == NONE {
            return;
        }
        if let Some(path) = self.tree.path_of(self.selected)
            && let Err(err) = opener::reveal(&path)
        {
            self.note(format!("Couldn't show this in the file manager: {err}"));
        }
    }

    pub fn copy_selected_path(&mut self) {
        if self.selected != NONE
            && let Some(path) = self.tree.path_of(self.selected)
        {
            self.pending_copy = Some(path.display().to_string());
            self.note("Path copied".to_string());
        }
    }

    pub fn note(&mut self, text: String) {
        self.status_note = Some((text, self.wall + 4.0));
        self.redraw = true;
    }

    pub fn toggle_hidden(&mut self) {
        self.tree.show_hidden = !self.tree.show_hidden;
        let ids: Vec<NodeId> = (0..self.tree.nodes.len() as NodeId)
            .filter(|i| {
                let n = self.tree.get(*i);
                n.alive && n.is_dir() && n.load == Load::Loaded && n.kind == Kind::Dir
            })
            .collect();
        for id in ids {
            if let Some(path) = self.tree.path_of(id) {
                self.disk.list(path);
            }
        }
        self.note(if self.tree.show_hidden { "Showing hidden items".into() } else { "Hiding hidden items".into() });
    }

    pub fn toggle_motion(&mut self) {
        self.reduced_motion = !self.reduced_motion;
        self.wake_wind();
    }

    // ------------------------------------------------------------ keyboard

    pub fn on_key(&mut self, key: Key) {
        self.wake_wind();
        if key == Key::Paste && self.acts.dialog.is_some() {
            if let Some(text) = self.system_text()
                && let Some(field) = self.acts.dialog.as_mut().and_then(|d| d.field.as_mut())
            {
                field.insert(text.trim());
            }
            return;
        }
        if self.overlay_key(key) {
            return;
        }
        if key == Key::Reset {
            self.reset_tree();
            return;
        }
        if self.search_active {
            match key {
                Key::Escape => {
                    self.clear_search();
                    return;
                }
                Key::Backspace => {
                    self.search_text.pop();
                    self.search_changed();
                    return;
                }
                Key::DeleteWord => {
                    let t = self.search_text.trim_end();
                    let cut = t.rfind([' ', '\\', '/']).map_or(0, |i| i + 1);
                    self.search_text.truncate(cut);
                    self.search_changed();
                    return;
                }
                Key::Enter => {
                    self.commit_search();
                    return;
                }
                Key::Paste => {
                    if let Some(text) = self.system_text() {
                        self.on_paste(&text);
                    }
                    return;
                }
                Key::Down => {
                    self.move_row(1);
                    return;
                }
                Key::Up => {
                    self.move_row(-1);
                    return;
                }
                _ => return,
            }
        }
        match key {
            Key::Find => {
                self.search_active = true;
                self.search_text.clear();
                self.list_dirty = true;
            }
            Key::Fit | Key::Home => {
                self.follow = true;
                self.fit_all(false);
            }
            Key::ToggleHidden => self.toggle_hidden(),
            Key::CopyPath => self.copy_selected_path(),
            Key::Paste => self.run_cmd(Cmd::Paste),
            Key::Rename => self.run_cmd(Cmd::Rename),
            Key::Delete => self.run_cmd(Cmd::Delete),
            Key::CopyFiles => self.run_cmd(Cmd::Copy),
            Key::CutFiles => self.run_cmd(Cmd::Cut),
            Key::Duplicate => self.run_cmd(Cmd::Duplicate),
            Key::NewFile => self.run_cmd(Cmd::NewFile),
            Key::NewFolder => self.run_cmd(Cmd::NewFolder),
            Key::Undo => self.run_cmd(Cmd::Undo),
            Key::SelectAll => self.run_cmd(Cmd::SelectAll),
            Key::AltLeft => self.go_back(),
            Key::AltRight => self.go_forward(),
            Key::AltUp | Key::Backspace => self.go_up(),
            Key::Escape => {
                if self.acts.bin.take().is_some() {
                    self.acts.panel_in = 0.0;
                    self.panel_scroll.snap(0.0);
                } else if !self.acts.marked.is_empty() {
                    self.clear_marks();
                } else if self.acts.clip.as_ref().is_some_and(|c| c.1) {
                    self.acts.clip = None;
                    self.acts.cut_nodes.clear();
                } else if self.selected != self.tree.root {
                    let root = self.tree.root;
                    self.select(root, true);
                }
            }
            Key::Enter => {
                if self.selected == NONE {
                    return;
                }
                if self.tree.get(self.selected).is_file() {
                    self.open_selected_with_flourish();
                } else {
                    let id = self.selected;
                    self.toggle(id);
                }
            }
            Key::Space => {
                if self.selected == NONE {
                    return;
                }
                if self.tree.get(self.selected).is_file() {
                    self.open_quick_look();
                } else {
                    let id = self.selected;
                    self.toggle(id);
                }
            }
            // The tree grows upward, so Up goes outward along the branch.
            Key::Up => self.step_out(),
            Key::Down => self.go_up(),
            Key::Left => self.step_sibling(-1),
            Key::Right => self.step_sibling(1),
            Key::PageUp => self.scroll_panel(-400.0),
            Key::PageDown => self.scroll_panel(400.0),
            _ => {}
        }
    }

    fn step_out(&mut self) {
        let id = self.selected;
        if id == NONE || self.tree.get(id).is_file() {
            return;
        }
        if !self.tree.get(id).expanded {
            self.expand(id);
            return;
        }
        let mut kids = Vec::new();
        self.tree.logical_children(id, &mut kids);
        // Prefer the middle child, which sits straight ahead along the branch.
        if !kids.is_empty() {
            let pick = kids[kids.len() / 2];
            self.select(pick, true);
            self.frame_if_lost(pick);
        }
    }

    fn step_sibling(&mut self, dir: i32) {
        let id = self.selected;
        if id == NONE {
            return;
        }
        let parent = self.tree.logical_parent(id);
        if parent == NONE {
            return;
        }
        let mut kids = Vec::new();
        self.tree.logical_children(parent, &mut kids);
        // Move in the order they appear left to right on the tree.
        if let Some(pos) = kids.iter().position(|k| *k == id) {
            let next = (pos as i32 + dir).rem_euclid(kids.len() as i32) as usize;
            let pick = kids[next];
            self.select(pick, true);
            self.frame_if_lost(pick);
        }
    }

    pub fn on_char(&mut self, ch: char) {
        if ch.is_control() {
            return;
        }
        self.wake_wind();
        if let Some(dialog) = &mut self.acts.dialog {
            if let Some(field) = &mut dialog.field {
                field.insert(&ch.to_string());
            }
            return;
        }
        if self.acts.quick.is_some() || self.acts.drag.is_some() {
            return;
        }
        self.acts.menu = None;
        if !self.search_active {
            // Spaces are for toggling; anything else starts a search straight away.
            if ch == ' ' {
                return;
            }
            self.search_active = true;
            self.search_text.clear();
        }
        self.search_text.push(ch);
        self.search_changed();
    }

    pub fn on_paste(&mut self, text: &str) {
        self.search_active = true;
        self.search_text.push_str(text.trim().trim_matches('"'));
        self.search_changed();
    }

    fn search_changed(&mut self) {
        self.search_hits.clear();
        self.search_capped = false;
        self.labels_dirty = true;
        self.list_dirty = true;
        self.panel_scroll.snap(0.0);
        self.selected_row = None;
        if self.search_text.trim().is_empty() || self.looks_like_path() {
            self.disk.cancel_search();
            self.search_running = false;
            self.search_due = None;
        } else {
            // Wait for a pause in typing, so each keystroke doesn't restart a walk of the disk.
            self.search_due = Some(self.time + 0.14);
            self.search_running = true;
        }
        self.redraw = true;
    }

    pub fn looks_like_path(&self) -> bool {
        let t = self.search_text.trim();
        let bytes = t.as_bytes();
        (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
            || t.starts_with('/')
            || t.starts_with('\\')
            || t.starts_with('~')
    }

    fn commit_search(&mut self) {
        if self.looks_like_path() {
            let raw = self.search_text.trim().to_string();
            let path = match raw.strip_prefix('~') {
                Some(rest) => PathBuf::from(home_dir_string() + rest),
                None => PathBuf::from(raw),
            };
            self.clear_search();
            self.navigate_to(path, true);
            return;
        }
        if let Some(row) = self.selected_row.or(if self.list.is_empty() { None } else { Some(0) }) {
            self.activate_row(row, false);
        }
    }

    pub fn clear_search(&mut self) {
        self.disk.cancel_search();
        self.search_active = false;
        self.search_text.clear();
        self.search_hits = Vec::new();
        self.labels_dirty = true;
        self.search_running = false;
        self.search_due = None;
        self.list_dirty = true;
        self.selected_row = None;
        self.redraw = true;
    }

    pub fn scroll_panel(&mut self, amount: f32) {
        self.panel_scroll.target = (self.panel_scroll.target + amount * self.scale).max(0.0);
        self.redraw = true;
    }

    pub fn move_row(&mut self, dir: i32) {
        if self.list.is_empty() {
            return;
        }
        let max = self.list.len() as i32 - 1;
        let at = self.selected_row.map_or(if dir > 0 { -1 } else { 0 }, |r| r as i32);
        let next = (at + dir).clamp(0, max) as usize;
        self.selected_row = Some(next);
        self.redraw = true;
    }

    pub fn activate_row(&mut self, row: usize, open_it: bool) {
        let Some(item) = self.list.get(row) else { return };
        let (node, path, is_dir) = (item.node, item.path.clone(), item.is_dir);
        if let Some(path) = path {
            if open_it && !is_dir {
                self.open_path(&path);
                return;
            }
            self.navigate_to(path, true);
            return;
        }
        if self.tree.is_valid(node) {
            if open_it && !is_dir {
                self.select(node, true);
                self.open_selected_with_flourish();
                return;
            }
            if open_it && is_dir {
                self.expand(node);
            }
            self.select(node, true);
            self.follow = false;
            self.frame_if_lost(node);
        }
    }

    // ------------------------------------------------------------ pointer

    pub fn on_pointer_move(&mut self, x: f32, y: f32) {
        let (dx, dy) = (x - self.pointer[0], y - self.pointer[1]);
        self.pointer = [x, y];
        self.pointer_inside = true;
        if self.scrolling {
            self.drag_scrollbar();
            return;
        }
        if self.press_zone == Some(Action::AudioSeek) {
            self.audio_seek_to_pointer();
            return;
        }
        if self.press_zone == Some(Action::QuickContent) {
            if let Some(quick) = &mut self.acts.quick {
                quick.pan[0] += dx;
                quick.pan[1] += dy;
                self.redraw = true;
            }
            return;
        }
        if let Some(start) = self.pressed_at {
            let moved = ((x - start[0]).powi(2) + (y - start[1]).powi(2)).sqrt();
            if self.acts.press_node != NONE && self.acts.drag.is_none() && moved > 6.0 * self.scale {
                let id = self.acts.press_node;
                self.acts.press_node = NONE;
                self.start_drag(id);
            }
            if self.acts.drag.is_some() {
                self.refresh_hover();
                self.update_drag();
                self.wake_wind();
                return;
            }
            if !self.dragging
                && self.press_zone.is_none()
                && moved > 4.0 * self.scale
                && start[0] < self.stage()[2]
                && start[1] > self.stage()[1]
            {
                self.dragging = true;
            }
            if self.dragging {
                self.pan_by(dx, dy);
                self.wake_wind();
                return;
            }
        }
        self.refresh_hover();
    }

    /// The pointer left the window. Its last position is kept, so anything still in
    /// progress keeps a sensible anchor, but nothing stays highlighted.
    pub fn on_pointer_left(&mut self) {
        self.pointer_inside = false;
        self.refresh_hover();
        self.redraw = true;
    }

    pub fn refresh_hover(&mut self) {
        let [x, y] = if self.pointer_inside { self.pointer } else { [-1000.0, -1000.0] };
        let action =
            self.zones.iter().rev().find(|z| inside(z.rect, x, y)).map(|z| z.action).unwrap_or(Action::Nothing);
        let over_ui = self.zones.iter().any(|z| inside(z.rect, x, y));
        let carrying = self.acts.drag.as_ref().map(|d| d.target);
        let hit = match carrying {
            Some(target) => target,
            None if over_ui => NONE,
            None => self.hit_test(x, y),
        };
        if hit != self.hover || action != self.hover_action {
            if self.hover != NONE && self.tree.is_valid(self.hover) {
                let rest = self.rest_lift(self.hover);
                self.tree.get_mut(self.hover).lift.target = rest;
            }
            self.hover = hit;
            self.hover_action = action;
            if hit != NONE {
                let rest = self.rest_lift(hit).max(0.55);
                self.tree.get_mut(hit).lift.target = rest;
                self.wake_wind();
            }
            self.redraw = true;
        }
        self.cursor = if let Some(target) = carrying {
            if target == NONE {
                Cursor::NotAllowed
            } else if self.drop_copies(target) {
                Cursor::Copy
            } else {
                Cursor::Grabbing
            }
        } else if self.dragging {
            Cursor::Grabbing
        } else if matches!(action, Action::SearchField) {
            Cursor::Text
        } else if hit != NONE
            || !matches!(
                action,
                Action::Nothing | Action::PanelScroll | Action::QuickContent | Action::MenuClose | Action::DialogCancel | Action::QuickClose
            )
        {
            Cursor::Pointer
        } else {
            Cursor::Default
        };
    }

    /// How raised a flower sits when nothing is pointing at it.
    pub fn rest_lift(&self, id: NodeId) -> f32 {
        if id == self.selected || self.acts.marked.contains(&id) { 1.0 } else { 0.0 }
    }

    pub fn hit_test(&self, x: f32, y: f32) -> NodeId {
        let mut best = (u8::MAX, f32::MAX, NONE);
        for v in &self.visible {
            let d = ((v.x - x).powi(2) + (v.y - y).powi(2)).sqrt();
            if d <= v.r {
                let score = (v.rank, d);
                if score.0 < best.0 || (score.0 == best.0 && score.1 < best.1) {
                    best = (score.0, score.1, v.id);
                }
            }
        }
        best.2
    }

    pub fn on_pointer_down(&mut self) {
        self.pressed_at = Some(self.pointer);
        self.dragging = false;
        let [x, y] = self.pointer;
        let zone = self.zones.iter().rev().find(|z| inside(z.rect, x, y)).map(|z| z.action);
        self.press_zone = zone;
        if zone == Some(Action::AudioSeek) {
            self.audio_seek_to_pointer();
        }
        // Only something you could pick up starts a drag. Pressing bare branch still pans.
        let carried = match zone {
            Some(Action::Row(row)) => self.list.get(row).map_or(NONE, |item| item.node),
            None => self.hit_carryable(x, y),
            _ => NONE,
        };
        self.acts.press_node = if self.carryable(carried) { carried } else { NONE };
        if let Some((track, _)) = self.scroll_track
            && inside(track, self.pointer[0], self.pointer[1])
        {
            self.scrolling = true;
            self.drag_scrollbar();
        }
    }

    fn drag_scrollbar(&mut self) {
        let Some((track, max)) = self.scroll_track else { return };
        let fraction = ((self.pointer[1] - track[1]) / (track[3] - track[1]).max(1.0)).clamp(0.0, 1.0);
        self.panel_scroll.snap(fraction * max);
        self.redraw = true;
    }

    pub fn on_pointer_up(&mut self) {
        let was_drag = self.dragging || self.scrolling;
        let press_zone = self.press_zone.take();
        self.pressed_at = None;
        self.dragging = false;
        self.scrolling = false;
        self.acts.press_node = NONE;
        if self.acts.drag.is_some() {
            self.finish_drag();
            self.wake_wind();
            return;
        }
        if was_drag || press_zone == Some(Action::AudioSeek) {
            return;
        }
        let [x, y] = self.pointer;
        if let Some(action) = self.zones.iter().rev().find(|z| inside(z.rect, x, y)).map(|z| z.action) {
            if press_zone == Some(action) {
                self.on_action(action);
            }
            return;
        }
        if press_zone.is_some() {
            return;
        }
        let hit = self.hit_test(x, y);
        self.wake_wind();
        if hit == NONE {
            self.clear_marks();
            if self.search_active && self.search_text.is_empty() {
                self.clear_search();
            }
            return;
        }
        if self.tree.get(hit).is_sprig() {
            self.follow = false;
            self.frame_node(hit);
            return;
        }
        if self.acts.ctrl {
            self.toggle_mark(hit);
            return;
        }
        self.clear_marks();
        let double = self.last_click.1 == hit && self.time - self.last_click.0 < 0.38;
        self.last_click = (self.time, hit);
        if self.tree.get(hit).is_file() {
            self.select(hit, true);
            if double {
                self.open_selected_with_flourish();
            }
        } else {
            let was_selected = self.selected == hit;
            self.select(hit, true);
            if !self.tree.get(hit).expanded {
                self.expand(hit);
                self.frame_pending(hit);
            } else if was_selected && hit != self.tree.root {
                self.collapse(hit);
            }
        }
    }

    /// Once a branch's contents arrive, the camera follows them.
    fn frame_pending(&mut self, id: NodeId) {
        self.pending_nav = self.tree.path_of(id);
    }

    pub fn on_wheel(&mut self, lines: f32) {
        let [x, y] = self.pointer;
        if self.acts.dialog.is_some() || self.acts.drag.is_some() {
            return;
        }
        if self.acts.menu.take().is_some() {
            self.redraw = true;
            return;
        }
        if self.acts.quick.is_some() {
            self.quick_wheel(lines);
            return;
        }
        if x >= self.stage()[2] && y > self.toolbar_height() {
            self.scroll_panel(-lines * 54.0);
        } else {
            self.zoom_at(x, y, lines * 0.16);
        }
    }

    pub fn on_action(&mut self, action: Action) {
        self.wake_wind();
        match action {
            Action::Back => self.go_back(),
            Action::Forward => self.go_forward(),
            Action::Up => self.go_up(),
            Action::Crumb(index) => self.go_crumb(index),
            Action::SearchField => {
                self.search_active = true;
                self.list_dirty = true;
            }
            Action::ClearSearch => self.clear_search(),
            Action::Cmd(cmd) => self.run_cmd(cmd),
            Action::MenuItem(row) => {
                if let Some(cmd) = self.acts.menu.as_ref().and_then(|m| m.items.get(row)).and_then(|i| i.cmd) {
                    self.run_cmd(cmd);
                }
            }
            Action::MenuClose => self.acts.menu = None,
            Action::DialogOk => self.dialog_confirm(),
            Action::DialogCancel | Action::DialogCancelButton => self.acts.dialog = None,
            Action::ToastUndo(index) => self.undo_toast(index),
            Action::QuickClose | Action::QuickClose2 => self.close_quick_look(),
            Action::QuickNext => self.quick_step(1),
            Action::QuickPrev => self.quick_step(-1),
            Action::AudioToggle => self.audio_toggle(),
            Action::Place(index) => {
                if let Some(path) = self.acts.places.get(index).map(|p| p.1.clone()) {
                    self.navigate_to(path, true);
                }
            }
            Action::ShowBin => {
                self.acts.bin = Some(Vec::new());
                self.acts.panel_in = 0.0;
                self.panel_scroll.snap(0.0);
                self.disk.list_bin();
            }
            Action::BinClose => {
                self.acts.bin = None;
                self.acts.panel_in = 0.0;
                self.panel_scroll.snap(0.0);
            }
            Action::BinRestore(index) => {
                let was_at = self.acts.bin.as_ref().and_then(|b| b.get(index)).map(|i| i.was_at.clone());
                if let Some(was_at) = was_at {
                    self.start_op(crate::ops::Request::Undo(crate::ops::Undo::Trashed(vec![was_at])));
                }
            }
            Action::AudioSeek | Action::QuickContent => {}
            Action::ToggleHidden => self.toggle_hidden(),
            Action::ToggleMotion => self.toggle_motion(),
            Action::Fit => {
                self.follow = true;
                self.fit_all(false);
            }
            Action::Reset => self.reset_tree(),
            Action::ToggleSelected => {
                let id = self.selected;
                self.toggle_branch(id);
            }
            Action::CollapseBranch(id) => {
                self.toggle_branch(id);
                self.refresh_hover();
            }
            Action::Sort(by) => {
                self.sort = by;
                self.list_dirty = true;
                self.panel_scroll.snap(0.0);
            }
            Action::Row(row) if self.acts.ctrl || self.acts.shift => self.mark_row(row),
            Action::Row(row) => {
                self.clear_marks();
                self.acts.range_anchor = Some(row);
                let double = self.last_row_click.1 == row && self.time - self.last_row_click.0 < 0.38;
                self.last_row_click = (self.time, row);
                self.selected_row = Some(row);
                self.activate_row(row, double);
            }
            Action::PanelScroll | Action::Nothing => {}
        }
    }

    fn go_crumb(&mut self, index: usize) {
        let crumbs = self.crumbs();
        let Some(path) = crumbs.get(index).and_then(|c| c.1.clone()) else {
            // The very top: the list of disks.
            while self.tree.root_path.is_some() {
                if !self.reroot_up() {
                    break;
                }
            }
            let root = self.tree.root;
            self.select(root, true);
            return;
        };
        self.navigate_to(path, true);
    }

    /// Breadcrumbs for the current selection: label and path.
    pub fn crumbs(&self) -> Vec<(String, Option<PathBuf>)> {
        let mut out = Vec::new();
        let id = if self.tree.is_valid(self.selected) { self.selected } else { self.tree.root };
        let Some(path) = self.tree.path_of(id) else {
            out.push(("This PC".to_string(), None));
            return out;
        };
        if self.tree.root_path.is_none() {
            out.push(("This PC".to_string(), None));
        }
        let mut built = PathBuf::new();
        for comp in path.components() {
            built.push(comp);
            match comp {
                std::path::Component::Prefix(p) => {
                    built.push(std::path::MAIN_SEPARATOR_STR);
                    out.push((p.as_os_str().to_string_lossy().into_owned(), Some(built.clone())));
                }
                std::path::Component::RootDir => {
                    if out.is_empty() {
                        out.push(("/".to_string(), Some(built.clone())));
                    }
                }
                std::path::Component::Normal(name) => {
                    out.push((name.to_string_lossy().into_owned(), Some(built.clone())));
                }
                _ => {}
            }
        }
        out
    }

    // ------------------------------------------------------------ events and time

    pub fn pump_events(&mut self) {
        while let Ok(event) = self.disk.events.try_recv() {
            self.redraw = true;
            match event {
                FsEvent::Listed { path, entries, error } => {
                    let Some(id) = self.tree.find_path(&path) else {
                        // A folder that was collapsed or removed before its contents arrived.
                        continue;
                    };
                    if !self.tree.get(id).expanded && self.tree.get(id).load != Load::Loading {
                        continue;
                    }
                    self.apply_listing(id, &entries, error.as_deref());
                    if error.is_some() {
                        self.tree.get_mut(id).bloom.target = 0.0;
                        self.advance_failed_nav(id);
                    }
                }
                FsEvent::DriveInfo { name, info } => {
                    self.drive_info.insert(name, info);
                    self.list_dirty = true;
                    self.labels_dirty = true;
                }
                FsEvent::Changed { dir } => {
                    if let Some(id) = self.tree.find_path(&dir)
                        && self.tree.get(id).expanded
                        && self.tree.get(id).load == Load::Loaded
                    {
                        self.disk.list(dir);
                    }
                }
                FsEvent::SearchHits { generation, hits } => {
                    if generation == self.search_generation {
                        self.search_hits.extend(hits);
                        self.list_dirty = true;
                    }
                }
                FsEvent::SearchDone { generation, capped, .. } => {
                    if generation == self.search_generation {
                        self.search_running = false;
                        self.search_capped = capped;
                        self.list_dirty = true;
                    }
                }
                FsEvent::Preview { path, preview } => {
                    let current =
                        (self.selected != NONE).then(|| self.tree.path_of(self.selected)).flatten();
                    if current.as_deref() != Some(path.as_path()) {
                        continue;
                    }
                    // A slow small preview mustn't replace the large one Quick Look asked for.
                    if self.acts.quick.is_some()
                        && !self.acts.quick_stale
                        && let (Preview::Image { width, .. }, Some(LoadedPreview { preview: Preview::Image { width: have, .. } })) =
                            (&preview, &self.preview)
                        && width < have
                    {
                        continue;
                    }
                    let preview = match preview {
                        Preview::Image { width, height, full_width, full_height, rgba } => {
                            self.image_pending = Some((width, height, rgba));
                            self.image_size = (full_width, full_height);
                            Preview::Image { width, height, full_width, full_height, rgba: Vec::new() }
                        }
                        other => {
                            if self.acts.quick_stale {
                                self.image_clear = true;
                                self.image_size = (0, 0);
                            }
                            other
                        }
                    };
                    self.acts.quick_stale = false;
                    self.preview = Some(LoadedPreview { preview });
                }
                FsEvent::Facts { path, created, read_only } => {
                    if self.selected_path().as_deref() == Some(path.as_path()) {
                        self.acts.facts = Some((path, created, read_only));
                    }
                }
                FsEvent::FolderTotals { path, bytes, files, folders } => {
                    if self.selected_path().as_deref() == Some(path.as_path()) {
                        self.acts.totals = Some((path, bytes, files, folders));
                    }
                }
                FsEvent::OpProgress(progress) => self.acts.working = Some(progress),
                FsEvent::OpDone(outcome) => self.on_outcome(*outcome),
                FsEvent::Bin(result) => match result {
                    Ok(items) => {
                        if self.acts.bin.is_some() {
                            self.acts.bin = Some(items);
                            self.list_dirty = true;
                        }
                    }
                    Err(err) => {
                        self.acts.bin = None;
                        self.toast(err, None, true);
                    }
                },
            }
        }
    }

    fn advance_failed_nav(&mut self, id: NodeId) {
        if self.pending_nav.is_some() {
            self.pending_nav = None;
            self.select(id, false);
            self.settle_camera(id);
        }
    }

    /// Advances the world. Returns true if another frame will be needed.
    pub fn tick(&mut self, dt: f32) -> bool {
        self.time += dt;
        self.frame_count += 1;
        let mut moving = false;

        if let Some(due) = self.search_due
            && self.time >= due
        {
            self.search_due = None;
            let roots: Vec<PathBuf> = match self.tree.root_path.clone() {
                Some(root) => vec![root],
                None => crate::fs::list_places().iter().map(|e| PathBuf::from(format!("{}\\", e.name))).collect(),
            };
            self.search_hits.clear();
            self.search_generation =
                self.disk.search(roots, self.search_text.trim().to_string(), self.tree.show_hidden);
        }
        if self.search_due.is_some() {
            moving = true;
        }

        // Stillness is a feature: the breeze dies down soon after the last interaction.
        self.idle_for += dt;
        if self.idle_for > 3.0 || !self.focused || self.reduced_motion {
            self.wind.target = 0.0;
        }
        let breeze = self.wind.step(dt, 2.2);

        let mut layout = false;
        layout |= self.camera.ox.step_within(dt, 9.0, 0.05);
        layout |= self.camera.oy.step_within(dt, 9.0, 0.05);
        layout |= self.camera.log_zoom.step(dt, 9.0);
        moving |= self.panel_scroll.step(dt, 22.0);
        moving |= self.text_scroll.step(dt, 22.0);
        moving |= self.step_overlays(dt);

        for node in &mut self.tree.nodes {
            if !node.alive {
                continue;
            }
            if node.delay > 0.0 {
                node.delay -= dt;
                layout = true;
                continue;
            }
            let grow_speed = if node.is_file() { 9.0 } else { 6.5 };
            let bloom_speed = if node.is_file() { 3.2 } else { 7.0 };
            layout |= node.grow.step(dt, grow_speed);
            // A flower can't open until its stalk has reached out.
            if !node.is_file() || node.grow.value > 0.45 || node.bloom.target == 0.0 {
                moving |= node.bloom.step(dt, bloom_speed);
            } else if node.bloom.target != node.bloom.value {
                moving = true;
            }
            moving |= node.lift.step(dt, 18.0);
            layout |= node.a_theta.step(dt, 8.0);
            layout |= node.a_length.step_within(dt, 8.0, 0.05);
        }

        if !self.particles.is_empty() {
            for p in &mut self.particles {
                p.vel[1] -= 260.0 * dt;
                p.vel[0] *= 1.0 - 0.8 * dt;
                p.pos[0] += p.vel[0] * dt;
                p.pos[1] += p.vel[1] * dt;
                p.rotation += p.spin * dt;
                p.life -= dt * 0.75;
            }
            self.particles.retain(|p| p.life > 0.0);
            moving = true;
        }

        self.layout_moving = layout;
        moving |= layout;
        self.finish_retractions();
        self.only_breeze = breeze && !moving;
        moving |= breeze;

        if let Some((_, until)) = self.status_note
            && self.wall > until
        {
            self.status_note = None;
            self.redraw = true;
        }

        moving
    }

    fn finish_retractions(&mut self) {
        let mut i = 0;
        let mut changed = false;
        while i < self.collapsing.len() {
            let id = self.collapsing[i];
            let done = !self.tree.is_valid(id)
                || self.tree.get(id).children.iter().all(|c| self.tree.get(*c).grow.value < 0.02);
            if done {
                self.collapsing.swap_remove(i);
                if self.tree.is_valid(id) && !self.tree.get(id).expanded {
                    // Free the branch's contents; it'll be read fresh if opened again.
                    self.tree.clear_children(id);
                    let node = self.tree.get_mut(id);
                    node.load = Load::Unloaded;
                    node.dirty = true;
                    changed = true;
                }
            } else {
                i += 1;
            }
        }
        let mut i = 0;
        while i < self.dying.len() {
            let id = self.dying[i];
            let done = !self.tree.is_valid(id) || self.tree.get(id).grow.value < 0.02;
            if done {
                self.dying.swap_remove(i);
                if self.tree.is_valid(id) {
                    if self.tree.is_ancestor_or_self(id, self.selected) {
                        let parent = self.tree.logical_parent(id);
                        self.select(parent, false);
                    }
                    self.tree.remove(id);
                    changed = true;
                }
            } else {
                i += 1;
            }
        }
        if changed {
            layout::relayout(&mut self.tree);
            self.after_layout();
            self.list_dirty = true;
        }
    }

    pub fn resize(&mut self, width: f32, height: f32, scale: f32) {
        self.width = width;
        self.height = height;
        self.scale = scale;
        self.labels_dirty = true;
        if self.follow {
            self.fit_all(true);
        }
        self.redraw = true;
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
        if focused {
            self.wake_wind();
        }
        self.redraw = true;
    }

    /// How long until something needs a frame even though nothing is moving.
    pub fn next_due(&self) -> Option<f32> {
        let toast = self.acts.toasts.iter().map(|t| t.until).fold(f32::MAX, f32::min);
        let note = self.status_note.as_ref().map_or(f32::MAX, |n| n.1);
        let due = toast.min(note);
        (due < f32::MAX).then_some((due - self.wall).max(0.02))
    }

    pub fn selected_path(&self) -> Option<PathBuf> {
        if self.tree.is_valid(self.selected) { self.tree.path_of(self.selected) } else { None }
    }

    /// Whether this node is a whole disk, directly under This PC.
    pub fn is_drive(&self, id: NodeId) -> bool {
        let parent = self.tree.get(id).parent;
        parent != NONE && self.tree.get(parent).kind == Kind::Places
    }

    /// Finds a loaded node by name, for scripted captures.
    pub fn find_by_name(&self, name: &str) -> NodeId {
        for (i, node) in self.tree.nodes.iter().enumerate() {
            if node.alive && !node.is_sprig() && &*node.name == name {
                return i as NodeId;
            }
        }
        NONE
    }
}

fn rest_length(node: &crate::tree::Node) -> f32 {
    node.a_length.target
}

pub fn inside(r: [f32; 4], x: f32, y: f32) -> bool {
    x >= r[0] && x < r[2] && y >= r[1] && y < r[3]
}

pub fn home_dir_string() -> String {
    std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// Framing a tiny target past the zoom limit must still centre it.
    #[test]
    fn framing_past_the_zoom_limit_still_centres_the_target() {
        let waker: Waker = Arc::new(|| {});
        let mut app = App::new(None, waker, 1440.0, 900.0, 1.0);
        let target = [1000.0, 3000.0];
        let tiny = [target[0] - 0.01, target[1] - 0.01, target[0] + 0.01, target[1] + 0.01];
        app.frame_bounds(tiny, true, 10_000.0, false);
        let zoom = app.camera.zoom();
        assert!((zoom - MAX_ZOOM_LN.exp()).abs() < 0.01, "{zoom}");
        let stage = app.stage();
        let x = app.camera.ox.value + target[0] * zoom;
        let y = app.camera.oy.value - target[1] * zoom;
        assert!(x > stage[0] && x < stage[2] && y > stage[1] && y < stage[3], "landed at {x},{y}");
    }
}
