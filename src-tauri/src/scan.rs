//! Whole-drive scan: a size tree for the space map, per-category totals for the overview, and the
//! largest files.

use crate::util::{
    display, hard_link_identity, home, is_link, long_path, on_disk_len_at, unix_secs,
};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

/// Files at least this big are listed on the large-files page.
const LARGE_FILE_BYTES: u64 = 100 * 1024 * 1024;
const LARGE_FILE_KEEP: usize = 500;
const HARD_LINK_CHECK_BYTES: u64 = 8 * 1024 * 1024;
/// Once a scan is summarized, folders smaller than this are folded into their parent: the
/// browser never needs them one by one, and on a system drive they are most of the ~500k nodes.
const KEEP_FOLDER_BYTES: u64 = 1024 * 1024;

pub struct Node {
    pub name: String,
    /// Everything under this folder.
    pub size: u64,
    /// Files directly in this folder.
    pub own: u64,
    pub files: u64,
    /// The folder could not be read (no permission). Its size is unknown, not zero.
    pub denied: bool,
    /// Bytes in subfolders folded away by [`prune`].
    pub small: u64,
    pub children: Vec<Node>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LargeFile {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub modified: u64,
}

#[derive(Default)]
pub struct ScanCtx {
    pub files: AtomicU64,
    pub dirs: AtomicU64,
    pub bytes: AtomicU64,
    pub cancel: AtomicBool,
    pub current: Mutex<String>,
    large: Mutex<Vec<LargeFile>>,
    seen_links: Mutex<std::collections::HashSet<(u32, u64)>>,
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
    pub fn progress(&self) -> Progress {
        Progress {
            files: self.files.load(Ordering::Relaxed),
            dirs: self.dirs.load(Ordering::Relaxed),
            bytes: self.bytes.load(Ordering::Relaxed),
            current: self.current.lock().unwrap().clone(),
        }
    }
}

fn scan_dir(path: &Path, name: String, ctx: &ScanCtx) -> Node {
    let mut node = Node {
        name,
        size: 0,
        own: 0,
        files: 0,
        denied: false,
        small: 0,
        children: Vec::new(),
    };
    if ctx.cancel.load(Ordering::Relaxed) {
        return node;
    }
    let dirs_seen = ctx.dirs.fetch_add(1, Ordering::Relaxed);
    if dirs_seen.is_multiple_of(97) {
        *ctx.current.lock().unwrap() = display(path);
    }
    let rd = match std::fs::read_dir(long_path(path)) {
        Ok(rd) => rd,
        Err(_) => {
            node.denied = true;
            return node;
        }
    };
    let mut subdirs = Vec::new();
    let mut own_bytes = 0u64;
    for entry in rd.flatten() {
        let Ok(md) = entry.metadata() else { continue };
        if is_link(&md) {
            continue;
        }
        if md.is_dir() {
            subdirs.push((
                entry.path(),
                entry.file_name().to_string_lossy().into_owned(),
            ));
            continue;
        }
        let file_path = entry.path();
        let mut len = on_disk_len_at(&file_path, &md);
        // A big file reachable under two names (a hard link) is one file on disk. Checking costs
        // opening it, so only files large enough to skew the totals are checked.
        if len >= HARD_LINK_CHECK_BYTES {
            if let Some(id) = hard_link_identity(&file_path) {
                if !ctx.seen_links.lock().unwrap().insert(id) {
                    len = 0;
                }
            }
        }
        own_bytes += len;
        node.files += 1;
        if len >= LARGE_FILE_BYTES {
            ctx.large.lock().unwrap().push(LargeFile {
                name: entry.file_name().to_string_lossy().into_owned(),
                path: display(&file_path),
                size: len,
                modified: md.modified().map(unix_secs).unwrap_or(0),
            });
        }
    }
    ctx.files.fetch_add(node.files, Ordering::Relaxed);
    ctx.bytes.fetch_add(own_bytes, Ordering::Relaxed);
    node.own = own_bytes;
    node.children = subdirs
        .into_par_iter()
        .map(|(p, n)| scan_dir(&p, n, ctx))
        .collect();
    node.children.sort_unstable_by(|a, b| b.size.cmp(&a.size));
    node.size = own_bytes + node.children.iter().map(|c| c.size).sum::<u64>();
    node.files += node.children.iter().map(|c| c.files).sum::<u64>();
    node
}

pub struct ScanData {
    pub root: PathBuf,
    pub tree: Node,
    pub large: Vec<LargeFile>,
    pub summary: Summary,
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
    let tree = scan_dir(root, display(root), ctx);
    let mut large = std::mem::take(&mut *ctx.large.lock().unwrap());
    large.sort_unstable_by(|a, b| b.size.cmp(&a.size));
    large.truncate(LARGE_FILE_KEEP);

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
        denied_dirs: count_denied(&tree),
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
    }
}

fn count_denied(node: &Node) -> u64 {
    node.denied as u64 + node.children.iter().map(count_denied).sum::<u64>()
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
    totals.add(cat.unwrap_or("other"), node.own);
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

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NodeView {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub files: u64,
    pub has_children: bool,
    pub denied: bool,
    /// The pseudo entry standing for the files directly in the parent folder.
    pub is_files: bool,
}

/// Folds every folder under [`KEEP_FOLDER_BYTES`] into its parent's [`Node::small`]. Totals stay
/// exact; only the breakdown below 1 MB goes.
pub fn prune(node: &mut Node) {
    let mut small = 0;
    node.children.retain_mut(|c| {
        if c.size < KEEP_FOLDER_BYTES && !c.denied {
            small += c.size;
            false
        } else {
            prune(c);
            true
        }
    });
    node.small += small;
    node.children.shrink_to_fit();
}

/// Nodes in a tree, for measuring what a scan keeps in memory.
#[cfg(test)]
pub fn count_nodes(node: &Node) -> usize {
    1 + node.children.iter().map(count_nodes).sum::<usize>()
}

/// The children of `path` in the last scan, biggest first, plus one entry for the folder's own files.
pub fn children(data: &ScanData, path: &str) -> Option<Vec<NodeView>> {
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
    let mut out: Vec<NodeView> = node
        .children
        .iter()
        .filter(|c| c.size > 0 || c.denied)
        .map(|c| NodeView {
            name: c.name.clone(),
            path: display(&target.join(&c.name)),
            size: c.size,
            files: c.files,
            has_children: !c.children.is_empty() || c.own > 0 || c.small > 0,
            denied: c.denied,
            is_files: false,
        })
        .collect();
    if node.own + node.small > 0 {
        let direct = node.files - node.children.iter().map(|c| c.files).sum::<u64>();
        out.push(NodeView {
            name: if node.small > 0 { "零散文件和小文件夹" } else { "此文件夹中的文件" }.into(),
            path: display(&target),
            size: node.own + node.small,
            files: direct,
            has_children: false,
            denied: false,
            is_files: true,
        });
        out.sort_by(|a, b| b.size.cmp(&a.size));
    }
    Some(out)
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
        let mut data = run(&crate::util::system_drive(), &ctx);
        let before = count_nodes(&data.tree);
        prune(&mut data.tree);
        println!("tree nodes {before} -> {} after pruning", count_nodes(&data.tree));
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

    #[test]
    fn pruning_folds_small_folders_without_losing_bytes() {
        let leaf = |name: &str, size: u64| Node {
            name: name.into(),
            size,
            own: size,
            files: 1,
            denied: false,
            small: 0,
            children: vec![],
        };
        let mb = 1024 * 1024;
        let mut root = Node {
            name: "root".into(),
            size: 5 * mb + 3000,
            own: 0,
            files: 4,
            denied: false,
            small: 0,
            children: vec![leaf("big", 5 * mb), leaf("tiny", 1000), leaf("tiny2", 2000), leaf("empty", 0)],
        };
        root.files = 4;
        prune(&mut root);
        assert_eq!(root.children.len(), 1, "only the 5 MB folder stays");
        assert_eq!(root.small, 3000);
        let data = ScanData {
            root: PathBuf::from(r"C:\x"),
            tree: root,
            large: vec![],
            summary: Summary::default(),
        };
        let rows = children(&data, r"C:\x").unwrap();
        let total: u64 = rows.iter().map(|r| r.size).sum();
        assert_eq!(total, 5 * mb + 3000, "the listing still adds up");
        assert!(rows.iter().any(|r| r.is_files && r.name == "零散文件和小文件夹"));
    }

    #[test]
    fn scan_counts_sizes_and_skips_nothing_readable() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("a/b")).unwrap();
        std::fs::write(dir.path().join("a/one.bin"), vec![0u8; 1000]).unwrap();
        std::fs::write(dir.path().join("a/b/two.bin"), vec![0u8; 500]).unwrap();
        std::fs::write(dir.path().join("top.bin"), vec![0u8; 10]).unwrap();
        let ctx = ScanCtx::default();
        let tree = scan_dir(dir.path(), "root".into(), &ctx);
        assert_eq!(tree.size, 1510);
        assert_eq!(tree.files, 3);
        assert_eq!(tree.own, 10);
        assert_eq!(tree.children[0].name, "a");
        assert_eq!(tree.children[0].size, 1500);
    }
}
