//! Deleting what the user picked.
//!
//! Every path is checked against [`allowed`] first, however it got here: a bug elsewhere must not
//! be able to turn into deleting a home folder or a drive.

use crate::rules::{worktree_is_clean, Target};
use crate::util::{
    self, display, home, is_link, local_appdata, long_path, on_disk_len, program_data,
    roaming_appdata, system_root,
};
use rayon::prelude::*;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Serialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub freed: u64,
    pub removed_files: u64,
    /// Files that could not be deleted: in use, or no permission. Skipped, not fatal.
    pub skipped_files: u64,
    pub error: Option<String>,
}

impl Outcome {
    fn merge(&mut self, o: Outcome) {
        self.freed += o.freed;
        self.removed_files += o.removed_files;
        self.skipped_files += o.skipped_files;
        if self.error.is_none() {
            self.error = o.error;
        }
    }
}

fn lower(p: &Path) -> String {
    display(p).to_lowercase().trim_end_matches('\\').to_string()
}

/// Whether `path` may be deleted from at all: strictly inside one of the roots the rules use, and
/// never one of those roots itself.
pub fn allowed(path: &Path) -> bool {
    if !path.is_absolute() {
        return false;
    }
    let p = lower(path);
    let home = lower(&home());
    let windows = lower(&system_root());
    let roots = [
        home.clone(),
        format!(r"{windows}\temp"),
        format!(r"{windows}\softwaredistribution\download"),
        format!(
            r"{windows}\serviceprofiles\networkservice\appdata\local\microsoft\windows\deliveryoptimization\cache"
        ),
        format!(r"{}\microsoft\windows\wer", lower(&program_data())),
    ];
    let forbidden = [
        home.clone(),
        lower(&local_appdata()),
        lower(&roaming_appdata()),
        format!(r"{home}\appdata"),
        format!(r"{home}\documents"),
        format!(r"{home}\desktop"),
        format!(r"{home}\downloads"),
        format!(r"{home}\pictures"),
    ];
    if forbidden.contains(&p) {
        return false;
    }
    roots
        .iter()
        .any(|r| p.starts_with(&format!(r"{r}\")) || (p == *r && r != &home))
}

/// Deletes everything under `dir`. Files newer than `cutoff` and files that are in use stay, and
/// so does every folder that still holds one. `remove_self` also removes `dir` once it is empty.
fn remove_tree(dir: &Path, cutoff: Option<SystemTime>, remove_self: bool) -> Outcome {
    let mut out = Outcome::default();
    let Ok(rd) = std::fs::read_dir(long_path(dir)) else {
        return out;
    };
    let mut subdirs = Vec::new();
    for entry in rd.flatten() {
        let path = entry.path();
        let Ok(md) = entry.metadata() else { continue };
        if is_link(&md) {
            // Remove the link itself, never what it points to.
            if std::fs::remove_dir(&path).is_err() && std::fs::remove_file(&path).is_err() {
                out.skipped_files += 1;
            }
            continue;
        }
        if md.is_dir() {
            subdirs.push(path);
            continue;
        }
        if let Some(cutoff) = cutoff {
            if md.modified().map(|m| m > cutoff).unwrap_or(true) {
                continue;
            }
        }
        let len = on_disk_len(&md);
        let mut removed = std::fs::remove_file(&path).is_ok();
        if !removed && md.permissions().readonly() {
            let mut perms = md.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(false);
            removed = std::fs::set_permissions(&path, perms).is_ok()
                && std::fs::remove_file(&path).is_ok();
        }
        if removed {
            out.freed += len;
            out.removed_files += 1;
        } else {
            out.skipped_files += 1;
        }
    }
    for o in subdirs
        .par_iter()
        .map(|d| remove_tree(d, cutoff, true))
        .collect::<Vec<_>>()
    {
        out.merge(o);
    }
    if remove_self {
        let old_enough = cutoff.is_none()
            || std::fs::metadata(long_path(dir))
                .and_then(|m| m.modified())
                .map(|m| m <= cutoff.unwrap())
                .unwrap_or(false);
        if old_enough {
            // Fails, harmlessly, when something inside had to stay.
            let _ = std::fs::remove_dir(long_path(dir));
        }
    }
    out
}

fn refuse(path: &Path) -> Outcome {
    Outcome {
        error: Some(format!("出于安全考虑，不允许删除 {}", display(path))),
        ..Default::default()
    }
}

fn remove_worktree(repo: &Path, path: &Path) -> Outcome {
    if !worktree_is_clean(path) {
        return Outcome {
            error: Some(format!("{} 有未提交的改动，已跳过", display(path))),
            ..Default::default()
        };
    }
    let before = util::measure(path, None);
    // core.longpaths: without it git gives up on deep build output with "Filename too long", after
    // it has already unregistered the worktree — leaving a half-deleted folder nobody owns.
    let status = util::command("git")
        .args([
            "-C",
            &repo.to_string_lossy(),
            "-c",
            "core.longpaths=true",
            "worktree",
            "remove",
            "--force",
        ])
        .arg(path)
        .status();
    let mut out = Outcome::default();
    if path.exists() {
        // Whatever git left behind is build output of a worktree that was clean a moment ago.
        out.merge(remove_tree(path, None, true));
    }
    let _ = util::command("git")
        .args(["-C", &repo.to_string_lossy(), "worktree", "prune"])
        .status();
    let after = util::measure(path, None);
    out.freed = before.bytes.saturating_sub(after.bytes);
    out.removed_files = before.files.saturating_sub(after.files);
    if status.map(|s| !s.success()).unwrap_or(true) && path.exists() {
        out.error = Some(format!("git 没能删除 {}", display(path)));
    }
    out
}

pub fn execute(target: &Target) -> Outcome {
    match target {
        Target::Contents { path, min_age_days } => {
            if !allowed(path) {
                return refuse(path);
            }
            let cutoff = (*min_age_days > 0).then(|| util::days_ago(*min_age_days));
            remove_tree(path, cutoff, false)
        }
        Target::Dir { path } => {
            if !allowed(path) {
                return refuse(path);
            }
            remove_tree(path, None, true)
        }
        Target::File { path } => {
            if !allowed(path) {
                return refuse(path);
            }
            let len = util::file_len(path).unwrap_or(0);
            match std::fs::remove_file(long_path(path)) {
                Ok(()) => Outcome {
                    freed: len,
                    removed_files: 1,
                    ..Default::default()
                },
                Err(e) => Outcome {
                    skipped_files: 1,
                    error: Some(e.to_string()),
                    ..Default::default()
                },
            }
        }
        Target::Worktree { repo, path } => {
            if !allowed(path) {
                return refuse(path);
            }
            remove_worktree(repo, path)
        }
        Target::RecycleBin => {
            let (bytes, items) = crate::sys::recycle_bin_size();
            match crate::sys::empty_recycle_bin() {
                Ok(()) => Outcome {
                    freed: bytes,
                    removed_files: items,
                    ..Default::default()
                },
                Err(e) => Outcome {
                    error: Some(e),
                    ..Default::default()
                },
            }
        }
        Target::Hibernation => {
            let path = crate::sys::hiberfil_path();
            let before = util::file_len(&path).unwrap_or(0);
            match crate::sys::disable_hibernation() {
                Ok(()) => {
                    let after = util::file_len(&path).unwrap_or(0);
                    Outcome {
                        freed: before.saturating_sub(after),
                        removed_files: 1,
                        ..Default::default()
                    }
                }
                Err(e) => Outcome {
                    error: Some(e),
                    ..Default::default()
                },
            }
        }
    }
}

/// Moves files to the Recycle Bin (the large-files page). Each must be inside the user folder.
pub fn recycle(paths: &[PathBuf]) -> Outcome {
    let mut out = Outcome::default();
    for p in paths {
        // Same rule as the large-files page: app data (AppData) and tool data (.android, .gradle, ...)
        // are left to the app that owns them — one file pulled out of either breaks that app.
        let inside_app_data = p.components().any(|c| {
            let name = c.as_os_str().to_string_lossy();
            name.starts_with('.') || name.eq_ignore_ascii_case("appdata")
        });
        if !allowed(p) || !p.is_file() || inside_app_data {
            out.skipped_files += 1;
            continue;
        }
        let len = util::file_len(p).unwrap_or(0);
        match trash::delete(p) {
            Ok(()) => {
                out.freed += len;
                out.removed_files += 1;
            }
            Err(e) => {
                out.skipped_files += 1;
                out.error.get_or_insert(e.to_string());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_the_roots_themselves() {
        let home = home();
        assert!(!allowed(&home));
        assert!(!allowed(&local_appdata()));
        assert!(!allowed(&home.join("Documents")));
        assert!(!allowed(Path::new(r"C:\")));
        assert!(!allowed(Path::new(r"C:\Windows")));
        assert!(!allowed(Path::new(r"C:\Windows\System32")));
        assert!(!allowed(Path::new("relative\\path")));
        assert!(allowed(&local_appdata().join("Temp")));
        assert!(allowed(&home.join(r".gradle\daemon")));
        assert!(allowed(Path::new(r"C:\Windows\Temp\x")));
    }

    #[test]
    fn remove_tree_respects_the_cutoff_and_keeps_newer_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("sub/deeper")).unwrap();
        std::fs::write(dir.path().join("a.tmp"), vec![1u8; 100]).unwrap();
        std::fs::write(dir.path().join("sub/deeper/b.tmp"), vec![1u8; 50]).unwrap();
        // Everything was just written, so a cutoff in the past keeps all of it.
        let kept = remove_tree(dir.path(), Some(util::days_ago(1)), false);
        assert_eq!(kept.freed, 0);
        assert!(dir.path().join("sub/deeper/b.tmp").exists());
        // No cutoff: everything inside goes, the folder itself stays.
        let gone = remove_tree(dir.path(), None, false);
        assert_eq!(gone.freed, 150);
        assert_eq!(gone.removed_files, 2);
        assert!(dir.path().exists());
        assert!(!dir.path().join("sub").exists());
    }

    #[test]
    fn remove_tree_handles_paths_past_260_characters() {
        let dir = tempfile::tempdir().unwrap();
        let mut deep = dir.path().to_path_buf();
        for i in 0..30 {
            deep.push(format!("nested-folder-{i:02}"));
        }
        std::fs::create_dir_all(long_path(&deep)).unwrap();
        std::fs::write(long_path(&deep.join("f.bin")), vec![0u8; 10]).unwrap();
        assert!(display(&deep).len() > 260);
        let out = remove_tree(&dir.path().join("nested-folder-00"), None, true);
        assert_eq!(out.removed_files, 1);
        assert!(!dir.path().join("nested-folder-00").exists());
    }
}
