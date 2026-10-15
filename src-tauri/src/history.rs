//! Past scans, kept so the next one can say what grew.
//!
//! Each scan leaves two files per drive: its summary (small, read to draw the trend) and every
//! folder of at least [`FOLDER_MIN_BYTES`] (read only to compare two scans). Only big folders are
//! kept because a whole tree is half a million entries, and a folder that never reaches the
//! threshold cannot have grown enough to matter.

use crate::scan::{Node, ScanData, Summary};
use crate::util::display;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const FOLDER_MIN_BYTES: u64 = 50 * 1024 * 1024;
const KEEP_PER_DRIVE: usize = 30;
/// A change smaller than this is noise, not a trend.
const REPORT_MIN_BYTES: i64 = 100 * 1024 * 1024;
const REPORT_MAX: usize = 30;

#[derive(Serialize, Deserialize)]
struct Folders(Vec<(String, u64)>);

fn drive_tag(root: &str) -> String {
    root.chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect::<String>()
        .to_uppercase()
}

fn summary_file(dir: &Path, root: &str, at: u64) -> PathBuf {
    dir.join(format!("{}_{at}.summary.json", drive_tag(root)))
}

fn folders_file(dir: &Path, root: &str, at: u64) -> PathBuf {
    dir.join(format!("{}_{at}.folders.json", drive_tag(root)))
}

fn collect(node: &Node, path: &Path, out: &mut Vec<(String, u64)>) {
    if node.size < FOLDER_MIN_BYTES {
        return;
    }
    out.push((display(path), node.size));
    for child in &node.children {
        collect(child, &path.join(&child.name), out);
    }
}

/// Saves a finished scan. A cancelled scan is incomplete and would read as space that vanished.
pub fn save(dir: &Path, data: &ScanData) -> Result<(), String> {
    if data.summary.cancelled {
        return Ok(());
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut folders = Vec::new();
    collect(&data.tree, &data.root, &mut folders);
    let at = data.summary.scanned_at;
    let root = &data.summary.root;
    std::fs::write(
        summary_file(dir, root, at),
        serde_json::to_vec(&data.summary).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::write(
        folders_file(dir, root, at),
        serde_json::to_vec(&Folders(folders)).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    prune(dir, root);
    Ok(())
}

fn prune(dir: &Path, root: &str) {
    let all = list(dir, root);
    if all.len() <= KEEP_PER_DRIVE {
        return;
    }
    for s in &all[..all.len() - KEEP_PER_DRIVE] {
        let _ = std::fs::remove_file(summary_file(dir, root, s.scanned_at));
        let _ = std::fs::remove_file(folders_file(dir, root, s.scanned_at));
    }
}

/// Every saved scan of `root`, oldest first.
pub fn list(dir: &Path, root: &str) -> Vec<Summary> {
    let prefix = format!("{}_", drive_tag(root));
    let mut out: Vec<Summary> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.starts_with(&prefix) && n.ends_with(".summary.json")
        })
        .filter_map(|e| serde_json::from_slice::<Summary>(&std::fs::read(e.path()).ok()?).ok())
        .collect();
    out.sort_by_key(|s| s.scanned_at);
    out
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FolderChange {
    pub path: String,
    /// None: below the recorded threshold at that time (or not there at all).
    pub before: Option<u64>,
    pub after: Option<u64>,
    pub delta: i64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CategoryChange {
    pub id: String,
    pub before: u64,
    pub after: u64,
    pub delta: i64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Diff {
    pub base_at: u64,
    pub current_at: u64,
    pub used_delta: i64,
    pub categories: Vec<CategoryChange>,
    pub grew: Vec<FolderChange>,
    pub shrank: Vec<FolderChange>,
}

fn read_folders(dir: &Path, root: &str, at: u64) -> Option<HashMap<String, (String, u64)>> {
    let Folders(list) =
        serde_json::from_slice(&std::fs::read(folders_file(dir, root, at)).ok()?).ok()?;
    Some(
        list.into_iter()
            .map(|(p, s)| (p.to_lowercase(), (p, s)))
            .collect(),
    )
}

/// What changed between two saved scans of `root`.
pub fn diff(dir: &Path, root: &str, base_at: u64, current_at: u64) -> Result<Diff, String> {
    let all = list(dir, root);
    let find = |at: u64| {
        all.iter()
            .find(|s| s.scanned_at == at)
            .cloned()
            .ok_or("找不到这次扫描的记录")
    };
    let (base, current) = (find(base_at)?, find(current_at)?);
    let before = read_folders(dir, root, base_at).ok_or("这次扫描的文件夹记录丢失了")?;
    let after = read_folders(dir, root, current_at).ok_or("这次扫描的文件夹记录丢失了")?;

    let mut changes: Vec<FolderChange> = before
        .keys()
        .chain(after.keys())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .map(|k| {
            let b = before.get(k);
            let a = after.get(k);
            let path = a.or(b).map(|(p, _)| p.clone()).unwrap_or_default();
            let (bs, as_) = (b.map(|x| x.1), a.map(|x| x.1));
            FolderChange {
                path,
                before: bs,
                after: as_,
                delta: as_.unwrap_or(0) as i64 - bs.unwrap_or(0) as i64,
            }
        })
        .filter(|c| c.delta.abs() >= REPORT_MIN_BYTES)
        .collect();

    let grew = most_specific(changes.iter().filter(|c| c.delta > 0).cloned().collect());
    changes.retain(|c| c.delta < 0);
    let shrank = most_specific(changes);

    let cat_before: HashMap<&str, u64> = base
        .categories
        .iter()
        .map(|c| (c.id.as_str(), c.bytes))
        .collect();
    let cat_after: HashMap<&str, u64> = current
        .categories
        .iter()
        .map(|c| (c.id.as_str(), c.bytes))
        .collect();
    let mut categories: Vec<CategoryChange> = cat_before
        .keys()
        .chain(cat_after.keys())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .map(|id| {
            let (b, a) = (
                *cat_before.get(id).unwrap_or(&0),
                *cat_after.get(id).unwrap_or(&0),
            );
            CategoryChange {
                id: id.to_string(),
                before: b,
                after: a,
                delta: a as i64 - b as i64,
            }
        })
        .filter(|c| c.delta != 0)
        .collect();
    categories.sort_by_key(|c| std::cmp::Reverse(c.delta.abs()));

    let used = |s: &Summary| (s.total - s.free) as i64;
    Ok(Diff {
        base_at,
        current_at,
        used_delta: used(&current) - used(&base),
        categories,
        grew,
        shrank,
    })
}

/// Keeps only the folders where a change actually happened. A folder whose change is mostly one
/// listed subfolder's says nothing new: when `xwechat_files` grew 2 GB, its parents did too.
fn most_specific(mut changes: Vec<FolderChange>) -> Vec<FolderChange> {
    // Deepest first, so every folder is looked at after all of its subfolders.
    changes.sort_by_key(|c| std::cmp::Reverse(c.path.len()));
    let mut kept: Vec<FolderChange> = Vec::new();
    for c in changes {
        let prefix = format!("{}\\", c.path.to_lowercase().trim_end_matches('\\'));
        let explained: i64 = kept
            .iter()
            .filter(|k| k.path.to_lowercase().starts_with(&prefix))
            .map(|k| k.delta)
            .sum();
        // Kept only if at least 30% of its change happened outside the subfolders already shown.
        if (c.delta - explained).abs() as f64 >= c.delta.abs() as f64 * 0.3 {
            kept.push(c);
        }
    }
    kept.sort_by_key(|c| std::cmp::Reverse(c.delta.abs()));
    kept.truncate(REPORT_MAX);
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(path: &str, delta_mb: i64) -> FolderChange {
        FolderChange {
            path: path.into(),
            before: Some(0),
            after: Some(0),
            delta: delta_mb * 1024 * 1024,
        }
    }

    #[test]
    fn parents_explained_by_one_child_are_dropped() {
        let kept = most_specific(vec![
            change(r"C:\Users", 2100),
            change(r"C:\Users\me", 2100),
            change(r"C:\Users\me\xwechat_files", 2000),
            change(r"C:\Users\me\.gradle", 0),
        ]);
        let paths: Vec<&str> = kept.iter().map(|c| c.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![r"C:\Users\me\xwechat_files", r"C:\Users\me\.gradle"]
        );
    }

    #[test]
    fn a_parent_that_grew_on_its_own_stays() {
        // 1.5 GB under `me` came from somewhere not listed: `me` is where to look.
        let kept = most_specific(vec![
            change(r"C:\Users\me", 2500),
            change(r"C:\Users\me\Downloads", 1000),
        ]);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].path, r"C:\Users\me");
    }

    #[test]
    fn a_saved_scan_round_trips_and_diffs() {
        use crate::scan::{CategoryStat, Node, ScanData, Summary};
        let dir = tempfile::tempdir().unwrap();
        let gb = 1024 * 1024 * 1024u64;
        let make = |at: u64, wechat: u64| ScanData {
            root: PathBuf::from(r"C:\"),
            tree: Node {
                name: r"C:\".into(),
                size: 10 * gb + wechat,
                own: 0,
                files: 1,
                denied: false,
                children: vec![Node {
                    name: "xwechat_files".into(),
                    size: wechat,
                    own: wechat,
                    files: 1,
                    denied: false,
                    children: vec![],
                }],
            },
            large: vec![],
            summary: Summary {
                root: r"C:\".into(),
                total: 100 * gb,
                free: 90 * gb - wechat,
                scanned: 10 * gb + wechat,
                files: 1,
                dirs: 1,
                denied_dirs: 0,
                elapsed_ms: 1,
                cancelled: false,
                scanned_at: at,
                categories: vec![CategoryStat {
                    id: "personal".into(),
                    bytes: wechat,
                }],
            },
        };
        save(dir.path(), &make(100, gb)).unwrap();
        save(dir.path(), &make(200, 3 * gb)).unwrap();
        assert_eq!(list(dir.path(), r"C:\").len(), 2);
        let d = diff(dir.path(), r"C:\", 100, 200).unwrap();
        assert_eq!(d.used_delta, 2 * gb as i64);
        assert_eq!(d.grew[0].path, r"C:\xwechat_files");
        assert_eq!(d.categories[0].delta, 2 * gb as i64);
    }
}
