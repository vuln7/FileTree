//! Everything that changes the disk. Three rules hold for every operation here:
//! nothing is ever overwritten, deleting always goes through the Recycle Bin, and each
//! operation returns what it takes to undo it.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub enum Request {
    Rename { from: PathBuf, new_name: String },
    Move { items: Vec<PathBuf>, dest: PathBuf },
    Copy { items: Vec<PathBuf>, dest: PathBuf },
    NewFolder { parent: PathBuf, name: String },
    NewFile { parent: PathBuf, name: String },
    Trash { items: Vec<PathBuf> },
    /// Unpacks an archive into a new folder beside it.
    Extract { archive: PathBuf },
    /// Packs these into a new archive beside them.
    Compress { items: Vec<PathBuf> },
    Undo(Undo),
}

/// How to reverse a finished operation.
#[derive(Clone, Debug, PartialEq)]
pub enum Undo {
    /// Move each (now, was) back where it was.
    Moved(Vec<(PathBuf, PathBuf)>),
    /// These were created, so they go to the Recycle Bin.
    Created(Vec<PathBuf>),
    /// These went to the Recycle Bin, so they come back.
    Trashed(Vec<PathBuf>),
}

#[derive(Debug, Default)]
pub struct Outcome {
    pub message: String,
    pub undo: Option<Undo>,
    /// Folders whose contents changed.
    pub changed: Vec<PathBuf>,
    /// What to select afterwards.
    pub focus: Option<PathBuf>,
    pub failed: bool,
}

impl Outcome {
    fn failure(message: String) -> Outcome {
        Outcome { message, failed: true, ..Default::default() }
    }
}

/// Why a name can't be used, in words for the person typing it.
pub fn name_problem(name: &str) -> Option<&'static str> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Some("A name can't be empty.");
    }
    if trimmed == "." || trimmed == ".." {
        return Some("That name is reserved.");
    }
    if cfg!(windows) {
        if name.chars().any(|c| matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || (c as u32) < 32) {
            return Some("A name can't contain any of these: < > : \" / \\ | ? *");
        }
        if name.ends_with('.') || name.ends_with(' ') {
            return Some("A name can't end with a dot or a space.");
        }
        let stem = trimmed.split('.').next().unwrap_or("").to_ascii_uppercase();
        let reserved = ["CON", "PRN", "AUX", "NUL"];
        let numbered = (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit();
        if reserved.contains(&stem.as_str()) || numbered {
            return Some("Windows reserves that name.");
        }
    } else if name.contains('/') {
        return Some("A name can't contain a slash.");
    }
    None
}

/// A name in `dir` that doesn't exist yet: `name`, then `name (2)`, `name (3)`, and so on.
pub fn unused_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if fs::symlink_metadata(&first).is_err() {
        return first;
    }
    let path = Path::new(name);
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| name.to_string());
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    (2u32..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|candidate| fs::symlink_metadata(candidate).is_err())
        .expect("some numbered name is free")
}

/// Places Windows itself depends on, which deserve a warning before being changed.
pub fn is_protected(path: &Path) -> bool {
    if path.parent().is_none() {
        return true;
    }
    let lower = path.to_string_lossy().to_lowercase().replace('/', "\\");
    if cfg!(windows) {
        let system = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into()).to_lowercase();
        let tops = [
            system,
            std::env::var("ProgramFiles").unwrap_or_default().to_lowercase(),
            std::env::var("ProgramFiles(x86)").unwrap_or_default().to_lowercase(),
            std::env::var("ProgramData").unwrap_or_default().to_lowercase(),
        ];
        if tops.iter().any(|t| !t.is_empty() && (lower == *t || lower.starts_with(&format!("{t}\\")))) {
            return true;
        }
        let home = std::env::var("USERPROFILE").unwrap_or_default().to_lowercase();
        if !home.is_empty() && lower == home {
            return true;
        }
    }
    false
}

fn describe(err: &std::io::Error) -> String {
    match err.kind() {
        std::io::ErrorKind::PermissionDenied => "you don't have permission".into(),
        std::io::ErrorKind::NotFound => "it no longer exists".into(),
        std::io::ErrorKind::AlreadyExists => "something with that name already exists".into(),
        _ => match err.raw_os_error() {
            Some(32) | Some(33) => "another program is using it".into(),
            Some(112) => "there isn't enough disk space".into(),
            _ => err.to_string(),
        },
    }
}

fn name_of(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string())
}

fn count(n: usize, one: &str) -> String {
    if n == 1 { format!("1 {one}") } else { format!("{n} {one}s") }
}

fn parents(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for path in paths {
        if let Some(parent) = path.parent().map(Path::to_path_buf)
            && !out.contains(&parent)
        {
            out.push(parent);
        }
    }
    out
}

/// Bytes to copy, to report progress against. Links are not followed.
fn tree_bytes(path: &Path) -> u64 {
    let Ok(meta) = fs::symlink_metadata(path) else { return 0 };
    if meta.is_dir() {
        fs::read_dir(path).map(|r| r.flatten().map(|e| tree_bytes(&e.path())).sum()).unwrap_or(0)
    } else if meta.file_type().is_symlink() {
        0
    } else {
        meta.len()
    }
}

/// Copies a file or folder to a target that must not exist. Links are skipped rather than followed.
fn copy_tree(from: &Path, to: &Path, done: &mut u64, total: u64, progress: &dyn Fn(f32)) -> std::io::Result<()> {
    let meta = fs::symlink_metadata(from)?;
    if meta.file_type().is_symlink() {
        return Ok(());
    }
    if meta.is_dir() {
        fs::create_dir(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            copy_tree(&entry.path(), &to.join(entry.file_name()), done, total, progress)?;
        }
        return Ok(());
    }
    if fs::symlink_metadata(to).is_ok() {
        return Err(std::io::ErrorKind::AlreadyExists.into());
    }
    fs::copy(from, to)?;
    *done += meta.len();
    if total > 0 {
        progress(*done as f32 / total as f32);
    }
    Ok(())
}

/// Moves by renaming, or by copying then removing the original when crossing disks.
fn move_one(from: &Path, to: &Path, progress: &dyn Fn(f32)) -> std::io::Result<()> {
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        // Windows reports a move between disks as error 17.
        Err(err) if err.raw_os_error() == Some(17) || err.kind() == std::io::ErrorKind::CrossesDevices => {
            let total = tree_bytes(from);
            let mut done = 0;
            if let Err(copy_err) = copy_tree(from, to, &mut done, total, progress) {
                // Leave no half-finished copy behind. The original is untouched.
                let _ = if to.is_dir() { fs::remove_dir_all(to) } else { fs::remove_file(to) };
                return Err(copy_err);
            }
            if from.is_dir() { fs::remove_dir_all(from) } else { fs::remove_file(from) }
        }
        Err(err) => Err(err),
    }
}

pub fn run(request: Request, progress: &dyn Fn(f32)) -> Outcome {
    match request {
        Request::Rename { from, new_name } => rename(&from, &new_name),
        Request::Move { items, dest } => move_items(&items, &dest, progress),
        Request::Copy { items, dest } => copy_items(&items, &dest, progress),
        Request::NewFolder { parent, name } => create(&parent, &name, true),
        Request::NewFile { parent, name } => create(&parent, &name, false),
        Request::Trash { items } => trash_items(&items),
        Request::Extract { archive } => extract(&archive, progress),
        Request::Compress { items } => compress(&items, progress),
        Request::Undo(undo) => undo_op(undo, progress),
    }
}

/// Unpacks an archive. Everything goes in a folder of its own that didn't exist before, so
/// nothing that was already there can be touched.
fn extract(archive: &Path, progress: &dyn Fn(f32)) -> Outcome {
    let Some(dir) = archive.parent() else { return Outcome::failure("That archive has no folder to unpack into.".into()) };
    let stem = archive.file_stem().map_or_else(|| "Archive".to_string(), |s| s.to_string_lossy().into_owned());
    let dest = unused_path(dir, &stem);
    let result = (|| -> Result<(), String> {
        let file = fs::File::open(archive).map_err(|e| describe(&e))?;
        let mut zip = zip::ZipArchive::new(file).map_err(|_| "it doesn't look like a zip file".to_string())?;
        fs::create_dir(&dest).map_err(|e| describe(&e))?;
        let total = zip.len().max(1);
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
            // A name that tries to climb out of the folder is skipped, not followed.
            let Some(relative) = entry.enclosed_name() else { continue };
            let to = dest.join(relative);
            if entry.is_dir() {
                fs::create_dir_all(&to).map_err(|e| describe(&e))?;
            } else {
                if let Some(parent) = to.parent() {
                    fs::create_dir_all(parent).map_err(|e| describe(&e))?;
                }
                let mut out = fs::File::create_new(&to).map_err(|e| describe(&e))?;
                std::io::copy(&mut entry, &mut out).map_err(|e| describe(&e))?;
            }
            progress((i + 1) as f32 / total as f32);
        }
        Ok(())
    })();
    match result {
        Ok(()) => Outcome {
            message: format!("Unpacked {} into {}", name_of(archive), name_of(&dest)),
            undo: Some(Undo::Created(vec![dest.clone()])),
            changed: vec![dir.to_path_buf()],
            focus: Some(dest),
            failed: false,
        },
        Err(err) => {
            // Only ever removes the folder made just above, which didn't exist before.
            let _ = fs::remove_dir_all(&dest);
            Outcome::failure(format!("Couldn't unpack {}: {err}.", name_of(archive)))
        }
    }
}

fn compress(items: &[PathBuf], progress: &dyn Fn(f32)) -> Outcome {
    let Some(dir) = items.first().and_then(|p| p.parent()) else {
        return Outcome::failure("There's nothing to compress.".into());
    };
    let stem = if items.len() == 1 {
        items[0].file_stem().map_or_else(|| "Archive".to_string(), |s| s.to_string_lossy().into_owned())
    } else {
        "Archive".to_string()
    };
    let dest = unused_path(dir, &format!("{stem}.zip"));
    let total: u64 = items.iter().map(|p| tree_bytes(p)).sum::<u64>().max(1);
    let result = (|| -> Result<(), String> {
        let file = fs::File::create_new(&dest).map_err(|e| describe(&e))?;
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .large_file(true);
        let mut done = 0u64;
        for item in items {
            add_to_zip(&mut zip, item, &name_of(item), options, &mut done, total, progress)?;
        }
        zip.finish().map_err(|e| e.to_string())?;
        Ok(())
    })();
    match result {
        Ok(()) => Outcome {
            message: format!("Compressed into {}", name_of(&dest)),
            undo: Some(Undo::Created(vec![dest.clone()])),
            changed: vec![dir.to_path_buf()],
            focus: Some(dest),
            failed: false,
        },
        Err(err) => {
            // Only ever removes the archive made just above, which didn't exist before.
            let _ = fs::remove_file(&dest);
            Outcome::failure(format!("Couldn't compress that: {err}."))
        }
    }
}

fn add_to_zip(
    zip: &mut zip::ZipWriter<fs::File>,
    path: &Path,
    name: &str,
    options: zip::write::SimpleFileOptions,
    done: &mut u64,
    total: u64,
    progress: &dyn Fn(f32),
) -> Result<(), String> {
    let meta = fs::symlink_metadata(path).map_err(|e| describe(&e))?;
    if meta.is_symlink() {
        return Ok(());
    }
    if meta.is_dir() {
        zip.add_directory(format!("{name}/"), options).map_err(|e| e.to_string())?;
        for entry in fs::read_dir(path).map_err(|e| describe(&e))?.flatten() {
            let child = format!("{name}/{}", entry.file_name().to_string_lossy());
            add_to_zip(zip, &entry.path(), &child, options, done, total, progress)?;
        }
        return Ok(());
    }
    zip.start_file(name, options).map_err(|e| e.to_string())?;
    let mut input = fs::File::open(path).map_err(|e| describe(&e))?;
    std::io::copy(&mut input, zip).map_err(|e| describe(&e))?;
    *done += meta.len();
    progress((*done as f32 / total as f32).min(1.0));
    Ok(())
}

/// Something in the Recycle Bin.
#[derive(Clone, Debug)]
pub struct BinItem {
    pub name: String,
    pub was_at: PathBuf,
    pub deleted: i64,
}

#[cfg(any(windows, all(unix, not(target_os = "macos"))))]
pub fn list_bin() -> Result<Vec<BinItem>, String> {
    let mut items: Vec<BinItem> = trash::os_limited::list()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|t| BinItem {
            name: t.name.to_string_lossy().into_owned(),
            was_at: t.original_parent.join(&t.name),
            deleted: t.time_deleted,
        })
        .collect();
    items.sort_by_key(|item| std::cmp::Reverse(item.deleted));
    Ok(items)
}

#[cfg(not(any(windows, all(unix, not(target_os = "macos")))))]
pub fn list_bin() -> Result<Vec<BinItem>, String> {
    Err("The bin can't be looked into on this system.".into())
}

fn rename(from: &Path, new_name: &str) -> Outcome {
    if let Some(problem) = name_problem(new_name) {
        return Outcome::failure(problem.to_string());
    }
    let Some(dir) = from.parent() else { return Outcome::failure("A whole disk can't be renamed here.".into()) };
    let to = dir.join(new_name);
    let same_item = name_of(from).to_lowercase() == new_name.to_lowercase();
    if !same_item && fs::symlink_metadata(&to).is_ok() {
        return Outcome::failure(format!("There's already something called “{new_name}” here."));
    }
    match fs::rename(from, &to) {
        Ok(()) => Outcome {
            message: format!("Renamed to {new_name}"),
            undo: Some(Undo::Moved(vec![(to.clone(), from.to_path_buf())])),
            changed: vec![dir.to_path_buf()],
            focus: Some(to),
            failed: false,
        },
        Err(err) => Outcome::failure(format!("Couldn't rename {}: {}.", name_of(from), describe(&err))),
    }
}

fn move_items(items: &[PathBuf], dest: &Path, progress: &dyn Fn(f32)) -> Outcome {
    let mut moved = Vec::new();
    let mut problem = None;
    for item in items {
        if dest.starts_with(item) {
            problem = Some(format!("{} can't go inside itself.", name_of(item)));
            continue;
        }
        if item.parent() == Some(dest) {
            continue;
        }
        let to = unused_path(dest, &name_of(item));
        match move_one(item, &to, progress) {
            Ok(()) => moved.push((to, item.clone())),
            Err(err) => problem = Some(format!("Couldn't move {}: {}.", name_of(item), describe(&err))),
        }
    }
    finish_transfer("Moved", moved.len(), moved.iter().map(|p| p.1.clone()).collect(), dest, moved.first().map(|p| p.0.clone()), problem, {
        (!moved.is_empty()).then(|| Undo::Moved(moved.clone()))
    })
}

fn copy_items(items: &[PathBuf], dest: &Path, progress: &dyn Fn(f32)) -> Outcome {
    let total: u64 = items.iter().map(|p| tree_bytes(p)).sum();
    let mut done = 0;
    let mut made = Vec::new();
    let mut problem = None;
    for item in items {
        if dest.starts_with(item) && dest != item.parent().unwrap_or(dest) {
            problem = Some(format!("{} can't be copied inside itself.", name_of(item)));
            continue;
        }
        let to = unused_path(dest, &name_of(item));
        match copy_tree(item, &to, &mut done, total, progress) {
            Ok(()) => made.push(to),
            Err(err) => {
                // A half-finished copy helps nobody, and it was made by this operation.
                let _ = if to.is_dir() { fs::remove_dir_all(&to) } else { fs::remove_file(&to) };
                problem = Some(format!("Couldn't copy {}: {}.", name_of(item), describe(&err)));
            }
        }
    }
    finish_transfer("Copied", made.len(), Vec::new(), dest, made.first().cloned(), problem, {
        (!made.is_empty()).then(|| Undo::Created(made.clone()))
    })
}

fn finish_transfer(
    verb: &str,
    done: usize,
    sources: Vec<PathBuf>,
    dest: &Path,
    focus: Option<PathBuf>,
    problem: Option<String>,
    undo: Option<Undo>,
) -> Outcome {
    let mut changed = parents(sources);
    if !changed.iter().any(|p| p == dest) {
        changed.push(dest.to_path_buf());
    }
    let message = match (&problem, done) {
        (Some(p), 0) => p.clone(),
        (Some(p), _) => format!("{verb} {}, but: {p}", count(done, "item")),
        (None, 0) => "Nothing to do: they're already there.".to_string(),
        (None, _) => format!("{verb} {} to {}", count(done, "item"), name_of(dest)),
    };
    Outcome { message, undo, changed, focus, failed: problem.is_some() }
}

fn create(parent: &Path, name: &str, folder: bool) -> Outcome {
    if let Some(problem) = name_problem(name) {
        return Outcome::failure(problem.to_string());
    }
    let path = unused_path(parent, name);
    let made = if folder {
        fs::create_dir(&path)
    } else {
        fs::OpenOptions::new().write(true).create_new(true).open(&path).map(|_| ())
    };
    let what = if folder { "folder" } else { "file" };
    match made {
        Ok(()) => Outcome {
            message: format!("Created {}", name_of(&path)),
            undo: Some(Undo::Created(vec![path.clone()])),
            changed: vec![parent.to_path_buf()],
            focus: Some(path),
            failed: false,
        },
        Err(err) => Outcome::failure(format!("Couldn't create that {what}: {}.", describe(&err))),
    }
}

fn trash_items(items: &[PathBuf]) -> Outcome {
    let names = if items.len() == 1 { name_of(&items[0]) } else { count(items.len(), "item") };
    match trash::delete_all(items) {
        Ok(()) => Outcome {
            message: format!("Moved {names} to the Recycle Bin"),
            undo: Some(Undo::Trashed(items.to_vec())),
            changed: parents(items.iter().cloned()),
            focus: None,
            failed: false,
        },
        Err(err) => Outcome::failure(format!("Couldn't move {names} to the Recycle Bin: {err}.")),
    }
}

#[cfg(any(windows, all(unix, not(target_os = "macos"))))]
fn restore(items: &[PathBuf]) -> Result<(), String> {
    let everything = trash::os_limited::list().map_err(|e| e.to_string())?;
    let mut chosen = Vec::new();
    for item in items {
        // If the same path was deleted more than once, the latest is the one being undone.
        let found = everything
            .iter()
            .filter(|t| t.original_parent.join(&t.name) == *item)
            .max_by_key(|t| t.time_deleted)
            .cloned();
        match found {
            Some(entry) => chosen.push(entry),
            None => return Err(format!("{} isn't in the Recycle Bin any more", name_of(item))),
        }
    }
    trash::os_limited::restore_all(chosen).map_err(|e| e.to_string())
}

#[cfg(not(any(windows, all(unix, not(target_os = "macos")))))]
fn restore(_items: &[PathBuf]) -> Result<(), String> {
    Err("restoring from the bin isn't available on this system".into())
}

fn undo_op(undo: Undo, progress: &dyn Fn(f32)) -> Outcome {
    match undo {
        Undo::Moved(pairs) => {
            let mut problem = None;
            let mut back = Vec::new();
            for (now, was) in &pairs {
                if fs::symlink_metadata(was).is_ok() {
                    problem = Some(format!("Something new is already at {}'s old spot.", name_of(was)));
                    continue;
                }
                match move_one(now, was, progress) {
                    Ok(()) => back.push(was.clone()),
                    Err(err) => problem = Some(format!("Couldn't put {} back: {}.", name_of(now), describe(&err))),
                }
            }
            let mut changed = parents(pairs.iter().map(|p| p.0.clone()));
            for parent in parents(pairs.iter().map(|p| p.1.clone())) {
                if !changed.contains(&parent) {
                    changed.push(parent);
                }
            }
            Outcome {
                message: problem.clone().unwrap_or_else(|| "Undone".to_string()),
                undo: None,
                changed,
                focus: back.first().cloned(),
                failed: problem.is_some(),
            }
        }
        Undo::Created(paths) => match trash::delete_all(&paths) {
            Ok(()) => Outcome {
                message: "Undone. What was created is in the Recycle Bin.".to_string(),
                changed: parents(paths),
                ..Default::default()
            },
            Err(err) => Outcome::failure(format!("Couldn't undo that: {err}.")),
        },
        Undo::Trashed(paths) => match restore(&paths) {
            Ok(()) => Outcome {
                message: "Restored from the Recycle Bin".to_string(),
                focus: paths.first().cloned(),
                changed: parents(paths),
                ..Default::default()
            },
            Err(err) => Outcome::failure(format!("Couldn't restore that: {err}.")),
        },
    }
}

/// Adds up a folder's size. Stops as soon as `keep_going` says the answer is no longer wanted.
pub fn folder_totals(path: &Path, keep_going: &dyn Fn() -> bool) -> Option<(u64, u64, u64)> {
    let (mut bytes, mut files, mut folders) = (0u64, 0u64, 0u64);
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(reader) = fs::read_dir(&dir) else { continue };
        for entry in reader.flatten() {
            if !keep_going() {
                return None;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                folders += 1;
                stack.push(entry.path());
            } else {
                files += 1;
                bytes += meta.len();
            }
        }
    }
    Some((bytes, files, folders))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway folder, removed when the test ends.
    struct Sandbox(PathBuf);

    impl Sandbox {
        fn new(name: &str) -> Sandbox {
            let dir = std::env::temp_dir().join(format!("filetree-ops-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Sandbox(dir)
        }

        fn file(&self, rel: &str, body: &str) -> PathBuf {
            let path = self.0.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, body).unwrap();
            path
        }

        fn dir(&self, rel: &str) -> PathBuf {
            let path = self.0.join(rel);
            fs::create_dir_all(&path).unwrap();
            path
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn go(request: Request) -> Outcome {
        run(request, &|_| {})
    }

    #[test]
    fn names_are_checked() {
        assert!(name_problem("report.pdf").is_none());
        assert!(name_problem("").is_some());
        assert!(name_problem("..").is_some());
        if cfg!(windows) {
            assert!(name_problem("a:b").is_some());
            assert!(name_problem("trailing.").is_some());
            assert!(name_problem("CON.txt").is_some());
            assert!(name_problem("COM1").is_some());
            assert!(name_problem("COMPUTER").is_none());
        }
    }

    #[test]
    fn free_names_count_upward() {
        let s = Sandbox::new("unique");
        s.file("a.txt", "1");
        s.file("a (2).txt", "2");
        assert_eq!(unused_path(&s.0, "a.txt"), s.0.join("a (3).txt"));
        assert_eq!(unused_path(&s.0, "b.txt"), s.0.join("b.txt"));
    }

    #[test]
    fn rename_refuses_to_overwrite_and_can_be_undone() {
        let s = Sandbox::new("rename");
        let a = s.file("a.txt", "keep me");
        let b = s.file("b.txt", "keep me too");
        let refused = go(Request::Rename { from: a.clone(), new_name: "b.txt".into() });
        assert!(refused.failed);
        assert_eq!(fs::read_to_string(&b).unwrap(), "keep me too");
        assert_eq!(fs::read_to_string(&a).unwrap(), "keep me");

        let done = go(Request::Rename { from: a.clone(), new_name: "c.txt".into() });
        assert!(!done.failed, "{}", done.message);
        assert!(!a.exists());
        assert!(s.0.join("c.txt").exists());
        go(Request::Undo(done.undo.unwrap()));
        assert_eq!(fs::read_to_string(&a).unwrap(), "keep me");
        assert!(!s.0.join("c.txt").exists());
    }

    #[test]
    fn a_case_only_rename_works() {
        let s = Sandbox::new("case");
        let a = s.file("report.txt", "x");
        let done = go(Request::Rename { from: a, new_name: "REPORT.txt".into() });
        assert!(!done.failed, "{}", done.message);
        let names: Vec<String> = fs::read_dir(&s.0).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names, vec!["REPORT.txt"]);
    }

    #[test]
    fn moving_never_overwrites_and_can_be_undone() {
        let s = Sandbox::new("move");
        let a = s.file("from/a.txt", "new");
        let existing = s.file("to/a.txt", "old");
        let dest = s.0.join("to");
        let done = go(Request::Move { items: vec![a.clone()], dest: dest.clone() });
        assert!(!done.failed, "{}", done.message);
        assert_eq!(fs::read_to_string(&existing).unwrap(), "old");
        assert_eq!(fs::read_to_string(dest.join("a (2).txt")).unwrap(), "new");
        assert!(!a.exists());
        go(Request::Undo(done.undo.unwrap()));
        assert_eq!(fs::read_to_string(&a).unwrap(), "new");
        assert_eq!(fs::read_to_string(&existing).unwrap(), "old");
    }

    #[test]
    fn a_folder_cant_move_into_itself() {
        let s = Sandbox::new("self");
        let outer = s.dir("outer");
        let inner = s.dir("outer/inner");
        let done = go(Request::Move { items: vec![outer.clone()], dest: inner.clone() });
        assert!(done.failed);
        assert!(inner.exists());
    }

    #[test]
    fn compressing_then_unpacking_gives_the_same_files_and_never_overwrites() {
        let s = Sandbox::new("zip-round-trip");
        let folder = s.0.join("seeds");
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("tomato.txt"), "red").unwrap();
        fs::create_dir(folder.join("inner")).unwrap();
        fs::write(folder.join("inner").join("basil.txt"), "green").unwrap();

        let packed = go(Request::Compress { items: vec![folder.clone()] });
        assert!(!packed.failed, "{}", packed.message);
        let archive = packed.focus.clone().unwrap();
        assert_eq!(archive, s.0.join("seeds.zip"));

        // A folder called "seeds" is already here, so unpacking must go somewhere new.
        let unpacked = go(Request::Extract { archive: archive.clone() });
        assert!(!unpacked.failed, "{}", unpacked.message);
        let dest = unpacked.focus.clone().unwrap();
        assert_eq!(dest, s.0.join("seeds (2)"));
        assert_eq!(fs::read_to_string(dest.join("seeds").join("tomato.txt")).unwrap(), "red");
        assert_eq!(fs::read_to_string(dest.join("seeds").join("inner").join("basil.txt")).unwrap(), "green");
        assert_eq!(fs::read_to_string(folder.join("tomato.txt")).unwrap(), "red");
    }

    #[test]
    fn unpacking_something_that_isnt_a_zip_leaves_nothing_behind() {
        let s = Sandbox::new("zip-bad");
        let fake = s.0.join("fake.zip");
        fs::write(&fake, "not a zip").unwrap();
        let done = go(Request::Extract { archive: fake });
        assert!(done.failed);
        assert!(!s.0.join("fake").exists());
    }

    #[test]
    fn undo_wont_overwrite_something_new() {
        let s = Sandbox::new("undo-clash");
        let a = s.file("a.txt", "original");
        let done = go(Request::Rename { from: a.clone(), new_name: "b.txt".into() });
        fs::write(&a, "newer").unwrap();
        let undone = go(Request::Undo(done.undo.unwrap()));
        assert!(undone.failed);
        assert_eq!(fs::read_to_string(&a).unwrap(), "newer");
        assert_eq!(fs::read_to_string(s.0.join("b.txt")).unwrap(), "original");
    }

    #[test]
    fn copying_keeps_the_original_and_copies_folders_whole() {
        let s = Sandbox::new("copy");
        let folder = s.dir("src");
        s.file("src/one.txt", "1");
        s.file("src/deep/two.txt", "2");
        let dest = s.dir("dest");
        let done = go(Request::Copy { items: vec![folder.clone()], dest: dest.clone() });
        assert!(!done.failed, "{}", done.message);
        assert_eq!(fs::read_to_string(dest.join("src/deep/two.txt")).unwrap(), "2");
        assert!(folder.join("one.txt").exists());
        let again = go(Request::Copy { items: vec![folder.clone()], dest: dest.clone() });
        assert_eq!(again.focus, Some(dest.join("src (2)")));
    }

    #[test]
    fn duplicating_in_place_gets_a_new_name() {
        let s = Sandbox::new("dup");
        let a = s.file("a.txt", "x");
        let done = go(Request::Copy { items: vec![a.clone()], dest: s.0.clone() });
        assert_eq!(done.focus, Some(s.0.join("a (2).txt")));
        assert!(a.exists());
    }

    #[test]
    fn new_items_never_replace_old_ones() {
        let s = Sandbox::new("new");
        let a = s.file("note.txt", "precious");
        let done = go(Request::NewFile { parent: s.0.clone(), name: "note.txt".into() });
        assert_eq!(done.focus, Some(s.0.join("note (2).txt")));
        assert_eq!(fs::read_to_string(&a).unwrap(), "precious");
        let folder = go(Request::NewFolder { parent: s.0.clone(), name: "stuff".into() });
        assert!(folder.focus.unwrap().is_dir());
    }

    #[test]
    fn system_places_are_recognised() {
        if cfg!(windows) {
            assert!(is_protected(Path::new("C:\\")));
            assert!(is_protected(Path::new("C:\\Windows\\System32")));
            assert!(!is_protected(Path::new("C:\\Users\\someone\\Documents\\x.txt")));
        }
    }

    #[test]
    fn folder_totals_add_up_and_can_be_cancelled() {
        let s = Sandbox::new("totals");
        s.file("a.txt", "12345");
        s.file("d/b.txt", "123");
        assert_eq!(folder_totals(&s.0, &|| true), Some((8, 2, 1)));
        assert_eq!(folder_totals(&s.0, &|| false), None);
    }
}
