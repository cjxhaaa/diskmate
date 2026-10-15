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
    scan: Mutex<Option<Arc<scan::ScanData>>>,
    scanning: Mutex<Option<Arc<scan::ScanCtx>>>,
    plans: Mutex<HashMap<String, rules::Planned>>,
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
        *state.scan.lock().unwrap() = Some(Arc::new(data));
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

#[tauri::command]
fn get_children(state: State<AppState>, path: String) -> Result<Vec<scan::NodeView>, String> {
    let data = state.scan.lock().unwrap().clone().ok_or("还没有扫描结果")?;
    scan::children(&data, &path).ok_or_else(|| format!("扫描结果里没有 {path}"))
}

#[tauri::command]
fn large_files(state: State<AppState>) -> Vec<scan::LargeFile> {
    state
        .scan
        .lock()
        .unwrap()
        .as_ref()
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
            get_children,
            large_files,
            analyze_cleanup,
            run_cleanup,
            recycle_files,
            reveal_path
        ])
        .run(tauri::generate_context!())
        .expect("error while running DiskMate");
}
