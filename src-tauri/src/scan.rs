//! Whole-drive scan: a size tree for the space map, per-category totals for the overview, file
//! types, and the largest files. The tree of the last scan of each drive is saved (see [`save`]),
//! so the space map is there after a restart without scanning again.

use crate::filetype::{ext_of, kind_of, KINDS};
use crate::util::{display, entry_on_disk_len, home, list_dir, system_root, unix_secs};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

/// Files at least this big are listed on the large-files page.
pub(crate) const LARGE_FILE_BYTES: u64 = 100 * 1024 * 1024;
const LARGE_FILE_KEEP: usize = 500;
const HARD_LINK_CHECK_BYTES: u64 = 8 * 1024 * 1024;
/// Folders smaller than this are folded into their parent as soon as they are measured: the map
/// never needs them one by one, and on a system drive they are most of the ~500k folders.
const KEEP_FOLDER_BYTES: u64 = 1024 * 1024;
/// Files at least this big are remembered by name, so the map can show them as tiles of their own
/// and search and the file types page can find them...
const TOP_FILE_BYTES: u64 = 4 * 1024 * 1024;
/// ...up to this many per folder, the biggest.
const TOP_FILES_PER_DIR: usize = 24;
/// Extensions kept for the file types page, the biggest by bytes.
const EXTS_KEEP: usize = 400;

/// Bytes by kind of file ([`crate::filetype`]).
pub type Types = [u64; KINDS];

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Node {
    #[serde(rename = "n")]
    pub name: String,
    /// Everything under this folder.
    #[serde(rename = "s")]
    pub size: u64,
    /// Files directly in this folder.
    #[serde(rename = "o")]
    pub own: u64,
    #[serde(rename = "f")]
    pub files: u64,
    /// The folder could not be read (no permission). Its size is unknown, not zero.
    #[serde(rename = "d", default, skip_serializing_if = "std::ops::Not::not")]
    pub denied: bool,
    /// Bytes in subfolders folded away for being small (see [`KEEP_FOLDER_BYTES`]).
    #[serde(rename = "m", default)]
    pub small: u64,
    /// Bytes under this folder by kind of file.
    #[serde(rename = "t", default)]
    pub types: Types,
    /// When a file under this folder last changed, as Unix seconds; 0 with no files.
    #[serde(rename = "w", default)]
    pub newest: u64,
    /// The biggest files directly in this folder, biggest first (see [`TOP_FILE_BYTES`]).
    #[serde(rename = "b", default, skip_serializing_if = "Vec::is_empty")]
    pub top: Vec<FileItem>,
    #[serde(rename = "c", default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Node>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct FileItem {
    #[serde(rename = "n")]
    pub name: String,
    #[serde(rename = "s")]
    pub size: u64,
    #[serde(rename = "w")]
    pub modified: u64,
    #[serde(rename = "k")]
    pub kind: u8,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LargeFile {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub modified: u64,
}

/// Bytes and files of one extension across a scan.
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExtStat {
    /// Lowercase, without the dot; empty for files with no extension.
    pub ext: String,
    pub kind: u8,
    pub bytes: u64,
    pub files: u64,
}

#[derive(Default)]
pub struct ScanCtx {
    pub files: AtomicU64,
    pub dirs: AtomicU64,
    pub bytes: AtomicU64,
    pub denied: AtomicU64,
    pub cancel: AtomicBool,
    pub current: Mutex<String>,
    large: Mutex<Vec<LargeFile>>,
    seen_links: Mutex<std::collections::HashSet<u64>>,
    exts: Mutex<HashMap<String, (u64, u64)>>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    pub current: String,
}

impl ScanCtx {
    pub(crate) fn add_large(&self, path: String, name: String, size: u64, modified: u64) {
        self.large.lock().unwrap().push(LargeFile {
            path,
            name,
            size,
            modified,
        });
    }

    pub(crate) fn add_exts(&self, exts: HashMap<String, (u64, u64)>) {
        if exts.is_empty() {
            return;
        }
        let mut all = self.exts.lock().unwrap();
        for (ext, (bytes, count)) in exts {
            let e = all.entry(ext).or_default();
            e.0 += bytes;
            e.1 += count;
        }
    }

    pub fn progress(&self) -> Progress {
        Progress {
            files: self.files.load(Ordering::Relaxed),
            dirs: self.dirs.load(Ordering::Relaxed),
            bytes: self.bytes.load(Ordering::Relaxed),
            current: self.current.lock().unwrap().clone(),
        }
    }
}

/// What every folder of one scan needs to know about the volume.
struct Volume {
    /// Hard links can be recognised by file id (see [`crate::util::is_ntfs`]).
    ntfs: bool,
    /// `C:\Windows`: WinSxS hard-links most of System32, Servicing and the driver store, so here
    /// every file is checked, not just big ones.
    system_root: PathBuf,
}

fn add_types(to: &mut Types, from: &Types) {
    for (a, b) in to.iter_mut().zip(from) {
        *a += b;
    }
}

fn sub_types(from: &mut Types, what: &Types) {
    for (a, b) in from.iter_mut().zip(what) {
        *a = a.saturating_sub(*b);
    }
}

fn one_type(kind: u8, bytes: u64) -> Types {
    let mut t = Types::default();
    t[kind as usize] = bytes;
    t
}

/// Folds `node`'s small subfolders into [`Node::small`]. Totals stay exact; only the breakdown
/// below 1 MB goes.
fn fold_small(node: &mut Node) {
    let mut small = 0;
    node.children.retain(|c| {
        if c.size < KEEP_FOLDER_BYTES && !c.denied {
            small += c.size;
            false
        } else {
            true
        }
    });
    node.small += small;
    node.children.shrink_to_fit();
}

fn scan_dir(path: &Path, name: String, ctx: &ScanCtx, vol: &Volume, all_links: bool) -> Node {
    let mut node = Node {
        name,
        ..Default::default()
    };
    if ctx.cancel.load(Ordering::Relaxed) {
        return node;
    }
    let dirs_seen = ctx.dirs.fetch_add(1, Ordering::Relaxed);
    if dirs_seen.is_multiple_of(97) {
        *ctx.current.lock().unwrap() = display(path);
    }
    let entries = match list_dir(path) {
        Ok(entries) => entries,
        Err(_) => {
            node.denied = true;
            ctx.denied.fetch_add(1, Ordering::Relaxed);
            return node;
        }
    };
    let mut subdirs = Vec::new();
    let mut files = Vec::new();
    for entry in entries {
        if entry.is_link() {
            continue;
        }
        if entry.is_dir() {
            subdirs.push(entry.name);
            continue;
        }
        let len = entry_on_disk_len(path, &entry);
        files.push((entry, len));
    }
    // A file reachable under two names (a hard link) is one file on disk: only the first name the
    // scan meets counts it. Remembering ids costs memory, so outside the Windows folder only files
    // big enough to skew the totals are remembered.
    if vol.ntfs {
        let mut seen = None;
        for (entry, len) in files.iter_mut() {
            if entry.file_id != 0 && (all_links || *len >= HARD_LINK_CHECK_BYTES) {
                let seen = seen.get_or_insert_with(|| ctx.seen_links.lock().unwrap());
                if !seen.insert(entry.file_id) {
                    *len = 0;
                }
            }
        }
    }
    let mut exts: HashMap<String, (u64, u64)> = HashMap::new();
    for (entry, len) in files {
        let name = entry.name.to_string_lossy();
        count_file(&mut node, &mut exts, &name, len, entry.modified);
        if len >= LARGE_FILE_BYTES {
            ctx.add_large(
                display(&path.join(&entry.name)),
                name.into_owned(),
                len,
                entry.modified,
            );
        }
    }
    ctx.add_exts(exts);
    finish_files(&mut node);
    ctx.files.fetch_add(node.files, Ordering::Relaxed);
    ctx.bytes.fetch_add(node.own, Ordering::Relaxed);
    let scan_all = |dirs: Vec<std::ffi::OsString>| -> Vec<Node> {
        dirs.into_par_iter()
            .map(|n| {
                let p = path.join(&n);
                let links = all_links || p.as_os_str().eq_ignore_ascii_case(&vol.system_root);
                scan_dir(&p, n.to_string_lossy().into_owned(), ctx, vol, links)
            })
            .collect()
    };
    if vol.ntfs && path.as_os_str().eq_ignore_ascii_case(&vol.system_root) {
        // A file in WinSxS and System32 counts under whichever name is met first. Walking WinSxS
        // last makes that the same every time — otherwise gigabytes would hop between the two
        // from one scan to the next, and the history page would report growth that never was.
        // WinSxS keeps only what is its own, as DISM counts the component store.
        let (sxs, rest): (Vec<_>, Vec<_>) = subdirs
            .into_iter()
            .partition(|n| n.eq_ignore_ascii_case("WinSxS"));
        node.children = scan_all(rest);
        node.children.extend(scan_all(sxs));
    } else {
        node.children = scan_all(subdirs);
    }
    finish_dir(&mut node);
    node
}

/// Adds one file directly in `node` (its own bytes, kind, extension, age; its name when big).
pub(crate) fn count_file(
    node: &mut Node,
    exts: &mut HashMap<String, (u64, u64)>,
    name: &str,
    len: u64,
    modified: u64,
) {
    node.own += len;
    node.files += 1;
    node.newest = node.newest.max(modified);
    let ext = ext_of(name);
    let kind = kind_of(&ext);
    node.types[kind as usize] += len;
    let e = exts.entry(ext).or_default();
    e.0 += len;
    e.1 += 1;
    if len >= TOP_FILE_BYTES {
        node.top.push(FileItem {
            name: name.to_string(),
            size: len,
            modified,
            kind,
        });
    }
}

/// Once a folder's own files are all counted: only the biggest are kept by name.
pub(crate) fn finish_files(node: &mut Node) {
    node.top.sort_unstable_by(|a, b| b.size.cmp(&a.size));
    node.top.truncate(TOP_FILES_PER_DIR);
    node.top.shrink_to_fit();
}

/// Once a folder's subfolders are in: totals, kinds and age from them, then the small ones folded.
pub(crate) fn finish_dir(node: &mut Node) {
    node.children.sort_unstable_by(|a, b| b.size.cmp(&a.size));
    node.size = node.own + node.children.iter().map(|c| c.size).sum::<u64>();
    node.files += node.children.iter().map(|c| c.files).sum::<u64>();
    for c in &node.children {
        add_types(&mut node.types, &c.types);
        node.newest = node.newest.max(c.newest);
    }
    fold_small(node);
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ScanData {
    pub root: PathBuf,
    pub tree: Node,
    pub large: Vec<LargeFile>,
    pub summary: Summary,
    #[serde(default)]
    pub exts: Vec<ExtStat>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CategoryStat {
    pub id: String,
    pub bytes: u64,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub root: String,
    pub total: u64,
    pub free: u64,
    pub scanned: u64,
    pub files: u64,
    pub dirs: u64,
    pub denied_dirs: u64,
    pub elapsed_ms: u64,
    pub cancelled: bool,
    /// When the scan finished, as Unix seconds.
    pub scanned_at: u64,
    pub categories: Vec<CategoryStat>,
}

pub fn run(root: &Path, ctx: &ScanCtx) -> ScanData {
    let started = Instant::now();
    let vol = Volume {
        ntfs: crate::util::is_ntfs(root),
        system_root: system_root(),
    };
    let at_system_root = root.as_os_str().eq_ignore_ascii_case(&vol.system_root);
    let tree = scan_dir(root, display(root), ctx, &vol, at_system_root);
    // The ids were only needed while walking; on a system drive they are a few hundred thousand.
    *ctx.seen_links.lock().unwrap() = Default::default();
    finish(root, tree, ctx, started)
}

/// Everything after the walk, however the tree was gathered: the large files, the extensions, the
/// overview's categories and the summary.
pub(crate) fn finish(root: &Path, tree: Node, ctx: &ScanCtx, started: Instant) -> ScanData {
    let mut large = std::mem::take(&mut *ctx.large.lock().unwrap());
    large.sort_unstable_by(|a, b| b.size.cmp(&a.size));
    large.truncate(LARGE_FILE_KEEP);
    let exts = ext_stats(std::mem::take(&mut *ctx.exts.lock().unwrap()));

    let root_str = display(root);
    let (total, free) = crate::sys::drive_space(&root_str).unwrap_or((0, 0));
    let mut totals = Totals::default();
    categorize(
        &tree,
        &mut Vec::new(),
        None,
        &home_components(root),
        &mut totals,
    );
    let used = total.saturating_sub(free);
    // What the disk says is used but no folder we could read accounts for: System Volume
    // Information (restore points), NTFS metadata, and folders we were not allowed into.
    totals.add("hidden", used.saturating_sub(tree.size));

    let summary = Summary {
        root: root_str,
        total,
        free,
        scanned: tree.size,
        files: tree.files,
        dirs: ctx.dirs.load(Ordering::Relaxed),
        denied_dirs: ctx.denied.load(Ordering::Relaxed),
        elapsed_ms: started.elapsed().as_millis() as u64,
        cancelled: ctx.cancel.load(Ordering::Relaxed),
        scanned_at: unix_secs(std::time::SystemTime::now()),
        categories: totals.into_stats(),
    };
    ScanData {
        root: root.to_path_buf(),
        tree,
        large,
        summary,
        exts,
    }
}

fn ext_stats(all: HashMap<String, (u64, u64)>) -> Vec<ExtStat> {
    let mut out: Vec<ExtStat> = all
        .into_iter()
        .filter(|(_, (bytes, _))| *bytes > 0)
        .map(|(ext, (bytes, files))| ExtStat {
            kind: kind_of(&ext),
            ext,
            bytes,
            files,
        })
        .collect();
    out.sort_unstable_by(|a, b| b.bytes.cmp(&a.bytes));
    out.truncate(EXTS_KEEP);
    out
}

/// The user profile's path relative to the scanned root, lowercased, when it lies under it.
fn home_components(root: &Path) -> Option<Vec<String>> {
    let home = home();
    let rel = home.strip_prefix(root).ok()?;
    Some(
        rel.components()
            .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
            .collect(),
    )
}

#[derive(Default)]
struct Totals(Vec<(&'static str, u64)>);

impl Totals {
    fn add(&mut self, id: &'static str, bytes: u64) {
        match self.0.iter_mut().find(|(k, _)| *k == id) {
            Some((_, v)) => *v += bytes,
            None => self.0.push((id, bytes)),
        }
    }

    fn into_stats(mut self) -> Vec<CategoryStat> {
        self.0.sort_unstable_by(|a, b| b.1.cmp(&a.1));
        self.0
            .into_iter()
            .filter(|(_, b)| *b > 0)
            .map(|(id, bytes)| CategoryStat {
                id: id.to_string(),
                bytes,
            })
            .collect()
    }
}

const PERSONAL: &[&str] = &[
    "documents",
    "desktop",
    "downloads",
    "pictures",
    "videos",
    "music",
    "onedrive",
    "xwechat_files",
    "wps cloud files",
    "saved games",
    "favorites",
    "contacts",
];

/// Which category a folder starts, if any; folders that start none inherit their parent's.
/// `rel` is the lowercased path from the scanned root down to (and including) this folder.
fn category_of(rel: &[String], home: Option<&[String]>) -> Option<&'static str> {
    let name = rel.last()?.as_str();
    if name == "node_modules" {
        return Some("dev");
    }
    if rel.len() == 1 {
        return Some(match name {
            "windows"
            | "system volume information"
            | "recovery"
            | "$winreagent"
            | "$windows.~bt"
            | "$windows.~ws"
            | "windows.old" => "system",
            "program files" | "program files (x86)" | "programdata" => "programs",
            "$recycle.bin" => "recycle",
            "users" => "personal",
            _ => "other",
        });
    }
    let home = home?;
    if rel.len() <= home.len() || rel[..home.len()] != *home {
        return None;
    }
    let sub: Vec<&str> = rel[home.len()..].iter().map(String::as_str).collect();
    match sub.as_slice() {
        [n] if n.starts_with('.') => Some("dev"),
        [n] if PERSONAL.contains(n) => Some("personal"),
        ["appdata"] => Some("appdata"),
        [_] => Some("projects"),
        ["appdata", "local", "temp"] | ["appdata", "local", "crashdumps"] => Some("temp"),
        ["appdata", "local", n]
            if matches!(
                *n,
                "android"
                    | "jetbrains"
                    | "npm-cache"
                    | "pip"
                    | "yarn"
                    | "pnpm"
                    | "go-build"
                    | "gradle"
                    | "nuget"
            ) =>
        {
            Some("dev")
        }
        ["appdata", "roaming", n] if matches!(*n, "npm" | "jetbrains" | "code" | "cursor") => {
            Some("dev")
        }
        ["appdata", "local", "google", n] if n.starts_with("androidstudio") => Some("dev"),
        _ => None,
    }
}

fn categorize(
    node: &Node,
    rel: &mut Vec<String>,
    inherited: Option<&'static str>,
    home: &Option<Vec<String>>,
    totals: &mut Totals,
) {
    let cat = if rel.is_empty() {
        // Files sitting in the drive root are, in practice, pagefile.sys / hiberfil.sys / swapfile.sys.
        Some("vm")
    } else {
        category_of(rel, home.as_deref()).or(inherited)
    };
    // Small subfolders were folded in while scanning; under 1 MB each, they go with their parent.
    totals.add(cat.unwrap_or("other"), node.own + node.small);
    for child in &node.children {
        rel.push(child.name.to_lowercase());
        categorize(
            child,
            rel,
            if rel.len() == 1 { None } else { cat },
            home,
            totals,
        );
        rel.pop();
    }
}

// ---------------------------------------------------------------------------------------------
// Reading the tree: the space map, search, stale folders, files by type.

/// The node at `path` and the names leading to it from the root.
fn find<'a>(data: &'a ScanData, path: &str) -> Option<&'a Node> {
    let target = PathBuf::from(path);
    let rel = target.strip_prefix(&data.root).ok()?;
    let mut node = &data.tree;
    for comp in rel.components() {
        let name = comp.as_os_str().to_string_lossy();
        node = node
            .children
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(&name))?;
    }
    Some(node)
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum ItemKind {
    Dir,
    File,
    /// What is left of a folder after the items shown: small files and folders, lumped together.
    Rest,
}

/// One item of the space map or its list: a folder, a file, or a folder's leftovers.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TreeItem {
    pub name: String,
    pub path: String,
    pub kind: ItemKind,
    pub size: u64,
    pub files: u64,
    /// The newest file under a folder, or the file's own last change, as Unix seconds.
    pub modified: u64,
    pub denied: bool,
    /// Bytes by kind of file (a file has all of its size under one kind).
    pub types: Types,
    pub has_children: bool,
    /// Present down to the depth asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<TreeItem>>,
}

fn dir_item(node: &Node, path: String) -> TreeItem {
    TreeItem {
        name: node.name.clone(),
        path,
        kind: ItemKind::Dir,
        size: node.size,
        files: node.files,
        modified: node.newest,
        denied: node.denied,
        types: node.types,
        has_children: !node.children.is_empty() || node.own > 0 || node.small > 0,
        children: None,
    }
}

fn join(parent: &str, name: &str) -> String {
    if parent.ends_with('\\') {
        format!("{parent}{name}")
    } else {
        format!("{parent}\\{name}")
    }
}

/// `node` at `path` with its contents `depth` levels down. Items under `min` bytes, and those past
/// `limit` in one folder, are left in the folder's leftovers.
fn build(node: &Node, path: String, depth: u32, min: u64, limit: usize) -> TreeItem {
    let mut item = dir_item(node, path);
    if depth == 0 || node.denied {
        return item;
    }
    let mut kids = Vec::new();
    let mut rest_types = node.types;
    let mut rest_size = node.small + node.own;
    let mut rest_files = node.files - node.children.iter().map(|c| c.files).sum::<u64>();
    let mut rest_has_dirs = node.small > 0;
    let mut rest_newest = 0;
    for c in &node.children {
        if c.size == 0 && !c.denied {
            continue;
        }
        if (c.size >= min || c.denied) && kids.len() < limit {
            sub_types(&mut rest_types, &c.types);
            kids.push(build(c, join(&item.path, &c.name), depth - 1, min, limit));
        } else {
            rest_size += c.size;
            rest_files += c.files;
            rest_has_dirs = true;
            rest_newest = rest_newest.max(c.newest);
        }
    }
    for f in &node.top {
        if f.size >= min && kids.len() < limit {
            rest_size -= f.size;
            rest_files = rest_files.saturating_sub(1);
            rest_types[f.kind as usize] = rest_types[f.kind as usize].saturating_sub(f.size);
            kids.push(TreeItem {
                name: f.name.clone(),
                path: join(&item.path, &f.name),
                kind: ItemKind::File,
                size: f.size,
                files: 1,
                modified: f.modified,
                denied: false,
                types: one_type(f.kind, f.size),
                has_children: false,
                children: None,
            });
        }
    }
    if rest_size > 0 {
        let name = if rest_has_dirs {
            "小文件夹和零散文件".to_string()
        } else if kids.iter().any(|k| k.kind == ItemKind::File) {
            format!("其他 {rest_files} 个文件")
        } else {
            format!("此文件夹中的 {rest_files} 个文件")
        };
        kids.push(TreeItem {
            name,
            path: item.path.clone(),
            kind: ItemKind::Rest,
            size: rest_size,
            files: rest_files,
            modified: rest_newest,
            denied: false,
            types: rest_types,
            has_children: false,
            children: None,
        });
    }
    kids.sort_by(|a, b| b.size.cmp(&a.size));
    item.children = Some(kids);
    item
}

/// The folder at `path` with `depth` levels of contents; items smaller than `min_share` of it are
/// lumped together (0 keeps everything).
pub fn tree(
    data: &ScanData,
    path: &str,
    depth: u32,
    min_share: f64,
    limit: usize,
) -> Option<TreeItem> {
    let node = find(data, path)?;
    let min = (node.size as f64 * min_share) as u64;
    let path = if std::ptr::eq(node, &data.tree) {
        display(&data.root)
    } else {
        path.trim_end_matches('\\').to_string()
    };
    Some(build(node, path, depth, min, limit))
}

/// A folder or file found by [`search`] or [`stale`].
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub files: u64,
    pub modified: u64,
    pub types: Types,
}

impl Hit {
    fn dir(node: &Node, path: String) -> Hit {
        Hit {
            name: node.name.clone(),
            path,
            is_dir: true,
            size: node.size,
            files: node.files,
            modified: node.newest,
            types: node.types,
        }
    }
    fn file(f: &FileItem, path: String) -> Hit {
        Hit {
            name: f.name.clone(),
            path,
            is_dir: false,
            size: f.size,
            files: 1,
            modified: f.modified,
            types: one_type(f.kind, f.size),
        }
    }
}

/// What [`search`] looks for. Every field set must match.
#[derive(Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Query {
    /// Part of the name, any case.
    #[serde(default)]
    pub text: String,
    /// Only files of this kind (folders never match a kind).
    pub kind: Option<u8>,
    /// Only files with this extension.
    pub ext: Option<String>,
    #[serde(default)]
    pub min_size: u64,
    /// Nothing changed since this Unix time.
    pub older_than: Option<u64>,
    /// Only folders, or only files.
    pub only: Option<String>,
    /// Only under this folder.
    pub under: Option<String>,
}

const SEARCH_MAX: usize = 300;

/// Folders and files of the last scan matching `q`, biggest first. Only what the scan kept by name
/// can be found: folders of 1 MB and up, and each folder's biggest files of 4 MB and up.
pub fn search(data: &ScanData, q: &Query) -> Vec<Hit> {
    let text = q.text.trim().to_lowercase();
    let ext = q.ext.as_deref().map(str::to_lowercase);
    let want_dirs = q.only.as_deref() != Some("files") && q.kind.is_none() && ext.is_none();
    let want_files = q.only.as_deref() != Some("dirs");
    let (start, start_path) = match q.under.as_deref() {
        Some(p) => match find(data, p) {
            Some(n) => (n, p.trim_end_matches('\\').to_string()),
            None => return Vec::new(),
        },
        None => (&data.tree, display(&data.root)),
    };
    let old_enough = |t: u64| q.older_than.is_none_or(|cut| t > 0 && t < cut);
    let mut out = Vec::new();
    fn walk(node: &Node, path: &str, visit: &mut dyn FnMut(&Node, &str)) {
        visit(node, path);
        for c in &node.children {
            walk(c, &join(path, &c.name), visit);
        }
    }
    walk(start, &start_path, &mut |node, path| {
        // The folder searched in is not a result of its own search.
        let is_start = std::ptr::eq(node, start);
        if want_dirs
            && !is_start
            && node.size >= q.min_size
            && (text.is_empty() || node.name.to_lowercase().contains(&text))
            && old_enough(node.newest)
        {
            out.push(Hit::dir(node, path.to_string()));
        }
        if want_files {
            for f in &node.top {
                if f.size >= q.min_size
                    && q.kind.is_none_or(|k| k == f.kind)
                    && ext.as_deref().is_none_or(|e| ext_of(&f.name) == e)
                    && (text.is_empty() || f.name.to_lowercase().contains(&text))
                    && old_enough(f.modified)
                {
                    out.push(Hit::file(f, join(path, &f.name)));
                }
            }
        }
    });
    out.sort_unstable_by(|a, b| b.size.cmp(&a.size));
    out.truncate(SEARCH_MAX);
    out
}

/// Top-level folders left out of [`stale`]: Windows and installed programs are rarely written to
/// however much they are used, so "unchanged for a year" would say nothing about them.
const NOT_STALE: &[&str] = &[
    "windows",
    "program files",
    "program files (x86)",
    "programdata",
    "$recycle.bin",
    "system volume information",
    "recovery",
    "$winreagent",
    "windows.old",
    "$windows.~bt",
    "$windows.~ws",
];

/// The biggest folders in which nothing has changed since `cutoff` (Unix seconds), at least
/// `min_size` big. A folder found is not looked into: its subfolders are as old.
pub fn stale(data: &ScanData, cutoff: u64, min_size: u64) -> Vec<Hit> {
    fn walk(node: &Node, path: &str, depth: usize, cutoff: u64, min: u64, out: &mut Vec<Hit>) {
        for c in &node.children {
            if c.size < min || c.denied {
                continue;
            }
            if depth == 0 && NOT_STALE.contains(&c.name.to_lowercase().as_str()) {
                continue;
            }
            let path = join(path, &c.name);
            if c.newest > 0 && c.newest < cutoff {
                out.push(Hit::dir(c, path));
            } else {
                walk(c, &path, depth + 1, cutoff, min, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(
        &data.tree,
        &display(&data.root),
        0,
        cutoff,
        min_size,
        &mut out,
    );
    out.sort_unstable_by(|a, b| b.size.cmp(&a.size));
    out.truncate(SEARCH_MAX);
    out
}

// ---------------------------------------------------------------------------------------------
// Changing the tree after files were deleted or moved from the map.

/// Takes `path` out of the tree, as if it had not been there when scanned: the sizes of every
/// folder above it go down. Only items the tree has by name (folders, big files) can be taken.
pub fn forget(data: &mut ScanData, path: &str) -> bool {
    let target = PathBuf::from(path);
    let Ok(rel) = target.strip_prefix(&data.root) else {
        return false;
    };
    let names: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    fn go(node: &mut Node, names: &[String]) -> Option<(u64, u64, Types)> {
        let (first, rest) = names.split_first()?;
        let removed = if rest.is_empty() {
            if let Some(i) = node
                .children
                .iter()
                .position(|c| c.name.eq_ignore_ascii_case(first))
            {
                let c = node.children.remove(i);
                (c.size, c.files, c.types)
            } else if let Some(i) = node
                .top
                .iter()
                .position(|f| f.name.eq_ignore_ascii_case(first))
            {
                let f = node.top.remove(i);
                node.own = node.own.saturating_sub(f.size);
                (f.size, 1, one_type(f.kind, f.size))
            } else {
                return None;
            }
        } else {
            let child = node
                .children
                .iter_mut()
                .find(|c| c.name.eq_ignore_ascii_case(first))?;
            go(child, rest)?
        };
        node.size = node.size.saturating_sub(removed.0);
        node.files = node.files.saturating_sub(removed.1);
        sub_types(&mut node.types, &removed.2);
        Some(removed)
    }
    let found = go(&mut data.tree, &names).is_some();
    let lower = path.to_lowercase();
    let under = format!("{}\\", lower.trim_end_matches('\\'));
    data.large.retain(|f| {
        let p = f.path.to_lowercase();
        p != lower && !p.starts_with(&under)
    });
    found
}

// ---------------------------------------------------------------------------------------------
// Saving the last scan of each drive.

/// Bumped when what is saved changes shape; older files are then ignored.
const SAVED_VERSION: u32 = 1;

#[derive(Deserialize)]
struct Saved {
    v: u32,
    data: ScanData,
}

fn saved_file(dir: &Path, root: &str) -> PathBuf {
    let letter: String = root
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    dir.join(format!("{}.json", letter.to_uppercase()))
}

/// Saves `data` as the last scan of its drive. A cancelled scan is incomplete and is not kept.
pub fn save(dir: &Path, data: &ScanData) -> Result<(), String> {
    if data.summary.cancelled {
        return Ok(());
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let file = saved_file(dir, &data.summary.root);
    // Written next to the old one and swapped in, so a crash mid-write keeps the previous scan.
    let tmp = file.with_extension("json.tmp");
    write_file(&tmp, data)?;
    std::fs::rename(&tmp, &file).map_err(|e| e.to_string())
}

/// The last scan saved for the drive `root`, if any.
pub fn load(dir: &Path, root: &str) -> Option<ScanData> {
    read_file(&saved_file(dir, root))
}

/// Writes a scan to `path` (also how the elevated fast-scan helper hands its result over).
pub fn write_file(path: &Path, data: &ScanData) -> Result<(), String> {
    #[derive(Serialize)]
    struct SavedRef<'a> {
        v: u32,
        data: &'a ScanData,
    }
    let out = std::io::BufWriter::new(std::fs::File::create(path).map_err(|e| e.to_string())?);
    serde_json::to_writer(
        out,
        &SavedRef {
            v: SAVED_VERSION,
            data,
        },
    )
    .map_err(|e| e.to_string())
}

pub fn read_file(path: &Path) -> Option<ScanData> {
    let file = std::fs::File::open(path).ok()?;
    let saved: Saved = serde_json::from_reader(std::io::BufReader::new(file)).ok()?;
    (saved.v == SAVED_VERSION).then_some(saved.data)
}

/// Nodes in a tree, for measuring what a scan keeps in memory.
#[cfg(test)]
pub fn count_nodes(node: &Node) -> usize {
    1 + node.children.iter().map(count_nodes).sum::<usize>()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(p: &str) -> Vec<String> {
        p.split('/').map(str::to_string).collect()
    }

    #[test]
    fn categories_follow_the_path() {
        let home = rel("users/me");
        let h = Some(home.as_slice());
        assert_eq!(category_of(&rel("windows"), h), Some("system"));
        assert_eq!(
            category_of(&rel("program files (x86)"), h),
            Some("programs")
        );
        assert_eq!(category_of(&rel("users/me/.gradle"), h), Some("dev"));
        assert_eq!(category_of(&rel("users/me/downloads"), h), Some("personal"));
        assert_eq!(category_of(&rel("users/me/chorus"), h), Some("projects"));
        assert_eq!(
            category_of(&rel("users/me/appdata/local/temp"), h),
            Some("temp")
        );
        assert_eq!(
            category_of(&rel("users/me/appdata/local/android"), h),
            Some("dev")
        );
        assert_eq!(
            category_of(&rel("users/me/chorus/node_modules"), h),
            Some("dev")
        );
        // Deeper folders with no rule of their own inherit.
        assert_eq!(category_of(&rel("users/me/chorus/apps"), h), None);
    }

    /// Read-only scan of the system drive: `cargo test real_scan -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_scan() {
        let ctx = ScanCtx::default();
        let data = run(&crate::util::system_drive(), &ctx);
        println!("tree nodes {}", count_nodes(&data.tree));
        let s = &data.summary;
        let gb = |b: u64| b as f64 / 1073741824.0;
        println!(
            "total {:.1} GB, free {:.1} GB, scanned {:.1} GB, {} files, {} dirs, {} denied, {} ms",
            gb(s.total),
            gb(s.free),
            gb(s.scanned),
            s.files,
            s.dirs,
            s.denied_dirs,
            s.elapsed_ms
        );
        for c in &s.categories {
            println!("  {:<10} {:>8.1} GB", c.id, gb(c.bytes));
        }
        for f in data.large.iter().take(8) {
            println!("  {:>8.2} GB  {}", gb(f.size), f.path);
        }
    }

    const MB: u64 = 1024 * 1024;

    /// A small made-up drive: `C:\x` with a 6 MB folder holding a 5 MB video, and tiny leftovers.
    fn sample() -> ScanData {
        let mut video = Node {
            name: "videos".into(),
            size: 6 * MB,
            own: 6 * MB,
            files: 3,
            newest: 100,
            top: vec![FileItem {
                name: "a.mp4".into(),
                size: 5 * MB,
                modified: 100,
                kind: crate::filetype::VIDEO,
            }],
            ..Default::default()
        };
        video.types[crate::filetype::VIDEO as usize] = 5 * MB;
        video.types[crate::filetype::OTHER as usize] = MB;
        let mut root = Node {
            name: r"C:\x".into(),
            own: 2000,
            files: 4,
            children: vec![
                video,
                Node {
                    name: "tiny".into(),
                    size: 3000,
                    own: 3000,
                    files: 1,
                    newest: 900,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        root.size = 6 * MB + 5000;
        root.files = 5;
        root.types = root.children[0].types;
        root.types[crate::filetype::OTHER as usize] += 5000;
        fold_small(&mut root);
        ScanData {
            root: PathBuf::from(r"C:\x"),
            tree: root,
            large: vec![],
            summary: Summary::default(),
            exts: vec![],
        }
    }

    #[test]
    fn small_folders_fold_into_their_parent_without_losing_bytes() {
        let data = sample();
        assert_eq!(data.tree.children.len(), 1, "only the 6 MB folder stays");
        assert_eq!(data.tree.small, 3000);
        let t = tree(&data, r"C:\x", 2, 0.0, 100).unwrap();
        let kids = t.children.unwrap();
        let total: u64 = kids.iter().map(|k| k.size).sum();
        assert_eq!(total, data.tree.size, "the listing still adds up");
        let rest = kids.iter().find(|k| k.kind == ItemKind::Rest).unwrap();
        assert_eq!(rest.name, "小文件夹和零散文件");
        assert_eq!(rest.size, 5000);
    }

    #[test]
    fn big_files_are_tiles_of_their_own() {
        let data = sample();
        let t = tree(&data, r"C:\x\videos", 1, 0.0, 100).unwrap();
        let kids = t.children.unwrap();
        assert_eq!(kids[0].kind, ItemKind::File);
        assert_eq!(kids[0].path, r"C:\x\videos\a.mp4");
        assert_eq!(kids[0].types[crate::filetype::VIDEO as usize], 5 * MB);
        assert_eq!(kids[1].kind, ItemKind::Rest);
        assert_eq!(kids[1].name, "其他 2 个文件");
        assert_eq!(kids[1].size, MB);
        assert_eq!(kids[1].types[crate::filetype::VIDEO as usize], 0);
        // Asked to leave out what is under half the folder, the video still shows, the rest not.
        let t = tree(&data, r"C:\x\videos", 1, 0.5, 100).unwrap();
        assert_eq!(t.children.unwrap().len(), 2);
    }

    #[test]
    fn search_stale_and_forget() {
        let mut data = sample();
        let hits = search(
            &data,
            &Query {
                text: "MP4".into(),
                ..Default::default()
            },
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, r"C:\x\videos\a.mp4");
        let videos = search(
            &data,
            &Query {
                kind: Some(crate::filetype::VIDEO),
                ..Default::default()
            },
        );
        assert_eq!(videos.len(), 1, "a kind finds files only");
        let old = stale(&data, 500, MB);
        assert_eq!(old.len(), 1);
        assert_eq!(old[0].name, "videos");
        assert!(stale(&data, 50, MB).is_empty());

        assert!(forget(&mut data, r"C:\x\videos\a.mp4"));
        assert_eq!(data.tree.size, MB + 5000);
        assert_eq!(data.tree.children[0].size, MB);
        assert_eq!(data.tree.types[crate::filetype::VIDEO as usize], 0);
        assert!(!forget(&mut data, r"C:\x\nothing"));
    }

    #[test]
    fn saved_scans_load_back() {
        let dir = tempfile::tempdir().unwrap();
        let mut data = sample();
        data.summary.root = r"C:\".into();
        data.summary.scanned_at = 42;
        save(dir.path(), &data).unwrap();
        let back = load(dir.path(), r"C:\").unwrap();
        assert_eq!(back.summary.scanned_at, 42);
        assert_eq!(back.tree.size, data.tree.size);
        assert_eq!(back.tree.children[0].top[0].name, "a.mp4");
        assert!(load(dir.path(), r"D:\").is_none());
    }

    #[test]
    fn scan_counts_sizes_and_skips_nothing_readable() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("a/b")).unwrap();
        std::fs::write(dir.path().join("a/one.bin"), vec![0u8; 1000]).unwrap();
        std::fs::write(dir.path().join("a/b/two.mp4"), vec![0u8; 500]).unwrap();
        std::fs::write(dir.path().join("top.bin"), vec![0u8; 10]).unwrap();
        let ctx = ScanCtx::default();
        let vol = Volume {
            ntfs: crate::util::is_ntfs(dir.path()),
            system_root: system_root(),
        };
        let tree = scan_dir(dir.path(), "root".into(), &ctx, &vol, false);
        assert_eq!(tree.size, 1510);
        assert_eq!(tree.files, 3);
        assert_eq!(tree.own, 10);
        // Under 1 MB, "a" was folded into the root; its bytes and types still count.
        assert_eq!(tree.small, 1500);
        assert_eq!(tree.types[crate::filetype::VIDEO as usize], 500);
        assert_eq!(tree.types[crate::filetype::OTHER as usize], 1010);
        assert!(tree.newest > 0);
        let exts = ext_stats(std::mem::take(&mut *ctx.exts.lock().unwrap()));
        assert_eq!(exts[0].ext, "bin");
        assert_eq!(exts[0].bytes, 1010);
    }

    #[test]
    fn big_files_and_folders_are_kept_by_name() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("keep")).unwrap();
        std::fs::write(dir.path().join("keep/big.iso"), vec![0u8; 5 * MB as usize]).unwrap();
        let vol = Volume {
            ntfs: crate::util::is_ntfs(dir.path()),
            system_root: system_root(),
        };
        let tree = scan_dir(dir.path(), "root".into(), &ScanCtx::default(), &vol, false);
        assert_eq!(tree.children.len(), 1);
        assert_eq!(tree.children[0].top[0].name, "big.iso");
        assert_eq!(tree.children[0].top[0].kind, crate::filetype::DISK);
    }

    /// The WinSxS case: one small file under two names counts once.
    #[test]
    fn hard_links_count_once() {
        let dir = tempfile::tempdir().unwrap();
        if !crate::util::is_ntfs(dir.path()) {
            return;
        }
        std::fs::create_dir_all(dir.path().join("winsxs")).unwrap();
        std::fs::create_dir_all(dir.path().join("system32")).unwrap();
        std::fs::write(dir.path().join("winsxs/k.dll"), vec![0u8; 4000]).unwrap();
        std::fs::hard_link(
            dir.path().join("winsxs/k.dll"),
            dir.path().join("system32/k.dll"),
        )
        .unwrap();
        let vol = Volume {
            ntfs: true,
            system_root: system_root(),
        };
        // Small files are only checked inside the Windows folder: here, every folder is treated so.
        let tree = scan_dir(dir.path(), "root".into(), &ScanCtx::default(), &vol, true);
        assert_eq!(tree.size, 4000);
        assert_eq!(tree.files, 2);
        // Elsewhere, a small hard link is not worth remembering and still counts twice.
        let tree = scan_dir(dir.path(), "root".into(), &ScanCtx::default(), &vol, false);
        assert_eq!(tree.size, 8000);
    }
}
