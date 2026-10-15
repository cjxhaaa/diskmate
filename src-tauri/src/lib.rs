mod apps;
mod clean;
mod downloads;
mod dupes;
mod history;
mod migrate;
mod rules;
mod scan;
mod sys;
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
        // The app list is reused when the apps page has loaded it; otherwise read it here.
        let installed = if known.is_empty() {
            apps::list().into_iter().map(|e| e.info.name).collect()
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

/// Every installed app, sizes not yet known. Sizes follow one app at a time as `app-measured`,
/// icons all at once as `app-icons`.
#[tauri::command]
async fn list_apps(app: AppHandle) -> Result<Vec<apps::AppInfo>, String> {
    let entries = tauri::async_runtime::spawn_blocking(apps::list)
        .await
        .map_err(|e| e.to_string())?;
    let infos: Vec<apps::AppInfo> = entries.iter().map(|e| e.info.clone()).collect();
    {
        let state = app.state::<AppState>();
        *state.apps.lock().unwrap() = entries
            .iter()
            .map(|e| (e.info.id.clone(), e.clone()))
            .collect();
    }
    {
        let (app, entries) = (app.clone(), entries.clone());
        std::thread::spawn(move || {
            use rayon::prelude::*;
            // Biggest registered size first, so the rows most worth looking at fill in first.
            let mut order = entries;
            order.sort_by_key(|e| std::cmp::Reverse(e.info.estimated_size));
            order.par_iter().for_each(|entry| {
                let m = apps::measure_app(entry);
                let state = app.state::<AppState>();
                if let Some(stored) = state.apps.lock().unwrap().get_mut(&m.id) {
                    stored.info.install_size = m.install_size;
                    stored.info.data_size = Some(m.data_size);
                    stored.info.data_dirs = m.data_dirs.clone();
                    stored.info.last_used = m.last_used;
                    stored.info.measured = true;
                }
                let _ = app.emit("app-measured", m);
            });
        });
    }
    std::thread::spawn(move || {
        let icons = apps::icons(&entries);
        let _ = app.emit("app-icons", icons);
    });
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
fn reveal_path(path: String) -> Result<(), String> {
    sys::reveal(&PathBuf::from(path))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
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
            reveal_path
        ])
        .run(tauri::generate_context!())
        .expect("error while running DiskMate");
}
