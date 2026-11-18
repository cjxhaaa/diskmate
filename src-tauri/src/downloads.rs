//! The Downloads folder, sorted into what can probably go: installers for software that is already
//! installed, archives that have already been extracted, and big things nobody has touched in months.

use crate::util::{days_ago, display, home, measure, unix_secs};
use rayon::prelude::*;
use serde::Serialize;
use std::path::{Path, PathBuf};

const STALE_DAYS: u64 = 90;
const STALE_MIN_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DownloadItem {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub modified: u64,
    pub is_dir: bool,
    /// `installer` | `archive` | `image` | `video` | `document` | `folder` | `other`
    pub kind: &'static str,
    /// `installed` | `extracted` | `copy` | `stale` | `other`
    pub group: &'static str,
    /// Why it is in its group, in words: "已安装：Android Studio", "已解压到 Fooocus_win64".
    pub reason: Option<String>,
}

pub fn downloads_dir() -> PathBuf {
    home().join("Downloads")
}

fn ext_of(name: &str) -> String {
    let lower = name.to_lowercase();
    for double in [".tar.gz", ".tar.xz", ".tar.bz2"] {
        if lower.ends_with(double) {
            return double[1..].to_string();
        }
    }
    Path::new(&lower)
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn kind_of(name: &str, is_dir: bool) -> &'static str {
    if is_dir {
        return "folder";
    }
    match ext_of(name).as_str() {
        "exe" | "msi" | "msix" | "msixbundle" | "appx" | "appxbundle" | "apk" | "aab" | "dmg"
        | "pkg" => "installer",
        "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" | "xz" | "bz2" | "tar.gz" | "tar.xz"
        | "tar.bz2" => "archive",
        "iso" | "img" | "vhd" | "vhdx" | "vmdk" | "qcow2" | "wim" => "image",
        "mp4" | "mkv" | "mov" | "avi" | "wmv" | "flv" | "webm" | "m4v" => "video",
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "md" => "document",
        _ => "other",
    }
}

/// The part of a file name that names the product: `android-studio-2025.3.3-windows.exe` →
/// `androidstudio`, `WeChatSetup_4.1.exe` → `wechat`.
fn product_key(name: &str) -> String {
    const NOISE: &[&str] = &[
        "setup",
        "installer",
        "install",
        "windows",
        "win",
        "win64",
        "win32",
        "x64",
        "x86",
        "amd64",
        "arm64",
        "64bit",
        "32bit",
        "full",
        "offline",
        "online",
        "latest",
        "release",
        "stable",
        "portable",
        "user",
        "system",
        "exe",
        "msi",
    ];
    let stem = name.split('.').next().unwrap_or(name);
    // Split camel case too, so `WeChatSetup` loses its `Setup`.
    let mut spaced = String::new();
    for (i, c) in stem.chars().enumerate() {
        if i > 0 && c.is_uppercase() && stem.chars().nth(i - 1).is_some_and(|p| p.is_lowercase()) {
            spaced.push(' ');
        }
        spaced.push(c);
    }
    spaced
        .split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|w| {
            !w.is_empty() && !w.chars().any(|c| c.is_ascii_digit()) && !NOISE.contains(&w.as_str())
        })
        .collect()
}

fn compact(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

/// The installed app this installer is for, if any. `installed` are display names.
fn installed_match(name: &str, installed: &[String]) -> Option<String> {
    let key = product_key(name);
    if key.chars().count() < 3 {
        return None;
    }
    // Equal names match first. Otherwise one has to START with the other, and the shorter must be
    // long enough not to line up by chance. Containment anywhere was too loose: "Neo4j Desktop"
    // reduces to "desktop", which is inside "dockerdesktop".
    installed
        .iter()
        .filter_map(|app| {
            let core = compact(&crate::apps::core_name(app));
            if core.chars().count() < 3 {
                return None;
            }
            if core == key {
                return Some((0, app));
            }
            let (short, long) = if core.len() <= key.len() {
                (&core, &key)
            } else {
                (&key, &core)
            };
            (short.chars().count() >= 5 && long.starts_with(short.as_str())).then_some((1, app))
        })
        .min_by_key(|(rank, app)| (*rank, app.len()))
        .map(|(_, app)| app.clone())
}

/// `Fooocus_win64_2-5-0.7z` and a folder `Fooocus_win64_2-5-0` beside it: already extracted.
fn extracted_into(name: &str, folders: &[String]) -> Option<String> {
    let ext = ext_of(name);
    let stem = &name[..name.len().saturating_sub(ext.len() + 1)];
    let stem_l = stem.to_lowercase();
    folders.iter().find(|f| f.to_lowercase() == stem_l).cloned()
}

/// `report (2).pdf` → `report.pdf`: the name a browser downloaded the same file under before,
/// when it numbered this one to keep both.
fn numbered_from(name: &str) -> Option<String> {
    let ext = ext_of(name);
    let stem = if ext.is_empty() {
        name
    } else {
        name.get(..name.len().checked_sub(ext.len() + 1)?)?
    };
    let open = stem.strip_suffix(')')?.rfind('(')?;
    let number = &stem[open + 1..stem.len() - 1];
    if number.is_empty() || !number.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let base = stem[..open].trim_end_matches([' ', '_', '-']);
    if base.is_empty() {
        return None;
    }
    Some(if ext.is_empty() {
        base.to_string()
    } else {
        format!("{base}.{ext}")
    })
}

/// Whether two files of `len` bytes start and end alike: with the same name and size, enough to
/// tell a download saved twice from a different file that happens to share both.
fn same_ends(a: &Path, b: &Path, len: u64) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    const PART: u64 = 64 * 1024;
    let read = |p: &Path| -> Option<Vec<u8>> {
        let mut f = std::fs::File::open(crate::util::long_path(p)).ok()?;
        let mut out = Vec::new();
        (&mut f).take(PART).read_to_end(&mut out).ok()?;
        if len > PART {
            f.seek(SeekFrom::Start(len.saturating_sub(PART).max(PART)))
                .ok()?;
            f.take(PART).read_to_end(&mut out).ok()?;
        }
        Some(out)
    };
    matches!((read(a), read(b)), (Some(x), Some(y)) if x == y)
}

pub fn analyze(installed: &[String]) -> Vec<DownloadItem> {
    let dir = downloads_dir();
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let entries: Vec<(PathBuf, String, bool, u64)> = rd
        .flatten()
        .filter_map(|e| {
            let md = e.metadata().ok()?;
            if crate::util::is_link(&md) {
                return None;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            // Browser placeholders for downloads still running.
            if name.ends_with(".crdownload")
                || name.ends_with(".part")
                || name.eq_ignore_ascii_case("desktop.ini")
            {
                return None;
            }
            Some((
                e.path(),
                name,
                md.is_dir(),
                md.modified().map(unix_secs).unwrap_or(0),
            ))
        })
        .collect();
    let folders: Vec<String> = entries
        .iter()
        .filter(|e| e.2)
        .map(|e| e.1.clone())
        .collect();
    let stale_before = unix_secs(days_ago(STALE_DAYS));

    let measured: Vec<(&PathBuf, &String, bool, u64, u64)> = entries
        .par_iter()
        .map(|(path, name, is_dir, modified)| {
            let size = if *is_dir {
                measure(path, None).bytes
            } else {
                std::fs::metadata(crate::util::long_path(path))
                    .map(|m| crate::util::on_disk_len_at(path, &m))
                    .unwrap_or(0)
            };
            // A folder's own time is when an entry was last added or removed; what is inside may be
            // newer, which is what decides whether it is still in use.
            let modified = if *is_dir {
                crate::util::newest_mtime(path, &[])
                    .map(unix_secs)
                    .unwrap_or(*modified)
                    .max(*modified)
            } else {
                *modified
            };
            (path, name, *is_dir, size, modified)
        })
        .collect();
    // Files by name, to find the original a numbered copy was downloaded again from.
    let files: std::collections::HashMap<String, (&PathBuf, &String, u64)> = measured
        .iter()
        .filter(|e| !e.2)
        .map(|e| (e.1.to_lowercase(), (e.0, e.1, e.3)))
        .collect();

    let mut items: Vec<DownloadItem> = measured
        .par_iter()
        .map(|&(path, name, is_dir, size, modified)| {
            let kind = kind_of(name, is_dir);
            let installed_as = (kind == "installer")
                .then(|| installed_match(name, installed))
                .flatten();
            let extracted_to = (kind == "archive")
                .then(|| extracted_into(name, &folders))
                .flatten();
            let copy_of = (!is_dir)
                .then(|| numbered_from(name))
                .flatten()
                .and_then(|o| files.get(&o.to_lowercase()))
                .filter(|(original, _, len)| *len == size && same_ends(path, original, size))
                .map(|(_, original, _)| original.to_string());
            let (group, reason) = if let Some(app) = installed_as {
                ("installed", Some(format!("已安装：{app}")))
            } else if let Some(folder) = extracted_to {
                ("extracted", Some(format!("已解压到「{folder}」")))
            } else if let Some(original) = copy_of {
                (
                    "copy",
                    Some(format!("和「{original}」是同一个文件，重复下载了")),
                )
            } else {
                ("other", None)
            };
            let (group, reason) =
                if group == "other" && modified < stale_before && size >= STALE_MIN_BYTES {
                    (
                        "stale",
                        Some(format!(
                            "{} 天没动过",
                            (unix_secs(std::time::SystemTime::now()) - modified) / 86_400
                        )),
                    )
                } else {
                    (group, reason)
                };
            DownloadItem {
                path: display(path),
                name: name.clone(),
                size,
                modified,
                is_dir,
                kind,
                group,
                reason,
            }
        })
        .collect();
    items.sort_by(|a, b| b.size.cmp(&a.size));
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installer_names_reduce_to_the_product() {
        assert_eq!(
            product_key("android-studio-2025.3.3-windows.exe"),
            "androidstudio"
        );
        assert_eq!(product_key("WeChatSetup_4.1.2.exe"), "wechat");
        assert_eq!(product_key("VSCodeUserSetup-x64-1.105.1.exe"), "vscode");
        assert_eq!(product_key("Git-2.47.0-64-bit.exe"), "git");
    }

    #[test]
    fn installers_match_installed_apps() {
        let installed = vec![
            "Android Studio".to_string(),
            "WeChat".to_string(),
            "Git".to_string(),
        ];
        assert_eq!(
            installed_match("android-studio-2025.3.3-windows.exe", &installed),
            Some("Android Studio".into())
        );
        assert_eq!(
            installed_match("WeChatSetup.exe", &installed),
            Some("WeChat".into())
        );
        assert_eq!(
            installed_match("Git-2.47.0-64-bit.exe", &installed),
            Some("Git".into())
        );
        assert_eq!(
            installed_match("ClashVerge_1.7.7_x64-setup.exe", &installed),
            None
        );
        let more = vec![
            "Neo4j Desktop 2 2.0.3".to_string(),
            "Antigravity Tools".to_string(),
            "Antigravity (User)".to_string(),
        ];
        assert_eq!(
            installed_match("Docker Desktop Installer (2).exe", &more),
            None
        );
        assert_eq!(
            installed_match("Antigravity (1).exe", &more),
            Some("Antigravity (User)".into())
        );
    }

    #[test]
    fn archives_with_a_matching_folder_count_as_extracted() {
        let folders = vec!["Fooocus_win64_2-5-0".to_string()];
        assert_eq!(
            extracted_into("Fooocus_win64_2-5-0.7z", &folders),
            Some("Fooocus_win64_2-5-0".into())
        );
        assert_eq!(
            extracted_into("Fooocus_win64_2-5-0.tar.gz", &folders),
            Some("Fooocus_win64_2-5-0".into())
        );
        assert_eq!(extracted_into("other.zip", &folders), None);
    }

    #[test]
    fn numbered_copies_name_their_original() {
        assert_eq!(numbered_from("report (2).pdf"), Some("report.pdf".into()));
        assert_eq!(numbered_from("Setup(1).exe"), Some("Setup.exe".into()));
        assert_eq!(numbered_from("a_(3).tar.gz"), Some("a.tar.gz".into()));
        assert_eq!(numbered_from("notes (1)"), Some("notes".into()));
        assert_eq!(numbered_from("report.pdf"), None);
        assert_eq!(numbered_from("(1).pdf"), None);
        assert_eq!(numbered_from("photo (a).jpg"), None);
    }

    #[test]
    fn a_numbered_copy_needs_the_same_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let p = |n: &str| dir.path().join(n);
        let body: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(p("a.bin"), &body).unwrap();
        std::fs::write(p("b.bin"), &body).unwrap();
        let mut other = body.clone();
        *other.last_mut().unwrap() ^= 1;
        std::fs::write(p("c.bin"), &other).unwrap();
        let len = body.len() as u64;
        assert!(same_ends(&p("a.bin"), &p("b.bin"), len));
        assert!(!same_ends(&p("a.bin"), &p("c.bin"), len));
    }
}

#[cfg(test)]
mod real {
    /// Read-only: `cargo test --release real_downloads -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_downloads() {
        let installed: Vec<String> = crate::apps::list()
            .into_iter()
            .map(|e| e.info.name)
            .collect();
        let started = std::time::Instant::now();
        let items = super::analyze(&installed);
        println!("{} items in {:?}", items.len(), started.elapsed());
        for g in ["installed", "extracted", "copy", "stale", "other"] {
            let in_group: Vec<_> = items.iter().filter(|i| i.group == g).collect();
            let total: u64 = in_group.iter().map(|i| i.size).sum();
            println!(
                "== {g}: {} items, {:.2} GB",
                in_group.len(),
                total as f64 / 1073741824.0
            );
            for i in in_group.iter().take(8) {
                println!(
                    "   {:>8.2} GB  {}  {}",
                    i.size as f64 / 1073741824.0,
                    i.name,
                    i.reason.clone().unwrap_or_default()
                );
            }
        }
    }
}
