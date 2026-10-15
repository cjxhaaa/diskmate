//! Moving big folders to another drive without anything noticing.
//!
//! A folder is copied whole, checked file for file, and only then swapped: the original path becomes
//! a directory junction to the copy, so every program that knows the old path keeps working without
//! a setting changed. The user's own folders (Downloads, Documents, ...) are relocated the way
//! Windows' own "Location" tab does it instead.
//!
//! Every step before the swap can be undone by deleting the copy, and is: the original is touched
//! only once the copy is known to be complete.

use crate::util::{
    self, display, home, is_link, local_appdata, long_path, measure, roaming_appdata, Size,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum Method {
    /// Copy, then leave a junction at the old path.
    Junction,
    /// One of the user's shell folders: move, then point Windows at the new place.
    KnownFolder,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub id: String,
    pub title: String,
    pub desc: String,
    pub path: String,
    pub size: u64,
    pub method: Method,
    /// `app` | `dev` | `system` | `project`: how the list is grouped.
    pub group: &'static str,
    /// Running programs that have to be closed first.
    pub blockers: Vec<String>,
    /// Already moved: where to.
    pub moved_to: Option<String>,
    /// False when `size` was not worked out yet (the quick status check skips measuring).
    pub measured: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub id: String,
    pub source: String,
    pub target: String,
    pub method: Method,
    pub size: u64,
    pub moved_at: u64,
}

struct Known {
    id: &'static str,
    title: &'static str,
    desc: &'static str,
    path: PathBuf,
    method: Method,
    group: &'static str,
    /// Executable names (lowercase) that hold this folder open.
    blockers: &'static [&'static str],
}

const ANDROID: &[&str] = &[
    "studio64.exe",
    "adb.exe",
    "emulator.exe",
    "qemu-system-x86_64.exe",
];
const JETBRAINS: &[&str] = &[
    "clion64.exe",
    "goland64.exe",
    "pycharm64.exe",
    "idea64.exe",
    "webstorm64.exe",
    "rider64.exe",
    "studio64.exe",
];
const WECHAT: &[&str] = &["weixin.exe", "wechat.exe", "wechatappex.exe"];

fn known() -> Vec<Known> {
    let home = home();
    let local = local_appdata();
    let roaming = roaming_appdata();
    let k = |id, title, desc, path: PathBuf, method, blockers| Known {
        id,
        title,
        desc,
        path,
        method,
        group: match id {
            "wechat" | "qqmusic" => "app",
            _ if method == Method::KnownFolder => "system",
            _ => "dev",
        },
        blockers,
    };
    vec![
        k("downloads", "下载", "Windows 会把「下载」文件夹指向新位置，浏览器下载的文件以后也直接存到那里。", home.join("Downloads"), Method::KnownFolder, &[]),
        k("documents", "文档", "「文档」文件夹整体搬走。微信默认把聊天文件存在「文档」里，会跟着一起搬。", home.join("Documents"), Method::KnownFolder, WECHAT),
        k("videos", "视频", "「视频」文件夹整体搬走，录屏默认保存在这里。", home.join("Videos"), Method::KnownFolder, &[]),
        k("pictures", "图片", "「图片」文件夹整体搬走，截图默认保存在这里。", home.join("Pictures"), Method::KnownFolder, &[]),
        k("music", "音乐", "「音乐」文件夹整体搬走。", home.join("Music"), Method::KnownFolder, &[]),
        k("wechat", "微信聊天文件", "微信 4.x 的聊天图片、视频和文件。搬走后微信照常读写，但搬之前必须退出微信。", home.join("xwechat_files"), Method::Junction, WECHAT),
        k("qqmusic", "QQ 音乐", "下载的歌曲和缓存。搬之前要退出 QQ 音乐。", roaming.join(r"Tencent\QQMusic"), Method::Junction, &["qqmusic.exe"]),
        k("android-sdk", "Android SDK", "SDK、模拟器系统镜像和构建工具。项目里写的 SDK 路径不用改。", local.join(r"Android\Sdk"), Method::Junction, ANDROID),
        k("avd", "Android 模拟器", "模拟器的虚拟磁盘和快照。", home.join(r".android\avd"), Method::Junction, ANDROID),
        k("gradle", "Gradle 缓存", "所有 Android / Java 项目下载过的依赖。搬之前要关掉 Android Studio，并停止 Gradle（在项目里运行 gradlew --stop）。", home.join(".gradle"), Method::Junction, &["studio64.exe", "java.exe"]),
        k("jetbrains", "JetBrains 缓存", "CLion、GoLand 等 IDE 的索引和缓存。", local.join("JetBrains"), Method::Junction, JETBRAINS),
    ]
}

fn records_file(dir: &Path) -> PathBuf {
    dir.join("migrations.json")
}

pub fn records(dir: &Path) -> Vec<Record> {
    std::fs::read(records_file(dir))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_records(dir: &Path, list: &[Record]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    std::fs::write(
        records_file(dir),
        serde_json::to_vec_pretty(list).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

/// Lowercase names of every running process.
pub fn running_processes() -> Vec<String> {
    util::processes()
        .into_iter()
        .map(|(_, name)| name)
        .collect()
}

fn blockers_for(names: &[&str], running: &[String]) -> Vec<String> {
    let label = |exe: &str| -> String {
        match exe {
            "weixin.exe" | "wechat.exe" | "wechatappex.exe" => "微信",
            "qqmusic.exe" => "QQ 音乐",
            "studio64.exe" => "Android Studio",
            "adb.exe" => "adb",
            "emulator.exe" | "qemu-system-x86_64.exe" => "Android 模拟器",
            "java.exe" => "Java / Gradle 后台进程",
            "clion64.exe" => "CLion",
            "goland64.exe" => "GoLand",
            "pycharm64.exe" => "PyCharm",
            "idea64.exe" => "IntelliJ IDEA",
            "webstorm64.exe" => "WebStorm",
            "rider64.exe" => "Rider",
            other => other,
        }
        .to_string()
    };
    let mut out: Vec<String> = names
        .iter()
        .filter(|n| running.iter().any(|r| r == *n))
        .map(|n| label(n))
        .collect();
    out.dedup();
    out
}

/// Big folders directly under the user folder that no known item covers: usually projects.
fn project_folders(exclude: &[PathBuf]) -> Vec<PathBuf> {
    let home = home();
    std::fs::read_dir(&home)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let md = e.metadata().ok()?;
            let name = e.file_name().to_string_lossy().to_lowercase();
            let p = e.path();
            let skip = !md.is_dir()
                || is_link(&md)
                || name.starts_with('.')
                || matches!(
                    name.as_str(),
                    "appdata"
                        | "onedrive"
                        | "desktop"
                        | "favorites"
                        | "links"
                        | "saved games"
                        | "searches"
                        | "contacts"
                )
                || exclude.iter().any(|x| x.eq(&p));
            (!skip).then_some(p)
        })
        .collect()
}

const SIZES_CACHE: &str = "move-sizes.json";

/// Everything worth moving, with what is already moved. With `measure` false nothing is measured:
/// sizes are the last measured ones (or 0 for a folder never measured) and every top-level folder
/// is listed, big or not — it answers in a blink, for showing the page and re-checking what runs.
pub fn candidates(dir: &Path, measure_sizes: bool) -> Vec<Candidate> {
    use rayon::prelude::*;
    let moved = records(dir);
    let running = running_processes();
    let known = known();
    let known_paths: Vec<PathBuf> = known.iter().map(|k| k.path.clone()).collect();
    let last: std::collections::HashMap<String, u64> =
        crate::cache::load(SIZES_CACHE).unwrap_or_default();
    let size_of = |p: &Path, rec: Option<&Record>| match rec {
        Some(r) => (r.size, true),
        None if measure_sizes => (measure(p, None).bytes, true),
        None => last.get(&display(p)).map_or((0, false), |&s| (s, true)),
    };
    let known: Vec<&Known> = known.iter().filter(|k| k.path.exists()).collect();
    let make_known = |k: &&Known| {
        let rec = moved.iter().find(|r| r.id == k.id);
        let (size, measured) = size_of(&k.path, rec);
        Candidate {
            id: k.id.into(),
            title: k.title.into(),
            desc: k.desc.into(),
            path: display(&k.path),
            size,
            method: k.method,
            group: k.group,
            blockers: blockers_for(k.blockers, &running),
            moved_to: rec.map(|r| r.target.clone()),
            measured,
        }
    };
    let folders = project_folders(&known_paths);
    let make_project = |p: &PathBuf| {
        let id = format!("folder:{}", display(p).to_lowercase());
        let rec = moved.iter().find(|r| r.id == id);
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let (size, measured) = size_of(p, rec);
        Candidate {
                id,
                title: name,
                desc: "用户目录下的文件夹，通常是代码项目。搬走后原路径照常可用；IDE、终端里打开着的话要先关掉。".into(),
                path: display(p),
                size,
                method: Method::Junction,
                group: "project",
                blockers: Vec::new(),
                moved_to: rec.map(|r| r.target.clone()),
                measured,
            }
    };
    // Measuring runs on the worker pool; the quick status stays on this thread, so it answers at
    // once even while a scan or measuring keeps the pool busy.
    let (mut out, projects): (Vec<Candidate>, Vec<Candidate>) = if measure_sizes {
        (
            known.par_iter().map(make_known).collect(),
            folders.par_iter().map(make_project).collect(),
        )
    } else {
        (
            known.iter().map(make_known).collect(),
            folders.iter().map(make_project).collect(),
        )
    };
    let projects: Vec<Candidate> = projects
        .into_iter()
        .filter(|c| !c.measured || c.size >= 1024 * 1024 * 1024 || c.moved_to.is_some())
        .collect();
    out.extend(projects);
    if measure_sizes {
        let sizes: std::collections::HashMap<String, u64> = out
            .iter()
            .filter(|c| c.moved_to.is_none())
            .map(|c| (c.path.clone(), c.size))
            .collect();
        crate::cache::save(SIZES_CACHE, &sizes);
    }
    out.sort_by(|a, b| {
        a.moved_to
            .is_some()
            .cmp(&b.moved_to.is_some())
            .then(b.size.cmp(&a.size))
    });
    out
}

/// The folder behind a candidate id. Ids come from the UI, so the path is always worked out here
/// again — never taken from the caller.
pub fn source_for(id: &str) -> Option<PathBuf> {
    if let Some(k) = known().into_iter().find(|k| k.id == id) {
        return Some(k.path);
    }
    let lower = id.strip_prefix("folder:")?;
    let known_paths: Vec<PathBuf> = known().into_iter().map(|k| k.path).collect();
    project_folders(&known_paths)
        .into_iter()
        .find(|p| display(p).to_lowercase() == lower)
}

/// `C:\Users\me\.gradle` on `D:\` → `D:\Users\me\.gradle`: the same path on the other drive, so
/// what is where stays obvious.
pub fn target_for(source: &Path, drive_root: &str) -> PathBuf {
    let s = display(source);
    let rest = s.get(3..).unwrap_or("");
    PathBuf::from(drive_root).join(rest)
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MoveProgress {
    pub id: String,
    /// `copy` → `verify` → `switch` → `cleanup`
    pub phase: &'static str,
    pub done: u64,
    pub total: u64,
}

/// Copies `from` into `to` with robocopy (multithreaded, long paths, timestamps kept) while
/// `report` hears how much has arrived.
fn copy_tree(
    from: &Path,
    to: &Path,
    total: u64,
    report: &(dyn Fn(u64) + Sync),
) -> Result<(), String> {
    let mut child = util::command("robocopy")
        .arg(from)
        .arg(to)
        .args([
            "/E",
            "/COPY:DAT",
            "/DCOPY:DAT",
            "/XJ",
            "/R:1",
            "/W:1",
            "/MT:16",
            "/NFL",
            "/NDL",
            "/NJH",
            "/NJS",
            "/NP",
        ])
        .spawn()
        .map_err(|e| format!("无法启动 robocopy：{e}"))?;
    let done = AtomicBool::new(false);
    let status = std::thread::scope(|s| {
        s.spawn(|| {
            while !done.load(Ordering::Relaxed) {
                report(measure(to, None).bytes.min(total));
                std::thread::sleep(Duration::from_millis(1500));
            }
        });
        let status = child.wait();
        done.store(true, Ordering::Relaxed);
        status
    })
    .map_err(|e| e.to_string())?;
    // robocopy: 0-7 are kinds of success, 8 and above mean something failed to copy.
    match status.code() {
        Some(c) if c < 8 => Ok(()),
        Some(c) => Err(format!(
            "复制没有完成（robocopy 返回 {c}），可能有文件被占用或磁盘空间不够"
        )),
        None => Err("复制被中断".into()),
    }
}

fn same_contents(a: &Path, b: &Path) -> Result<(), String> {
    let (x, y): (Size, Size) = (measure(a, None), measure(b, None));
    if x.files == y.files && x.bytes == y.bytes {
        Ok(())
    } else {
        Err(format!(
            "复制后核对不一致（原来 {} 个文件，复制了 {} 个）。可能有程序在复制期间改动了文件，请关闭相关程序后重试",
            x.files, y.files
        ))
    }
}

fn make_junction(link: &Path, target: &Path) -> Result<(), String> {
    let out = util::command("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "创建链接失败：{}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

fn known_folder_id(id: &str) -> Option<windows::core::GUID> {
    use windows::Win32::UI::Shell::{
        FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Music, FOLDERID_Pictures, FOLDERID_Videos,
    };
    Some(match id {
        "downloads" => FOLDERID_Downloads,
        "documents" => FOLDERID_Documents,
        "videos" => FOLDERID_Videos,
        "pictures" => FOLDERID_Pictures,
        "music" => FOLDERID_Music,
        _ => return None,
    })
}

fn set_known_folder(id: &str, path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::SHSetKnownFolderPath;
    let guid = known_folder_id(id).ok_or("不是系统文件夹")?;
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe { SHSetKnownFolderPath(&guid, 0, None, PCWSTR(wide.as_ptr())) }.map_err(|e| e.message())
}

/// Moves the candidate `id` at `source` to the same path on `drive_root`.
pub fn migrate(
    dir: &Path,
    id: &str,
    source: &Path,
    drive_root: &str,
    report: &(dyn Fn(&'static str, u64, u64) + Sync),
) -> Result<Record, String> {
    let home = home();
    if !source.starts_with(&home) || source == home {
        return Err("只能搬用户文件夹里的内容".into());
    }
    let md = std::fs::symlink_metadata(source).map_err(|_| "找不到这个文件夹".to_string())?;
    if is_link(&md) {
        return Err("这个文件夹已经是一个链接，可能之前搬过".into());
    }
    let method = if known_folder_id(id).is_some() {
        Method::KnownFolder
    } else {
        Method::Junction
    };
    let target = target_for(source, drive_root);
    if target.exists()
        && std::fs::read_dir(&target)
            .map(|mut r| r.next().is_some())
            .unwrap_or(true)
    {
        return Err(format!(
            "目标位置 {} 已经有内容了，请先处理掉",
            display(&target)
        ));
    }
    let size = measure(source, None);
    let (_, free) = crate::sys::drive_space(drive_root).ok_or("读不到目标磁盘的空间")?;
    if free < size.bytes + size.bytes / 20 + 1024 * 1024 * 1024 {
        return Err(format!(
            "{drive_root} 空间不够：需要 {}，只剩 {}",
            gb(size.bytes),
            gb(free)
        ));
    }

    // 0. Probe: a folder something holds open cannot be renamed, so try now — before spending minutes
    // copying a folder whose swap is bound to fail (a project open in an IDE or a terminal).
    if method == Method::Junction {
        let probe = source.with_file_name(format!(
            "{}.diskmate-probe",
            source.file_name().unwrap().to_string_lossy()
        ));
        std::fs::rename(long_path(source), long_path(&probe)).map_err(|_| {
            "有程序正在使用这个文件夹，请先关闭用到它的程序（编辑器、终端等）".to_string()
        })?;
        std::fs::rename(long_path(&probe), long_path(source))
            .map_err(|e| format!("试探后没能改回原名，请检查 {}：{e}", display(&probe)))?;
    }

    // 1. Copy. Nothing at the source has changed yet; any failure just removes the copy.
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    let undo_copy = |e: String| {
        let _ = crate::clean::remove_tree_pub(&target);
        Err(e)
    };
    if let Err(e) = copy_tree(source, &target, size.bytes, &|done| {
        report("copy", done, size.bytes)
    }) {
        return undo_copy(e);
    }
    report("verify", size.bytes, size.bytes);
    if let Err(e) = same_contents(source, &target) {
        return undo_copy(e);
    }

    // 2. Swap.
    report("switch", size.bytes, size.bytes);
    match method {
        Method::Junction => {
            // Renaming fails while anything inside is open — exactly the check that is needed.
            let aside = source.with_file_name(format!(
                "{}.diskmate-moving",
                source.file_name().unwrap().to_string_lossy()
            ));
            if let Err(e) = std::fs::rename(long_path(source), long_path(&aside)) {
                return undo_copy(format!("有程序正在使用这个文件夹，请关闭后再试（{e}）"));
            }
            if let Err(e) = make_junction(source, &target) {
                let _ = std::fs::rename(long_path(&aside), long_path(source));
                return undo_copy(e);
            }
            report("cleanup", size.bytes, size.bytes);
            let _ = crate::clean::remove_tree_pub(&aside);
        }
        Method::KnownFolder => {
            if let Err(e) = set_known_folder(id, &target) {
                return undo_copy(format!("没能修改文件夹位置：{e}"));
            }
            report("cleanup", size.bytes, size.bytes);
            // Old copy goes; the empty folder stays harmlessly if something holds it.
            let _ = crate::clean::remove_tree_pub(source);
        }
    }

    let rec = Record {
        id: id.into(),
        source: display(source),
        target: display(&target),
        method,
        size: size.bytes,
        moved_at: util::unix_secs(std::time::SystemTime::now()),
    };
    let mut all = records(dir);
    all.retain(|r| r.id != id);
    all.push(rec.clone());
    save_records(dir, &all)?;
    Ok(rec)
}

/// Brings a moved folder back to where it was.
pub fn restore(
    dir: &Path,
    id: &str,
    report: &(dyn Fn(&'static str, u64, u64) + Sync),
) -> Result<(), String> {
    let mut all = records(dir);
    let rec = all
        .iter()
        .find(|r| r.id == id)
        .cloned()
        .ok_or("没有这次搬家的记录")?;
    let (source, target) = (PathBuf::from(&rec.source), PathBuf::from(&rec.target));
    let size = measure(&target, None);
    let (_, free) = crate::sys::drive_space(&display(&util::system_drive())).unwrap_or((0, 0));
    if free < size.bytes + 1024 * 1024 * 1024 {
        return Err(format!(
            "C 盘空间不够：需要 {}，只剩 {}",
            gb(size.bytes),
            gb(free)
        ));
    }
    match rec.method {
        Method::Junction => {
            let md = std::fs::symlink_metadata(&source).map_err(|e| e.to_string())?;
            if !is_link(&md) {
                return Err("原位置已经不是链接了，不能自动搬回".into());
            }
            std::fs::remove_dir(&source).map_err(|e| format!("删不掉原位置的链接：{e}"))?;
            if let Err(e) = copy_tree(&target, &source, size.bytes, &|d| {
                report("copy", d, size.bytes)
            })
            .and_then(|_| {
                report("verify", size.bytes, size.bytes);
                same_contents(&target, &source)
            }) {
                let _ = crate::clean::remove_tree_pub(&source);
                let _ = make_junction(&source, &target);
                return Err(e);
            }
        }
        Method::KnownFolder => {
            std::fs::create_dir_all(&source).map_err(|e| e.to_string())?;
            copy_tree(&target, &source, size.bytes, &|d| {
                report("copy", d, size.bytes)
            })?;
            report("verify", size.bytes, size.bytes);
            same_contents(&target, &source)?;
            set_known_folder(id, &source)?;
        }
    }
    report("cleanup", size.bytes, size.bytes);
    let _ = crate::clean::remove_tree_pub(&target);
    all.retain(|r| r.id != id);
    save_records(dir, &all)
}

fn gb(b: u64) -> String {
    format!("{:.1} GB", b as f64 / 1073741824.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_mirror_the_source_path() {
        assert_eq!(
            target_for(Path::new(r"C:\Users\me\.gradle"), r"D:\"),
            PathBuf::from(r"D:\Users\me\.gradle")
        );
        assert_eq!(
            target_for(Path::new(r"C:\Users\me\AppData\Local\Android\Sdk"), r"E:\"),
            PathBuf::from(r"E:\Users\me\AppData\Local\Android\Sdk")
        );
    }

    #[test]
    fn copy_verify_and_junction_round_trip() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("src");
        let dst = root.path().join("dst");
        std::fs::create_dir_all(src.join("a/b")).unwrap();
        std::fs::write(src.join("a/b/f.bin"), vec![3u8; 5000]).unwrap();
        std::fs::write(src.join("top.txt"), b"hello").unwrap();
        copy_tree(&src, &dst, 5005, &|_| {}).unwrap();
        same_contents(&src, &dst).unwrap();
        let aside = root.path().join("src.old");
        std::fs::rename(&src, &aside).unwrap();
        make_junction(&src, &dst).unwrap();
        // The old path reads the moved files.
        assert_eq!(std::fs::read(src.join("top.txt")).unwrap(), b"hello");
        assert!(is_link(&std::fs::symlink_metadata(&src).unwrap()));
        // Removing the junction leaves the moved files alone.
        std::fs::remove_dir(&src).unwrap();
        assert!(dst.join("a/b/f.bin").exists());
    }

    #[test]
    fn running_programs_map_to_names_people_know() {
        let running = vec!["weixin.exe".to_string(), "explorer.exe".to_string()];
        assert_eq!(blockers_for(WECHAT, &running), vec!["微信".to_string()]);
        assert!(blockers_for(ANDROID, &running).is_empty());
    }
}

#[cfg(test)]
mod real {
    /// Read-only: `cargo test --release real_candidates -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_candidates() {
        let started = std::time::Instant::now();
        for c in super::candidates(&std::env::temp_dir().join("diskmate-none"), true) {
            println!(
                "{:<26} {:>7.2} GB  {:?}  blockers={:?}  {}",
                c.title.chars().take(26).collect::<String>(),
                c.size as f64 / 1073741824.0,
                c.method,
                c.blockers,
                c.path
            );
        }
        println!("took {:?}", started.elapsed());
        for d in crate::vdisk::list(&[]) {
            println!(
                "vdisk {:>6.2} GB in_use={} {} {}",
                d.size as f64 / 1073741824.0,
                d.in_use,
                d.owner,
                d.path
            );
        }
    }
}

#[cfg(test)]
mod real_move {
    use super::*;

    /// Moves a throwaway folder from the user folder to D:\ and back:
    /// `cargo test --release real_move_round_trip -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_move_round_trip() {
        let src = home().join("diskmate-move-test");
        let _ = std::fs::remove_dir_all(&src);
        std::fs::create_dir_all(src.join("deep/er")).unwrap();
        for i in 0..50 {
            std::fs::write(src.join(format!("deep/er/f{i}.bin")), vec![i as u8; 20_000]).unwrap();
        }
        std::fs::write(src.join("hello.txt"), b"hello from C").unwrap();
        let store = tempfile::tempdir().unwrap();
        let target = target_for(&src, r"D:\");
        let _ = std::fs::remove_dir_all(&target);

        let rec = migrate(store.path(), "test", &src, r"D:\", &|p, d, t| {
            println!("  {p} {d}/{t}")
        })
        .unwrap();
        println!("moved to {}", rec.target);
        assert!(
            is_link(&std::fs::symlink_metadata(&src).unwrap()),
            "old path is a junction"
        );
        assert_eq!(
            std::fs::read(src.join("hello.txt")).unwrap(),
            b"hello from C",
            "readable through the junction"
        );
        assert!(target.join("deep/er/f49.bin").is_file(), "files live on D:");
        assert!(
            !src.with_file_name("diskmate-move-test.diskmate-moving")
                .exists(),
            "aside copy removed"
        );

        restore(store.path(), "test", &|p, d, t| println!("  {p} {d}/{t}")).unwrap();
        assert!(
            !is_link(&std::fs::symlink_metadata(&src).unwrap()),
            "back to a real folder"
        );
        assert_eq!(
            std::fs::read(src.join("hello.txt")).unwrap(),
            b"hello from C"
        );
        assert_eq!(measure(&src, None).files, 51);
        assert!(!target.exists(), "D: copy removed");
        assert!(records(store.path()).is_empty());

        std::fs::remove_dir_all(&src).unwrap();
        // The empty parents created on D: for the mirrored path.
        let _ = std::fs::remove_dir(PathBuf::from(r"D:\Users\40501"));
        let _ = std::fs::remove_dir(PathBuf::from(r"D:\Users"));
        println!("round trip ok");
    }
}
