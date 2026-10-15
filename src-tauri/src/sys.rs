//! Thin wrappers over the Windows APIs the app needs: drives, the Recycle Bin, hibernation.

use serde::Serialize;
use std::path::Path;
use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
};
use windows::Win32::UI::Shell::{SHEmptyRecycleBinW, SHQueryRecycleBinW, SHQUERYRBINFO};

const DRIVE_FIXED: u32 = 3;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DriveInfo {
    /// `C:\`
    pub root: String,
    pub label: String,
    pub total: u64,
    pub free: u64,
    pub is_system: bool,
}

pub fn drive_space(root: &str) -> Option<(u64, u64)> {
    let w = wide(root);
    let mut free = 0u64;
    let mut total = 0u64;
    unsafe {
        GetDiskFreeSpaceExW(PCWSTR(w.as_ptr()), Some(&mut free), Some(&mut total), None).ok()?;
    }
    Some((total, free))
}

pub fn list_drives() -> Vec<DriveInfo> {
    let mask = unsafe { GetLogicalDrives() };
    let system = crate::util::system_drive().to_string_lossy().to_uppercase();
    (0..26u8)
        .filter(|i| mask & (1 << i) != 0)
        .filter_map(|i| {
            let root = format!("{}:\\", (b'A' + i) as char);
            let w = wide(&root);
            if unsafe { GetDriveTypeW(PCWSTR(w.as_ptr())) } != DRIVE_FIXED {
                return None;
            }
            let (total, free) = drive_space(&root)?;
            let mut name = [0u16; 261];
            let label = unsafe {
                GetVolumeInformationW(PCWSTR(w.as_ptr()), Some(&mut name), None, None, None, None)
                    .ok()
                    .map(|_| {
                        let len = name.iter().position(|c| *c == 0).unwrap_or(0);
                        String::from_utf16_lossy(&name[..len])
                    })
                    .unwrap_or_default()
            };
            Some(DriveInfo {
                is_system: root.eq_ignore_ascii_case(&system),
                root,
                label,
                total,
                free,
            })
        })
        .collect()
}

/// Bytes and item count in the Recycle Bin across every drive.
pub fn recycle_bin_size() -> (u64, u64) {
    let mut info = SHQUERYRBINFO {
        cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32,
        ..Default::default()
    };
    match unsafe { SHQueryRecycleBinW(PCWSTR::null(), &mut info) } {
        Ok(()) => (info.i64Size.max(0) as u64, info.i64NumItems.max(0) as u64),
        Err(_) => (0, 0),
    }
}

pub fn empty_recycle_bin() -> Result<(), String> {
    const SHERB_NOCONFIRMATION: u32 = 0x1;
    const SHERB_NOPROGRESSUI: u32 = 0x2;
    const SHERB_NOSOUND: u32 = 0x4;
    let (bytes, _) = recycle_bin_size();
    if bytes == 0 {
        return Ok(());
    }
    unsafe {
        SHEmptyRecycleBinW(
            None,
            PCWSTR::null(),
            SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND,
        )
        .map_err(|e| e.message())
    }
}

pub fn hiberfil_path() -> std::path::PathBuf {
    crate::util::system_drive().join("hiberfil.sys")
}

/// Turns hibernation off, which makes Windows delete hiberfil.sys. Needs elevation, so this raises
/// the UAC prompt and waits for it; declining the prompt comes back as an error.
pub fn disable_hibernation() -> Result<(), String> {
    let status = crate::util::command("powershell.exe")
        .args([
            "-NoProfile",
            "-Command",
            "Start-Process -FilePath powercfg.exe -ArgumentList '/hibernate','off' -Verb RunAs -Wait -WindowStyle Hidden",
        ])
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("没有获得管理员授权".into())
    }
}

/// Opens Explorer with `path` selected (or opened, if it is a folder that should be browsed).
pub fn reveal(path: &Path) -> Result<(), String> {
    let mut cmd = std::process::Command::new("explorer.exe");
    if path.is_dir() {
        cmd.arg(path);
    } else {
        cmd.arg(format!("/select,{}", path.display()));
    }
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
}
