//! The living tree: an arena of nodes. Folders become branches, files become flowers,
//! and very full folders are split into sprigs so every flower still has a place.

use crate::fs::{Entry, natural_cmp};
use std::path::{Path, PathBuf};

pub type NodeId = u32;
pub const NONE: NodeId = u32::MAX;

/// Most children one tip can carry before they are gathered into sprigs.
pub const SPRIG_THRESHOLD: usize = 28;
/// How many children a sprig starts with on first load.
pub const SPRIG_SIZE: usize = 10;
/// A sprig that grows past this through live changes is split in two.
const SPRIG_SPLIT_AT: usize = 20;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Species {
    Daisy,    // documents
    Poppy,    // images
    Trillium, // video
    Bellflower, // audio
    Star,     // code
    Quince,   // data
    Allium,   // archives
    Dahlia,   // apps and installers
    Clover,   // everything else
}

impl Species {
    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_name(name: &str) -> Species {
        let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
        match ext.as_str() {
            "pdf" | "doc" | "docx" | "odt" | "rtf" | "txt" | "md" | "markdown" | "tex" | "epub" | "pages" | "ppt"
            | "pptx" | "key" | "odp" | "log" | "rst" | "org" => Species::Daisy,
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" | "tif" | "tiff" | "heic" | "avif" | "raw" | "cr2"
            | "nef" | "psd" | "ai" | "ico" | "fig" | "sketch" | "xcf" => Species::Poppy,
            "mp4" | "mov" | "mkv" | "avi" | "webm" | "wmv" | "m4v" | "flv" | "mpg" | "mpeg" => Species::Trillium,
            "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" | "opus" | "aiff" | "wma" | "mid" | "midi" => {
                Species::Bellflower
            }
            "rs" | "py" | "js" | "ts" | "tsx" | "jsx" | "c" | "h" | "cpp" | "hpp" | "cc" | "cs" | "go" | "java" | "kt"
            | "swift" | "rb" | "php" | "lua" | "sh" | "bat" | "ps1" | "html" | "htm" | "css" | "scss" | "vue"
            | "svelte" | "sql" | "wgsl" | "glsl" | "zig" | "dart" | "r" | "pl" | "ex" | "exs" | "hs" | "ml" => {
                Species::Star
            }
            "csv" | "tsv" | "xls" | "xlsx" | "ods" | "json" | "jsonl" | "xml" | "yaml" | "yml" | "toml" | "ini"
            | "cfg" | "conf" | "db" | "sqlite" | "parquet" | "lock" | "env" => Species::Quince,
            "zip" | "7z" | "rar" | "tar" | "gz" | "bz2" | "xz" | "zst" | "iso" | "dmg" | "cab" | "tgz" => {
                Species::Allium
            }
            "exe" | "msi" | "app" | "deb" | "rpm" | "apk" | "appimage" | "dll" | "so" | "dylib" | "lnk" | "url" => {
                Species::Dahlia
            }
            _ => Species::Clover,
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            Species::Daisy => "Document",
            Species::Poppy => "Image",
            Species::Trillium => "Video",
            Species::Bellflower => "Audio",
            Species::Star => "Source code",
            Species::Quince => "Data",
            Species::Allium => "Archive",
            Species::Dahlia => "Application",
            Species::Clover => "File",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// A real folder.
    Dir,
    /// A real file.
    File(Species),
    /// A display-only group: holds a slice of a full folder's children.
    Sprig,
    /// The synthetic root that holds the drives.
    Places,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Load {
    Unloaded,
    Loading,
    Loaded,
    Failed,
}

/// A value that eases toward a target with a critically damped spring.
#[derive(Clone, Copy, Debug, Default)]
pub struct Spring {
    pub value: f32,
    pub velocity: f32,
    pub target: f32,
}

impl Spring {
    pub fn new(value: f32) -> Self {
        Spring { value, velocity: 0.0, target: value }
    }

    /// Returns true while still moving.
    #[inline]
    pub fn step(&mut self, dt: f32, omega: f32) -> bool {
        self.step_within(dt, omega, 0.0004)
    }

    /// As `step`, settling once within `tolerance`. Screen positions need a coarser one:
    /// far into a deep zoom they're so large that a fine tolerance is below what an f32
    /// can even represent, and the spring would never come to rest.
    #[inline]
    pub fn step_within(&mut self, dt: f32, omega: f32, tolerance: f32) -> bool {
        if self.value == self.target && self.velocity == 0.0 {
            return false;
        }
        let x = omega * dt;
        let decay = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x);
        let change = self.value - self.target;
        let temp = (self.velocity + omega * change) * dt;
        self.velocity = (self.velocity - omega * temp) * decay;
        self.value = self.target + (change + temp) * decay;
        if (self.value - self.target).abs() < tolerance && self.velocity.abs() < tolerance * 5.0 {
            self.value = self.target;
            self.velocity = 0.0;
            return false;
        }
        true
    }

    pub fn snap(&mut self, value: f32) {
        self.value = value;
        self.target = value;
        self.velocity = 0.0;
    }
}

pub struct Node {
    pub parent: NodeId,
    pub kind: Kind,
    pub name: Box<str>,
    pub children: Vec<NodeId>,
    pub size: u64,
    pub modified: i64,
    pub hidden: bool,
    pub load: Load,
    pub expanded: bool,
    pub error: Option<Box<str>>,
    pub seed: u32,
    /// All entries seen in this folder, used to report counts for hidden items.
    pub file_count: u32,
    pub dir_count: u32,

    // Layout output, in the parent's frame.
    pub theta: f32,
    pub length: f32,
    pub ext_center: f32,
    pub ext_radius: f32,
    /// How far this limb's branches fan out from its tip, as a half-angle.
    pub wedge: f32,
    /// How far they reach from its tip.
    pub reach: f32,
    /// Room needed around the outermost flowers and buds.
    pub margin: f32,
    pub weight: u32,
    pub dirty: bool,
    /// Everything beyond this limb's tip is drawn at this size, so a giant folder forms
    /// a dense crown of small flowers instead of dwarfing the rest of the tree.
    pub subscale: f32,

    // Animated state.
    pub a_theta: Spring,
    pub a_length: Spring,
    /// 0 = not grown out at all, 1 = fully out.
    pub grow: Spring,
    /// 0 = closed bud, 1 = fully open.
    pub bloom: Spring,
    /// How far this piece of paper is lifted off the page.
    pub lift: Spring,
    /// Seconds to wait before growth begins, for cascades.
    pub delay: f32,
    pub dying: bool,
    pub alive: bool,

    // World-space values, refreshed each frame.
    pub base: [f32; 2],
    pub tip: [f32; 2],
    pub heading: f32,
    pub width: f32,
    pub dim: f32,
    pub label_alpha: f32,
    /// Which side the label last sat on, so it doesn't hop between frames.
    pub label_slot: u8,
    /// Where the label sits relative to its anchor, eased so it glides to a new spot.
    pub label_off: [f32; 2],
}

impl Node {
    fn new(parent: NodeId, kind: Kind, name: Box<str>) -> Node {
        let seed = hash_str(&name);
        Node {
            parent,
            kind,
            name,
            children: Vec::new(),
            size: 0,
            modified: 0,
            hidden: false,
            load: Load::Unloaded,
            expanded: false,
            error: None,
            seed,
            file_count: 0,
            dir_count: 0,
            theta: 0.0,
            length: 0.0,
            ext_center: 0.0,
            ext_radius: 0.0,
            wedge: 0.0,
            reach: 0.0,
            margin: 0.0,
            weight: 1,
            dirty: true,
            subscale: 1.0,
            a_theta: Spring::new(0.0),
            a_length: Spring::new(0.0),
            grow: Spring::new(0.0),
            bloom: Spring::new(0.0),
            lift: Spring::new(0.0),
            delay: 0.0,
            dying: false,
            alive: true,
            base: [0.0; 2],
            tip: [0.0; 2],
            heading: 0.0,
            width: 1.0,
            dim: 0.0,
            label_alpha: 0.0,
            label_slot: 255,
            label_off: [0.0, 0.0],
        }
    }

    pub fn is_dir(&self) -> bool {
        matches!(self.kind, Kind::Dir | Kind::Places)
    }

    pub fn is_file(&self) -> bool {
        matches!(self.kind, Kind::File(_))
    }

    pub fn is_sprig(&self) -> bool {
        matches!(self.kind, Kind::Sprig)
    }

    pub fn species(&self) -> Species {
        match self.kind {
            Kind::File(s) => s,
            _ => Species::Clover,
        }
    }
}

pub fn hash_str(text: &str) -> u32 {
    let mut h: u32 = 2166136261;
    for b in text.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// A stable pseudo-random value in 0..1 from a seed and a channel.
#[inline]
pub fn rand01(seed: u32, channel: u32) -> f32 {
    let mut x = seed.wrapping_add(channel.wrapping_mul(0x9E3779B9));
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B3C6D);
    x ^= x >> 12;
    x = x.wrapping_mul(0x297A2D39);
    x ^= x >> 15;
    (x as f32) / (u32::MAX as f32)
}

pub struct Tree {
    pub nodes: Vec<Node>,
    free: Vec<NodeId>,
    pub root: NodeId,
    /// Absolute path of the root, or None when rooted at the drive list.
    pub root_path: Option<PathBuf>,
    pub show_hidden: bool,
}

impl Tree {
    pub fn new(root_path: Option<PathBuf>) -> Tree {
        let mut tree = Tree { nodes: Vec::new(), free: Vec::new(), root: NONE, root_path: None, show_hidden: false };
        tree.root_path = root_path.clone();
        let (kind, name): (Kind, Box<str>) = match &root_path {
            Some(path) => (Kind::Dir, display_name(path).into_boxed_str()),
            None => (Kind::Places, "This PC".into()),
        };
        tree.root = tree.alloc(NONE, kind, name);
        tree
    }

    fn alloc(&mut self, parent: NodeId, kind: Kind, name: Box<str>) -> NodeId {
        let node = Node::new(parent, kind, name);
        if let Some(id) = self.free.pop() {
            self.nodes[id as usize] = node;
            id
        } else {
            self.nodes.push(node);
            (self.nodes.len() - 1) as NodeId
        }
    }

    #[inline]
    pub fn get(&self, id: NodeId) -> &Node {
        &self.nodes[id as usize]
    }

    #[inline]
    pub fn get_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id as usize]
    }

    pub fn is_valid(&self, id: NodeId) -> bool {
        id != NONE && (id as usize) < self.nodes.len() && self.nodes[id as usize].alive
    }

    /// Marks a node and everything above it as needing new layout.
    pub fn mark_dirty(&mut self, mut id: NodeId) {
        while id != NONE {
            let node = &mut self.nodes[id as usize];
            node.dirty = true;
            id = node.parent;
        }
    }

    /// The real folder or file above a sprig, skipping display-only groups.
    pub fn logical_parent(&self, id: NodeId) -> NodeId {
        let mut cur = self.get(id).parent;
        while cur != NONE && self.get(cur).is_sprig() {
            cur = self.get(cur).parent;
        }
        cur
    }

    /// Real children of a folder, looking through any sprigs.
    pub fn logical_children(&self, id: NodeId, out: &mut Vec<NodeId>) {
        for &child in &self.get(id).children {
            if self.get(child).is_sprig() {
                self.logical_children(child, out);
            } else {
                out.push(child);
            }
        }
    }

    pub fn path_of(&self, id: NodeId) -> Option<PathBuf> {
        if self.get(id).kind == Kind::Places {
            return None;
        }
        let mut names: Vec<&str> = Vec::new();
        let mut cur = id;
        loop {
            let node = self.get(cur);
            if node.parent == NONE {
                break;
            }
            if !node.is_sprig() {
                names.push(&node.name);
            }
            cur = node.parent;
        }
        let mut path = match &self.root_path {
            Some(p) => p.clone(),
            None => PathBuf::new(),
        };
        for (i, name) in names.iter().rev().enumerate() {
            if i == 0 && self.root_path.is_none() {
                // A drive root needs its trailing separator to mean the drive itself.
                #[cfg(windows)]
                path.push(format!("{name}\\"));
                #[cfg(not(windows))]
                path.push(name);
            } else {
                path.push(name);
            }
        }
        Some(path)
    }

    /// Finds a loaded node by absolute path.
    pub fn find_path(&self, path: &Path) -> Option<NodeId> {
        let mut cur = self.root;
        let rest: &Path = match &self.root_path {
            Some(root) => path.strip_prefix(root).ok()?,
            None => path,
        };
        let mut kids = Vec::new();
        for comp in rest.components() {
            let name = component_name(&comp);
            if name.is_empty() {
                continue;
            }
            kids.clear();
            self.logical_children(cur, &mut kids);
            cur = *kids.iter().find(|k| names_equal(&self.get(**k).name, &name))?;
        }
        Some(cur)
    }

    pub fn is_ancestor_or_self(&self, ancestor: NodeId, mut id: NodeId) -> bool {
        while id != NONE {
            if id == ancestor {
                return true;
            }
            id = self.get(id).parent;
        }
        false
    }

    /// Applies a fresh listing. Unchanged items keep their place; new ones grow in and
    /// removed ones shed. Returns (added, removed) counts.
    pub fn apply_listing(&mut self, dir: NodeId, entries: &[Entry], error: Option<&str>) -> (usize, usize) {
        let first_load = self.get(dir).load != Load::Loaded;
        self.get_mut(dir).error = error.map(|e| e.to_string().into_boxed_str());
        self.get_mut(dir).load = if error.is_some() { Load::Failed } else { Load::Loaded };
        if error.is_some() {
            return (0, 0);
        }

        let (mut files, mut dirs) = (0u32, 0u32);
        let visible: Vec<&Entry> = entries
            .iter()
            .filter(|e| {
                if e.is_dir {
                    dirs += 1;
                } else {
                    files += 1;
                }
                self.show_hidden || !e.hidden
            })
            .collect();
        {
            let node = self.get_mut(dir);
            node.file_count = files;
            node.dir_count = dirs;
        }

        let mut existing = Vec::new();
        self.logical_children(dir, &mut existing);
        let mut by_name: rustc_hash::FxHashMap<&str, NodeId> = rustc_hash::FxHashMap::default();
        for &id in &existing {
            if !self.get(id).dying {
                by_name.insert(&self.nodes[id as usize].name, id);
            }
        }

        let mut seen = rustc_hash::FxHashSet::default();
        let mut updates: Vec<(NodeId, &Entry)> = Vec::new();
        let mut additions: Vec<&Entry> = Vec::new();
        for entry in &visible {
            match by_name.get(&*entry.name) {
                Some(&id) => {
                    seen.insert(id);
                    updates.push((id, entry));
                }
                None => additions.push(entry),
            }
        }
        let removals: Vec<NodeId> =
            existing.iter().copied().filter(|id| !seen.contains(id) && !self.get(*id).dying).collect();

        for (id, entry) in updates {
            let node = self.get_mut(id);
            node.size = entry.size;
            node.modified = entry.modified;
        }
        for &id in &removals {
            self.begin_dying(id);
        }

        let added = additions.len();
        let removed = removals.len();
        if first_load && self.get(dir).children.is_empty() {
            self.seed_children(dir, &additions);
        } else {
            for entry in additions {
                self.insert_live(dir, entry);
            }
            if self.get(dir).children.len() > SPRIG_THRESHOLD * 2 {
                self.regather(dir);
            }
        }
        self.mark_dirty(dir);
        (added, removed)
    }

    fn make_child(&mut self, parent: NodeId, entry: &Entry) -> NodeId {
        let kind = if entry.is_dir { Kind::Dir } else { Kind::File(Species::from_name(&entry.name)) };
        let id = self.alloc(parent, kind, entry.name.clone());
        let node = self.get_mut(id);
        node.size = entry.size;
        node.modified = entry.modified;
        node.hidden = entry.hidden;
        id
    }

    /// First load: creates the children and arranges them for display.
    fn seed_children(&mut self, dir: NodeId, entries: &[&Entry]) {
        let ids: Vec<NodeId> = entries.iter().map(|e| self.make_child(dir, e)).collect();
        self.arrange(dir, ids);
    }

    /// Sets a folder's display children from its real children: folders central, files
    /// flanking them, and gathered into sprigs when there are too many for one tip.
    fn arrange(&mut self, dir: NodeId, ids: Vec<NodeId>) {
        let (folder_ids, file_ids): (Vec<NodeId>, Vec<NodeId>) = ids.into_iter().partition(|i| self.get(*i).is_dir());
        let mut display = Vec::new();
        let total = folder_ids.len() + file_ids.len();
        if total <= SPRIG_THRESHOLD {
            for &id in folder_ids.iter().chain(file_ids.iter()) {
                self.get_mut(id).parent = dir;
            }
            let (mut left, mut right) = (Vec::new(), Vec::new());
            for (i, id) in file_ids.iter().enumerate() {
                if i % 2 == 0 {
                    left.push(*id);
                } else {
                    right.push(*id);
                }
            }
            right.reverse();
            display.extend(left);
            display.extend(folder_ids);
            display.extend(right);
        } else {
            let file_groups = self.gather(dir, file_ids);
            let folder_groups = self.gather(dir, folder_ids);
            let split = file_groups.len().div_ceil(2);
            display.extend_from_slice(&file_groups[..split]);
            display.extend(folder_groups);
            display.extend_from_slice(&file_groups[split..]);
        }
        self.get_mut(dir).children = display;
    }

    /// Rebuilds sprigs after many direct children arrived, keeping every real node.
    fn regather(&mut self, dir: NodeId) {
        let mut ids = Vec::new();
        self.logical_children(dir, &mut ids);
        ids.sort_by(|a, b| natural_cmp(&self.get(*a).name, &self.get(*b).name));
        let old = std::mem::take(&mut self.get_mut(dir).children);
        for id in old {
            if self.get(id).is_sprig() {
                self.free_sprigs(id);
            }
        }
        self.arrange(dir, ids);
    }

    /// Frees a sprig and any sprigs inside it, without touching the real nodes they held.
    fn free_sprigs(&mut self, sprig: NodeId) {
        let kids = std::mem::take(&mut self.get_mut(sprig).children);
        for kid in kids {
            if self.get(kid).is_sprig() {
                self.free_sprigs(kid);
            }
        }
        self.get_mut(sprig).alive = false;
        self.free.push(sprig);
    }

    /// Gathers items into sprigs, nesting sprigs when needed, returning the top-level ids.
    fn gather(&mut self, parent: NodeId, items: Vec<NodeId>) -> Vec<NodeId> {
        if items.len() <= SPRIG_SIZE {
            for &id in &items {
                self.get_mut(id).parent = parent;
            }
            return items;
        }
        let size = sprig_size_for(items.len());
        let mut level = items;
        while level.len() > SPRIG_SIZE {
            let groups = level.len().div_ceil(size);
            let mut next = Vec::with_capacity(groups);
            for group in 0..groups {
                let chunk = &level[group * level.len() / groups..(group + 1) * level.len() / groups];
                let sprig = self.alloc(parent, Kind::Sprig, "".into());
                self.get_mut(sprig).expanded = true;
                self.get_mut(sprig).load = Load::Loaded;
                let label = format!("{}{}", first_label(self, chunk[0]), "");
                self.get_mut(sprig).name = label.into_boxed_str();
                self.get_mut(sprig).seed = self.get(chunk[0]).seed ^ 0x5bd1_e995;
                for &id in chunk {
                    self.get_mut(id).parent = sprig;
                }
                self.get_mut(sprig).children = chunk.to_vec();
                next.push(sprig);
            }
            level = next;
        }
        for &id in &level {
            self.get_mut(id).parent = parent;
        }
        level
    }

    /// Live change: finds a stable home without reshuffling anything else.
    fn insert_live(&mut self, dir: NodeId, entry: &Entry) {
        let id = self.make_child(dir, entry);
        let is_dir = entry.is_dir;
        let mut home = dir;
        loop {
            let kids = &self.get(home).children;
            // Descend into the last sprig whose first real name is not after this one.
            let sprigs: Vec<NodeId> = kids
                .iter()
                .copied()
                .filter(|k| self.get(*k).is_sprig() && self.sprig_holds_dirs(*k) == is_dir)
                .collect();
            if sprigs.is_empty() {
                break;
            }
            let mut pick = sprigs[0];
            for s in sprigs {
                if natural_cmp(&self.get(s).name, &entry.name).is_le() {
                    pick = s;
                }
            }
            home = pick;
        }
        self.get_mut(id).parent = home;
        let position = {
            let kids = &self.get(home).children;
            kids.iter()
                .position(|k| {
                    let node = self.get(*k);
                    node.is_dir() == is_dir && natural_cmp(&node.name, &entry.name).is_gt()
                })
                .unwrap_or(kids.len())
        };
        self.get_mut(home).children.insert(position, id);
        if self.get(home).is_sprig() && self.get(home).children.len() > SPRIG_SPLIT_AT {
            self.split_sprig(home);
        }
    }

    fn sprig_holds_dirs(&self, mut sprig: NodeId) -> bool {
        loop {
            let Some(&first) = self.get(sprig).children.first() else { return false };
            let node = self.get(first);
            if node.is_sprig() {
                sprig = first;
            } else {
                return node.is_dir();
            }
        }
    }

    fn split_sprig(&mut self, sprig: NodeId) {
        let parent = self.get(sprig).parent;
        let mid = self.get(sprig).children.len() / 2;
        let tail = self.get_mut(sprig).children.split_off(mid);
        let new_id = self.alloc(parent, Kind::Sprig, "".into());
        let label = first_label(self, tail[0]);
        for &id in &tail {
            self.get_mut(id).parent = new_id;
        }
        let seed = self.get(tail[0]).seed ^ 0x5bd1_e995;
        let node = self.get_mut(new_id);
        node.children = tail;
        node.expanded = true;
        node.load = Load::Loaded;
        node.name = label.into_boxed_str();
        node.seed = seed;
        let at = self.get(parent).children.iter().position(|c| *c == sprig).map_or(0, |p| p + 1);
        self.get_mut(parent).children.insert(at, new_id);
    }

    pub fn begin_dying(&mut self, id: NodeId) {
        let node = self.get_mut(id);
        node.dying = true;
        node.delay = 0.0;
        node.grow.target = 0.0;
        node.bloom.target = 0.0;
    }

    /// Frees a node and everything below it.
    pub fn remove(&mut self, id: NodeId) {
        let parent = self.get(id).parent;
        if parent != NONE {
            self.get_mut(parent).children.retain(|c| *c != id);
            self.mark_dirty(parent);
        }
        self.release(id);
        if parent != NONE && self.get(parent).is_sprig() && self.get(parent).children.is_empty() {
            self.remove(parent);
        }
    }

    /// Frees everything below a node, keeping the node.
    pub fn clear_children(&mut self, id: NodeId) {
        let kids = std::mem::take(&mut self.get_mut(id).children);
        for kid in kids {
            self.release(kid);
        }
    }

    fn release(&mut self, id: NodeId) {
        let kids = std::mem::take(&mut self.get_mut(id).children);
        for kid in kids {
            self.release(kid);
        }
        let node = self.get_mut(id);
        node.alive = false;
        node.name = "".into();
        node.children = Vec::new();
        node.error = None;
        self.free.push(id);
    }

    /// Roots the tree one level higher, keeping the existing tree as a branch of the new one.
    /// Returns the new root's id, or None if already at the top.
    pub fn reroot_up(&mut self) -> Option<NodeId> {
        let current = self.root_path.clone()?;
        let old_root = self.root;
        let parent_path = current.parent().map(Path::to_path_buf);
        let (kind, name, new_path): (Kind, Box<str>, Option<PathBuf>) = match parent_path {
            Some(p) => (Kind::Dir, display_name(&p).into_boxed_str(), Some(p)),
            None => (Kind::Places, "This PC".into(), None),
        };
        let new_root = self.alloc(NONE, kind, name);
        self.get_mut(new_root).expanded = true;
        self.get_mut(new_root).load = Load::Loading;
        self.get_mut(old_root).parent = new_root;
        self.get_mut(old_root).name = child_name(&current).into_boxed_str();
        self.get_mut(new_root).children.push(old_root);
        self.root = new_root;
        self.root_path = new_path;
        self.mark_dirty(old_root);
        Some(new_root)
    }

}

/// Picks a sprig size so that repeated grouping ends in a generous top level.
fn sprig_size_for(count: usize) -> usize {
    let mut levels = 1u32;
    while (SPRIG_SIZE as f64).powi(levels as i32) * 1.2 < count as f64 {
        levels += 1;
    }
    let size = (count as f64).powf(1.0 / levels as f64).ceil() as usize;
    size.clamp(4, SPRIG_SIZE + 2)
}

fn first_label(tree: &Tree, mut id: NodeId) -> String {
    while tree.get(id).is_sprig() {
        match tree.get(id).children.first() {
            Some(&c) => id = c,
            None => break,
        }
    }
    tree.get(id).name.to_string()
}

fn component_name(comp: &std::path::Component) -> String {
    match comp {
        std::path::Component::Prefix(p) => p.as_os_str().to_string_lossy().trim_end_matches('\\').to_string(),
        std::path::Component::Normal(s) => s.to_string_lossy().into_owned(),
        _ => String::new(),
    }
}

fn names_equal(a: &str, b: &str) -> bool {
    if cfg!(windows) { a.eq_ignore_ascii_case(b) || a.to_lowercase() == b.to_lowercase() } else { a == b }
}

/// The name a folder shows as a branch, or the whole drive for a drive root.
pub fn display_name(path: &Path) -> String {
    match path.file_name() {
        Some(name) => name.to_string_lossy().into_owned(),
        None => path.to_string_lossy().trim_end_matches(['\\', '/']).to_string(),
    }
}

fn child_name(path: &Path) -> String {
    display_name(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, is_dir: bool) -> Entry {
        Entry { name: name.into(), is_dir, hidden: false, size: 10, modified: 0 }
    }

    fn sorted(mut v: Vec<Entry>) -> Vec<Entry> {
        v.sort_by(|a, b| natural_cmp(&a.name, &b.name));
        v
    }

    #[test]
    fn a_spring_on_a_huge_coordinate_still_comes_to_rest() {
        let mut spring = Spring::new(0.0);
        spring.target = 105_373.6;
        let mut steps = 0;
        while spring.step_within(1.0 / 60.0, 9.0, 0.05) {
            steps += 1;
            assert!(steps < 600, "still moving after 10 seconds: {spring:?}");
        }
        assert_eq!(spring.value, spring.target);
    }

    #[test]
    fn species_are_chosen_by_extension() {
        assert_eq!(Species::from_name("a.PDF"), Species::Daisy);
        assert_eq!(Species::from_name("photo.jpeg"), Species::Poppy);
        assert_eq!(Species::from_name("main.rs"), Species::Star);
        assert_eq!(Species::from_name("noextension"), Species::Clover);
    }

    #[test]
    fn small_folder_stays_direct_with_folders_central() {
        let mut tree = Tree::new(Some(PathBuf::from("/root")));
        let entries = sorted(vec![entry("a.txt", false), entry("b", true), entry("c.txt", false), entry("d.txt", false)]);
        tree.apply_listing(tree.root, &entries, None);
        let kids = tree.get(tree.root).children.clone();
        assert_eq!(kids.len(), 4);
        assert!(tree.get(kids[2]).is_dir() || tree.get(kids[1]).is_dir());
    }

    #[test]
    fn big_folder_is_gathered_without_losing_anything() {
        let mut tree = Tree::new(Some(PathBuf::from("/root")));
        let entries: Vec<Entry> = (0..5000).map(|i| entry(&format!("f{i:05}.txt"), false)).collect();
        tree.apply_listing(tree.root, &entries, None);
        assert!(tree.get(tree.root).children.len() <= SPRIG_SIZE);
        let mut real = Vec::new();
        tree.logical_children(tree.root, &mut real);
        assert_eq!(real.len(), 5000);
        assert_eq!(tree.logical_parent(real[17]), tree.root);
        assert_eq!(tree.path_of(real[3]).unwrap(), PathBuf::from("/root").join("f00003.txt"));
    }

    #[test]
    fn live_changes_keep_existing_nodes() {
        let mut tree = Tree::new(Some(PathBuf::from("/root")));
        let first: Vec<Entry> = sorted(vec![entry("a.txt", false), entry("b.txt", false)]);
        tree.apply_listing(tree.root, &first, None);
        let mut before = Vec::new();
        tree.logical_children(tree.root, &mut before);

        let second: Vec<Entry> = sorted(vec![entry("a.txt", false), entry("c.txt", false)]);
        let (added, removed) = tree.apply_listing(tree.root, &second, None);
        assert_eq!((added, removed), (1, 1));

        let mut after = Vec::new();
        tree.logical_children(tree.root, &mut after);
        let a = before.iter().find(|i| &*tree.get(**i).name == "a.txt").unwrap();
        assert!(after.contains(a), "a.txt must keep its node");
        let b = before.iter().find(|i| &*tree.get(**i).name == "b.txt").unwrap();
        assert!(tree.get(*b).dying, "b.txt sheds before leaving");
    }

    #[test]
    fn live_insert_into_big_folder_stays_within_sprigs() {
        let mut tree = Tree::new(Some(PathBuf::from("/root")));
        let entries: Vec<Entry> = (0..300).map(|i| entry(&format!("f{i:05}.txt"), false)).collect();
        tree.apply_listing(tree.root, &entries, None);
        let top = tree.get(tree.root).children.clone();
        let mut more = entries.clone();
        for i in 0..40 {
            more.push(entry(&format!("f00100-{i}.txt"), false));
        }
        tree.apply_listing(tree.root, &more, None);
        let mut real = Vec::new();
        tree.logical_children(tree.root, &mut real);
        assert_eq!(real.len(), 340);
        for id in top {
            assert!(tree.get(id).alive);
        }
    }

    #[test]
    fn find_path_walks_through_sprigs() {
        let mut tree = Tree::new(Some(PathBuf::from("/root")));
        let entries: Vec<Entry> = (0..200).map(|i| entry(&format!("f{i:05}.txt"), false)).collect();
        tree.apply_listing(tree.root, &entries, None);
        let path = PathBuf::from("/root").join("f00123.txt");
        let id = tree.find_path(&path).expect("found");
        assert_eq!(&*tree.get(id).name, "f00123.txt");
    }

    #[test]
    fn reroot_keeps_the_old_tree_as_a_branch() {
        let mut tree = Tree::new(Some(PathBuf::from("/a/b")));
        let old = tree.root;
        tree.apply_listing(old, &[entry("x.txt", false)], None);
        let new_root = tree.reroot_up().unwrap();
        assert_eq!(tree.get(old).parent, new_root);
        assert_eq!(&*tree.get(old).name, "b");
        let kid = tree.get(old).children[0];
        assert_eq!(tree.path_of(kid).unwrap(), PathBuf::from("/a/b").join("x.txt"));
    }
}
