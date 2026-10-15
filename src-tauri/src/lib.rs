mod apps;
mod clean;
mod rules;
mod scan;
mod sys;
mod util;

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

#[tauri::command]
async fn recycle_files(paths: Vec<String>) -> Result<clean::Outcome, String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    tauri::async_runtime::spawn_blocking(move || clean::recycle(&paths))
        .await
        .map_err(|e| e.to_string())
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
            recycle_files,
            list_apps,
            uninstall_app,
            stop_uninstall_wait,
            clean_app_leftovers,
            reveal_path
        ])
        .run(tauri::generate_context!())
        .expect("error while running DiskMate");
}
