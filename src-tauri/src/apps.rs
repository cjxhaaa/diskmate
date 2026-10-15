//! Installed applications: what is installed, how much each one takes (the program and the data it
//! keeps elsewhere), when it was last used, and uninstalling through its own uninstaller.

use crate::rules::Detail;
use crate::util::{
    self, display, home, local_appdata, measure, newest_mtime, roaming_appdata, unix_secs,
};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use winreg::enums::{
    HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
};
use winreg::RegKey;

const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub publisher: String,
    /// `win32` or `store`.
    pub kind: &'static str,
    pub install_date: Option<String>,
    pub install_location: Option<String>,
    /// What the app registered about its own size, in bytes. Often missing or stale.
    pub estimated_size: u64,
    /// Measured size of the install folder; None until measured, or when there is no folder to measure.
    pub install_size: Option<u64>,
    pub data_size: Option<u64>,
    pub data_dirs: Vec<Detail>,
    /// Best guess at the last time it was used, as Unix seconds.
    pub last_used: Option<u64>,
    /// `runtime`, `driver`, `dev`, `store`, `user`.
    pub tags: Vec<&'static str>,
    pub measured: bool,
}

#[derive(Clone, Debug)]
pub enum Uninstaller {
    /// The app's own uninstaller: program and arguments.
    Exe {
        program: String,
        args: String,
    },
    /// `msiexec /x {product}` — interactive.
    Msi {
        product: String,
    },
    Appx {
        full_name: String,
    },
}

#[derive(Clone, Debug)]
pub enum Source {
    Registry { hive: Hive, view: u32, key: String },
    Appx { name: String },
}

#[derive(Clone, Copy, Debug)]
pub enum Hive {
    Machine,
    User,
}

#[derive(Clone, Debug)]
pub struct AppEntry {
    pub info: AppInfo,
    pub uninstaller: Uninstaller,
    pub source: Source,
    /// Executable used for the icon.
    pub icon_source: Option<String>,
    /// Store apps carry their own logo image.
    pub logo: Option<PathBuf>,
}

// ---------------------------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------------------------

fn hive_key(hive: Hive) -> RegKey {
    RegKey::predef(match hive {
        Hive::Machine => HKEY_LOCAL_MACHINE,
        Hive::User => HKEY_CURRENT_USER,
    })
}

fn open_uninstall(hive: Hive, view: u32) -> Option<RegKey> {
    hive_key(hive)
        .open_subkey_with_flags(UNINSTALL, KEY_READ | view)
        .ok()
}

/// Whether the registry entry for `source` still exists — the uninstaller's own definition of
/// "still installed".
pub fn still_installed(source: &Source) -> bool {
    match source {
        Source::Registry { hive, view, key } => open_uninstall(*hive, *view)
            .map(|u| u.open_subkey_with_flags(key, KEY_READ | *view).is_ok())
            .unwrap_or(false),
        Source::Appx { name } => {
            let script = format!(
                "if (Get-AppxPackage -Name '{}') {{ 'yes' }}",
                name.replace('\'', "''")
            );
            powershell(&script)
                .map(|o| o.trim() == "yes")
                .unwrap_or(true)
        }
    }
}

fn get_str(key: &RegKey, name: &str) -> String {
    key.get_value::<String, _>(name)
        .map(|s| fix_mojibake(s.trim().to_string()))
        .unwrap_or_default()
}

/// Some Chinese installers write GBK bytes into a Unicode registry value, which reads back as
/// Latin-1 gibberish ("¶·ÓãÖ±²¥" for 斗鱼直播). A string made only of bytes that decodes as valid
/// GBK into Chinese is repaired; anything else is left as it was.
fn fix_mojibake(s: String) -> String {
    use windows::Win32::Globalization::{MultiByteToWideChar, MB_ERR_INVALID_CHARS};
    if !s.chars().all(|c| (c as u32) < 0x100) || !s.chars().any(|c| (c as u32) >= 0x80) {
        return s;
    }
    let bytes: Vec<u8> = s.chars().map(|c| c as u8).collect();
    const GBK: u32 = 936;
    let len = unsafe { MultiByteToWideChar(GBK, MB_ERR_INVALID_CHARS, &bytes, None) };
    if len <= 0 {
        return s;
    }
    let mut wide = vec![0u16; len as usize];
    unsafe { MultiByteToWideChar(GBK, MB_ERR_INVALID_CHARS, &bytes, Some(&mut wide)) };
    let fixed = String::from_utf16_lossy(&wide);
    if fixed
        .chars()
        .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
    {
        fixed
    } else {
        s
    }
}

fn get_u32(key: &RegKey, name: &str) -> Option<u32> {
    key.get_value::<u32, _>(name).ok()
}

/// `"C:\x\app.exe",0` → `C:\x\app.exe`.
fn clean_icon_path(raw: &str) -> Option<String> {
    let mut s = raw.trim().trim_matches('"').to_string();
    if let Some(i) = s.rfind(',') {
        if s[i + 1..]
            .trim()
            .trim_start_matches('-')
            .chars()
            .all(|c| c.is_ascii_digit())
        {
            s.truncate(i);
        }
    }
    let s = s.trim().trim_matches('"').to_string();
    (!s.is_empty() && Path::new(&s).is_file()).then_some(s)
}

/// Splits an UninstallString into program and arguments: `"C:\a b\unins.exe" /x` or `C:\a\u.exe /x`.
fn split_command(cmd: &str) -> Option<(String, String)> {
    let cmd = cmd.trim();
    if let Some(rest) = cmd.strip_prefix('"') {
        let end = rest.find('"')?;
        return Some((rest[..end].to_string(), rest[end + 1..].trim().to_string()));
    }
    let lower = cmd.to_lowercase();
    match lower.find(".exe") {
        Some(i) => Some((cmd[..i + 4].to_string(), cmd[i + 4..].trim().to_string())),
        None => Some((cmd.to_string(), String::new())),
    }
}

fn is_guid(s: &str) -> bool {
    s.len() == 38 && s.starts_with('{') && s.ends_with('}')
}

fn uninstaller_for(key_name: &str, key: &RegKey) -> Option<Uninstaller> {
    let uninstall = get_str(key, "UninstallString");
    let is_msi =
        get_u32(key, "WindowsInstaller") == Some(1) || uninstall.to_lowercase().contains("msiexec");
    if is_msi {
        let product = if is_guid(key_name) {
            key_name.to_string()
        } else {
            let start = uninstall.find('{')?;
            uninstall.get(start..start + 38)?.to_string()
        };
        return Some(Uninstaller::Msi { product });
    }
    if uninstall.is_empty() {
        return None;
    }
    let (program, args) = split_command(&uninstall)?;
    Some(Uninstaller::Exe { program, args })
}

fn format_install_date(raw: &str) -> Option<String> {
    let d: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    (d.len() == 8).then(|| format!("{}-{}-{}", &d[..4], &d[4..6], &d[6..8]))
}

fn registry_apps() -> Vec<AppEntry> {
    let mut out = Vec::new();
    let views = [
        (Hive::Machine, KEY_WOW64_64KEY),
        (Hive::Machine, KEY_WOW64_32KEY),
        (Hive::User, KEY_WOW64_64KEY),
    ];
    for (hive, view) in views {
        let Some(root) = open_uninstall(hive, view) else {
            continue;
        };
        for key_name in root.enum_keys().flatten() {
            let Ok(key) = root.open_subkey_with_flags(&key_name, KEY_READ | view) else {
                continue;
            };
            let name = get_str(&key, "DisplayName");
            if name.is_empty()
                || get_u32(&key, "SystemComponent") == Some(1)
                || !get_str(&key, "ParentKeyName").is_empty()
                || matches!(
                    get_str(&key, "ReleaseType").as_str(),
                    "Update" | "Hotfix" | "Security Update" | "Service Pack"
                )
                || name.starts_with("KB") && name[2..].chars().take(6).all(|c| c.is_ascii_digit())
            {
                continue;
            }
            let Some(uninstaller) = uninstaller_for(&key_name, &key) else {
                continue;
            };
            let location = get_str(&key, "InstallLocation")
                .trim_matches('"')
                .trim_end_matches('\\')
                .to_string();
            let icon_source = clean_icon_path(&get_str(&key, "DisplayIcon"));
            let install_location = if !location.is_empty() && Path::new(&location).is_dir() {
                Some(location)
            } else {
                // No location registered: the uninstaller usually sits in the install root, and
                // failing that, so does the program its icon comes from.
                let uninstaller_dir = match &uninstaller {
                    Uninstaller::Exe { program, .. } => Path::new(program).parent().map(display),
                    _ => None,
                };
                uninstaller_dir
                    .into_iter()
                    .chain(
                        icon_source
                            .as_deref()
                            .and_then(|p| Path::new(p).parent())
                            .map(display),
                    )
                    .find(|p| plausible_install_dir(Path::new(p)))
            };
            let publisher = get_str(&key, "Publisher");
            let mut tags = classify(&name, &publisher);
            if matches!(hive, Hive::User) {
                tags.push("user");
            }
            out.push(AppEntry {
                info: AppInfo {
                    id: format!(
                        "reg|{}|{}|{}",
                        matches!(hive, Hive::Machine) as u8,
                        view,
                        key_name
                    ),
                    name,
                    version: get_str(&key, "DisplayVersion"),
                    publisher,
                    kind: "win32",
                    install_date: format_install_date(&get_str(&key, "InstallDate")),
                    install_location,
                    estimated_size: get_u32(&key, "EstimatedSize")
                        .map(|kb| kb as u64 * 1024)
                        .unwrap_or(0),
                    install_size: None,
                    data_size: None,
                    data_dirs: Vec::new(),
                    last_used: None,
                    tags,
                    measured: false,
                },
                uninstaller,
                source: Source::Registry {
                    hive,
                    view,
                    key: key_name,
                },
                icon_source,
                logo: None,
            });
        }
    }
    // The same product often registers in both the 32- and 64-bit views. Keep the entry that says
    // where it is installed.
    out.sort_by_key(|e| {
        (
            e.info.name.to_lowercase(),
            e.info.install_location.is_none(),
        )
    });
    out.dedup_by(|b, a| {
        a.info.name.eq_ignore_ascii_case(&b.info.name) && a.info.version == b.info.version
    });
    out
}

/// A folder that can be measured as one app's install: deep enough not to be a shared root.
fn plausible_install_dir(p: &Path) -> bool {
    if !p.is_absolute() || !p.is_dir() {
        return false;
    }
    let lower = display(p).to_lowercase();
    let windows = display(&util::system_root()).to_lowercase();
    if lower.starts_with(&windows) {
        return false;
    }
    let shared = [
        display(&home()),
        display(&local_appdata()),
        display(&roaming_appdata()),
        display(&local_appdata().join("Programs")),
        r"C:\Program Files".into(),
        r"C:\Program Files (x86)".into(),
        r"C:\ProgramData".into(),
    ];
    !shared.iter().any(|s| s.eq_ignore_ascii_case(&lower)) && p.components().count() >= 3
}

// ---------------------------------------------------------------------------------------------
// Store apps
// ---------------------------------------------------------------------------------------------

/// Runs a PowerShell script (passed encoded, so nothing in it needs quoting) and returns stdout.
pub fn powershell(script: &str) -> Option<String> {
    let full = format!("[Console]::OutputEncoding=[Text.Encoding]::UTF8; $ErrorActionPreference='SilentlyContinue'; {script}");
    let utf16: Vec<u8> = full.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    let out = util::command("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-EncodedCommand",
            &base64_encode(&utf16),
        ])
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn base64_encode(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        s.push(T[(n >> 18) as usize & 63] as char);
        s.push(T[(n >> 12) as usize & 63] as char);
        s.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        s.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    s
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct AppxRow {
    name: String,
    full_name: String,
    family: String,
    version: Option<String>,
    location: Option<String>,
    display_name: Option<String>,
    publisher: Option<String>,
    logo: Option<String>,
}

/// `Microsoft.WindowsCalculator` → `Windows Calculator`.
fn pretty_package_name(name: &str) -> String {
    let base = name.rsplit('.').next().unwrap_or(name);
    let mut out = String::new();
    for (i, c) in base.chars().enumerate() {
        if i > 0 && c.is_uppercase() && !out.ends_with(' ') {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

fn store_apps() -> Vec<AppEntry> {
    let script = r#"
Get-AppxPackage | Where-Object { -not $_.IsFramework -and -not $_.NonRemovable -and [string]$_.SignatureKind -ne 'System' } | ForEach-Object {
  $m = Get-AppxPackageManifest $_
  [pscustomobject]@{
    Name = $_.Name; FullName = $_.PackageFullName; Family = $_.PackageFamilyName; Version = [string]$_.Version;
    Location = $_.InstallLocation; DisplayName = [string]$m.Package.Properties.DisplayName;
    Publisher = [string]$m.Package.Properties.PublisherDisplayName; Logo = [string]$m.Package.Properties.Logo
  }
} | ConvertTo-Json -Compress
"#;
    let Some(json) = powershell(script) else {
        return Vec::new();
    };
    let json = json.trim();
    let rows: Vec<AppxRow> = if json.starts_with('[') {
        serde_json::from_str(json).unwrap_or_default()
    } else {
        serde_json::from_str::<AppxRow>(json)
            .map(|r| vec![r])
            .unwrap_or_default()
    };
    rows.into_iter()
        .map(|r| {
            let display_name = r
                .display_name
                .filter(|d| !d.is_empty() && !d.starts_with("ms-resource:"))
                .unwrap_or_else(|| pretty_package_name(&r.name));
            let publisher = r
                .publisher
                .filter(|p| !p.starts_with("ms-resource:"))
                .unwrap_or_default();
            let logo = r
                .location
                .as_deref()
                .zip(r.logo.as_deref())
                .and_then(|(loc, logo)| find_logo(Path::new(loc), logo));
            let data = local_appdata().join("Packages").join(&r.family);
            let mut tags = classify(&display_name, &publisher);
            tags.push("store");
            AppEntry {
                info: AppInfo {
                    id: format!("appx|{}", r.full_name),
                    name: display_name,
                    version: r.version.unwrap_or_default(),
                    publisher,
                    kind: "store",
                    install_date: None,
                    install_location: r.location,
                    estimated_size: 0,
                    install_size: None,
                    data_size: None,
                    data_dirs: if data.is_dir() {
                        vec![Detail {
                            label: "应用数据".into(),
                            path: display(&data),
                            size: 0,
                        }]
                    } else {
                        Vec::new()
                    },
                    last_used: None,
                    tags,
                    measured: false,
                },
                uninstaller: Uninstaller::Appx {
                    full_name: r.full_name,
                },
                source: Source::Appx { name: r.name },
                icon_source: None,
                logo,
            }
        })
        .collect()
}

/// A package logo is declared as `Assets\StoreLogo.png` but shipped as `StoreLogo.scale-100.png`.
fn find_logo(location: &Path, logo: &str) -> Option<PathBuf> {
    let declared = location.join(logo);
    if declared.is_file() {
        return Some(declared);
    }
    let dir = declared.parent()?;
    let stem = declared.file_stem()?.to_string_lossy().to_lowercase();
    let mut candidates: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let n = p
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            n.starts_with(&stem) && n.ends_with(".png") && !n.contains("contrast")
        })
        .collect();
    candidates.sort();
    candidates
        .into_iter()
        .find(|p| p.to_string_lossy().contains("scale-100"))
        .or_else(|| {
            std::fs::read_dir(dir)
                .ok()?
                .flatten()
                .map(|e| e.path())
                .find(|p| {
                    let n = p
                        .file_name()
                        .map(|n| n.to_string_lossy().to_lowercase())
                        .unwrap_or_default();
                    n.starts_with(&stem) && n.ends_with(".png")
                })
        })
}

// ---------------------------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------------------------

fn classify(name: &str, publisher: &str) -> Vec<&'static str> {
    let n = name.to_lowercase();
    let p = publisher.to_lowercase();
    let mut tags = Vec::new();
    const RUNTIME: &[&str] = &[
        "visual c++",
        "redistributable",
        ".net",
        "runtime",
        "directx",
        "webview2",
        "vcredist",
        "windows app sdk",
        "windowsappruntime",
        "vs_",
        "windows sdk",
        "microsoft update health",
        "python launcher",
        "java(tm)",
    ];
    const DRIVER_PUBLISHERS: &[&str] = &[
        "nvidia",
        "intel",
        "realtek",
        "advanced micro devices",
        "synaptics",
        "dolby",
        "qualcomm",
        "mediatek",
        "elan microelectronics",
        "goodix",
        "waves audio",
        "dts, inc",
        "conexant",
    ];
    const DEV: &[&str] = &[
        "android studio",
        "visual studio",
        "jetbrains",
        "clion",
        "goland",
        "pycharm",
        "intellij",
        "webstorm",
        "git",
        "node.js",
        "python",
        "rust",
        "cmake",
        "docker",
        "postman",
        "sdk",
        "cursor",
        "android",
        "wsl",
    ];
    if RUNTIME.iter().any(|k| n.contains(k)) {
        tags.push("runtime");
    }
    if n.contains("driver") || n.contains("驱动") || DRIVER_PUBLISHERS.iter().any(|k| p.contains(k))
    {
        tags.push("driver");
    }
    if !tags.contains(&"runtime") && DEV.iter().any(|k| n.contains(k) || p.contains(k)) {
        tags.push("dev");
    }
    tags
}

// ---------------------------------------------------------------------------------------------
// Data folders and last use
// ---------------------------------------------------------------------------------------------

/// Folders many apps share; a match on one of these says nothing about a particular app.
const GENERIC: &[&str] = &[
    "microsoft",
    "google",
    "temp",
    "packages",
    "programs",
    "windows",
    "intel",
    "nvidia",
    "nvidia corporation",
    "amd",
    "apple",
    "tencent",
    "adobe",
    "mozilla",
    "jetbrains",
    "crashdumps",
    "common files",
    "app",
    "data",
    "cache",
    "application",
    "bin",
    "program",
    "launcher",
    "update",
    "updater",
    "uninstall",
    "client",
    "desktop",
];

/// "JetBrains CLion 2024.1 (x64)" → "JetBrains CLion".
pub fn core_name(name: &str) -> String {
    let mut s = name.to_string();
    if let Some(i) = s.find(['(', '（']) {
        s.truncate(i);
    }
    s.split_whitespace()
        .filter(|w| {
            !w.chars().any(|c| c.is_ascii_digit())
                && !matches!(
                    w.to_lowercase().as_str(),
                    "x64" | "x86" | "64-bit" | "32-bit"
                )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn compact(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

/// Names that, as a folder under AppData, would be this app's data.
fn candidate_names(entry: &AppEntry) -> Vec<String> {
    let info = &entry.info;
    let core = core_name(&info.name);
    let publisher_word = info
        .publisher
        .split([' ', ',', '.'])
        .next()
        .unwrap_or("")
        .to_string();
    let mut names = vec![compact(&core)];
    if !publisher_word.is_empty() {
        let stripped = core.trim_start_matches(publisher_word.as_str()).trim();
        if !stripped.is_empty() {
            names.push(compact(stripped));
        }
    }
    if let Some(loc) = &info.install_location {
        if let Some(folder) = Path::new(loc).file_name() {
            names.push(compact(&core_name(&folder.to_string_lossy())));
        }
    }
    if let Some(icon) = &entry.icon_source {
        if let Some(stem) = Path::new(icon).file_stem() {
            names.push(compact(&stem.to_string_lossy()));
        }
    }
    names.retain(|n| n.chars().count() >= 3 && !GENERIC.contains(&n.as_str()));
    names.sort();
    names.dedup();
    names
}

/// Data some apps keep where no name-based guess would look.
fn known_data_dirs(name: &str) -> Vec<(&'static str, PathBuf)> {
    let n = name.to_lowercase();
    let home = home();
    let roaming = roaming_appdata();
    if n.contains("wechat") || n.contains("微信") {
        return vec![
            ("聊天文件", home.join(r"Documents\WeChat Files")),
            ("聊天文件", home.join("xwechat_files")),
            ("应用数据", roaming.join(r"Tencent\xwechat")),
            ("旧版数据", roaming.join(r"Tencent\WeChat")),
        ];
    }
    if n.contains("qq音乐") || n.contains("qqmusic") {
        return vec![("下载与缓存", roaming.join(r"Tencent\QQMusic"))];
    }
    if n.contains("android studio") {
        return vec![
            ("Android SDK", local_appdata().join(r"Android\Sdk")),
            ("模拟器与配置", home.join(".android")),
        ];
    }
    Vec::new()
}

fn find_data_dirs(entry: &AppEntry, claimed: &mut HashSet<String>) -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = known_data_dirs(&entry.info.name)
        .into_iter()
        .map(|(l, p)| (l.to_string(), p))
        .collect();
    let names = candidate_names(entry);
    let publisher_word = compact(
        entry
            .info
            .publisher
            .split([' ', ',', '.'])
            .next()
            .unwrap_or(""),
    );
    let locations = [
        ("Roaming", roaming_appdata()),
        ("Local", local_appdata()),
        ("LocalLow", home().join(r"AppData\LocalLow")),
    ];
    for (loc_label, loc) in &locations {
        let Ok(rd) = std::fs::read_dir(loc) else {
            continue;
        };
        for e in rd.flatten() {
            let folder = compact(&e.file_name().to_string_lossy());
            let path = e.path();
            if names.contains(&folder) {
                out.push((format!("{loc_label} 数据"), path));
            } else if !publisher_word.is_empty() && folder == publisher_word && path.is_dir() {
                // Under the publisher's folder: `Local\Google\Chrome`, `Local\JetBrains\CLion2024.1`.
                for sub in std::fs::read_dir(&path).into_iter().flatten().flatten() {
                    let sub_name = compact(&sub.file_name().to_string_lossy());
                    // `CLion2024.1` belongs to CLion; `ChromeForTesting` does not belong to Chrome. A
                    // prefix only counts when what follows it is a version.
                    let versioned = |n: &str| {
                        n.len() >= 4
                            && sub_name.starts_with(n)
                            && sub_name[n.len()..]
                                .chars()
                                .next()
                                .is_some_and(|c| c.is_ascii_digit())
                    };
                    if names.iter().any(|n| sub_name == *n || versioned(n)) {
                        out.push((format!("{loc_label} 数据"), sub.path()));
                    }
                }
            }
        }
    }
    let install = entry
        .info
        .install_location
        .as_deref()
        .map(|l| l.to_lowercase());
    out.retain(|(_, p)| {
        let lower = display(p).to_lowercase();
        let overlaps = install
            .as_deref()
            .map(|i| lower.starts_with(i) || i.starts_with(&lower))
            .unwrap_or(false);
        p.is_dir() && !overlaps && claimed.insert(lower)
    });
    out
}

/// When each program or shortcut was last launched from the shell, from UserAssist: the paths are
/// ROT13-encoded, and the last-run FILETIME sits at offset 60 of the 72-byte value.
fn user_assist() -> Vec<(String, u64)> {
    const KNOWN: &[(&str, &str)] = &[
        (
            "{6D809377-6AF0-444B-8957-A3773F02200E}",
            r"C:\Program Files",
        ),
        (
            "{7C5A40EF-A0FB-4BFC-874A-C0F2E0B9FA8E}",
            r"C:\Program Files (x86)",
        ),
        ("{F38BF404-1D43-42F2-9305-67DE0B28FC23}", r"C:\Windows"),
        (
            "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}",
            r"C:\Windows\System32",
        ),
    ];
    let local = display(&local_appdata());
    let roaming = display(&roaming_appdata());
    let Ok(root) = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Explorer\UserAssist")
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for guid in root.enum_keys().flatten() {
        let Ok(count) = root.open_subkey(format!(r"{guid}\Count")) else {
            continue;
        };
        for (name, value) in count.enum_values().flatten() {
            if value.bytes.len() < 68 {
                continue;
            }
            let ft = u64::from_le_bytes(value.bytes[60..68].try_into().unwrap());
            if ft == 0 {
                continue;
            }
            let secs = (ft / 10_000_000).saturating_sub(11_644_473_600);
            let mut path: String = name
                .chars()
                .map(|c| match c {
                    'a'..='z' => (((c as u8 - b'a' + 13) % 26) + b'a') as char,
                    'A'..='Z' => (((c as u8 - b'A' + 13) % 26) + b'A') as char,
                    _ => c,
                })
                .collect();
            for (g, p) in KNOWN {
                path = path.replace(g, p);
            }
            path = path
                .replace("{F1B32785-6FBA-4FCF-9D55-7B8E7F157091}", &local)
                .replace("{3EB685DB-65F9-4CF6-A03A-E3EF65729F3D}", &roaming);
            out.push((path.to_lowercase(), secs));
        }
    }
    out
}

fn last_launch(entry: &AppEntry, launches: &[(String, u64)]) -> Option<u64> {
    let install = entry
        .info
        .install_location
        .as_deref()
        .map(|l| format!("{}\\", l.to_lowercase()));
    let core = compact(&core_name(&entry.info.name));
    launches
        .iter()
        .filter(|(path, _)| {
            if let Some(i) = &install {
                if path.starts_with(i.as_str()) {
                    return true;
                }
            }
            // Start menu / taskbar shortcuts: match the shortcut's name against the app's.
            path.ends_with(".lnk")
                && Path::new(path)
                    .file_stem()
                    .map(|s| {
                        let stem = compact(&s.to_string_lossy());
                        stem.len() >= 3
                            && core.len() >= 3
                            && (stem.contains(&core) || core.contains(&stem))
                    })
                    .unwrap_or(false)
        })
        .map(|(_, t)| *t)
        .max()
}

// ---------------------------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------------------------

/// Everything installed, sizes not yet measured. Fast: registry and the Store package list only.
pub fn list() -> Vec<AppEntry> {
    let (mut apps, store) = rayon::join(registry_apps, store_apps);
    apps.extend(store);
    let launches = user_assist();
    for a in &mut apps {
        a.info.last_used = last_launch(a, &launches);
    }
    // Data folders are claimed in order, so a folder never counts for two apps. Longer, more
    // specific names go first: "Google Chrome" should get `Local\Google\Chrome`, not "Google".
    let mut order: Vec<usize> = (0..apps.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(apps[i].info.name.len()));
    let mut claimed = HashSet::new();
    for i in order {
        let found = find_data_dirs(&apps[i], &mut claimed);
        for (label, path) in found {
            apps[i].info.data_dirs.push(Detail {
                label,
                path: display(&path),
                size: 0,
            });
        }
    }
    apps
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Measured {
    pub id: String,
    pub install_size: Option<u64>,
    pub data_size: u64,
    pub data_dirs: Vec<Detail>,
    pub last_used: Option<u64>,
}

/// Measures one app: its install folder and each data folder. The newest activity in the data
/// folders also counts as use — an app that has not been launched from the shell can still have
/// been running.
pub fn measure_app(entry: &AppEntry) -> Measured {
    let install_size = entry
        .info
        .install_location
        .as_deref()
        .map(Path::new)
        .filter(|p| plausible_install_dir(p))
        .map(|p| measure(p, None).bytes);
    let mut data_dirs: Vec<Detail> = entry
        .info
        .data_dirs
        .par_iter()
        .map(|d| Detail {
            size: measure(Path::new(&d.path), None).bytes,
            ..d.clone()
        })
        .collect();
    data_dirs.sort_by(|a, b| b.size.cmp(&a.size));
    let activity = data_dirs
        .iter()
        .filter_map(|d| newest_mtime(Path::new(&d.path), &[]))
        .max()
        .map(unix_secs);
    Measured {
        id: entry.info.id.clone(),
        install_size,
        data_size: data_dirs.iter().map(|d| d.size).sum(),
        data_dirs,
        last_used: entry.info.last_used.max(activity),
    }
}

/// App icons as PNG data URLs, keyed by app id. Win32 icons come out of one PowerShell batch;
/// Store apps ship their own logo file.
pub fn icons(apps: &[AppEntry]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for a in apps {
        if let Some(logo) = &a.logo {
            if let Ok(bytes) = std::fs::read(logo) {
                out.insert(
                    a.info.id.clone(),
                    format!("data:image/png;base64,{}", base64_encode(&bytes)),
                );
            }
        }
    }
    let wanted: Vec<(&str, &str)> = apps
        .iter()
        .filter_map(|a| a.icon_source.as_deref().map(|p| (a.info.id.as_str(), p)))
        .collect();
    if wanted.is_empty() {
        return out;
    }
    let list = std::env::temp_dir().join("diskmate-icons.json");
    let paths: Vec<&str> = wanted.iter().map(|(_, p)| *p).collect();
    if std::fs::write(&list, serde_json::to_vec(&paths).unwrap_or_default()).is_err() {
        return out;
    }
    let script = format!(
        r#"Add-Type -AssemblyName System.Drawing
$paths = Get-Content -Raw -Encoding UTF8 '{}' | ConvertFrom-Json
$out = @{{}}
foreach ($p in $paths) {{
  try {{
    $ico = [System.Drawing.Icon]::ExtractAssociatedIcon($p)
    $bmp = $ico.ToBitmap(); $ms = New-Object IO.MemoryStream
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $out[$p] = [Convert]::ToBase64String($ms.ToArray())
  }} catch {{}}
}}
$out | ConvertTo-Json -Compress"#,
        list.display().to_string().replace('\'', "''")
    );
    let by_path: HashMap<String, String> = powershell(&script)
        .and_then(|json| serde_json::from_str(json.trim()).ok())
        .unwrap_or_default();
    let _ = std::fs::remove_file(&list);
    for (id, path) in wanted {
        if let Some(b64) = by_path.get(path) {
            out.insert(id.to_string(), format!("data:image/png;base64,{b64}"));
        }
    }
    out
}

/// Starts the app's own uninstaller and waits for it to exit. Elevation, if the uninstaller wants
/// it, is the system's UAC prompt.
pub fn run_uninstaller(entry: &AppEntry) -> Result<(), String> {
    let quote = |s: &str| s.replace('\'', "''");
    let script = match &entry.uninstaller {
        Uninstaller::Exe { program, args } if args.is_empty() => {
            format!("Start-Process -FilePath '{}' -Wait", quote(program))
        }
        Uninstaller::Exe { program, args } => {
            format!(
                "Start-Process -FilePath '{}' -ArgumentList '{}' -Wait",
                quote(program),
                quote(args)
            )
        }
        Uninstaller::Msi { product } => {
            format!(
                "Start-Process -FilePath msiexec.exe -ArgumentList '/x {}' -Wait",
                quote(product)
            )
        }
        Uninstaller::Appx { full_name } => {
            format!("Remove-AppxPackage -Package '{}'", quote(full_name))
        }
    };
    let ran = powershell(&script);
    ran.map(|_| ()).ok_or_else(|| "无法启动卸载程序".into())
}

/// What the app left behind: data folders and the install folder, whichever still exist.
pub fn leftovers(entry: &AppEntry) -> Vec<Detail> {
    let mut out: Vec<Detail> = entry
        .info
        .data_dirs
        .iter()
        .filter(|d| Path::new(&d.path).is_dir())
        .map(|d| Detail {
            size: measure(Path::new(&d.path), None).bytes,
            ..d.clone()
        })
        .collect();
    if let Some(loc) = &entry.info.install_location {
        let p = Path::new(loc);
        if entry.info.kind == "win32" && p.is_dir() {
            out.push(Detail {
                label: "安装目录".into(),
                path: loc.clone(),
                size: measure(p, None).bytes,
            });
        }
    }
    out.retain(|d| d.size > 0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uninstall_commands_split_into_program_and_arguments() {
        assert_eq!(
            split_command(r#""C:\Program Files\App\unins000.exe" /SILENT"#),
            Some((
                r"C:\Program Files\App\unins000.exe".into(),
                "/SILENT".into()
            ))
        );
        assert_eq!(
            split_command(r"C:\Apps\uninst.exe /x"),
            Some((r"C:\Apps\uninst.exe".into(), "/x".into()))
        );
        assert_eq!(
            split_command(r"C:\Apps\uninst.exe"),
            Some((r"C:\Apps\uninst.exe".into(), "".into()))
        );
    }

    #[test]
    fn display_icons_lose_their_index() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("app.exe");
        std::fs::write(&exe, b"x").unwrap();
        assert_eq!(
            clean_icon_path(&format!("\"{}\",0", exe.display())),
            Some(exe.display().to_string())
        );
        assert_eq!(
            clean_icon_path(&format!("{},-101", exe.display())),
            Some(exe.display().to_string())
        );
        assert_eq!(clean_icon_path(r"C:\nope\missing.exe,0"), None);
    }

    #[test]
    fn names_shed_versions_and_architectures() {
        assert_eq!(core_name("JetBrains CLion 2024.1 (x64)"), "JetBrains CLion");
        assert_eq!(core_name("Python 3.13.1 (64-bit)"), "Python");
        assert_eq!(core_name("Google Chrome"), "Google Chrome");
    }

    #[test]
    fn runtimes_and_drivers_are_flagged() {
        assert!(classify(
            "Microsoft Visual C++ 2015-2022 Redistributable (x64)",
            "Microsoft Corporation"
        )
        .contains(&"runtime"));
        assert!(classify("NVIDIA 图形驱动程序", "NVIDIA Corporation").contains(&"driver"));
        assert!(classify("Android Studio", "Google LLC").contains(&"dev"));
        assert!(classify("微信", "腾讯科技(深圳)有限公司").is_empty());
    }

    #[test]
    fn gbk_written_into_unicode_values_is_repaired() {
        assert_eq!(fix_mojibake("¶·ÓãÖ±²¥".into()), "斗鱼直播");
        assert_eq!(fix_mojibake("Google Chrome".into()), "Google Chrome");
        assert_eq!(fix_mojibake("微信".into()), "微信");
        assert_eq!(fix_mojibake("Café".into()), "Café");
    }

    #[test]
    fn base64_matches_the_standard_alphabet() {
        assert_eq!(base64_encode(b"Man"), "TWFu");
        assert_eq!(base64_encode(b"Ma"), "TWE=");
        assert_eq!(base64_encode(b"M"), "TQ==");
    }

    /// Read-only: `cargo test --release real_apps -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_apps() {
        let started = std::time::Instant::now();
        let apps = list();
        println!("listed {} apps in {:?}", apps.len(), started.elapsed());
        let measured: Vec<Measured> = apps.par_iter().map(measure_app).collect();
        println!("measured in {:?}", started.elapsed());
        let mut rows: Vec<(&AppEntry, &Measured)> = apps.iter().zip(&measured).collect();
        rows.sort_by_key(|(_, m)| std::cmp::Reverse(m.install_size.unwrap_or(0) + m.data_size));
        let gb = |b: u64| b as f64 / 1073741824.0;
        for (a, m) in rows.iter().take(40) {
            println!(
                "{:<44} inst {:>6.2} data {:>6.2} last {:>10} {:?} {}",
                a.info.name.chars().take(44).collect::<String>(),
                gb(m.install_size.unwrap_or(0)),
                gb(m.data_size),
                m.last_used.unwrap_or(0),
                a.info.tags,
                m.data_dirs
                    .iter()
                    .map(|d| d.path.as_str())
                    .collect::<Vec<_>>()
                    .join(" | ")
            );
        }
        let started = std::time::Instant::now();
        let icons = icons(&apps);
        println!("{} icons in {:?}", icons.len(), started.elapsed());
    }
}
