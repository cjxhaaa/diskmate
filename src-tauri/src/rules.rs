//! What can be cleaned, how much it frees, and why it is (or is not) safe.
//!
//! The frontend never sends paths to delete. It gets a list of items from [`analyze`], shows them,
//! and sends back the ids the user picked; the paths behind each id stay here.

use crate::util::{
    self, days_ago, display, home, local_appdata, measure, newest_mtime, program_data,
    roaming_appdata, system_root, Size,
};
use rayon::prelude::*;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub enum Target {
    /// Everything inside the folder, optionally only files older than `min_age_days`. The folder stays.
    Contents {
        path: PathBuf,
        min_age_days: u64,
    },
    /// The folder itself.
    Dir {
        path: PathBuf,
    },
    File {
        path: PathBuf,
    },
    /// A git worktree: `git worktree remove`, so the repository forgets it too. Its branch is kept.
    Worktree {
        repo: PathBuf,
        path: PathBuf,
    },
    RecycleBin,
    Hibernation,
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    /// Regenerated automatically; nothing is lost.
    Safe,
    /// Nothing personal is lost, but something has to be re-downloaded or rebuilt.
    Caution,
    /// Worth a look before deleting.
    Review,
}

#[derive(Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Detail {
    pub label: String,
    pub path: String,
    pub size: u64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CleanItem {
    pub id: String,
    /// `system` | `dev` | `app` | `review` | `guide`
    pub group: &'static str,
    pub title: String,
    pub desc: String,
    pub risk: Risk,
    pub size: u64,
    pub files: u64,
    /// Ticked by default.
    pub selected: bool,
    /// Only explained, never cleaned here: the owning app has to do it (WeChat, QQ Music, ...).
    pub guide_only: bool,
    pub needs_admin: bool,
    pub note: Option<String>,
    pub details: Vec<Detail>,
}

pub struct Planned {
    pub item: CleanItem,
    pub targets: Vec<Target>,
}

struct Spec {
    id: &'static str,
    group: &'static str,
    title: String,
    desc: &'static str,
    risk: Risk,
    selected: bool,
    guide_only: bool,
    needs_admin: bool,
    note: Option<&'static str>,
    targets: Vec<(String, Target)>,
}

impl Spec {
    fn new(
        id: &'static str,
        group: &'static str,
        title: impl Into<String>,
        desc: &'static str,
        risk: Risk,
    ) -> Self {
        Spec {
            id,
            group,
            title: title.into(),
            desc,
            risk,
            selected: risk == Risk::Safe,
            guide_only: false,
            needs_admin: false,
            note: None,
            targets: Vec::new(),
        }
    }

    fn contents(mut self, label: impl Into<String>, path: PathBuf, min_age_days: u64) -> Self {
        if path.is_dir() {
            self.targets
                .push((label.into(), Target::Contents { path, min_age_days }));
        }
        self
    }

    fn dir(mut self, label: impl Into<String>, path: PathBuf) -> Self {
        if path.is_dir() {
            self.targets.push((label.into(), Target::Dir { path }));
        }
        self
    }

    fn admin(mut self) -> Self {
        self.needs_admin = true;
        self
    }

    fn unselected(mut self) -> Self {
        self.selected = false;
        self
    }

    fn guide(mut self) -> Self {
        self.guide_only = true;
        self.selected = false;
        self
    }

    fn note(mut self, note: &'static str) -> Self {
        self.note = Some(note);
        self
    }
}

fn target_size(t: &Target) -> Size {
    match t {
        Target::Contents { path, min_age_days } => {
            measure(path, (*min_age_days > 0).then(|| days_ago(*min_age_days)))
        }
        Target::Dir { path } | Target::Worktree { path, .. } => measure(path, None),
        Target::File { path } => Size {
            bytes: util::file_len(path).unwrap_or(0),
            files: 1,
        },
        Target::RecycleBin => {
            let (bytes, files) = crate::sys::recycle_bin_size();
            Size { bytes, files }
        }
        Target::Hibernation => Size {
            bytes: util::file_len(&crate::sys::hiberfil_path()).unwrap_or(0),
            files: 1,
        },
    }
}

fn path_of(t: &Target) -> String {
    match t {
        Target::Contents { path, .. }
        | Target::Dir { path }
        | Target::File { path }
        | Target::Worktree { path, .. } => display(path),
        Target::RecycleBin => "回收站".into(),
        Target::Hibernation => display(&crate::sys::hiberfil_path()),
    }
}

/// Folders directly inside `dir`.
fn subdirs(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect()
        })
        .unwrap_or_default()
}

fn name_of(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------
// System
// ---------------------------------------------------------------------------------------------

fn system_specs() -> Vec<Spec> {
    let local = local_appdata();
    let windows = system_root();
    let wer = program_data().join(r"Microsoft\Windows\WER");
    vec![
        Spec::new("user-temp", "system", "临时文件", "程序运行时留下的临时文件。只清理 1 天前的，正在使用的会自动跳过。", Risk::Safe)
            .contents("用户临时文件夹", local.join("Temp"), 1)
            .contents("系统临时文件夹", windows.join("Temp"), 1),
        Spec::new("windows-update", "system", "Windows 更新缓存", "更新安装完成后留下的下载包，Windows 不会再用到。", Risk::Safe)
            .contents("SoftwareDistribution\\Download", windows.join(r"SoftwareDistribution\Download"), 0)
            .contents("传递优化缓存", windows.join(r"ServiceProfiles\NetworkService\AppData\Local\Microsoft\Windows\DeliveryOptimization\Cache"), 0)
            .admin(),
        Spec::new("crash-dumps", "system", "崩溃转储与错误报告", "程序崩溃时生成的诊断文件，只在给开发者排查问题时有用。", Risk::Safe)
            .contents("CrashDumps", local.join("CrashDumps"), 0)
            .contents("错误报告存档", wer.join("ReportArchive"), 0)
            .contents("错误报告队列", wer.join("ReportQueue"), 0)
            .contents("用户错误报告", local.join(r"Microsoft\Windows\WER"), 0),
        Spec::new("shader-cache", "system", "显卡着色器缓存", "游戏和图形程序编译好的着色器，删除后下次启动会自动重新生成。", Risk::Safe)
            .contents("DirectX", local.join("D3DSCache"), 0)
            .contents("NVIDIA", local.join(r"NVIDIA\DXCache"), 0)
            .contents("NVIDIA GL", local.join(r"NVIDIA\GLCache"), 0)
            .contents("AMD", local.join(r"AMD\DxCache"), 0),
        {
            let mut s = Spec::new("recycle-bin", "system", "回收站", "已经删除、放在回收站里的文件。清空后无法再恢复。", Risk::Caution);
            s.targets.push(("回收站".into(), Target::RecycleBin));
            s.unselected()
        },
    ]
}

// ---------------------------------------------------------------------------------------------
// Developer caches
// ---------------------------------------------------------------------------------------------

fn android_sdk() -> PathBuf {
    std::env::var_os("ANDROID_HOME")
        .or_else(|| std::env::var_os("ANDROID_SDK_ROOT"))
        .map(PathBuf::from)
        .unwrap_or_else(|| local_appdata().join(r"Android\Sdk"))
}

fn avd_dir() -> PathBuf {
    std::env::var_os("ANDROID_AVD_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(r".android\avd"))
}

/// `system-images\android-36\google_apis\x86_64` paths every AVD refers to, lowercased.
fn system_images_in_use() -> Vec<String> {
    subdirs(&avd_dir())
        .into_iter()
        .filter_map(|avd| std::fs::read_to_string(avd.join("config.ini")).ok())
        .flat_map(|ini| {
            ini.lines()
                .filter_map(|l| l.strip_prefix("image.sysdir.1"))
                .filter_map(|l| {
                    l.split_once('=').map(|(_, v)| {
                        v.trim()
                            .replace('/', "\\")
                            .trim_end_matches('\\')
                            .to_lowercase()
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// `IntelliJIdea2024.1` → (`IntelliJIdea`, 2024.1).
fn split_versioned(name: &str) -> Option<(String, f64)> {
    let pos = name.find(|c: char| c.is_ascii_digit())?;
    let (product, version) = name.split_at(pos);
    if product.is_empty() {
        return None;
    }
    Some((product.to_string(), version.parse().ok()?))
}

/// Folders named `<Product><Version>` under `dir`, every version but the newest of each product.
fn older_versions(dir: &Path) -> Vec<PathBuf> {
    let mut by_product: std::collections::HashMap<String, Vec<(f64, PathBuf)>> = Default::default();
    for p in subdirs(dir) {
        if let Some((product, version)) = split_versioned(&name_of(&p)) {
            by_product.entry(product).or_default().push((version, p));
        }
    }
    by_product
        .into_values()
        .flat_map(|mut v| {
            v.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            v.into_iter().skip(1).map(|(_, p)| p)
        })
        .collect()
}

fn dev_specs() -> Vec<Spec> {
    let home = home();
    let local = local_appdata();
    let roaming = roaming_appdata();
    let mut specs = vec![
        Spec::new("gradle-daemon", "dev", "Gradle 守护进程日志", "Gradle 后台进程写的运行日志，只用于排查 Gradle 本身的问题。", Risk::Safe)
            .contents("~\\.gradle\\daemon", home.join(r".gradle\daemon"), 0),
        Spec::new("package-caches", "dev", "包管理器下载缓存", "npm / bun / pip / yarn / Go 下载过的安装包。下次安装时按需重新下载。", Risk::Safe)
            .contents("npm", local.join("npm-cache"), 0)
            .contents("bun", home.join(r".bun\install\cache"), 0)
            .contents("pip", local.join(r"pip\cache"), 0)
            .contents("yarn", local.join(r"Yarn\Cache"), 0)
            .contents("Go 编译缓存", local.join("go-build"), 0),
        Spec::new("gradle-caches", "dev", "Gradle 依赖缓存", "所有 Android / Java 项目下载过的依赖。删除后每个项目第一次编译都要重新下载，比较慢。", Risk::Caution)
            .contents("~\\.gradle\\caches", home.join(r".gradle\caches"), 0)
            .unselected(),
        Spec::new("ai-index", "dev", "AI 编程插件的代码索引", "Continue 等 AI 编程插件为代码搜索建的本地索引。删除后插件会在后台重新建立，期间代码检索效果会差一些。", Risk::Caution)
            .contents("Continue 索引", home.join(r".continue\index"), 0)
            .unselected(),
        Spec::new("cargo-cache", "dev", "Rust 依赖缓存", "cargo 下载过的 crate 源码包。删除后编译时重新下载。", Risk::Caution)
            .contents("registry\\cache", home.join(r".cargo\registry\cache"), 0)
            .contents("registry\\src", home.join(r".cargo\registry\src"), 0)
            .unselected(),
    ];

    // Gradle distributions: keep the newest, older ones are only needed by projects still pinned to them.
    let mut wrapper = Spec::new("gradle-wrapper", "dev", "旧版本 Gradle", "gradle wrapper 下载过的旧版本 Gradle。只保留最新一个；还在用旧版本的项目编译时会重新下载。", Risk::Caution).unselected();
    let mut dists = subdirs(&home.join(r".gradle\wrapper\dists"));
    dists.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
    dists.pop();
    for d in dists {
        wrapper = wrapper.dir(name_of(&d), d);
    }
    specs.push(wrapper);

    // Android system images no AVD uses.
    let sdk = android_sdk();
    let in_use = system_images_in_use();
    let mut images = Spec::new(
        "android-images",
        "dev",
        "没被模拟器使用的 Android 系统镜像",
        "SDK 里装着、但没有任何模拟器在用的系统镜像。需要时可以在 SDK Manager 里重新下载。",
        Risk::Safe,
    );
    for api in subdirs(&sdk.join("system-images")) {
        for tag in subdirs(&api) {
            for abi in subdirs(&tag) {
                let rel = format!(
                    "system-images\\{}\\{}\\{}",
                    name_of(&api),
                    name_of(&tag),
                    name_of(&abi)
                )
                .to_lowercase();
                if !in_use.contains(&rel) {
                    images = images.dir(
                        format!("{} / {} / {}", name_of(&api), name_of(&tag), name_of(&abi)),
                        abi,
                    );
                }
            }
        }
    }
    specs.push(images);

    let mut snapshots = Spec::new(
        "avd-snapshots",
        "dev",
        "模拟器快速启动快照",
        "Android 模拟器保存的开机快照。删除后模拟器下次会冷启动，慢几十秒，数据不受影响。",
        Risk::Caution,
    )
    .unselected();
    for avd in subdirs(&avd_dir()) {
        snapshots = snapshots.dir(name_of(&avd), avd.join("snapshots"));
    }
    specs.push(snapshots);

    let mut ides = Spec::new("ide-old-versions", "dev", "旧版本 IDE 缓存", "已升级的 JetBrains IDE / Android Studio 旧版本留下的缓存和日志。设置保存在另一个目录，不受影响。", Risk::Caution).unselected();
    for p in older_versions(&local.join("JetBrains")).into_iter().chain(
        older_versions(&local.join("Google"))
            .into_iter()
            .filter(|p| name_of(p).starts_with("AndroidStudio")),
    ) {
        ides = ides.dir(name_of(&p), p);
    }
    specs.push(ides);

    let mut editors = Spec::new(
        "editor-cache",
        "dev",
        "VS Code / Cursor 缓存",
        "编辑器的网页缓存、旧扩展安装包和日志，不影响设置和扩展。",
        Risk::Safe,
    );
    for app in ["Code", "Cursor"] {
        let base = roaming.join(app);
        for sub in [
            "Cache",
            "CachedData",
            "CachedExtensionVSIXs",
            "Code Cache",
            "GPUCache",
            "logs",
        ] {
            editors = editors.contents(format!("{app} / {sub}"), base.join(sub), 0);
        }
    }
    specs.push(editors);

    specs.push(stale_build_outputs(&home));
    specs
}

/// Build output of projects nobody has touched in a month: `node_modules`, Rust `target`, Gradle
/// `build` and `.gradle`. All of it comes back with one install/build.
fn stale_build_outputs(home: &Path) -> Spec {
    let mut spec = Spec::new(
        "stale-build-output",
        "dev",
        "久未改动项目的构建产物",
        "超过 30 天没改动的项目里的 node_modules、target、build 等目录。源代码不动；需要时重新 install / build 即可恢复。",
        Risk::Caution,
    )
    .unselected();
    let cutoff = days_ago(30);
    let mut found = Vec::new();
    find_projects(home, 0, cutoff, &mut found);
    for (project, artifact) in found {
        let label = format!(
            "{} › {}",
            name_of(&project),
            artifact
                .strip_prefix(&project)
                .map(display)
                .unwrap_or_default()
        );
        spec = spec.dir(label, artifact);
    }
    spec
}

const ARTIFACT_NAMES: &[&str] = &[
    "node_modules",
    "target",
    "build",
    ".gradle",
    ".next",
    ".nuxt",
    ".turbo",
];

fn find_projects(
    dir: &Path,
    depth: usize,
    cutoff: std::time::SystemTime,
    out: &mut Vec<(PathBuf, PathBuf)>,
) {
    if depth > 5 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let names: Vec<(String, PathBuf, bool)> = rd
        .flatten()
        .filter_map(|e| {
            let md = e.metadata().ok()?;
            if util::is_link(&md) {
                return None;
            }
            Some((
                e.file_name().to_string_lossy().into_owned(),
                e.path(),
                md.is_dir(),
            ))
        })
        .collect();
    let has = |n: &str| {
        names
            .iter()
            .any(|(name, _, _)| name.eq_ignore_ascii_case(n))
    };
    let is_node = has("package.json");
    let is_rust = has("Cargo.toml");
    let is_gradle = has("build.gradle")
        || has("build.gradle.kts")
        || has("settings.gradle")
        || has("settings.gradle.kts");
    let is_project = is_node || is_rust || is_gradle;
    let stale = is_project
        && newest_mtime(
            dir,
            &[
                "node_modules",
                "target",
                "build",
                ".gradle",
                ".git",
                ".idea",
                ".next",
                ".nuxt",
                ".turbo",
            ],
        )
        .map(|t| t < cutoff)
        .unwrap_or(false);

    for (name, path, is_dir) in &names {
        if !is_dir {
            continue;
        }
        let lower = name.to_lowercase();
        let is_artifact = (lower == "node_modules" && is_node)
            || (lower == "target" && is_rust)
            || ((lower == "build" || lower == ".gradle") && is_gradle)
            || (matches!(lower.as_str(), ".next" | ".nuxt" | ".turbo") && is_node);
        if is_artifact {
            if stale {
                out.push((dir.to_path_buf(), path.clone()));
            }
            continue;
        }
        // Never descend into hidden folders (.git, .claude/worktrees — those have their own rule),
        // AppData, or anything that is itself build output.
        if lower.starts_with('.') || lower == "appdata" || ARTIFACT_NAMES.contains(&lower.as_str())
        {
            continue;
        }
        if depth == 0 && matches!(lower.as_str(), "onedrive" | "music" | "videos" | "pictures") {
            continue;
        }
        find_projects(path, depth + 1, cutoff, out);
    }
}

// ---------------------------------------------------------------------------------------------
// App caches
// ---------------------------------------------------------------------------------------------

fn browser_cache(id: &'static str, title: &'static str, user_data: PathBuf) -> Spec {
    let mut spec = Spec::new(
        id,
        "app",
        title,
        "网页图片、脚本等缓存。不会删除书签、密码、历史记录和登录状态，网页第一次打开会稍慢。",
        Risk::Safe,
    )
    .note("浏览器开着时部分缓存被占用会跳过，关闭浏览器后清理更彻底。");
    for profile in subdirs(&user_data) {
        let name = name_of(&profile);
        if name != "Default" && !name.starts_with("Profile") {
            continue;
        }
        for sub in [
            "Cache",
            "Code Cache",
            "GPUCache",
            "DawnGraphiteCache",
            "DawnWebGPUCache",
        ] {
            spec = spec.contents(format!("{name} / {sub}"), profile.join(sub), 0);
        }
    }
    spec.contents("着色器缓存", user_data.join("ShaderCache"), 0)
        .contents("图形缓存", user_data.join("GrShaderCache"), 0)
}

fn app_specs() -> Vec<Spec> {
    let local = local_appdata();
    vec![
        browser_cache(
            "chrome-cache",
            "Chrome 缓存",
            local.join(r"Google\Chrome\User Data"),
        ),
        browser_cache(
            "edge-cache",
            "Edge 缓存",
            local.join(r"Microsoft\Edge\User Data"),
        ),
    ]
}

// ---------------------------------------------------------------------------------------------
// Needs a look
// ---------------------------------------------------------------------------------------------

fn git(args: &[&str]) -> Option<String> {
    let out = util::command("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Uncommitted work in a worktree. Untracked files count — except the local build config people
/// copy in by hand, which is gitignored in most repos but not all.
pub fn worktree_is_clean(path: &Path) -> bool {
    let Some(status) = git(&["-C", &path.to_string_lossy(), "status", "--porcelain"]) else {
        return false;
    };
    status.lines().all(|l| {
        let file = l.get(3..).unwrap_or("");
        l.starts_with("??") && (file.ends_with("local.properties") || file.contains("keystore"))
    })
}

fn worktree_last_activity(path: &Path) -> Option<std::time::SystemTime> {
    let commit = git(&["-C", &path.to_string_lossy(), "log", "-1", "--format=%ct"])
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(|secs| std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs));
    let touched = newest_mtime(path, &[]);
    commit.into_iter().chain(touched).max()
}

/// Git worktrees with nothing uncommitted and no activity for three days, in every repository
/// directly under the user folder.
fn worktree_spec(home: &Path) -> Spec {
    let mut spec = Spec::new(
        "git-worktrees",
        "review",
        "闲置的 Git worktree",
        "没有未提交改动、3 天没动过的 worktree（比如 Claude Code 为每个会话建的）。用 git worktree remove 删除，分支和提交都保留，需要时可以重新检出。",
        Risk::Review,
    )
    .unselected();
    let cutoff = days_ago(3);
    let repos: Vec<PathBuf> = subdirs(home)
        .into_iter()
        .filter(|p| p.join(".git").is_dir())
        .collect();
    let found: Vec<(String, Target)> = repos
        .par_iter()
        .flat_map(|repo| {
            let Some(list) = git(&[
                "-C",
                &repo.to_string_lossy(),
                "worktree",
                "list",
                "--porcelain",
            ]) else {
                return Vec::new();
            };
            let mut items = Vec::new();
            let mut current: Option<PathBuf> = None;
            let mut branch = String::new();
            for line in list.lines().chain(std::iter::once("")) {
                if let Some(p) = line.strip_prefix("worktree ") {
                    current = Some(PathBuf::from(p.replace('/', "\\")));
                    branch = "（游离）".into();
                } else if let Some(b) = line.strip_prefix("branch ") {
                    branch = b.trim_start_matches("refs/heads/").to_string();
                } else if line.is_empty() {
                    if let Some(wt) = current.take() {
                        let is_main = wt.eq(repo)
                            || wt
                                .to_string_lossy()
                                .eq_ignore_ascii_case(&repo.to_string_lossy());
                        if !is_main
                            && wt.is_dir()
                            && worktree_last_activity(&wt)
                                .map(|t| t < cutoff)
                                .unwrap_or(false)
                            && worktree_is_clean(&wt)
                        {
                            items.push((
                                format!("{} · {}", name_of(&wt), branch),
                                Target::Worktree {
                                    repo: repo.clone(),
                                    path: wt,
                                },
                            ));
                        }
                    }
                }
            }
            items
        })
        .collect();
    spec.targets = found;
    spec
}

fn review_specs() -> Vec<Spec> {
    let home = home();
    let mut specs = vec![worktree_spec(&home)];

    let mut avds = Spec::new("old-avds", "review", "60 天没用过的 Android 模拟器", "整台模拟器连同里面的数据一起删除。还要用的话，可以在 Android Studio 的 Device Manager 里重新创建。", Risk::Review).unselected();
    let cutoff = days_ago(60);
    for avd in subdirs(&avd_dir()) {
        if newest_mtime(&avd, &[]).map(|t| t < cutoff).unwrap_or(false) {
            let name = name_of(&avd);
            let ini = avd.with_extension("ini");
            avds = avds.dir(name.trim_end_matches(".avd").to_string(), avd);
            if ini.is_file() {
                avds.targets
                    .push((format!("{} 配置", name), Target::File { path: ini }));
            }
        }
    }
    specs.push(avds);

    let mut hib = Spec::new("hibernation", "review", "关闭休眠（删除 hiberfil.sys）", "休眠文件大小和内存差不多。关闭后电脑不能「休眠」，开机的「快速启动」也会失效，睡眠不受影响。会弹出管理员授权。", Risk::Review).unselected().admin();
    if util::file_len(&crate::sys::hiberfil_path()).is_some() {
        hib.targets
            .push(("hiberfil.sys".into(), Target::Hibernation));
    }
    specs.push(hib);
    specs
}

// ---------------------------------------------------------------------------------------------
// Only explained
// ---------------------------------------------------------------------------------------------

fn guide_specs() -> Vec<Spec> {
    let home = home();
    let local = local_appdata();
    let roaming = roaming_appdata();
    let mut claude = Spec::new(
        "claude",
        "guide",
        "Claude 桌面应用",
        "会话记录、代码 worktree 和虚拟机镜像。请在 Claude › 设置 › 存储 中清理闲置会话和缓存。",
        Risk::Review,
    )
    .dir("应用数据", roaming.join("Claude"))
    .guide();
    for p in subdirs(&local.join("Packages"))
        .into_iter()
        .filter(|p| name_of(p).starts_with("Claude_"))
    {
        claude = claude.dir("应用包数据", p);
    }
    vec![
        Spec::new("wechat", "guide", "微信", "聊天里收发的图片、视频和文件。直接删文件夹会让聊天记录里的文件打不开，请在 微信 › 设置 › 存储空间 里按聊天清理。", Risk::Review)
            .dir("WeChat Files", home.join(r"Documents\WeChat Files"))
            .dir("xwechat_files", home.join("xwechat_files"))
            .dir("微信数据", roaming.join(r"Tencent\xwechat"))
            .dir("旧版微信数据", roaming.join(r"Tencent\WeChat"))
            .guide(),
        Spec::new("qqmusic", "guide", "QQ 音乐", "下载的歌曲和播放缓存。请在 QQ 音乐 › 设置 › 下载与缓存 里清理缓存，或删除不听的下载歌曲。", Risk::Review)
            .dir("QQMusic", roaming.join(r"Tencent\QQMusic"))
            .guide(),
        claude,
        Spec::new("downloads", "guide", "下载文件夹", "下载过的安装包、压缩包常常用完就忘。建议按大小排序，手动删掉不再需要的。", Risk::Review)
            .dir("Downloads", home.join("Downloads"))
            .guide(),
        {
            let mut s = Spec::new("pagefile", "guide", "虚拟内存 pagefile.sys", "Windows 按内存大小自动分配。内存充足时可以在 系统 › 高级系统设置 › 性能 › 虚拟内存 里改成较小的固定大小；不建议关闭。", Risk::Review).guide();
            let path = util::system_drive().join("pagefile.sys");
            if util::file_len(&path).is_some() {
                s.targets.push(("pagefile.sys".into(), Target::File { path }));
            }
            s
        },
    ]
}

/// Builds every item and measures it. `on_progress(done, total, title)` fires as each one finishes.
pub fn analyze(on_progress: impl Fn(usize, usize, &str) + Sync) -> Vec<Planned> {
    let specs: Vec<Spec> = [
        system_specs(),
        dev_specs(),
        app_specs(),
        review_specs(),
        guide_specs(),
    ]
    .into_iter()
    .flatten()
    .filter(|s| !s.targets.is_empty())
    .collect();
    let total = specs.len();
    let done = std::sync::atomic::AtomicUsize::new(0);
    let mut planned: Vec<Planned> = specs
        .into_par_iter()
        .map(|spec| {
            let measured: Vec<(Detail, Size)> = spec
                .targets
                .par_iter()
                .map(|(label, t)| {
                    let size = target_size(t);
                    (
                        Detail {
                            label: label.clone(),
                            path: path_of(t),
                            size: size.bytes,
                        },
                        size,
                    )
                })
                .collect();
            let size = measured
                .iter()
                .fold(Size::default(), |acc, (_, s)| acc + *s);
            let mut details: Vec<Detail> = measured
                .into_iter()
                .map(|(d, _)| d)
                .filter(|d| d.size > 0)
                .collect();
            details.sort_by(|a, b| b.size.cmp(&a.size));
            let n = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            on_progress(n, total, &spec.title);
            Planned {
                item: CleanItem {
                    id: spec.id.to_string(),
                    group: spec.group,
                    title: spec.title,
                    desc: spec.desc.to_string(),
                    risk: spec.risk,
                    size: size.bytes,
                    files: size.files,
                    selected: spec.selected,
                    guide_only: spec.guide_only,
                    needs_admin: spec.needs_admin,
                    note: spec.note.map(str::to_string),
                    details,
                },
                targets: spec.targets.into_iter().map(|(_, t)| t).collect(),
            }
        })
        .collect();
    // Anything under 1 MB is not worth a row.
    planned.retain(|p| p.item.size >= 1024 * 1024);
    planned.sort_by(|a, b| b.item.size.cmp(&a.item.size));
    planned
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Read-only run against this machine: `cargo test real_analysis -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_analysis() {
        let started = std::time::Instant::now();
        let planned = analyze(|_, _, _| {});
        for p in &planned {
            println!(
                "{:<20} {:>10.2} GB  {:?}{}  {}",
                p.item.id,
                p.item.size as f64 / 1073741824.0,
                p.item.risk,
                if p.item.guide_only { " guide" } else { "" },
                p.item.title
            );
            for d in p.item.details.iter().take(4) {
                println!("{:>34.2} GB  {}", d.size as f64 / 1073741824.0, d.path);
            }
        }
        println!("analysis took {:?}", started.elapsed());
    }

    #[test]
    fn versioned_folders_keep_the_newest() {
        let dir = tempfile::tempdir().unwrap();
        for n in ["CLion2023.3", "CLion2024.1", "GoLand2024.1", "notes"] {
            std::fs::create_dir(dir.path().join(n)).unwrap();
        }
        let mut old: Vec<String> = older_versions(dir.path())
            .iter()
            .map(|p| name_of(p))
            .collect();
        old.sort();
        assert_eq!(old, vec!["CLion2023.3"]);
    }

    #[test]
    fn only_stale_projects_give_up_their_build_output() {
        let home = tempfile::tempdir().unwrap();
        let project = home.path().join("app");
        std::fs::create_dir_all(project.join("node_modules/pkg")).unwrap();
        std::fs::write(project.join("package.json"), "{}").unwrap();
        // Freshly written, so not stale.
        let mut found = Vec::new();
        find_projects(home.path(), 0, days_ago(30), &mut found);
        assert!(found.is_empty());
        // With a cutoff in the future every project counts as stale.
        find_projects(
            home.path(),
            0,
            std::time::SystemTime::now() + std::time::Duration::from_secs(60),
            &mut found,
        );
        assert_eq!(found.len(), 1);
        assert!(found[0].1.ends_with("node_modules"));
    }
}
