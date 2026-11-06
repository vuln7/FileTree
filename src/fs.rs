//! Everything that touches the disk. It runs on background threads and only ever
//! reads, so the interface never waits on a slow drive and nothing can be changed.

use notify::{RecursiveMode, Watcher};
use rustc_hash::FxHashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, UNIX_EPOCH};

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: Box<str>,
    pub is_dir: bool,
    pub hidden: bool,
    pub size: u64,
    /// Seconds since the Unix epoch, 0 when unknown.
    pub modified: i64,
}

#[derive(Debug)]
pub enum FsEvent {
    Listed { path: PathBuf, entries: Vec<Entry>, error: Option<String> },
    Changed { dir: PathBuf },
    SearchHits { generation: u64, hits: Vec<SearchHit> },
    SearchDone { generation: u64, capped: bool },
    Preview { path: PathBuf, preview: Preview },
    Facts { path: PathBuf, created: i64, read_only: bool },
    FolderTotals { path: PathBuf, bytes: u64, files: u64, folders: u64 },
    OpProgress(f32),
    OpDone(Box<crate::ops::Outcome>),
    Bin(Result<Vec<crate::ops::BinItem>, String>),
    DriveInfo { name: String, info: crate::platform::DriveInfo },
}

#[derive(Clone, Debug)]
pub struct SearchHit {
    pub path: PathBuf,
    pub is_dir: bool,
}

#[derive(Debug)]
pub enum Preview {
    Image { width: u32, height: u32, full_width: u32, full_height: u32, rgba: Vec<u8> },
    Text { lines: Vec<String>, truncated: bool },
    /// What's inside an archive.
    Listing { entries: Vec<(String, u64)>, total: usize },
    /// Played in the app.
    Audio,
    Unavailable { reason: String },
    None,
}

pub type Waker = Arc<dyn Fn() + Send + Sync>;

enum Job {
    List(PathBuf),
    Preview(PathBuf, u32),
    DriveInfo(String),
}

const SEARCH_RESULT_CAP: usize = 2000;
const PREVIEW_IMAGE_MAX_BYTES: u64 = 96 * 1024 * 1024;
/// Side of the picture shown in the panel. Quick Look asks for a larger one.
pub const PREVIEW_SIDE: u32 = 720;
pub const LARGE_SIDE: u32 = 2400;
const PREVIEW_TEXT_BYTES: usize = 256 * 1024;
const PREVIEW_TEXT_LINES: usize = 6000;
const LISTING_ENTRIES: usize = 2000;

/// Handle owned by the app. All replies arrive on `events`.
pub struct Disk {
    jobs: Sender<Job>,
    pub events: Receiver<FsEvent>,
    out: Sender<FsEvent>,
    waker: Waker,
    watch: Sender<WatchCmd>,
    search_generation: Arc<AtomicU64>,
    totals_generation: Arc<AtomicU64>,
    ops: Sender<crate::ops::Request>,
}

enum WatchCmd {
    Add(PathBuf),
    Remove(PathBuf),
}

impl Disk {
    pub fn new(waker: Waker) -> Self {
        let (jobs, job_rx) = channel::<Job>();
        let (out, events) = channel::<FsEvent>();
        let job_rx = Arc::new(Mutex::new(job_rx));
        // Two readers: one stalled network drive should not stop local browsing.
        for i in 0..2 {
            let job_rx = job_rx.clone();
            let out = out.clone();
            let waker = waker.clone();
            let _ = std::thread::Builder::new()
                .name(format!("disk-{i}"))
                .stack_size(256 * 1024)
                .spawn(move || {
                    loop {
                        let job = {
                            let Ok(guard) = job_rx.lock() else { return };
                            guard.recv()
                        };
                        let Ok(job) = job else { return };
                        let event = match job {
                            Job::List(path) => {
                                let (entries, error) = list_dir(&path);
                                FsEvent::Listed { path, entries, error }
                            }
                            Job::Preview(path, side) => {
                                if let Ok(meta) = std::fs::metadata(&path) {
                                    let created = meta
                                        .created()
                                        .ok()
                                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                                        .map_or(0, |d| d.as_secs() as i64);
                                    let facts = FsEvent::Facts {
                                        path: path.clone(),
                                        created,
                                        read_only: meta.permissions().readonly(),
                                    };
                                    let _ = out.send(facts);
                                }
                                let preview = load_preview(&path, side);
                                FsEvent::Preview { path, preview }
                            }
                            Job::DriveInfo(name) => match crate::platform::drive_info(&name) {
                                Some(info) => FsEvent::DriveInfo { name, info },
                                None => continue,
                            },
                        };
                        if out.send(event).is_err() {
                            return;
                        }
                        waker();
                    }
                });
        }
        let watch = spawn_watcher(out.clone(), waker.clone());
        // One at a time and in order: an undo must never overtake what it undoes.
        let (ops, op_rx) = channel::<crate::ops::Request>();
        {
            let out = out.clone();
            let waker = waker.clone();
            let _ = std::thread::Builder::new().name("file-ops".into()).spawn(move || {
                while let Ok(request) = op_rx.recv() {
                    let outcome = {
                        let out = out.clone();
                        let waker = waker.clone();
                        let progress = move |p: f32| {
                            let _ = out.send(FsEvent::OpProgress(p));
                            waker();
                        };
                        crate::ops::run(request, &progress)
                    };
                    let _ = out.send(FsEvent::OpDone(Box::new(outcome)));
                    waker();
                }
            });
        }
        Disk {
            jobs,
            events,
            out,
            waker,
            watch,
            search_generation: Arc::new(AtomicU64::new(0)),
            totals_generation: Arc::new(AtomicU64::new(0)),
            ops,
        }
    }

    pub fn list(&self, path: PathBuf) {
        let _ = self.jobs.send(Job::List(path));
    }

    pub fn preview(&self, path: PathBuf) {
        let _ = self.jobs.send(Job::Preview(path, PREVIEW_SIDE));
    }

    pub fn preview_large(&self, path: PathBuf) {
        let _ = self.jobs.send(Job::Preview(path, LARGE_SIDE));
    }

    /// Looks in the Recycle Bin, which can be slow when it's full.
    pub fn list_bin(&self) {
        let out = self.out.clone();
        let waker = self.waker.clone();
        let _ = std::thread::Builder::new().name("bin".into()).spawn(move || {
            let _ = out.send(FsEvent::Bin(crate::ops::list_bin()));
            waker();
        });
    }

    pub fn run_op(&self, request: crate::ops::Request) {
        let _ = self.ops.send(request);
    }

    /// Adds up a folder in the background, abandoning any count still running.
    pub fn folder_totals(&self, path: PathBuf) {
        let generation = self.totals_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let current = self.totals_generation.clone();
        let out = self.out.clone();
        let waker = self.waker.clone();
        let _ = std::thread::Builder::new().name("totals".into()).stack_size(256 * 1024).spawn(move || {
            let keep_going = || current.load(Ordering::Relaxed) == generation;
            if let Some((bytes, files, folders)) = crate::ops::folder_totals(&path, &keep_going) {
                let _ = out.send(FsEvent::FolderTotals { path, bytes, files, folders });
                waker();
            }
        });
    }

    pub fn cancel_totals(&self) {
        self.totals_generation.fetch_add(1, Ordering::SeqCst);
    }

    pub fn drive_info(&self, name: String) {
        let _ = self.jobs.send(Job::DriveInfo(name));
    }

    pub fn watch(&self, dir: PathBuf) {
        let _ = self.watch.send(WatchCmd::Add(dir));
    }

    pub fn unwatch(&self, dir: PathBuf) {
        let _ = self.watch.send(WatchCmd::Remove(dir));
    }

    /// Starts a recursive name search and cancels any search still running.
    pub fn search(&self, roots: Vec<PathBuf>, query: String, show_hidden: bool) -> u64 {
        let generation = self.search_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let current = self.search_generation.clone();
        let out = self.out.clone();
        let waker = self.waker.clone();
        let _ = std::thread::Builder::new().name("search".into()).stack_size(512 * 1024).spawn(move || {
            run_search(roots, &query, show_hidden, generation, &current, &out, &waker);
        });
        generation
    }

    pub fn cancel_search(&self) {
        self.search_generation.fetch_add(1, Ordering::SeqCst);
    }
}

pub fn list_dir(path: &Path) -> (Vec<Entry>, Option<String>) {
    let reader = match std::fs::read_dir(path) {
        Ok(reader) => reader,
        Err(err) => return (Vec::new(), Some(describe_io(&err))),
    };
    let mut entries = Vec::new();
    for item in reader.flatten() {
        let name = item.file_name().to_string_lossy().into_owned();
        let Ok(mut meta) = item.metadata() else { continue };
        // A link counts as whatever it points at, as long as the target resolves.
        if meta.file_type().is_symlink()
            && let Ok(target) = std::fs::metadata(item.path())
        {
            meta = target;
        }
        let hidden = is_hidden(&name, &meta);
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        entries.push(Entry {
            name: name.into_boxed_str(),
            is_dir: meta.is_dir(),
            hidden,
            size: if meta.is_dir() { 0 } else { meta.len() },
            modified,
        });
    }
    entries.sort_by(|a, b| natural_cmp(&a.name, &b.name));
    (entries, None)
}

/// Top-level places when the tree is rooted above any folder. On Windows these are drives.
pub fn list_places() -> Vec<Entry> {
    let mut entries = Vec::new();
    #[cfg(windows)]
    for letter in crate::platform::drive_letters() {
        entries.push(Entry {
            name: format!("{letter}:").into_boxed_str(),
            is_dir: true,
            hidden: false,
            size: 0,
            modified: 0,
        });
    }
    #[cfg(not(windows))]
    entries.push(Entry { name: "/".into(), is_dir: true, hidden: false, size: 0, modified: 0 });
    entries
}

#[cfg(windows)]
fn is_hidden(name: &str, meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const HIDDEN: u32 = 0x2;
    const SYSTEM: u32 = 0x4;
    meta.file_attributes() & (HIDDEN | SYSTEM) != 0 || name.starts_with('.')
}

#[cfg(not(windows))]
fn is_hidden(name: &str, _meta: &std::fs::Metadata) -> bool {
    name.starts_with('.')
}

fn describe_io(err: &std::io::Error) -> String {
    match err.kind() {
        std::io::ErrorKind::PermissionDenied => "You don't have permission to open this folder.".into(),
        std::io::ErrorKind::NotFound => "This folder no longer exists.".into(),
        _ => format!("Couldn't read this folder: {err}"),
    }
}

/// Orders names the way people expect: case-insensitive, and "file2" before "file10".
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering::*;
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, _) => return Less,
            (_, None) => return Greater,
            (Some(p), Some(q)) if p.is_ascii_digit() && q.is_ascii_digit() => {
                let (mut na, mut nb) = (String::new(), String::new());
                while let Some(c) = x.peek().copied().filter(char::is_ascii_digit) {
                    na.push(c);
                    x.next();
                }
                while let Some(c) = y.peek().copied().filter(char::is_ascii_digit) {
                    nb.push(c);
                    y.next();
                }
                let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                match ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb)) {
                    Equal => continue,
                    other => return other,
                }
            }
            (Some(p), Some(q)) => {
                let (lp, lq) = (p.to_lowercase().next().unwrap_or(p), q.to_lowercase().next().unwrap_or(q));
                match lp.cmp(&lq) {
                    Equal => {
                        x.next();
                        y.next();
                    }
                    other => return other,
                }
            }
        }
    }
}

fn run_search(
    roots: Vec<PathBuf>,
    query: &str,
    show_hidden: bool,
    generation: u64,
    current: &AtomicU64,
    out: &Sender<FsEvent>,
    waker: &Waker,
) {
    let terms: Vec<String> = query.to_lowercase().split_whitespace().map(str::to_owned).collect();
    if terms.is_empty() {
        return;
    }
    let mut queue: std::collections::VecDeque<PathBuf> = roots.into();
    let mut batch = Vec::new();
    let mut last_flush = Instant::now();
    let mut found = 0usize;
    let mut capped = false;

    'walk: while let Some(dir) = queue.pop_front() {
        if current.load(Ordering::Relaxed) != generation {
            return;
        }
        let Ok(reader) = std::fs::read_dir(&dir) else { continue };
        for item in reader.flatten() {
            if current.load(Ordering::Relaxed) != generation {
                return;
            }
            let name = item.file_name().to_string_lossy().into_owned();
            let Ok(meta) = item.metadata() else { continue };
            if !show_hidden && is_hidden(&name, &meta) {
                continue;
            }
            // Never follow links while walking, so a cycle can't trap the search.
            let walk_into = meta.is_dir();
            let lower = name.to_lowercase();
            if terms.iter().all(|t| lower.contains(t.as_str())) {
                batch.push(SearchHit { path: item.path(), is_dir: walk_into });
                found += 1;
                if found >= SEARCH_RESULT_CAP {
                    capped = true;
                    break 'walk;
                }
            }
            if walk_into {
                queue.push_back(item.path());
            }
        }
        if !batch.is_empty() && last_flush.elapsed() > Duration::from_millis(60) {
            let hits = std::mem::take(&mut batch);
            let _ = out.send(FsEvent::SearchHits { generation, hits });
            waker();
            last_flush = Instant::now();
        }
    }
    if !batch.is_empty() {
        let _ = out.send(FsEvent::SearchHits { generation, hits: batch });
    }
    let _ = out.send(FsEvent::SearchDone { generation, capped });
    waker();
}

fn spawn_watcher(out: Sender<FsEvent>, waker: Waker) -> Sender<WatchCmd> {
    let (tx, rx) = channel::<WatchCmd>();
    let _ = std::thread::Builder::new().name("watch".into()).stack_size(256 * 1024).spawn(move || {
        let (raw_tx, raw_rx) = channel::<notify::Result<notify::Event>>();
        let Ok(mut watcher) = notify::recommended_watcher(move |res| {
            let _ = raw_tx.send(res);
        }) else {
            return;
        };
        let mut pending: FxHashMap<PathBuf, Instant> = FxHashMap::default();
        const SETTLE: Duration = Duration::from_millis(180);
        loop {
            match rx.try_recv() {
                Ok(WatchCmd::Add(dir)) => {
                    let _ = watcher.watch(&dir, RecursiveMode::NonRecursive);
                    continue;
                }
                Ok(WatchCmd::Remove(dir)) => {
                    let _ = watcher.unwatch(&dir);
                    continue;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
            let timeout = if pending.is_empty() { Duration::from_millis(250) } else { Duration::from_millis(40) };
            if let Ok(Ok(event)) = raw_rx.recv_timeout(timeout)
                && !matches!(event.kind, notify::EventKind::Access(_))
            {
                for path in event.paths {
                    if let Some(parent) = path.parent() {
                        pending.insert(parent.to_path_buf(), Instant::now());
                    }
                }
            }
            let now = Instant::now();
            let ready: Vec<PathBuf> =
                pending.iter().filter(|(_, t)| now.duration_since(**t) >= SETTLE).map(|(p, _)| p.clone()).collect();
            for dir in ready {
                pending.remove(&dir);
                let _ = out.send(FsEvent::Changed { dir });
                waker();
            }
        }
    });
    tx
}

/// Whether reading this file would make OneDrive or similar download it first.
#[cfg(windows)]
fn is_cloud_only(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    const OFFLINE: u32 = 0x1000;
    const RECALL_ON_OPEN: u32 = 0x0004_0000;
    const RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;
    std::fs::symlink_metadata(path)
        .map(|m| m.file_attributes() & (OFFLINE | RECALL_ON_OPEN | RECALL_ON_DATA_ACCESS) != 0)
        .unwrap_or(false)
}

#[cfg(not(windows))]
fn is_cloud_only(_path: &Path) -> bool {
    false
}

fn load_preview(path: &Path, side: u32) -> Preview {
    if is_cloud_only(path) {
        return Preview::Unavailable { reason: "This file is only in the cloud. Open it to download it.".into() };
    }
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "tif" | "tiff" => load_image_preview(path, side),
        "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" => Preview::Audio,
        "zip" => load_zip_listing(path),
        // Windows has renderers for these. It's the only thing that can draw them without
        // shipping a video decoder or a PDF engine in the app.
        "mp4" | "mov" | "mkv" | "avi" | "webm" | "wmv" | "m4v" | "mpg" | "mpeg" | "pdf" | "doc" | "docx" | "ppt"
        | "pptx" | "xls" | "xlsx" | "psd" | "heic" | "avif" | "svg" | "raw" | "cr2" | "nef" => load_thumbnail(path, side),
        _ => match load_text_preview(path) {
            Preview::None => load_thumbnail(path, side),
            other => other,
        },
    }
}

fn load_thumbnail(path: &Path, side: u32) -> Preview {
    match crate::platform::thumbnail(path, side as i32) {
        Some((width, height, rgba)) => Preview::Image { width, height, full_width: 0, full_height: 0, rgba },
        None => Preview::None,
    }
}

fn load_zip_listing(path: &Path) -> Preview {
    let Ok(file) = std::fs::File::open(path) else {
        return Preview::Unavailable { reason: "This archive couldn't be opened.".into() };
    };
    let Ok(mut archive) = zip::ZipArchive::new(file) else {
        return Preview::Unavailable { reason: "This archive couldn't be read.".into() };
    };
    let total = archive.len();
    let mut entries = Vec::with_capacity(total.min(LISTING_ENTRIES));
    for i in 0..total.min(LISTING_ENTRIES) {
        if let Ok(entry) = archive.by_index_raw(i) {
            entries.push((entry.name().map(|n| n.to_string()).unwrap_or_default(), entry.size()));
        }
    }
    Preview::Listing { entries, total }
}

fn load_image_preview(path: &Path, side: u32) -> Preview {
    let too_big = std::fs::metadata(path).map(|m| m.len() > PREVIEW_IMAGE_MAX_BYTES).unwrap_or(false);
    if too_big {
        return Preview::Unavailable { reason: "This image is too large to preview.".into() };
    }
    let decoded = image::ImageReader::open(path).and_then(|r| r.with_guessed_format()).map_err(|e| e.to_string()).and_then(
        |reader| {
            let mut reader = reader;
            let mut limits = image::Limits::default();
            limits.max_alloc = Some(512 * 1024 * 1024);
            reader.limits(limits);
            reader.decode().map_err(|e| e.to_string())
        },
    );
    match decoded {
        Ok(img) => {
            let (full_width, full_height) = (img.width(), img.height());
            let small = if full_width.max(full_height) > side {
                img.thumbnail(side, side)
            } else {
                img
            };
            let rgba = small.to_rgba8();
            Preview::Image { width: rgba.width(), height: rgba.height(), full_width, full_height, rgba: rgba.into_raw() }
        }
        Err(_) => Preview::Unavailable { reason: "This image couldn't be decoded.".into() },
    }
}

fn load_text_preview(path: &Path) -> Preview {
    use std::io::Read;
    let Ok(mut file) = std::fs::File::open(path) else {
        return Preview::Unavailable { reason: "This file couldn't be opened.".into() };
    };
    let mut buf = vec![0u8; PREVIEW_TEXT_BYTES];
    let mut filled = 0;
    while filled < buf.len() {
        match file.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(_) => return Preview::Unavailable { reason: "This file couldn't be read.".into() },
        }
    }
    buf.truncate(filled);
    if filled == 0 {
        return Preview::Text { lines: Vec::new(), truncated: false };
    }
    let probe = &buf[..buf.len().min(4096)];
    if probe.contains(&0) {
        return Preview::None;
    }
    let text = String::from_utf8_lossy(&buf);
    let replacements = text.chars().filter(|c| *c == '\u{FFFD}').count();
    if replacements * 20 > text.chars().count() {
        return Preview::None;
    }
    let mut lines = Vec::new();
    for line in text.lines().take(PREVIEW_TEXT_LINES) {
        lines.push(line.replace('\t', "    "));
    }
    let truncated = filled == PREVIEW_TEXT_BYTES || text.lines().count() > PREVIEW_TEXT_LINES;
    Preview::Text { lines, truncated }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order_handles_numbers_and_case() {
        let mut names = vec!["file10", "File2", "file1", "alpha", "Beta", "file02b"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, vec!["alpha", "Beta", "file1", "File2", "file02b", "file10"]);
    }

    #[test]
    fn this_computer_has_at_least_one_disk() {
        assert!(!list_places().is_empty());
    }

    #[test]
    fn listing_a_missing_folder_reports_it() {
        let (entries, error) = list_dir(Path::new("this/does/not/exist/anywhere"));
        assert!(entries.is_empty());
        assert!(error.unwrap().contains("no longer exists"));
    }
}
