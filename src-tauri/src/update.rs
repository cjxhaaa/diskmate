//! Updates. A release publishes `latest.json` next to its installer; DiskMate reads it (at most
//! once a day, or when asked from the tray), and an installed copy downloads the new installer,
//! checks its signature against the public key built into this program, and runs it: the
//! installer shows a progress bar and opens the new version when done.
//!
//! A portable or Scoop copy is only told: replacing it with an installed copy would surprise its
//! user, and Scoop keeps its own record of what version is where.

use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

/// Where a release's page is, for copies that update by hand.
const RELEASES: &str = "https://github.com/cjxhaaa/diskmate/releases/latest";
/// An automatic check happens at most this often.
const CHECK_EVERY_SECS: u64 = 24 * 3600;
/// ...and not right at launch, when the window, the cleanup check and maybe a scan are starting.
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(20);
const LAST_CHECK_CACHE: &str = "update-check.json";

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum InstallKind {
    /// Installed with the setup program: updates itself.
    Installer,
    /// Unzipped somewhere: told where to download.
    Portable,
    /// Installed by Scoop: told to run `scoop update diskmate`.
    Scoop,
}

/// How this copy was installed, from where the program sits.
pub fn install_kind() -> InstallKind {
    let Ok(exe) = std::env::current_exe() else {
        return InstallKind::Portable;
    };
    kind_of(&exe)
}

fn kind_of(exe: &std::path::Path) -> InstallKind {
    let lower = exe.to_string_lossy().to_lowercase().replace('/', "\\");
    if lower.contains(r"\scoop\apps\") {
        InstallKind::Scoop
    } else if exe.with_file_name("uninstall.exe").exists() {
        // The setup program puts its uninstaller next to the app; a zip has none.
        InstallKind::Installer
    } else {
        InstallKind::Portable
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub current: String,
    /// The release notes, as written on the release page.
    pub notes: String,
    pub kind: InstallKind,
}

#[derive(Default)]
pub struct UpdateState {
    /// The newer release last found, ready to install.
    found: Mutex<Option<Update>>,
    /// A newer release the sidebar notes until the user updates or skips it. Kept here, and in
    /// the last-check cache, so a window built later or the next launch still shows it.
    available: Mutex<Option<UpdateInfo>>,
}

#[derive(Serialize, Deserialize, Default)]
struct LastCheck {
    at: u64,
    /// The newer release that check found, if any.
    #[serde(default)]
    found: Option<UpdateInfo>,
}

fn updater(app: &AppHandle) -> Result<tauri_plugin_updater::Updater, String> {
    #[allow(unused_mut)]
    let mut builder = app.updater_builder();
    // Debug builds only: point at a local test release signed with a test key.
    #[cfg(debug_assertions)]
    {
        if let Ok(url) = std::env::var("DISKMATE_UPDATE_ENDPOINT") {
            let url = url.parse().map_err(|e| format!("{e}"))?;
            builder = builder.endpoints(vec![url]).map_err(|e| e.to_string())?;
        }
        if let Ok(key) = std::env::var("DISKMATE_UPDATE_PUBKEY") {
            builder = builder.pubkey(key);
        }
    }
    builder.build().map_err(|e| e.to_string())
}

/// Asks whether a newer release exists. `Ok(None)`: this is the newest.
pub async fn check(app: &AppHandle) -> Result<Option<UpdateInfo>, String> {
    let update = updater(app)?
        .check()
        .await
        .map_err(|e| format!("检查更新失败：{e}"))?;
    let info = update.as_ref().map(|u| UpdateInfo {
        version: u.version.clone(),
        current: u.current_version.clone(),
        notes: u.body.clone().unwrap_or_default(),
        kind: install_kind(),
    });
    crate::cache::save(
        LAST_CHECK_CACHE,
        &LastCheck {
            at: crate::cache::now(),
            found: info.clone(),
        },
    );
    *app.state::<UpdateState>().found.lock().unwrap() = update;
    Ok(info)
}

/// The once-a-day check, started at launch. It never interrupts: what it finds is only noted in
/// the sidebar (the `update-available` event, and [`available`] for a window opened later), unless
/// the user chose to skip that version.
pub fn check_in_background(app: AppHandle, enabled: bool, skipped: Option<String>) {
    if !enabled {
        return;
    }
    let last = crate::cache::load::<LastCheck>(LAST_CHECK_CACHE).unwrap_or_default();
    // What the last check found still stands, until a new check or an update says otherwise:
    // shown at once, without going online. Found from this very version, or it is stale.
    if let Some(found) = last
        .found
        .filter(|f| f.current == env!("CARGO_PKG_VERSION"))
    {
        let kind = install_kind();
        note(&app, skipped.as_deref(), UpdateInfo { kind, ..found });
    }
    if crate::cache::now().saturating_sub(last.at) < CHECK_EVERY_SECS {
        return;
    }
    std::thread::spawn(move || {
        std::thread::sleep(FIRST_CHECK_DELAY);
        if let Ok(Some(info)) = tauri::async_runtime::block_on(check(&app)) {
            note(&app, skipped.as_deref(), info);
        }
    });
}

/// Puts a newer release in the sidebar, unless it is the one the user skipped.
fn note(app: &AppHandle, skipped: Option<&str>, info: UpdateInfo) {
    if skipped != Some(info.version.as_str()) {
        *app.state::<UpdateState>().available.lock().unwrap() = Some(info.clone());
        let _ = app.emit("update-available", info);
    }
}

/// The newer release the sidebar notes, for a window that opened after it was found.
pub fn available(app: &AppHandle) -> Option<UpdateInfo> {
    app.state::<UpdateState>().available.lock().unwrap().clone()
}

/// The user skipped this release: the sidebar stops noting it.
pub fn forget(app: &AppHandle) {
    *app.state::<UpdateState>().available.lock().unwrap() = None;
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Progress {
    downloaded: u64,
    total: Option<u64>,
    /// The download is verified and the installer is starting; DiskMate is about to close.
    installing: bool,
}

/// Downloads the release found by [`check`], verifies it, and runs its installer, which closes
/// DiskMate and opens the new version. Only for installed copies.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    if install_kind() != InstallKind::Installer {
        return Err("这个 DiskMate 不是用安装包装的，请手动下载新版本".into());
    }
    // Shown from the last-check cache after a restart, the release has not been fetched yet.
    if app.state::<UpdateState>().found.lock().unwrap().is_none() {
        check(app).await?;
    }
    let update = app
        .state::<UpdateState>()
        .found
        .lock()
        .unwrap()
        .clone()
        .ok_or("已经是最新版本了")?;
    let mut downloaded = 0u64;
    let mut last_sent = 0u64;
    let emitter = app.clone();
    let finisher = app.clone();
    update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;
                // A progress event per network chunk would be thousands; one per 256 KB is plenty.
                if downloaded - last_sent >= 256 * 1024 || Some(downloaded) == total {
                    last_sent = downloaded;
                    let _ = emitter.emit(
                        "update-progress",
                        Progress {
                            downloaded,
                            total,
                            installing: false,
                        },
                    );
                }
            },
            move || {
                let _ = finisher.emit(
                    "update-progress",
                    Progress {
                        downloaded: 0,
                        total: None,
                        installing: true,
                    },
                );
            },
        )
        .await
        .map_err(|e| format!("更新失败：{e}"))
}

/// Opens the latest release's page in the default browser.
pub fn open_releases() -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::{w, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let url: Vec<u16> = std::ffi::OsStr::new(RELEASES)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let r = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(url.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    if r.0 as isize > 32 {
        Ok(())
    } else {
        Err(format!("没能打开浏览器（{}）", r.0 as isize))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_kind_follows_where_the_program_is() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("diskmate.exe");
        std::fs::write(&exe, b"").unwrap();
        assert_eq!(kind_of(&exe), InstallKind::Portable);
        std::fs::write(dir.path().join("uninstall.exe"), b"").unwrap();
        assert_eq!(kind_of(&exe), InstallKind::Installer);
        assert_eq!(
            kind_of(std::path::Path::new(
                r"C:\Users\me\scoop\apps\diskmate\current\DiskMate.exe"
            )),
            InstallKind::Scoop
        );
    }
}
