mod apps;
mod cache;
mod clean;
mod downloads;
mod dupes;
mod filemeta;
mod history;
mod migrate;
mod perf;
mod rules;
mod scan;
mod startup;
mod sys;
mod tray;
mod util;
mod vdisk;

use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Default)]
struct AppState {
    /// The last finished scan of each drive, keyed by its uppercased root (`C:\`). Kept per drive so
    /// switching drives shows what was already found instead of scanning again.
    scans: Mutex<HashMap<String, Arc<scan::ScanData>>>,
    scanning: Mutex<Option<Arc<scan::ScanCtx>>>,
    plans: Mutex<HashMap<String, rules::Planned>>,
    apps: Mutex<HashMap<String, apps::AppEntry>>,
    /// Uninstalls being waited on; setting the flag stops waiting and checks once more.
    uninstall_waits: Mutex<HashMap<String, Arc<std::sync::atomic::AtomicBool>>>,
    dupes_running: Mutex<Option<Arc<dupes::DupeCtx>>>,
    /// The last duplicate search, which `delete_duplicates` checks every request against.
    dupes: Mutex<Vec<dupes::DupeGroup>>,
    /// Live counters for the performance page, opened on first use.
    perf: Mutex<Option<perf::Monitor>>,
    procs: Mutex<Option<perf::ProcMonitor>>,
    hardware: Mutex<Option<perf::HardwareInfo>>,
}

/// `%LOCALAPPDATA%\com.cjxhaaa.diskmate`: past scans, the record of moved folders.
fn data_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .app_local_data_dir()
        .unwrap_or_else(|_| util::local_appdata().join("DiskMate"))
}

fn history_dir(app: &AppHandle) -> PathBuf {
    data_dir(app).join("history")
}

#[tauri::command]
async fn migration_candidates(app: AppHandle) -> Result<Vec<migrate::Candidate>, String> {
    let dir = data_dir(&app);
    tauri::async_runtime::spawn_blocking(move || migrate::candidates(&dir, true))
        .await
        .map_err(|e| e.to_string())
}

/// The same list without measuring anything: which programs are running and what is moved.
#[tauri::command]
async fn migration_status(app: AppHandle) -> Result<Vec<migrate::Candidate>, String> {
    let dir = data_dir(&app);
    tauri::async_runtime::spawn_blocking(move || migrate::candidates(&dir, false))
        .await
        .map_err(|e| e.to_string())
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct MoveDone {
    id: String,
    ok: bool,
    error: Option<String>,
}

/// Moves (or with `restore`, brings back) one candidate. Long-running: progress arrives as
/// `migrate-progress`, the end as `migrate-done`.
#[tauri::command]
fn migrate_folder(
    app: AppHandle,
    id: String,
    drive_root: Option<String>,
    restore: bool,
) -> Result<(), String> {
    let dir = data_dir(&app);
    std::thread::spawn(move || {
        let report = |phase: &'static str, done: u64, total: u64| {
            let _ = app.emit(
                "migrate-progress",
                migrate::MoveProgress {
                    id: id.clone(),
                    phase,
                    done,
                    total,
                },
            );
        };
        let result = if restore {
            migrate::restore(&dir, &id, &report)
        } else {
            match (migrate::source_for(&id), drive_root.as_deref()) {
                (Some(source), Some(drive)) => {
                    migrate::migrate(&dir, &id, &source, drive, &report).map(|_| ())
                }
                (None, _) => Err("找不到这个文件夹".into()),
                (_, None) => Err("没有选择目标磁盘".into()),
            }
        };
        let _ = app.emit(
            "migrate-done",
            MoveDone {
                id,
                ok: result.is_ok(),
                error: result.err(),
            },
        );
    });
    Ok(())
}

#[tauri::command]
fn list_vdisks(state: State<AppState>) -> Vec<vdisk::VDisk> {
    let large: Vec<String> = state
        .scans
        .lock()
        .unwrap()
        .values()
        .flat_map(|d| d.large.iter().map(|f| f.path.clone()))
        .collect();
    vdisk::list(&large)
}

#[tauri::command]
async fn shutdown_wsl() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(vdisk::shutdown_wsl)
        .await
        .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Compacted {
    before: u64,
    after: u64,
}

/// Compacts a disk that `list_vdisks` reported; any other path is refused.
#[tauri::command]
async fn compact_vdisk(state: State<'_, AppState>, path: String) -> Result<Compacted, String> {
    let large: Vec<String> = state
        .scans
        .lock()
        .unwrap()
        .values()
        .flat_map(|d| d.large.iter().map(|f| f.path.clone()))
        .collect();
    tauri::async_runtime::spawn_blocking(move || {
        let disk = vdisk::list(&large)
            .into_iter()
            .find(|d| d.path.eq_ignore_ascii_case(&path))
            .ok_or("不是已知的虚拟磁盘")?;
        // A WSL disk first has its free blocks trimmed from inside, then WSL is stopped so the disk
        // is released. Both are part of compacting it; neither is optional.
        if let Some(distro) = vdisk::distro_of(&disk.owner) {
            vdisk::trim_wsl(distro)?;
            vdisk::shutdown_wsl()?;
            std::thread::sleep(Duration::from_secs(3));
        }
        vdisk::compact(std::path::Path::new(&path))
            .map(|(before, after)| Compacted { before, after })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn list_drives() -> Vec<sys::DriveInfo> {
    sys::list_drives()
}

/// Starts scanning `root` in the background. Progress arrives as `scan-progress`, the result as
/// `scan-done` (the summary); `get_children` / `large_files` then read the stored tree.
#[tauri::command]
fn start_scan(app: AppHandle, state: State<AppState>, root: String) -> Result<(), String> {
    let root = PathBuf::from(&root);
    if !root.is_dir() {
        return Err(format!("找不到 {}", root.display()));
    }
    let ctx = Arc::new(scan::ScanCtx::default());
    if let Some(previous) = state.scanning.lock().unwrap().replace(ctx.clone()) {
        previous.cancel.store(true, Ordering::Relaxed);
    }
    let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let (app, ctx, done) = (app.clone(), ctx.clone(), done.clone());
        std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                let _ = app.emit("scan-progress", ctx.progress());
                std::thread::sleep(Duration::from_millis(120));
            }
        });
    }
    std::thread::spawn(move || {
        let data = scan::run(&root, &ctx);
        done.store(true, Ordering::Relaxed);
        let state = app.state::<AppState>();
        let mut scanning = state.scanning.lock().unwrap();
        // A newer scan replaced this one: its result is stale, drop it.
        if !scanning
            .as_ref()
            .map(|c| Arc::ptr_eq(c, &ctx))
            .unwrap_or(false)
        {
            return;
        }
        *scanning = None;
        drop(scanning);
        let summary = data.summary.clone();
        let _ = history::save(&history_dir(&app), &data);
        // History and categories are taken; the browser only needs folders of 1 MB and up.
        let mut data = data;
        scan::prune(&mut data.tree);
        state
            .scans
            .lock()
            .unwrap()
            .insert(summary.root.to_uppercase(), Arc::new(data));
        let _ = app.emit("scan-done", summary);
    });
    Ok(())
}

#[tauri::command]
fn cancel_scan(state: State<AppState>) {
    if let Some(ctx) = state.scanning.lock().unwrap().as_ref() {
        ctx.cancel.store(true, Ordering::Relaxed);
    }
}

/// The stored scan of the drive `path` is on.
fn scan_for(state: &AppState, path: &str) -> Option<Arc<scan::ScanData>> {
    let upper = path.to_uppercase();
    state
        .scans
        .lock()
        .unwrap()
        .iter()
        .find(|(root, _)| upper.starts_with(root.as_str()))
        .map(|(_, data)| data.clone())
}

/// Every drive scanned in this session, so the UI can show them without scanning again.
#[tauri::command]
fn scan_summaries(state: State<AppState>) -> Vec<scan::Summary> {
    state
        .scans
        .lock()
        .unwrap()
        .values()
        .map(|d| d.summary.clone())
        .collect()
}

#[tauri::command]
fn get_children(state: State<AppState>, path: String) -> Result<Vec<scan::NodeView>, String> {
    let data = scan_for(&state, &path).ok_or("还没有扫描这个磁盘")?;
    scan::children(&data, &path).ok_or_else(|| format!("扫描结果里没有 {path}"))
}

#[tauri::command]
fn large_files(state: State<AppState>, root: String) -> Vec<scan::LargeFile> {
    scan_for(&state, &root)
        .map(|d| d.large.clone())
        .unwrap_or_default()
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct AnalyzeProgress {
    done: usize,
    total: usize,
    current: String,
}

#[tauri::command]
async fn analyze_cleanup(app: AppHandle) -> Result<Vec<rules::CleanItem>, String> {
    let emitter = app.clone();
    let planned = tauri::async_runtime::spawn_blocking(move || {
        rules::analyze(|done, total, current| {
            let _ = emitter.emit(
                "analyze-progress",
                AnalyzeProgress {
                    done,
                    total,
                    current: current.to_string(),
                },
            );
        })
    })
    .await
    .map_err(|e| e.to_string())?;
    let items: Vec<rules::CleanItem> = planned.iter().map(|p| p.item.clone()).collect();
    let state = app.state::<AppState>();
    *state.plans.lock().unwrap() = planned
        .into_iter()
        .map(|p| (p.item.id.clone(), p))
        .collect();
    Ok(items)
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct CleanResult {
    id: String,
    title: String,
    outcome: clean::Outcome,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct CleanProgress {
    id: String,
    done: usize,
    total: usize,
}

/// Cleans the items with these ids from the last analysis. Ids it does not know, and guide-only
/// items, are ignored.
#[tauri::command]
async fn run_cleanup(app: AppHandle, ids: Vec<String>) -> Result<Vec<CleanResult>, String> {
    let jobs: Vec<(String, String, Vec<rules::Target>)> = {
        let state = app.state::<AppState>();
        let plans = state.plans.lock().unwrap();
        ids.iter()
            .filter_map(|id| plans.get(id))
            .filter(|p| !p.item.guide_only)
            .map(|p| (p.item.id.clone(), p.item.title.clone(), p.targets.clone()))
            .collect()
    };
    let emitter = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let total = jobs.len();
        jobs.into_iter()
            .enumerate()
            .map(|(i, (id, title, targets))| {
                let _ = emitter.emit(
                    "clean-progress",
                    CleanProgress {
                        id: id.clone(),
                        done: i,
                        total,
                    },
                );
                let mut outcome = clean::Outcome::default();
                for t in &targets {
                    let o = clean::execute(t);
                    outcome.freed += o.freed;
                    outcome.removed_files += o.removed_files;
                    outcome.skipped_files += o.skipped_files;
                    if outcome.error.is_none() {
                        outcome.error = o.error;
                    }
                }
                CleanResult { id, title, outcome }
            })
            .collect()
    })
    .await
    .map_err(|e| e.to_string())
}

/// Files or folders picked by hand (large files, Downloads): to the Recycle Bin, or for good.
#[tauri::command]
async fn delete_files(paths: Vec<String>, permanent: bool) -> Result<clean::Outcome, String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    tauri::async_runtime::spawn_blocking(move || clean::remove_user_paths(&paths, permanent))
        .await
        .map_err(|e| e.to_string())
}

/// Every saved scan of `root`, oldest first.
#[tauri::command]
fn scan_history(app: AppHandle, root: String) -> Vec<scan::Summary> {
    history::list(&history_dir(&app), &root)
}

#[tauri::command]
fn scan_diff(
    app: AppHandle,
    root: String,
    base_at: u64,
    current_at: u64,
) -> Result<history::Diff, String> {
    history::diff(&history_dir(&app), &root, base_at, current_at)
}

#[tauri::command]
async fn analyze_downloads(app: AppHandle) -> Result<Vec<downloads::DownloadItem>, String> {
    let known: Vec<String> = app
        .state::<AppState>()
        .apps
        .lock()
        .unwrap()
        .values()
        .map(|e| e.info.name.clone())
        .collect();
    tauri::async_runtime::spawn_blocking(move || {
        // The app list is reused when the apps page has loaded it; otherwise the registry and the
        // last known Store list do, without waiting for PowerShell.
        let installed = if known.is_empty() {
            apps::names_quick()
        } else {
            known
        };
        downloads::analyze(&installed)
    })
    .await
    .map_err(|e| e.to_string())
}

/// Starts looking for duplicates of at least `min_size` bytes. Progress arrives as
/// `dupes-progress`, the groups as `dupes-done`.
#[tauri::command]
fn find_duplicates(app: AppHandle, state: State<AppState>, min_size: u64) {
    let ctx = Arc::new(dupes::DupeCtx::default());
    if let Some(previous) = state.dupes_running.lock().unwrap().replace(ctx.clone()) {
        previous.cancel.store(true, Ordering::Relaxed);
    }
    let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let (app, ctx, done) = (app.clone(), ctx.clone(), done.clone());
        std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                let _ = app.emit("dupes-progress", ctx.progress());
                std::thread::sleep(Duration::from_millis(150));
            }
        });
    }
    std::thread::spawn(move || {
        let groups = dupes::find(min_size, &ctx);
        done.store(true, Ordering::Relaxed);
        let state = app.state::<AppState>();
        let mut running = state.dupes_running.lock().unwrap();
        if !running
            .as_ref()
            .map(|c| Arc::ptr_eq(c, &ctx))
            .unwrap_or(false)
        {
            return;
        }
        *running = None;
        drop(running);
        *state.dupes.lock().unwrap() = groups.clone();
        let _ = app.emit("dupes-done", groups);
    });
}

#[tauri::command]
fn cancel_duplicates(state: State<AppState>) {
    if let Some(ctx) = state.dupes_running.lock().unwrap().as_ref() {
        ctx.cancel.store(true, Ordering::Relaxed);
    }
}

/// Deletes copies from the last duplicate search. Refused outright if any group would be left with
/// no copy at all, or if a path is not one of the duplicates found.
#[tauri::command]
async fn delete_duplicates(
    app: AppHandle,
    paths: Vec<String>,
    permanent: bool,
) -> Result<clean::Outcome, String> {
    let groups = app.state::<AppState>().dupes.lock().unwrap().clone();
    let picked: std::collections::HashSet<String> =
        paths.iter().map(|p| p.to_lowercase()).collect();
    let known: std::collections::HashSet<String> = groups
        .iter()
        .flat_map(|g| g.files.iter().map(|f| f.path.to_lowercase()))
        .collect();
    if let Some(stray) = picked.iter().find(|p| !known.contains(*p)) {
        return Err(format!("{stray} 不在重复文件结果里"));
    }
    if let Some(g) = groups.iter().find(|g| {
        g.files
            .iter()
            .all(|f| picked.contains(&f.path.to_lowercase()))
    }) {
        return Err(format!("「{}」的所有副本都被选中了，至少要留一份", g.name));
    }
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    let out =
        tauri::async_runtime::spawn_blocking(move || clean::remove_user_paths(&paths, permanent))
            .await
            .map_err(|e| e.to_string())?;
    // Forget what is gone, so a second request cannot pick the last remaining copy.
    let state = app.state::<AppState>();
    let mut stored = state.dupes.lock().unwrap();
    for g in stored.iter_mut() {
        g.files.retain(|f| {
            !picked.contains(&f.path.to_lowercase()) || std::path::Path::new(&f.path).exists()
        });
    }
    stored.retain(|g| g.files.len() > 1);
    Ok(out)
}

/// Sizes measured this recently are shown as they are, not measured again.
const SIZES_FRESH_SECS: u64 = 30 * 60;
const SIZES_CACHE: &str = "app-sizes.json";

#[derive(Serialize, serde::Deserialize, Clone)]
struct SizeRecord {
    at: u64,
    measured: apps::Measured,
}

/// Remembered sizes, shared by every measuring pass (a list and a background refresh can overlap).
static SIZES: Mutex<Option<HashMap<String, SizeRecord>>> = Mutex::new(None);

fn with_sizes<T>(f: impl FnOnce(&mut HashMap<String, SizeRecord>) -> T) -> T {
    let mut s = SIZES.lock().unwrap();
    f(s.get_or_insert_with(|| cache::load(SIZES_CACHE).unwrap_or_default()))
}

fn apply_size(info: &mut apps::AppInfo, m: &apps::Measured) {
    info.install_size = m.install_size;
    info.data_size = Some(m.data_size);
    info.data_dirs = m.data_dirs.clone();
    info.last_used = info.last_used.max(m.last_used);
    info.measured = true;
}

/// Takes a fresh app list: fills in remembered sizes, keeps it for the other commands, and in the
/// background measures what is not known (or, with `remeasure_old`, older than half an hour) and
/// gathers icons. Returns what the page shows right away.
fn adopt_apps(
    app: &AppHandle,
    mut entries: Vec<apps::AppEntry>,
    remeasure_old: bool,
) -> Vec<apps::AppInfo> {
    let now = cache::now();
    let mut stale = Vec::new();
    with_sizes(|sizes| {
        for e in &mut entries {
            match sizes.get(&e.info.id) {
                Some(r) => {
                    apply_size(&mut e.info, &r.measured);
                    if remeasure_old && now.saturating_sub(r.at) > SIZES_FRESH_SECS {
                        stale.push(e.clone());
                    }
                }
                None => stale.push(e.clone()),
            }
        }
    });
    let infos: Vec<apps::AppInfo> = entries.iter().map(|e| e.info.clone()).collect();
    *app.state::<AppState>().apps.lock().unwrap() = entries
        .iter()
        .map(|e| (e.info.id.clone(), e.clone()))
        .collect();

    if !stale.is_empty() {
        let app = app.clone();
        std::thread::spawn(move || {
            use rayon::prelude::*;
            // Biggest registered size first, so the rows most worth looking at fill in first.
            stale.sort_by_key(|e| std::cmp::Reverse(e.info.estimated_size));
            util::passive_pool().install(|| {
                stale.par_iter().for_each(|entry| {
                    let m = apps::measure_app(entry);
                    let state = app.state::<AppState>();
                    if let Some(stored) = state.apps.lock().unwrap().get_mut(&m.id) {
                        apply_size(&mut stored.info, &m);
                    }
                    with_sizes(|s| {
                        s.insert(
                            m.id.clone(),
                            SizeRecord {
                                at: cache::now(),
                                measured: m.clone(),
                            },
                        );
                    });
                    let _ = app.emit("app-measured", m);
                })
            });
            with_sizes(|s| cache::save(SIZES_CACHE, &*s));
        });
    }
    {
        let app = app.clone();
        std::thread::spawn(move || {
            let icons = apps::icons(&entries);
            let _ = app.emit("app-icons", icons);
        });
    }
    infos
}

/// Every installed app. The Store part comes from the last answer when there is one (asking
/// PowerShell takes ~3 s) and is asked again in the background; if it changed, the new list
/// arrives as `apps-updated`. Sizes follow as `app-measured`, icons as `app-icons`.
#[tauri::command]
async fn list_apps(app: AppHandle) -> Result<Vec<apps::AppInfo>, String> {
    let cached = apps::cached_store_json();
    let had_cache = cached.is_some();
    let entries = tauri::async_runtime::spawn_blocking(move || {
        let json = cached.or_else(|| {
            let j = apps::query_store_json();
            if let Some(j) = &j {
                apps::save_store_json(j);
            }
            j
        });
        apps::list_with(json.map(|j| apps::store_apps_from(&j)).unwrap_or_default())
    })
    .await
    .map_err(|e| e.to_string())?;
    let infos = adopt_apps(&app, entries, true);

    if had_cache {
        let app = app.clone();
        std::thread::spawn(move || {
            let Some(fresh) = apps::query_store_json() else {
                return;
            };
            if apps::cached_store_json().as_deref() == Some(fresh.as_str()) {
                return;
            }
            apps::save_store_json(&fresh);
            let entries = apps::list_with(apps::store_apps_from(&fresh));
            let infos = adopt_apps(&app, entries, false);
            let _ = app.emit("apps-updated", infos);
        });
    }
    Ok(infos)
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct UninstallResult {
    id: String,
    removed: bool,
    error: Option<String>,
    leftovers: Vec<rules::Detail>,
}

/// Runs the app's own uninstaller, then waits (up to ten minutes) for its registry entry to go.
/// Many uninstallers relaunch themselves from a temp copy and exit at once, so the uninstaller
/// exiting says little; the entry disappearing is what "uninstalled" means. The result, with any
/// leftovers, arrives as `app-uninstall`.
#[tauri::command]
fn uninstall_app(app: AppHandle, state: State<AppState>, id: String) -> Result<(), String> {
    let entry = state
        .apps
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or("找不到这个应用，请刷新列表")?;
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    state
        .uninstall_waits
        .lock()
        .unwrap()
        .insert(id.clone(), stop.clone());
    std::thread::spawn(move || {
        let launched = apps::run_uninstaller(&entry);
        let deadline = std::time::Instant::now() + Duration::from_secs(600);
        let mut removed = !apps::still_installed(&entry.source);
        while !removed && !stop.load(Ordering::Relaxed) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_secs(2));
            removed = !apps::still_installed(&entry.source);
        }
        let leftovers = if removed {
            apps::leftovers(&entry)
        } else {
            Vec::new()
        };
        app.state::<AppState>()
            .uninstall_waits
            .lock()
            .unwrap()
            .remove(&id);
        let _ = app.emit(
            "app-uninstall",
            UninstallResult {
                id,
                removed,
                error: launched.err(),
                leftovers,
            },
        );
    });
    Ok(())
}

/// The user says the uninstaller is closed: stop waiting and report what is there now.
#[tauri::command]
fn stop_uninstall_wait(state: State<AppState>, id: String) {
    if let Some(flag) = state.uninstall_waits.lock().unwrap().get(&id) {
        flag.store(true, Ordering::Relaxed);
    }
}

/// Deletes leftovers of an uninstalled app. Only folders this app was found to own are accepted,
/// and each still goes through the same whitelist as every other delete.
#[tauri::command]
async fn clean_app_leftovers(
    app: AppHandle,
    id: String,
    paths: Vec<String>,
) -> Result<clean::Outcome, String> {
    let entry = app
        .state::<AppState>()
        .apps
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or("找不到这个应用")?;
    let owned: Vec<String> = entry
        .info
        .data_dirs
        .iter()
        .map(|d| d.path.to_lowercase())
        .chain(entry.info.install_location.iter().map(|l| l.to_lowercase()))
        .collect();
    tauri::async_runtime::spawn_blocking(move || {
        let mut out = clean::Outcome::default();
        for p in paths {
            if !owned.contains(&p.to_lowercase()) {
                continue;
            }
            let o = clean::execute(&rules::Target::Dir {
                path: PathBuf::from(&p),
            });
            out.freed += o.freed;
            out.removed_files += o.removed_files;
            out.skipped_files += o.skipped_files;
            if out.error.is_none() {
                out.error = o.error;
            }
        }
        out
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_startup() -> Result<Vec<startup::StartupItem>, String> {
    tauri::async_runtime::spawn_blocking(startup::list)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_startup_enabled(id: String, enabled: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || startup::set_enabled(&id, enabled))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn perf_sample(state: State<AppState>) -> Result<perf::Sample, String> {
    let mut m = state.perf.lock().unwrap();
    if m.is_none() {
        *m = perf::Monitor::new();
    }
    m.as_mut()
        .map(|m| m.sample())
        .ok_or_else(|| "读不到性能计数器".into())
}

#[tauri::command]
async fn perf_processes(app: AppHandle) -> Result<Vec<perf::ProcGroup>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let mut m = state.procs.lock().unwrap();
        if m.is_none() {
            *m = perf::ProcMonitor::new();
        }
        m.as_mut()
            .map(|m| m.sample())
            .ok_or_else(|| "读不到进程信息".to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn perf_hardware(app: AppHandle) -> Result<perf::HardwareInfo, String> {
    if let Some(hw) = app.state::<AppState>().hardware.lock().unwrap().clone() {
        return Ok(hw);
    }
    // Reading the hardware takes ~9 s of PowerShell; show the last answer and re-read it once per
    // run in the background (an IP address or a new disk can change), sent as `perf-hardware`.
    if let Some(hw) = cache::load::<perf::HardwareInfo>("hardware.json") {
        *app.state::<AppState>().hardware.lock().unwrap() = Some(hw.clone());
        let app = app.clone();
        std::thread::spawn(move || {
            let fresh = perf::hardware();
            cache::save("hardware.json", &fresh);
            *app.state::<AppState>().hardware.lock().unwrap() = Some(fresh.clone());
            let _ = app.emit("perf-hardware", fresh);
        });
        return Ok(hw);
    }
    let hw = tauri::async_runtime::spawn_blocking(perf::hardware)
        .await
        .map_err(|e| e.to_string())?;
    cache::save("hardware.json", &hw);
    *app.state::<AppState>().hardware.lock().unwrap() = Some(hw.clone());
    Ok(hw)
}

#[tauri::command]
async fn file_details(
    paths: Vec<String>,
) -> Result<HashMap<String, filemeta::FileDetails>, String> {
    tauri::async_runtime::spawn_blocking(move || filemeta::details(&paths))
        .await
        .map_err(|e| e.to_string())
}

/// Leaving the performance page: close the counters instead of keeping them open.
#[tauri::command]
fn perf_stop(state: State<AppState>) {
    *state.perf.lock().unwrap() = None;
    *state.procs.lock().unwrap() = None;
}

#[tauri::command]
fn end_process(name: String) -> Result<u32, String> {
    perf::end_group(&name)
}

/// A page the window should open on, left by the tray when it had to build a new window.
#[tauri::command]
fn take_pending_page(state: State<tray::TrayState<tauri::Wry>>) -> Option<String> {
    state.pending_page.lock().unwrap().take()
}

/// The answer to 「关闭窗口后」: keep running in the tray, or quit. Remembered.
#[tauri::command]
fn set_close_to_tray(
    app: AppHandle,
    state: State<tray::TrayState<tauri::Wry>>,
    on: bool,
) -> Result<(), String> {
    state.set_close_to_tray(&data_dir(&app), on)
}

/// Closes the window the way the close button does, now that the user has chosen.
#[tauri::command]
fn close_main_window(app: AppHandle) {
    if let Some(w) = app.get_webview_window(tray::MAIN) {
        let _ = w.close();
    }
}

#[tauri::command]
fn reveal_path(path: String) -> Result<(), String> {
    sys::reveal(&PathBuf::from(path))
}

/// While minimized, asks WebView2 to keep as little memory as it can (it trims caches and
/// working sets); back to normal when the window comes back.
static WEBVIEW_LOW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn webview_memory_low(window: &tauri::WebviewWindow, low: bool) {
    if WEBVIEW_LOW.swap(low, Ordering::Relaxed) == low {
        return;
    }
    let _ = window.with_webview(move |wv| unsafe {
        use webview2_com::Microsoft::Web::WebView2::Win32::{
            ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
            COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
        };
        use windows_core::Interface;
        let Ok(core) = wv.controller().CoreWebView2() else {
            return;
        };
        if let Ok(core) = core.cast::<ICoreWebView2_19>() {
            let _ = core.SetMemoryUsageTargetLevel(if low {
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
            } else {
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
            });
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    util::init_worker_pool();
    tauri::Builder::default()
        // Launching DiskMate again (say, from the Start menu while it sits in the tray) brings
        // back the running one instead of starting a second.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main(app, None);
        }))
        .setup(|app| {
            let dir = data_dir(app.handle());
            cache::init(dir.clone());
            tray::setup(app.handle(), &dir)?;
            Ok(())
        })
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::Resized(_) => {
                let minimized = window.is_minimized().unwrap_or(false);
                if let Some(w) = window.app_handle().get_webview_window(window.label()) {
                    webview_memory_low(&w, minimized);
                }
            }
            tauri::WindowEvent::CloseRequested { api, .. } if window.label() == tray::MAIN => {
                let app = window.app_handle();
                let choice = app
                    .state::<tray::TrayState<tauri::Wry>>()
                    .settings
                    .lock()
                    .unwrap()
                    .close_to_tray;
                match choice {
                    // Not asked yet: the page asks, then closes the window itself.
                    None => {
                        api.prevent_close();
                        let _ = window.emit("close-asked", ());
                    }
                    Some(false) => app.exit(0),
                    // The window (and its WebView2) goes; the app stays, see `prevent_exit`.
                    Some(true) => {}
                }
            }
            tauri::WindowEvent::Destroyed => WEBVIEW_LOW.store(false, Ordering::Relaxed),
            _ => {}
        })
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            list_drives,
            start_scan,
            cancel_scan,
            scan_summaries,
            get_children,
            large_files,
            analyze_cleanup,
            run_cleanup,
            delete_files,
            scan_history,
            scan_diff,
            analyze_downloads,
            find_duplicates,
            cancel_duplicates,
            delete_duplicates,
            migration_candidates,
            migration_status,
            migrate_folder,
            list_vdisks,
            shutdown_wsl,
            compact_vdisk,
            list_apps,
            uninstall_app,
            stop_uninstall_wait,
            clean_app_leftovers,
            list_startup,
            set_startup_enabled,
            perf_sample,
            perf_processes,
            perf_hardware,
            file_details,
            end_process,
            perf_stop,
            take_pending_page,
            set_close_to_tray,
            close_main_window,
            reveal_path
        ])
        .build(tauri::generate_context!())
        .expect("error while running DiskMate")
        .run(|app, event| {
            // The last window closing asks to end the app; kept in the tray, it stays.
            if let tauri::RunEvent::ExitRequested {
                code: None, api, ..
            } = event
            {
                let keep = app
                    .state::<tray::TrayState<tauri::Wry>>()
                    .settings
                    .lock()
                    .unwrap()
                    .close_to_tray
                    == Some(true);
                if keep {
                    api.prevent_exit();
                }
            }
        });
}
