//! The tray icon, and what closing the window means.
//!
//! Kept in the tray, DiskMate closes its window for real — the WebView2 engine behind it is most of
//! the memory the app uses — and builds a fresh one when the icon is clicked. What lives in the
//! Rust side (finished scans, running jobs) carries over.

use crate::util::{self, display};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Runtime, WebviewWindowBuilder};

pub const MAIN: &str = "main";
const TRAY_ID: &str = "diskmate";

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// What the close button does: true keeps DiskMate in the tray, false quits. None until the
    /// user has been asked once.
    pub close_to_tray: Option<bool>,
}

fn settings_file(dir: &Path) -> PathBuf {
    dir.join("settings.json")
}

pub fn load_settings(dir: &Path) -> Settings {
    std::fs::read(settings_file(dir))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save_settings(dir: &Path, s: &Settings) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    std::fs::write(
        settings_file(dir),
        serde_json::to_vec_pretty(s).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

/// Shared between the tray, the window events and the commands.
pub struct TrayState<R: Runtime> {
    pub settings: Mutex<Settings>,
    /// A page the next window should open on (the tray's 「智能清理」 with no window open).
    pub pending_page: Mutex<Option<String>>,
    space: MenuItem<R>,
    keep_in_tray: CheckMenuItem<R>,
}

impl<R: Runtime> TrayState<R> {
    pub fn set_close_to_tray(&self, dir: &Path, on: bool) -> Result<(), String> {
        let mut s = self.settings.lock().unwrap();
        s.close_to_tray = Some(on);
        let _ = self.keep_in_tray.set_checked(on);
        save_settings(dir, &s)
    }
}

/// `C: 可用 91.1 GB / 共 476 GB`
fn space_line() -> String {
    let root = display(&util::system_drive());
    match crate::sys::drive_space(&root) {
        Some((total, free)) => format!(
            "{} 可用 {} / 共 {}",
            root.trim_end_matches('\\'),
            gb(free),
            gb(total)
        ),
        None => "读不到系统盘空间".into(),
    }
}

fn gb(b: u64) -> String {
    let g = b as f64 / 1073741824.0;
    if g >= 100.0 {
        format!("{g:.0} GB")
    } else {
        format!("{g:.1} GB")
    }
}

/// Brings the window back, building a new one if it was closed to the tray, and optionally
/// switches it to `page`.
pub fn show_main<R: Runtime>(app: &AppHandle<R>, page: Option<&str>) {
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        if let Some(p) = page {
            let _ = app.emit_to(MAIN, "navigate", p);
        }
        return;
    }
    let state = app.state::<TrayState<R>>();
    *state.pending_page.lock().unwrap() = page.map(str::to_string);
    let Some(config) = app.config().app.windows.first().cloned() else {
        return;
    };
    if let Ok(b) = WebviewWindowBuilder::from_config(app, &config) {
        if let Ok(w) = b.build() {
            let _ = w.set_focus();
        }
    }
}

pub fn setup<R: Runtime>(app: &AppHandle<R>, data_dir: &Path) -> tauri::Result<()> {
    let settings = load_settings(data_dir);
    let open = MenuItem::with_id(app, "open", "打开 DiskMate", true, None::<&str>)?;
    let space = MenuItem::with_id(app, "space", space_line(), false, None::<&str>)?;
    let cleanup = MenuItem::with_id(app, "cleanup", "智能清理", true, None::<&str>)?;
    let perf = MenuItem::with_id(app, "perf", "性能", true, None::<&str>)?;
    let keep_in_tray = CheckMenuItem::with_id(
        app,
        "keep-in-tray",
        "关闭窗口时保留在托盘",
        true,
        settings.close_to_tray != Some(false),
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "退出 DiskMate", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &open,
            &space,
            &PredefinedMenuItem::separator(app)?,
            &cleanup,
            &perf,
            &PredefinedMenuItem::separator(app)?,
            &keep_in_tray,
            &quit,
        ],
    )?;
    app.manage(TrayState {
        settings: Mutex::new(settings),
        pending_page: Mutex::new(None),
        space,
        keep_in_tray,
    });

    let dir = data_dir.to_path_buf();
    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(format!("DiskMate\n{}", space_line()))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "open" => show_main(app, None),
            "cleanup" => show_main(app, Some("cleanup")),
            "perf" => show_main(app, Some("perf")),
            "keep-in-tray" => {
                let state = app.state::<TrayState<R>>();
                // The check mark has already flipped itself.
                let on = state.keep_in_tray.is_checked().unwrap_or(true);
                let _ = state.set_close_to_tray(&dir, on);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } => show_main(tray.app_handle(), None),
            // Free space is read when the pointer arrives, so the tooltip and menu are current
            // without a timer running in the background.
            TrayIconEvent::Enter { .. } => {
                let line = space_line();
                let _ = tray.set_tooltip(Some(format!("DiskMate\n{line}")));
                let _ = tray.app_handle().state::<TrayState<R>>().space.set_text(line);
            }
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}
