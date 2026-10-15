//! Duplicate files under the user folder.
//!
//! Narrowed in three passes so almost nothing is read in full: files are grouped by size, then by a
//! hash of their first and last 64 KB, and only what still collides is hashed whole (BLAKE3).

use crate::util::{display, hard_link_identity, home, is_link, long_path, unix_secs};
use rayon::prelude::*;
use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

const EDGE: u64 = 64 * 1024;

#[derive(Default)]
pub struct DupeCtx {
    pub cancel: AtomicBool,
    pub files: AtomicU64,
    pub hashed_bytes: AtomicU64,
    pub to_hash_bytes: AtomicU64,
    pub phase: Mutex<&'static str>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DupeProgress {
    /// `walk` → `compare`
    pub phase: &'static str,
    pub files: u64,
    pub hashed_bytes: u64,
    pub to_hash_bytes: u64,
}

impl DupeCtx {
    pub fn progress(&self) -> DupeProgress {
        DupeProgress {
            phase: *self.phase.lock().unwrap(),
            files: self.files.load(Ordering::Relaxed),
            hashed_bytes: self.hashed_bytes.load(Ordering::Relaxed),
            to_hash_bytes: self.to_hash_bytes.load(Ordering::Relaxed),
        }
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DupeFile {
    pub path: String,
    pub modified: u64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DupeGroup {
    pub id: String,
    pub name: String,
    pub size: u64,
    /// Oldest first: the first is the likely original.
    pub files: Vec<DupeFile>,
    /// What deleting all but one would free.
    pub wasted: u64,
}

/// Folders a person's files do not live in. App data, tool caches and code repositories are full
/// of identical files that are identical on purpose — deleting one breaks whatever owns it.
fn skip_dir(name: &str) -> bool {
    let n = name.to_lowercase();
    n.starts_with('.')
        || matches!(
            n.as_str(),
            "appdata"
                | "node_modules"
                | "target"
                | "build"
                | "dist"
                | "out"
                | "__pycache__"
                | "venv"
                | "site-packages"
                | "$recycle.bin"
                | "system volume information"
                | "go"
        )
}

const FILE_ATTRIBUTE_OFFLINE: u32 = 0x1000;
const FILE_ATTRIBUTE_RECALL_ON_OPEN: u32 = 0x40000;
const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x400000;

fn walk(dir: &Path, min: u64, ctx: &DupeCtx, out: &Mutex<Vec<(PathBuf, u64, u64)>>) {
    if ctx.cancel.load(Ordering::Relaxed) {
        return;
    }
    let Ok(rd) = std::fs::read_dir(long_path(dir)) else {
        return;
    };
    let entries: Vec<_> = rd.flatten().collect();
    // A code repository's working tree: its files are the repository's, not loose copies.
    if entries.iter().any(|e| e.file_name() == ".git") {
        return;
    }
    let mut subdirs = Vec::new();
    let mut found = Vec::new();
    for e in entries {
        let Ok(md) = e.metadata() else { continue };
        if is_link(&md) {
            continue;
        }
        if md.is_dir() {
            if !skip_dir(&e.file_name().to_string_lossy()) {
                subdirs.push(e.path());
            }
            continue;
        }
        // An "online only" OneDrive file: reading it would download it.
        let cloud = FILE_ATTRIBUTE_OFFLINE
            | FILE_ATTRIBUTE_RECALL_ON_OPEN
            | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS;
        if md.file_attributes() & cloud != 0 || md.len() < min {
            continue;
        }
        found.push((
            e.path(),
            md.len(),
            md.modified().map(unix_secs).unwrap_or(0),
        ));
    }
    ctx.files.fetch_add(found.len() as u64, Ordering::Relaxed);
    out.lock().unwrap().extend(found);
    subdirs.par_iter().for_each(|d| walk(d, min, ctx, out));
}

fn hash_edges(path: &Path, size: u64) -> Option<[u8; 32]> {
    let mut f = std::fs::File::open(long_path(path)).ok()?;
    let mut h = blake3::Hasher::new();
    let mut buf = vec![0u8; EDGE as usize];
    let n = f.read(&mut buf).ok()?;
    h.update(&buf[..n]);
    if size > 2 * EDGE {
        f.seek(SeekFrom::End(-(EDGE as i64))).ok()?;
        let n = f.read(&mut buf).ok()?;
        h.update(&buf[..n]);
    }
    Some(*h.finalize().as_bytes())
}

fn hash_full(path: &Path, ctx: &DupeCtx) -> Option<[u8; 32]> {
    let mut f = std::fs::File::open(long_path(path)).ok()?;
    let mut h = blake3::Hasher::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        if ctx.cancel.load(Ordering::Relaxed) {
            return None;
        }
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        ctx.hashed_bytes.fetch_add(n as u64, Ordering::Relaxed);
    }
    Some(*h.finalize().as_bytes())
}

/// Groups `items` (path, size, mtime) by `key`, keeping only groups of two or more.
fn regroup<K: std::hash::Hash + Eq + Send>(
    items: Vec<(PathBuf, u64, u64)>,
    key: impl Fn(&(PathBuf, u64, u64)) -> Option<K> + Sync,
) -> Vec<Vec<(PathBuf, u64, u64)>> {
    let keyed: Vec<(K, (PathBuf, u64, u64))> = items
        .into_par_iter()
        .filter_map(|it| Some((key(&it)?, it)))
        .collect();
    let mut map: HashMap<K, Vec<(PathBuf, u64, u64)>> = HashMap::new();
    for (k, it) in keyed {
        map.entry(k).or_default().push(it);
    }
    map.into_values().filter(|g| g.len() > 1).collect()
}

/// Two paths that are hard links of one file are one copy on disk, not two.
fn drop_hard_links(group: Vec<(PathBuf, u64, u64)>) -> Vec<(PathBuf, u64, u64)> {
    let mut seen = std::collections::HashSet::new();
    group
        .into_iter()
        .filter(|(p, _, _)| match hard_link_identity(p) {
            Some(id) => seen.insert(id),
            None => true,
        })
        .collect()
}

pub fn find(min_size: u64, ctx: &DupeCtx) -> Vec<DupeGroup> {
    *ctx.phase.lock().unwrap() = "walk";
    let all = Mutex::new(Vec::new());
    walk(&home(), min_size, ctx, &all);
    let all = all.into_inner().unwrap();

    *ctx.phase.lock().unwrap() = "compare";
    let mut by_size: HashMap<u64, Vec<(PathBuf, u64, u64)>> = HashMap::new();
    for it in all {
        by_size.entry(it.1).or_default().push(it);
    }
    let candidates: Vec<Vec<_>> = by_size.into_values().filter(|g| g.len() > 1).collect();

    let mut groups = Vec::new();
    for same_size in candidates {
        if ctx.cancel.load(Ordering::Relaxed) {
            break;
        }
        for same_edges in regroup(same_size, |(p, s, _)| hash_edges(p, *s)) {
            let same_edges = drop_hard_links(same_edges);
            if same_edges.len() < 2 {
                continue;
            }
            // Files no bigger than both edges were already read whole.
            let size = same_edges[0].1;
            if size <= 2 * EDGE {
                groups.push(same_edges);
                continue;
            }
            ctx.to_hash_bytes
                .fetch_add(size * same_edges.len() as u64, Ordering::Relaxed);
            groups.extend(regroup(same_edges, |(p, _, _)| hash_full(p, ctx)));
        }
    }

    let mut out: Vec<DupeGroup> = groups
        .into_iter()
        .map(|mut g| {
            g.sort_by_key(|(_, _, m)| *m);
            let size = g[0].1;
            DupeGroup {
                id: display(&g[0].0),
                name: g[0]
                    .0
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                size,
                wasted: size * (g.len() as u64 - 1),
                files: g
                    .into_iter()
                    .map(|(p, _, m)| DupeFile {
                        path: display(&p),
                        modified: m,
                    })
                    .collect(),
            }
        })
        .collect();
    out.sort_by_key(|g| std::cmp::Reverse(g.wasted));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_size_different_content_is_not_a_duplicate() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.bin");
        let b = dir.path().join("b.bin");
        let c = dir.path().join("c.bin");
        let big = vec![7u8; 300 * 1024];
        let mut other = big.clone();
        other[150 * 1024] = 8; // differs only in the middle: edges match, the full hash does not
        std::fs::write(&a, &big).unwrap();
        std::fs::write(&b, &big).unwrap();
        std::fs::write(&c, &other).unwrap();
        let ctx = DupeCtx::default();
        let items: Vec<_> = [&a, &b, &c]
            .iter()
            .map(|p| ((*p).clone(), 300 * 1024u64, 0u64))
            .collect();
        let edges = regroup(items, |(p, s, _)| hash_edges(p, *s));
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].len(), 3);
        let full = regroup(edges.into_iter().next().unwrap(), |(p, _, _)| {
            hash_full(p, &ctx)
        });
        assert_eq!(full.len(), 1);
        assert_eq!(full[0].len(), 2);
    }

    #[test]
    fn hard_links_are_one_file() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.bin");
        let b = dir.path().join("b.bin");
        std::fs::write(&a, vec![1u8; 4096]).unwrap();
        std::fs::hard_link(&a, &b).unwrap();
        let kept = drop_hard_links(vec![(a, 4096, 0), (b, 4096, 0)]);
        assert_eq!(kept.len(), 1);
    }

    #[test]
    fn tool_and_app_folders_are_skipped() {
        for n in ["AppData", ".gradle", "node_modules", "target", "build"] {
            assert!(skip_dir(n), "{n}");
        }
        for n in ["Downloads", "Documents", "Pictures", "projects"] {
            assert!(!skip_dir(n), "{n}");
        }
    }
}

#[cfg(test)]
mod real {
    /// Read-only: `cargo test --release real_dupes -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_dupes() {
        let ctx = super::DupeCtx::default();
        let started = std::time::Instant::now();
        let groups = super::find(1024 * 1024, &ctx);
        let p = ctx.progress();
        let wasted: u64 = groups.iter().map(|g| g.wasted).sum();
        println!(
            "{} groups, {:.2} GB wasted, {} files walked, {:.2} GB hashed, {:?}",
            groups.len(),
            wasted as f64 / 1073741824.0,
            p.files,
            p.hashed_bytes as f64 / 1073741824.0,
            started.elapsed()
        );
        for g in groups.iter().take(12) {
            println!(
                "  {:>8.2} GB x{}  {}",
                g.size as f64 / 1073741824.0,
                g.files.len(),
                g.name
            );
            for f in g.files.iter().take(3) {
                println!("        {}", f.path);
            }
        }
    }
}
