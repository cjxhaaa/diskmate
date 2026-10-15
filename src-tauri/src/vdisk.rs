//! Virtual disks (.vhdx) that only ever grow: WSL distros, Docker, VM images.
//!
//! A dynamic VHDX expands as the guest writes and never shrinks by itself, even after the guest
//! deletes everything. Compacting hands the unused blocks back. It needs the disk detached, so
//! whatever runs it has to be stopped first, and it needs administrator rights (diskpart).

use crate::util::{self, display, home, local_appdata, roaming_appdata};
use serde::Serialize;
use std::path::{Path, PathBuf};
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VDisk {
    pub path: String,
    pub size: u64,
    /// What it belongs to, in words: "WSL · Ubuntu", "Docker Desktop", "Claude 桌面应用".
    pub owner: String,
    /// `wsl` | `docker` | `claude` | `other`
    pub kind: &'static str,
    /// Open by something right now; it cannot be compacted until that stops.
    pub in_use: bool,
}

fn wsl_disks() -> Vec<(PathBuf, String)> {
    let Ok(lxss) = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Lxss")
    else {
        return Vec::new();
    };
    lxss.enum_keys()
        .flatten()
        .filter_map(|guid| {
            let k = lxss.open_subkey(&guid).ok()?;
            let base: String = k.get_value("BasePath").ok()?;
            let name: String = k.get_value("DistributionName").unwrap_or_default();
            let base = base.trim_start_matches(r"\\?\").to_string();
            let vhd = Path::new(&base).join("ext4.vhdx");
            vhd.is_file().then(|| (vhd, format!("WSL · {name}")))
        })
        .collect()
}

fn glob_vhdx(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth == 0 {
        return;
    }
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        let Ok(md) = e.metadata() else { continue };
        if util::is_link(&md) {
            continue;
        }
        if md.is_dir() {
            glob_vhdx(&p, depth - 1, out);
        } else if p
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("vhdx") || x.eq_ignore_ascii_case("vhd"))
        {
            out.push(p);
        }
    }
}

/// Every virtual disk worth listing: known owners first, then any other the last scan saw.
pub fn list(extra: &[String]) -> Vec<VDisk> {
    let mut found: Vec<(PathBuf, String, &'static str)> = wsl_disks()
        .into_iter()
        .map(|(p, o)| (p, o, "wsl"))
        .collect();

    let docker = local_appdata().join(r"Docker\wsl");
    let mut d = Vec::new();
    glob_vhdx(&docker, 3, &mut d);
    found.extend(
        d.into_iter()
            .map(|p| (p, "Docker Desktop".to_string(), "docker")),
    );

    let mut c = Vec::new();
    glob_vhdx(&roaming_appdata().join(r"Claude\vm_bundles"), 3, &mut c);
    for pkg in std::fs::read_dir(local_appdata().join("Packages"))
        .into_iter()
        .flatten()
        .flatten()
    {
        if pkg.file_name().to_string_lossy().starts_with("Claude_") {
            glob_vhdx(
                &pkg.path().join(r"LocalCache\Roaming\Claude\vm_bundles"),
                3,
                &mut c,
            );
        }
    }
    found.extend(
        c.into_iter()
            .map(|p| (p, "Claude 桌面应用".to_string(), "claude")),
    );

    for e in extra {
        let p = PathBuf::from(e);
        if p.extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("vhdx") || x.eq_ignore_ascii_case("vhd"))
        {
            found.push((p, "其他虚拟磁盘".to_string(), "other"));
        }
    }

    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<VDisk> = found
        .into_iter()
        .filter(|(p, _, _)| seen.insert(display(p).to_lowercase()))
        .filter_map(|(p, owner, kind)| {
            let size = util::file_len(&p)?;
            Some(VDisk {
                path: display(&p),
                size,
                owner,
                kind,
                in_use: in_use(&p),
            })
        })
        .collect();
    out.sort_by(|a, b| b.size.cmp(&a.size));
    out
}

/// Whether some process has the file open. Asking for it with no sharing fails if anyone does.
pub fn in_use(path: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(util::long_path(path))
        .is_err()
}

/// Has the distro tell its disk which blocks are free (fstrim). Without this a WSL disk has nothing
/// to give back: ext4 marks deleted files free in its own tables, but the blocks in the VHDX stay
/// written. Runs as root inside the distro, so no password is asked.
pub fn trim_wsl(distro: &str) -> Result<(), String> {
    let s = util::command("wsl.exe")
        .args(["-d", distro, "-u", "root", "--", "fstrim", "-av"])
        .status()
        .map_err(|e| e.to_string())?;
    s.success()
        .then_some(())
        .ok_or_else(|| format!("在 {distro} 里执行 fstrim 失败"))
}

/// The distro name in an owner label like "WSL · Ubuntu".
pub fn distro_of(owner: &str) -> Option<&str> {
    owner.strip_prefix("WSL · ").filter(|d| !d.is_empty())
}

/// Stops every WSL distro (and the WSL VM with them), which releases their disks.
pub fn shutdown_wsl() -> Result<(), String> {
    let s = util::command("wsl.exe")
        .arg("--shutdown")
        .status()
        .map_err(|e| e.to_string())?;
    s.success()
        .then_some(())
        .ok_or_else(|| "wsl --shutdown 失败".into())
}

/// Compacts one disk with diskpart, elevated. Returns the size before and after.
pub fn compact(path: &Path) -> Result<(u64, u64), String> {
    if !home().exists() || !path.is_file() {
        return Err("找不到这个虚拟磁盘".into());
    }
    if in_use(path) {
        return Err("这个虚拟磁盘正在使用，请先关闭用到它的程序".into());
    }
    let before = util::file_len(path).unwrap_or(0);
    let script = std::env::temp_dir().join("diskmate-compact.txt");
    let p = display(path);
    std::fs::write(
        &script,
        format!("select vdisk file=\"{p}\"\r\nattach vdisk readonly\r\ncompact vdisk\r\ndetach vdisk\r\nexit\r\n"),
    )
    .map_err(|e| e.to_string())?;
    let ps = format!(
        "Start-Process -FilePath diskpart.exe -ArgumentList '/s','\"{}\"' -Verb RunAs -Wait -WindowStyle Hidden",
        display(&script).replace('\'', "''")
    );
    let status = util::command("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
        .status()
        .map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&script);
    if !status.success() {
        return Err("没有获得管理员授权".into());
    }
    let after = util::file_len(path).unwrap_or(before);
    Ok((before, after))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_open_file_is_in_use() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("disk.vhdx");
        std::fs::write(&p, b"x").unwrap();
        assert!(!in_use(&p));
        let _held = std::fs::File::open(&p).unwrap();
        assert!(in_use(&p));
    }

    #[test]
    fn only_virtual_disks_from_the_scan_are_listed() {
        let dir = tempfile::tempdir().unwrap();
        let v = dir.path().join("a.vhdx");
        let iso = dir.path().join("b.iso");
        std::fs::write(&v, b"x").unwrap();
        std::fs::write(&iso, b"x").unwrap();
        let listed = list(&[display(&v), display(&iso)]);
        assert!(listed
            .iter()
            .any(|d| d.path == display(&v) && d.kind == "other"));
        assert!(!listed.iter().any(|d| d.path == display(&iso)));
    }
}
