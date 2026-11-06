//! What the person can do to their files, and the state of the things that do it: the
//! context menu, dialogs, toasts, Quick Look, multi-selection, clipboard, and drag and drop.

use crate::app::{Action, App, Key};
use crate::audio::AudioPlayer;
use crate::layout::FLOWER_RADIUS;
use crate::ops::{self, Request, Undo};
use crate::tree::{Kind, NONE, NodeId, Spring};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Cmd {
    Open,
    OpenWith,
    QuickLook,
    Rename,
    Duplicate,
    Copy,
    Cut,
    Paste,
    Delete,
    NewFolder,
    NewFile,
    CopyPath,
    Reveal,
    Toggle,
    Reset,
    SelectAll,
    Undo,
    More,
    Extract,
    Compress,
    Terminal,
}

impl Cmd {
    pub fn label(self) -> &'static str {
        match self {
            Cmd::Open => "Open",
            Cmd::OpenWith => "Open with…",
            Cmd::QuickLook => "Quick Look",
            Cmd::Rename => "Rename",
            Cmd::Duplicate => "Duplicate",
            Cmd::Copy => "Copy",
            Cmd::Cut => "Cut",
            Cmd::Paste => "Paste",
            Cmd::Delete => "Move to Recycle Bin",
            Cmd::NewFolder => "New folder",
            Cmd::NewFile => "New file",
            Cmd::CopyPath => "Copy path",
            Cmd::Reveal => if cfg!(windows) { "Show in Explorer" } else { "Show in folder" },
            Cmd::Toggle => "Open or close branch",
            Cmd::Reset => "Collapse everything",
            Cmd::SelectAll => "Select all",
            Cmd::Undo => "Undo",
            Cmd::More => "More",
            Cmd::Extract => "Unpack here",
            Cmd::Compress => "Compress to zip",
            Cmd::Terminal => "Open terminal here",
        }
    }

    pub fn shortcut(self) -> &'static str {
        match self {
            Cmd::Open => "Enter",
            Cmd::QuickLook => "Space",
            Cmd::Rename => "F2",
            Cmd::Duplicate => "Ctrl+D",
            Cmd::Copy => "Ctrl+C",
            Cmd::Cut => "Ctrl+X",
            Cmd::Paste => "Ctrl+V",
            Cmd::Delete => "Del",
            Cmd::NewFolder => "Ctrl+Shift+N",
            Cmd::NewFile => "Ctrl+N",
            Cmd::CopyPath => "Ctrl+Shift+C",
            Cmd::Reset => "Ctrl+R",
            Cmd::SelectAll => "Ctrl+A",
            Cmd::Undo => "Ctrl+Z",
            _ => "",
        }
    }
}

pub struct MenuItem {
    pub cmd: Option<Cmd>,
    pub label: String,
    pub enabled: bool,
    pub danger: bool,
}

pub struct Menu {
    pub x: f32,
    pub y: f32,
    pub items: Vec<MenuItem>,
    /// Row the keyboard is on.
    pub cursor: Option<usize>,
    pub open: Spring,
}

/// A single line of text being edited, with a caret and a selection.
#[derive(Default, Clone)]
pub struct Field {
    pub chars: Vec<char>,
    pub caret: usize,
    pub anchor: usize,
}

impl Field {
    pub fn new(text: &str, select: std::ops::Range<usize>) -> Field {
        Field { chars: text.chars().collect(), caret: select.end, anchor: select.start }
    }

    pub fn text(&self) -> String {
        self.chars.iter().collect()
    }

    fn cut_selection(&mut self) {
        let (a, b) = (self.anchor.min(self.caret), self.anchor.max(self.caret));
        self.chars.drain(a..b);
        self.caret = a;
        self.anchor = a;
    }

    pub fn insert(&mut self, text: &str) {
        self.cut_selection();
        for ch in text.chars().filter(|c| !c.is_control()) {
            self.chars.insert(self.caret, ch);
            self.caret += 1;
        }
        self.anchor = self.caret;
    }

    pub fn backspace(&mut self) {
        if self.anchor == self.caret && self.caret > 0 {
            self.caret -= 1;
            self.chars.remove(self.caret);
            self.anchor = self.caret;
        } else {
            self.cut_selection();
        }
    }

    pub fn delete(&mut self) {
        if self.anchor == self.caret && self.caret < self.chars.len() {
            self.chars.remove(self.caret);
        } else {
            self.cut_selection();
        }
    }

    pub fn go(&mut self, to: usize, extend: bool) {
        self.caret = to.min(self.chars.len());
        if !extend {
            self.anchor = self.caret;
        }
    }
}

pub enum DialogKind {
    Rename(PathBuf),
    NewFolder(PathBuf),
    NewFile(PathBuf),
    Confirm(Request),
}

pub struct Dialog {
    pub kind: DialogKind,
    pub title: String,
    pub body: String,
    pub field: Option<Field>,
    pub confirm: String,
    pub open: Spring,
}

pub struct Toast {
    pub text: String,
    pub undo: Option<Undo>,
    pub error: bool,
    pub until: f32,
    pub shown: Spring,
}

pub struct QuickLook {
    pub zoom: f32,
    pub pan: [f32; 2],
    pub open: Spring,
    pub scroll: Spring,
}

pub struct Drag {
    pub ids: Vec<NodeId>,
    pub items: Vec<PathBuf>,
    pub flowers: Vec<f32>,
    pub target: NodeId,
    pub since: f32,
}

/// All the state behind the file-managing side of the app.
#[derive(Default)]
pub struct Actions {
    pub marked: Vec<NodeId>,
    pub clip: Option<(Vec<PathBuf>, bool)>,
    /// What's been cut and is still waiting to be pasted: it stays faded on the tree.
    pub cut_nodes: rustc_hash::FxHashSet<NodeId>,
    pub clipboard: Option<arboard::Clipboard>,
    pub no_system_clipboard: bool,
    /// Draws a pointer into the picture, for recording. A real window has its own.
    pub film_cursor: bool,
    pub range_anchor: Option<usize>,
    /// The flower currently raised for being selected.
    pub lifted: NodeId,
    pub handle_for: NodeId,
    pub handle_in: f32,
    pub places: Vec<(&'static str, PathBuf)>,
    /// The Recycle Bin, while the panel is showing it.
    pub bin: Option<Vec<ops::BinItem>>,
    pub undo: Vec<Undo>,
    pub menu: Option<Menu>,
    pub dialog: Option<Dialog>,
    pub toasts: Vec<Toast>,
    pub quick: Option<QuickLook>,
    pub drag: Option<Drag>,
    pub press_node: NodeId,
    pub working: Option<f32>,
    pub focus_after: Option<(PathBuf, f32)>,
    pub totals: Option<(PathBuf, u64, u64, u64)>,
    pub totals_due: Option<f32>,
    pub facts: Option<(PathBuf, i64, bool)>,
    pub audio: Option<AudioPlayer>,
    pub audio_bar: [f32; 4],
    pub dropped: Vec<PathBuf>,
    pub ctrl: bool,
    pub shift: bool,
    /// Hover and press animation amounts for each control.
    pub hover: rustc_hash::FxHashMap<Action, f32>,
    pub panel_in: f32,
    pub ui_dt: f32,
}

impl App {
    /// The paths the next command applies to.
    pub fn targets(&self) -> Vec<PathBuf> {
        let ids: Vec<NodeId> = if self.acts.marked.is_empty() { vec![self.selected] } else { self.acts.marked.clone() };
        ids.into_iter()
            .filter(|id| self.tree.is_valid(*id) && self.tree.get(*id).kind != Kind::Places && !self.tree.get(*id).dying)
            .filter_map(|id| self.tree.path_of(id))
            .collect()
    }

    /// The folder new things go into.
    pub fn current_dir(&self) -> Option<PathBuf> {
        if !self.tree.is_valid(self.selected) {
            return None;
        }
        let id = if self.tree.get(self.selected).is_file() { self.tree.logical_parent(self.selected) } else { self.selected };
        if id == NONE { None } else { self.tree.path_of(id) }
    }

    fn can_change_selection(&self) -> bool {
        let id = self.selected;
        !self.acts.marked.is_empty()
            || (self.tree.is_valid(id) && id != self.tree.root && self.tree.get(id).kind != Kind::Places && !self.is_drive(id))
    }

    pub fn toast(&mut self, text: String, undo: Option<Undo>, error: bool) {
        let until = self.wall + if error { 9.0 } else { 6.0 };
        // Saying the same thing twice just needs it to stay a little longer.
        if undo.is_none()
            && let Some(first) = self.acts.toasts.first_mut()
            && first.text == text
            && first.undo.is_none()
        {
            first.until = until;
            first.shown.target = 1.0;
            self.redraw = true;
            return;
        }
        self.acts.toasts.truncate(2);
        self.acts.toasts.insert(0, Toast { text, undo, error, until, shown: Spring { value: 0.0, velocity: 0.0, target: 1.0 } });
        self.redraw = true;
    }

    pub fn start_op(&mut self, request: Request) {
        self.acts.working = Some(0.0);
        self.disk.run_op(request);
        self.redraw = true;
    }

    /// Asks first if this touches somewhere Windows depends on.
    fn guarded(&mut self, request: Request, paths: &[PathBuf], verb: &str) {
        if paths.iter().any(|p| ops::is_protected(p)) {
            let what = if paths.len() == 1 {
                paths[0].file_name().map_or_else(|| paths[0].display().to_string(), |n| n.to_string_lossy().into_owned())
            } else {
                format!("{} items", paths.len())
            };
            self.acts.dialog = Some(Dialog {
                kind: DialogKind::Confirm(request),
                title: format!("{verb} a system folder?"),
                body: format!("{what} is somewhere Windows or your programs rely on. Changing it could stop things working."),
                field: None,
                confirm: verb.split(' ').next().unwrap_or(verb).to_string(),
                open: Spring { value: 0.0, velocity: 0.0, target: 1.0 },
            });
            self.redraw = true;
        } else {
            self.start_op(request);
        }
    }

    pub fn run_cmd(&mut self, cmd: Cmd) {
        self.acts.menu = None;
        self.wake_wind();
        match cmd {
            Cmd::Open => self.open_selected_with_flourish(),
            Cmd::OpenWith => {
                if let Some(path) = self.targets().into_iter().next()
                    && let Err(err) = crate::platform::open_with(&path)
                {
                    self.toast(format!("Couldn't ask Windows about that: {err}"), None, true);
                }
            }
            Cmd::QuickLook => self.open_quick_look(),
            Cmd::Rename => self.begin_rename(),
            Cmd::Duplicate => {
                let items = self.targets();
                if let Some(dest) = items.first().and_then(|p| p.parent()).map(Path::to_path_buf) {
                    self.start_op(Request::Copy { items, dest });
                }
            }
            Cmd::Copy | Cmd::Cut => {
                let items = self.targets();
                let cut = cmd == Cmd::Cut;
                if items.is_empty() || (cut && !self.can_change_selection()) {
                    return;
                }
                let n = items.len();
                self.set_system_files(&items);
                self.acts.cut_nodes.clear();
                if cut {
                    let ids: Vec<NodeId> =
                        if self.acts.marked.is_empty() { vec![self.selected] } else { self.acts.marked.clone() };
                    self.acts.cut_nodes.extend(ids);
                }
                self.acts.clip = Some((items, cut));
                let what = if n == 1 { "1 item".to_string() } else { format!("{n} items") };
                let verb = if cut { "Cut" } else { "Copied" };
                self.toast(format!("{verb} {what}. Paste it wherever you like."), None, false);
            }
            Cmd::Paste => {
                let files = self.system_files();
                // With no files anywhere, pasted text is a path or a name to go looking for.
                if files.is_none() && self.acts.clip.is_none() {
                    if let Some(text) = self.system_text() {
                        self.on_paste(&text);
                        return;
                    }
                }
                self.paste(files);
            }
            Cmd::Delete => {
                let items = self.targets();
                if !items.is_empty() && self.can_change_selection() {
                    self.guarded(Request::Trash { items: items.clone() }, &items, "Delete from");
                }
            }
            Cmd::NewFolder => self.begin_new(true),
            Cmd::NewFile => self.begin_new(false),
            Cmd::CopyPath => self.copy_selected_path(),
            Cmd::Reveal => self.reveal_selected(),
            Cmd::Toggle => {
                let id = self.selected;
                self.toggle_branch(id);
            }
            Cmd::Reset => self.reset_tree(),
            Cmd::SelectAll => self.select_all(),
            Cmd::Undo => self.undo_last(),
            Cmd::Extract => {
                if let Some(archive) = self.targets().into_iter().next() {
                    self.start_op(Request::Extract { archive });
                }
            }
            Cmd::Compress => {
                let items = self.targets();
                if !items.is_empty() {
                    self.start_op(Request::Compress { items });
                }
            }
            Cmd::Terminal => {
                if let Some(dir) = self.current_dir()
                    && let Err(err) = crate::platform::open_terminal(&dir)
                {
                    self.toast(format!("Couldn't open a terminal: {err}"), None, true);
                }
            }
            Cmd::More => {
                let (x, y) = (self.pointer[0], self.pointer[1]);
                let id = self.selected;
                self.open_menu(x, y, id);
            }
        }
    }

    pub fn open_selected_with_flourish(&mut self) {
        let id = self.selected;
        if self.tree.is_valid(id) && self.tree.get(id).is_file() {
            self.burst_petals(id);
        }
        if !self.tree.is_valid(id) {
            return;
        }
        if self.tree.get(id).is_file() {
            self.open_selected();
        } else {
            self.toggle_branch(id);
        }
    }

    fn begin_rename(&mut self) {
        if !self.can_change_selection() {
            return;
        }
        let Some(path) = self.targets().into_iter().next() else { return };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        // Select the name but not the extension, which is rarely what's being changed.
        let stem = match name.rfind('.') {
            Some(dot) if dot > 0 && !path.is_dir() => name[..dot].chars().count(),
            _ => name.chars().count(),
        };
        self.acts.dialog = Some(Dialog {
            title: "Rename".to_string(),
            body: String::new(),
            field: Some(Field::new(&name, 0..stem)),
            confirm: "Rename".to_string(),
            kind: DialogKind::Rename(path),
            open: Spring { value: 0.0, velocity: 0.0, target: 1.0 },
        });
        self.redraw = true;
    }

    fn begin_new(&mut self, folder: bool) {
        let Some(dir) = self.current_dir() else {
            self.toast("Open a disk or folder first, then make something in it.".to_string(), None, true);
            return;
        };
        let name = if folder { "New folder" } else { "New file.txt" };
        let stem = if folder { name.chars().count() } else { 8 };
        self.acts.dialog = Some(Dialog {
            title: if folder { "New folder".to_string() } else { "New file".to_string() },
            body: String::new(),
            field: Some(Field::new(name, 0..stem)),
            confirm: "Create".to_string(),
            kind: if folder { DialogKind::NewFolder(dir) } else { DialogKind::NewFile(dir) },
            open: Spring { value: 0.0, velocity: 0.0, target: 1.0 },
        });
        self.redraw = true;
    }

    pub fn dialog_problem(&self) -> Option<&'static str> {
        let dialog = self.acts.dialog.as_ref()?;
        ops::name_problem(&dialog.field.as_ref()?.text())
    }

    pub fn dialog_confirm(&mut self) {
        if self.dialog_problem().is_some() {
            return;
        }
        let Some(dialog) = self.acts.dialog.take() else { return };
        let text = dialog.field.map(|f| f.text()).unwrap_or_default();
        match dialog.kind {
            DialogKind::Rename(from) => {
                if from.file_name().is_some_and(|n| n.to_string_lossy() == text) {
                    return;
                }
                let request = Request::Rename { from: from.clone(), new_name: text };
                self.guarded(request, &[from], "Rename");
            }
            DialogKind::NewFolder(parent) => self.start_op(Request::NewFolder { parent, name: text }),
            DialogKind::NewFile(parent) => self.start_op(Request::NewFile { parent, name: text }),
            DialogKind::Confirm(request) => self.start_op(request),
        }
        self.redraw = true;
    }

    /// Pastes files: from the Windows clipboard when another app put them there, otherwise
    /// what was copied or cut here.
    pub fn paste(&mut self, from_system: Option<Vec<PathBuf>>) {
        let Some(dest) = self.current_dir() else {
            self.toast("Open a disk or folder first, then paste into it.".to_string(), None, true);
            return;
        };
        self.open_for_arrival(&dest);
        let (items, cut) = match (&self.acts.clip, from_system) {
            (Some((ours, cut)), Some(theirs)) if *ours == theirs => (ours.clone(), *cut),
            (_, Some(theirs)) => (theirs, false),
            (Some((ours, cut)), None) => (ours.clone(), *cut),
            (None, None) => {
                self.toast("There's nothing to paste yet.".to_string(), None, false);
                return;
            }
        };
        if cut {
            self.acts.clip = None;
            self.acts.cut_nodes.clear();
            let request = Request::Move { items: items.clone(), dest: dest.clone() };
            let mut touched = items;
            touched.push(dest);
            self.guarded(request, &touched, "Move");
        } else {
            let request = Request::Copy { items, dest: dest.clone() };
            self.guarded(request, &[dest], "Copy into");
        }
    }

    pub fn undo_last(&mut self) {
        match self.acts.undo.pop() {
            Some(undo) => {
                self.acts.toasts.clear();
                self.start_op(Request::Undo(undo));
            }
            None => self.toast("There's nothing to undo.".to_string(), None, false),
        }
    }

    fn select_all(&mut self) {
        let dir = if self.tree.is_valid(self.selected) && self.tree.get(self.selected).is_file() {
            self.tree.logical_parent(self.selected)
        } else {
            self.selected
        };
        if dir == NONE || !self.tree.is_valid(dir) || self.tree.get(dir).kind == Kind::Places {
            return;
        }
        let mut kids = Vec::new();
        self.tree.logical_children(dir, &mut kids);
        self.acts.marked = kids.into_iter().filter(|k| !self.tree.get(*k).dying).collect();
        self.list_dirty = true;
        self.labels_dirty = true;
        self.redraw = true;
    }

    pub fn toggle_mark(&mut self, id: NodeId) {
        if self.tree.get(id).kind == Kind::Places || self.is_drive(id) || id == self.tree.root {
            return;
        }
        // The first Ctrl-click also keeps what was already selected, the way Explorer does.
        if self.acts.marked.is_empty()
            && self.tree.is_valid(self.selected)
            && self.selected != id
            && self.tree.get(self.selected).is_file()
        {
            self.acts.marked.push(self.selected);
        }
        match self.acts.marked.iter().position(|m| *m == id) {
            Some(at) => {
                self.acts.marked.remove(at);
            }
            None => self.acts.marked.push(id),
        }
        let rest = self.rest_lift(id);
        self.tree.get_mut(id).lift.target = rest;
        self.labels_dirty = true;
        self.list_dirty = true;
        self.redraw = true;
    }

    pub fn clear_marks(&mut self) {
        if !self.acts.marked.is_empty() {
            for id in std::mem::take(&mut self.acts.marked) {
                if self.tree.is_valid(id) {
                    let rest = self.rest_lift(id);
                    self.tree.get_mut(id).lift.target = rest;
                }
            }
            self.labels_dirty = true;
            self.list_dirty = true;
            self.redraw = true;
        }
    }

    /// A folder opens when something's put in it, so you can see it land.
    fn open_for_arrival(&mut self, dest: &Path) {
        if let Some(id) = self.tree.find_path(dest)
            && !self.tree.get(id).expanded
        {
            self.expand(id);
        }
    }

    // ------------------------------------------------------------ results of operations

    pub fn on_outcome(&mut self, outcome: ops::Outcome) {
        self.acts.working = None;
        if self.acts.bin.is_some() {
            self.disk.list_bin();
        }
        for dir in &outcome.changed {
            if let Some(id) = self.tree.find_path(dir)
                && self.tree.get(id).expanded
            {
                self.disk.list(dir.clone());
            }
        }
        if let Some(focus) = &outcome.focus {
            self.acts.focus_after = Some((focus.clone(), self.wall + 3.0));
            self.settle_focus();
        }
        self.clear_marks();
        let can_undo = outcome.undo.is_some();
        if let Some(undo) = &outcome.undo {
            self.acts.undo.push(undo.clone());
            if self.acts.undo.len() > 100 {
                self.acts.undo.remove(0);
            }
        }
        self.list_dirty = true;
        self.toast(outcome.message, if can_undo { outcome.undo } else { None }, outcome.failed);
    }

    /// Called when a folder's new contents have arrived, to select what was just made or moved.
    pub fn settle_focus(&mut self) {
        let Some((path, until)) = self.acts.focus_after.clone() else { return };
        if self.wall > until {
            self.acts.focus_after = None;
        } else if let Some(id) = self.tree.find_path(&path)
            && !self.tree.get(id).dying
        {
            self.acts.focus_after = None;
            self.select(id, true);
        }
    }

    // ------------------------------------------------------------ menu

    pub fn open_menu(&mut self, x: f32, y: f32, id: NodeId) {
        let valid = id != NONE && self.tree.is_valid(id);
        let places = valid && self.tree.get(id).kind == Kind::Places;
        let drive = valid && self.is_drive(id);
        let file = valid && self.tree.get(id).is_file();
        let many = self.acts.marked.len() > 1;
        let mut rows: Vec<(Option<Cmd>, bool)> = Vec::new();
        let mut add = |cmd: Cmd, enabled: bool| rows.push((Some(cmd), enabled));
        let is_zip = file && self.tree.get(id).name.to_ascii_lowercase().ends_with(".zip");
        if many {
            add(Cmd::Copy, true);
            add(Cmd::Cut, true);
            add(Cmd::Duplicate, true);
            add(Cmd::Compress, true);
            add(Cmd::Delete, true);
        } else if file {
            add(Cmd::Open, true);
            add(Cmd::OpenWith, true);
            add(Cmd::QuickLook, true);
            if is_zip {
                add(Cmd::Extract, true);
            }
            rows.push((None, false));
            let mut add = |cmd: Cmd, enabled: bool| rows.push((Some(cmd), enabled));
            add(Cmd::Rename, true);
            add(Cmd::Duplicate, true);
            add(Cmd::Copy, true);
            add(Cmd::Cut, true);
            add(Cmd::Delete, true);
            rows.push((None, false));
            let mut add = |cmd: Cmd, enabled: bool| rows.push((Some(cmd), enabled));
            add(Cmd::CopyPath, true);
            add(Cmd::Reveal, true);
        } else if places {
            add(Cmd::Reset, true);
        } else if !valid {
            // The empty space around the tree: things that apply to the open folder.
            let in_folder = self.current_dir().is_some();
            add(Cmd::NewFolder, in_folder);
            add(Cmd::NewFile, in_folder);
            add(Cmd::Paste, in_folder);
            add(Cmd::Terminal, in_folder);
            add(Cmd::Undo, !self.acts.undo.is_empty());
            rows.push((None, false));
            rows.push((Some(Cmd::Reset), true));
        } else {
            if id != self.tree.root {
                add(Cmd::Toggle, true);
            }
            add(Cmd::NewFolder, true);
            add(Cmd::NewFile, true);
            add(Cmd::Paste, true);
            rows.push((None, false));
            let mut add = |cmd: Cmd, enabled: bool| rows.push((Some(cmd), enabled));
            if !drive && id != self.tree.root {
                add(Cmd::Rename, true);
                add(Cmd::Duplicate, true);
                add(Cmd::Copy, true);
                add(Cmd::Cut, true);
                add(Cmd::Compress, true);
                add(Cmd::Delete, true);
                rows.push((None, false));
            }
            let mut add = |cmd: Cmd, enabled: bool| rows.push((Some(cmd), enabled));
            add(Cmd::Terminal, true);
            add(Cmd::CopyPath, true);
            add(Cmd::Reveal, true);
        }
        let expanded = valid && self.tree.get(id).expanded;
        let items = rows
            .into_iter()
            .map(|(cmd, enabled)| MenuItem {
                label: match cmd {
                    Some(Cmd::Toggle) => if expanded { "Collapse branch" } else { "Open branch" }.to_string(),
                    Some(c) => c.label().to_string(),
                    None => String::new(),
                },
                danger: cmd == Some(Cmd::Delete),
                cmd,
                enabled,
            })
            .collect();
        self.acts.menu = Some(Menu { x, y, items, cursor: None, open: Spring { value: 0.0, velocity: 0.0, target: 1.0 } });
        self.redraw = true;
    }

    pub fn on_right_click(&mut self) {
        let [x, y] = self.pointer;
        self.acts.menu = None;
        let zone = self
            .zones
            .iter()
            .rev()
            .find(|z| crate::app::inside(z.rect, x, y) && !matches!(z.action, Action::MenuClose | Action::MenuItem(_)))
            .map(|z| z.action);
        let hit = match zone {
            None => self.hit_test(x, y),
            Some(Action::Row(row)) => match self.list.get(row).map(|item| item.node) {
                Some(node) if node != NONE => node,
                _ => return,
            },
            // The panel itself stands for whatever it's showing.
            Some(Action::PanelScroll) => self.selected,
            Some(_) => return,
        };
        // A sprig stands for the folder it's part of.
        let mut id = hit;
        while id != NONE && self.tree.get(id).is_sprig() {
            id = self.tree.get(id).parent;
        }
        if id != NONE {
            if !self.acts.marked.contains(&id) {
                self.clear_marks();
            }
            self.select(id, true);
        }
        self.open_menu(x, y, id);
    }

    // ------------------------------------------------------------ Quick Look

    pub fn open_quick_look(&mut self) {
        if !self.tree.is_valid(self.selected) || !self.tree.get(self.selected).is_file() {
            return;
        }
        if let Some(path) = self.tree.path_of(self.selected) {
            self.disk.preview_large(path);
        }
        self.acts.quick = Some(QuickLook {
            zoom: 1.0,
            pan: [0.0, 0.0],
            open: Spring { value: if self.reduced_motion { 1.0 } else { 0.0 }, velocity: 0.0, target: 1.0 },
            scroll: Spring::new(0.0),
        });
        self.redraw = true;
    }

    /// Next or previous file in the same folder.
    pub fn quick_step(&mut self, by: i32) {
        let parent = self.tree.logical_parent(self.selected);
        if parent == NONE {
            return;
        }
        let mut kids = Vec::new();
        self.tree.logical_children(parent, &mut kids);
        kids.retain(|k| self.tree.get(*k).is_file() && !self.tree.get(*k).dying);
        let Some(at) = kids.iter().position(|k| *k == self.selected) else { return };
        let next = kids[(at as i32 + by).rem_euclid(kids.len() as i32) as usize];
        self.select(next, true);
        if let Some(quick) = &mut self.acts.quick {
            quick.zoom = 1.0;
            quick.pan = [0.0, 0.0];
            quick.scroll.snap(0.0);
        }
    }

    // ------------------------------------------------------------ audio

    pub fn audio_toggle(&mut self) {
        let Some(path) = self.tree.path_of(self.selected) else { return };
        match &self.acts.audio {
            Some(player) if player.path == path && !player.finished() => player.toggle(),
            _ => match AudioPlayer::open(&path) {
                Ok(player) => self.acts.audio = Some(player),
                Err(err) => self.toast(err, None, true),
            },
        }
        self.redraw = true;
    }

    pub fn audio_seek_to_pointer(&mut self) {
        let bar = self.acts.audio_bar;
        let Some(player) = &self.acts.audio else { return };
        let fraction = ((self.pointer[0] - bar[0]) / (bar[2] - bar[0]).max(1.0)).clamp(0.0, 1.0);
        player.seek(fraction * player.duration);
        self.redraw = true;
    }

    // ------------------------------------------------------------ drag and drop

    pub fn start_drag(&mut self, id: NodeId) {
        if !self.acts.marked.contains(&id) {
            self.clear_marks();
            self.select(id, true);
        }
        let ids: Vec<NodeId> = if self.acts.marked.is_empty() { vec![id] } else { self.acts.marked.clone() };
        let items: Vec<PathBuf> = ids.iter().filter_map(|i| self.tree.path_of(*i)).collect();
        let flowers = ids
            .iter()
            .take(3)
            .map(|i| {
                let n = self.tree.get(*i);
                if n.is_file() { n.species().index() as f32 } else { crate::palette::BUD }
            })
            .collect();
        self.acts.menu = None;
        self.acts.drag = Some(Drag { ids, items, flowers, target: NONE, since: self.time });
        self.redraw = true;
    }

    /// Whether a drop on this folder would make sense.
    fn drop_allowed(&self, id: NodeId) -> bool {
        if !self.tree.is_valid(id) || !self.tree.get(id).is_dir() || self.tree.get(id).kind == Kind::Places {
            return false;
        }
        let (Some(drag), Some(path)) = (&self.acts.drag, self.tree.path_of(id)) else { return false };
        !drag.items.iter().any(|item| path.starts_with(item)) && !drag.items.iter().all(|item| item.parent() == Some(path.as_path()))
    }

    pub fn update_drag(&mut self) {
        let [x, y] = self.pointer;
        let zone = self.zones.iter().rev().find(|z| crate::app::inside(z.rect, x, y)).map(|z| z.action);
        let mut hit = match zone {
            Some(Action::Row(row)) => self.list.get(row).filter(|i| i.is_dir).map_or(NONE, |i| i.node),
            Some(_) => NONE,
            None => self.hit_test(x, y),
        };
        while hit != NONE && !self.tree.get(hit).is_dir() {
            hit = self.tree.get(hit).parent;
        }
        let target = if hit != NONE && self.drop_allowed(hit) { hit } else { NONE };
        let time = self.time;
        if let Some(drag) = &mut self.acts.drag
            && drag.target != target
        {
            drag.target = target;
            drag.since = time;
        }
        // Holding over a closed folder opens it, so you can drop deeper in.
        let open_it = self.acts.drag.as_ref().is_some_and(|d| d.target != NONE && time - d.since > 0.75);
        if open_it && !self.tree.get(target).expanded {
            self.expand(target);
        }
        self.redraw = true;
    }

    /// Whether this drop would copy rather than move: Ctrl asks for it, and crossing disks
    /// defaults to it, the same as Explorer.
    pub fn drop_copies(&self, target: NodeId) -> bool {
        if self.acts.ctrl {
            return true;
        }
        if self.acts.shift {
            return false;
        }
        let (Some(drag), Some(dest)) = (&self.acts.drag, self.tree.path_of(target)) else { return false };
        let root_of = |p: &Path| p.components().next().map(|c| c.as_os_str().to_ascii_lowercase());
        drag.items.first().is_some_and(|i| root_of(i) != root_of(&dest))
    }

    pub fn finish_drag(&mut self) {
        let copies = self.acts.drag.as_ref().is_some_and(|d| d.target != NONE && self.drop_copies(d.target));
        let Some(drag) = self.acts.drag.take() else { return };
        if drag.target == NONE {
            return;
        }
        let Some(dest) = self.tree.path_of(drag.target) else { return };
        self.open_for_arrival(&dest);
        let mut touched = drag.items.clone();
        if copies {
            self.guarded(Request::Copy { items: drag.items, dest: dest.clone() }, &[dest], "Copy into");
        } else {
            touched.push(dest.clone());
            self.guarded(Request::Move { items: drag.items, dest }, &touched, "Move");
        }
    }

    /// Files dragged in from another app.
    pub fn flush_dropped(&mut self) {
        if self.acts.dropped.is_empty() {
            return;
        }
        let items = std::mem::take(&mut self.acts.dropped);
        // The pointer's position isn't reported during a drag from another app, so aiming at a
        // particular branch can't be trusted. The open folder is what the status line named.
        match self.current_dir() {
            Some(dest) => self.guarded(Request::Copy { items, dest: dest.clone() }, &[dest], "Copy into"),
            None => self.toast("Open a disk or folder first, then drop things onto it.".to_string(), None, true),
        }
    }

    /// A flower pops as it opens.
    pub fn burst_petals(&mut self, id: NodeId) {
        if self.reduced_motion {
            return;
        }
        let node = self.tree.get(id);
        let species = node.species().index() as f32;
        let (tip, seed) = (node.tip, node.seed);
        let scale = FLOWER_RADIUS / 19.0;
        for i in 0..9 {
            let r = crate::tree::rand01(seed, 70 + i);
            let a = i as f32 / 9.0 * std::f32::consts::TAU + r;
            self.particles.push(crate::app::Particle {
                pos: tip,
                vel: [a.cos() * (70.0 + r * 50.0) * scale, a.sin() * (70.0 + r * 50.0) * scale + 50.0],
                rotation: a,
                spin: (r - 0.5) * 8.0,
                life: 0.9,
                species,
                size: 7.0 + r * 3.0,
            });
        }
        self.redraw = true;
    }

    // ------------------------------------------------------------ small helpers

    pub fn counts_totals(&self, id: NodeId) -> bool {
        let node = self.tree.get(id);
        node.kind == Kind::Dir
            && !self.is_drive(id)
            && self.tree.path_of(id).is_some_and(|path| path.parent().is_some() && !ops::is_protected(&path))
    }

    /// What can be picked up under the pointer: flowers and the tips of folders.
    pub fn hit_carryable(&self, x: f32, y: f32) -> NodeId {
        let mut best = (f32::MAX, NONE);
        for v in &self.visible {
            let d = ((v.x - x).powi(2) + (v.y - y).powi(2)).sqrt();
            if v.rank <= 1 && d <= v.r && d < best.0 {
                best = (d, v.id);
            }
        }
        best.1
    }

    pub fn carryable(&self, id: NodeId) -> bool {
        if id == NONE || !self.tree.is_valid(id) || id == self.tree.root {
            return false;
        }
        let node = self.tree.get(id);
        !node.is_sprig() && node.kind != Kind::Places && !self.is_drive(id)
    }

    /// Ctrl-click adds to the selection, Shift-click takes everything from the last row clicked.
    pub fn mark_row(&mut self, row: usize) {
        let Some(node) = self.list.get(row).map(|i| i.node).filter(|n| *n != NONE) else { return };
        self.selected_row = None;
        if self.acts.shift
            && let Some(anchor) = self.acts.range_anchor
        {
            let (a, b) = (anchor.min(row), anchor.max(row));
            self.acts.marked = (a..=b)
                .filter_map(|i| self.list.get(i).map(|item| item.node))
                .filter(|n| *n != NONE)
                .collect();
            for id in self.acts.marked.clone() {
                self.tree.get_mut(id).lift.target = 1.0;
            }
            self.labels_dirty = true;
            self.redraw = true;
            return;
        }
        self.acts.range_anchor = Some(row);
        self.toggle_mark(node);
    }

    pub fn undo_toast(&mut self, index: usize) {
        if index >= self.acts.toasts.len() {
            return;
        }
        let toast = self.acts.toasts.remove(index);
        if let Some(undo) = toast.undo {
            if let Some(at) = self.acts.undo.iter().rposition(|u| *u == undo) {
                self.acts.undo.remove(at);
            }
            self.start_op(Request::Undo(undo));
        }
    }

    pub fn close_quick_look(&mut self) {
        if let Some(quick) = &mut self.acts.quick {
            quick.open.target = 0.0;
            self.redraw = true;
        }
    }

    pub fn quick_wheel(&mut self, lines: f32) {
        let s = self.scale;
        let [x, y] = self.pointer;
        let (w, h) = (self.width, self.height);
        let is_image = matches!(self.preview.as_ref().map(|l| &l.preview), Some(crate::fs::Preview::Image { .. }));
        let Some(quick) = &mut self.acts.quick else { return };
        if is_image {
            let next = (quick.zoom * (lines * 0.2).exp()).clamp(1.0, 12.0);
            let ratio = next / quick.zoom;
            // Keep the point under the pointer where it is.
            let center = [w * 0.5, (h + 62.0 * s) * 0.5];
            quick.pan[0] = (x - center[0]) - (x - center[0] - quick.pan[0]) * ratio;
            quick.pan[1] = (y - center[1]) - (y - center[1] - quick.pan[1]) * ratio;
            quick.zoom = next;
            if next <= 1.0 {
                quick.pan = [0.0, 0.0];
            }
        } else {
            quick.scroll.target = (quick.scroll.target - lines * 54.0 * s).max(0.0);
        }
        self.redraw = true;
    }

    pub fn on_file_hovering(&mut self) {
        match self.current_dir() {
            Some(dir) => {
                let name = dir.file_name().map_or_else(|| dir.display().to_string(), |n| n.to_string_lossy().into_owned());
                self.note(format!("Drop to copy into {name}"));
            }
            None => self.note("Open a disk or folder first, then drop things onto it".to_string()),
        }
    }

    pub fn system_files(&mut self) -> Option<Vec<PathBuf>> {
        if self.acts.no_system_clipboard {
            return None;
        }
        if self.acts.clipboard.is_none() {
            self.acts.clipboard = arboard::Clipboard::new().ok();
        }
        self.acts.clipboard.as_mut()?.get().file_list().ok().filter(|files| !files.is_empty())
    }

    pub fn system_text(&mut self) -> Option<String> {
        if self.acts.no_system_clipboard {
            return None;
        }
        if self.acts.clipboard.is_none() {
            self.acts.clipboard = arboard::Clipboard::new().ok();
        }
        self.acts.clipboard.as_mut()?.get_text().ok().filter(|t| !t.is_empty())
    }

    fn set_system_files(&mut self, items: &[PathBuf]) {
        if self.acts.no_system_clipboard {
            return;
        }
        if self.acts.clipboard.is_none() {
            self.acts.clipboard = arboard::Clipboard::new().ok();
        }
        if let Some(clipboard) = self.acts.clipboard.as_mut() {
            let _ = clipboard.set().file_list(items);
        }
    }

    // ------------------------------------------------------------ keys for what's on top

    /// Handles a key if a dialog, Quick Look, a menu or a drag has it. True if it was used.
    pub fn overlay_key(&mut self, key: Key) -> bool {
        if self.acts.drag.is_some() {
            if key == Key::Escape {
                self.acts.drag = None;
                self.hover = NONE;
            }
            return true;
        }
        if self.acts.dialog.is_some() {
            let shift = self.acts.shift;
            match key {
                Key::Escape => self.acts.dialog = None,
                Key::Enter => self.dialog_confirm(),
                Key::Undo | Key::CopyFiles | Key::CutFiles | Key::Rename | Key::Space => {}
                _ => {
                    if let Some(field) = self.acts.dialog.as_mut().and_then(|d| d.field.as_mut()) {
                        match key {
                            Key::Left => field.go(field.caret.saturating_sub(1), shift),
                            Key::Right => field.go(field.caret + 1, shift),
                            Key::Home => field.go(0, shift),
                            Key::End => field.go(usize::MAX, shift),
                            Key::Backspace => field.backspace(),
                            Key::Delete => field.delete(),
                            Key::SelectAll => {
                                field.anchor = 0;
                                field.caret = field.chars.len();
                            }
                            _ => {}
                        }
                    }
                }
            }
            self.redraw = true;
            return true;
        }
        if self.acts.menu.is_some() {
            let Some(menu) = self.acts.menu.as_mut() else { return true };
            let rows = menu.items.len();
            let stepped = |from: Option<usize>, dir: i32, items: &[MenuItem]| -> Option<usize> {
                let mut at = from.map_or(if dir > 0 { -1 } else { rows as i32 }, |c| c as i32);
                for _ in 0..rows {
                    at = (at + dir).rem_euclid(rows as i32);
                    if items[at as usize].cmd.is_some() && items[at as usize].enabled {
                        return Some(at as usize);
                    }
                }
                None
            };
            match key {
                Key::Down => menu.cursor = stepped(menu.cursor, 1, &menu.items),
                Key::Up => menu.cursor = stepped(menu.cursor, -1, &menu.items),
                Key::Escape => self.acts.menu = None,
                Key::Enter => {
                    if let Some(cmd) = menu.cursor.and_then(|c| menu.items[c].cmd) {
                        self.run_cmd(cmd);
                    }
                }
                _ => self.acts.menu = None,
            }
            self.redraw = true;
            return true;
        }
        if self.acts.quick.is_some() {
            match key {
                Key::Escape | Key::Space => self.close_quick_look(),
                Key::Left => self.quick_step(-1),
                Key::Right => self.quick_step(1),
                Key::Enter => self.run_cmd(Cmd::Open),
                Key::Up | Key::PageUp => {
                    if let Some(quick) = &mut self.acts.quick {
                        quick.scroll.target = (quick.scroll.target - 240.0 * self.scale).max(0.0);
                    }
                }
                Key::Down | Key::PageDown => {
                    if let Some(quick) = &mut self.acts.quick {
                        quick.scroll.target += 240.0 * self.scale;
                    }
                }
                _ => {}
            }
            self.redraw = true;
            return true;
        }
        false
    }

    /// Advances everything the overlays animate. True while anything moves.
    pub fn step_overlays(&mut self, dt: f32) -> bool {
        let mut moving = false;
        self.acts.ui_dt = dt;
        if self.reduced_motion {
            if let Some(menu) = &mut self.acts.menu {
                menu.open.snap(1.0);
            }
            if let Some(dialog) = &mut self.acts.dialog {
                dialog.open.snap(1.0);
            }
            if let Some(quick) = &mut self.acts.quick {
                let target = quick.open.target;
                quick.open.snap(target);
            }
            for toast in &mut self.acts.toasts {
                let target = toast.shown.target;
                toast.shown.snap(target);
            }
        }
        if let Some(menu) = &mut self.acts.menu {
            moving |= menu.open.step(dt, 26.0);
        }
        if let Some(dialog) = &mut self.acts.dialog {
            moving |= dialog.open.step(dt, 22.0);
        }
        let mut close_quick = false;
        if let Some(quick) = &mut self.acts.quick {
            moving |= quick.open.step(dt, 20.0);
            moving |= quick.scroll.step(dt, 22.0);
            close_quick = quick.open.target == 0.0 && quick.open.value < 0.02;
        }
        if close_quick {
            self.acts.quick = None;
            moving = true;
            // Back to a preview small enough not to sit in memory.
            if let Some(path) = self.selected_path().filter(|_| self.tree.get(self.selected).is_file()) {
                self.disk.preview(path);
            }
        }
        if self.acts.drag.is_some() {
            self.update_drag();
            moving = true;
        }
        let now = self.wall;
        let hovered_toast = match self.hover_action {
            Action::ToastUndo(i) => Some(i),
            _ => None,
        };
        for (i, toast) in self.acts.toasts.iter_mut().enumerate() {
            // A message being reached for doesn't leave.
            if hovered_toast == Some(i) {
                toast.until = toast.until.max(now + 2.0);
            }
            if now > toast.until {
                toast.shown.target = 0.0;
            }
            moving |= toast.shown.step(dt, 16.0);
        }
        self.acts.toasts.retain(|t| t.shown.target > 0.0 || t.shown.value > 0.01);
        if self.reduced_motion {
            self.acts.panel_in = 1.0;
        } else if self.acts.panel_in < 1.0 {
            self.acts.panel_in = (self.acts.panel_in + dt * 7.0).min(1.0);
            moving = true;
        }
        // Add up a folder's size once the selection has stayed put for a moment.
        if let Some(due) = self.acts.totals_due
            && self.time >= due
        {
            self.acts.totals_due = None;
            if let Some(path) = self.tree.is_valid(self.selected).then(|| self.tree.path_of(self.selected)).flatten() {
                self.disk.folder_totals(path);
            }
        }
        if self.acts.totals_due.is_some() {
            moving = true;
        }
        self.flush_dropped();
        moving
    }

    /// What a selection change needs to refresh. Called from `select`.
    pub fn on_selection_changed(&mut self, id: NodeId) {
        self.acts.panel_in = 0.0;
        self.acts.bin = None;
        self.acts.totals = None;
        self.acts.facts = None;
        self.disk.cancel_totals();
        // Add up folders people are looking at, but don't go walking a whole disk or Windows.
        let countable = self.counts_totals(id);
        self.acts.totals_due = countable.then_some(self.time + 0.45);
        if let Some(player) = &self.acts.audio
            && self.tree.path_of(id).as_deref() != Some(player.path.as_path())
        {
            self.acts.audio = None;
        }
    }
}
