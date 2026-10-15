//! Programs that start with Windows, and switching them off and on.
//!
//! Turning one off works exactly like Task Manager's Startup apps page: the entry itself stays, and
//! an "approved" flag next to it says whether Explorer should run it. Task Manager, Settings and
//! this page therefore always agree, and nothing is ever deleted.

use crate::util::{self, display, program_data, roaming_appdata, system_root};
use crate::{apps, filemeta};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, REG_BINARY};
use winreg::{RegKey, RegValue};

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN32: &str = r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run";
const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved";
const STORE_TASKS: &str = r"Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\SystemAppData";

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Advice {
    /// `disable` (fine to turn off) | `keep` (better left on)
    pub kind: &'static str,
    pub reason: &'static str,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StartupItem {
    pub id: String,
    pub name: String,
    pub command: String,
    /// The program that runs (a shortcut's target, a command's executable).
    pub path: Option<String>,
    pub publisher: Option<String>,
    pub icon: Option<String>,
    /// `registry` | `folder` | `store`
    pub source: &'static str,
    /// Registered for every user: changing it needs administrator rights.
    pub all_users: bool,
    pub enabled: bool,
    /// The program file no longer exists, so the entry does nothing at all.
    pub missing: bool,
    pub advice: Option<Advice>,
}

/// Where the on/off flag of an entry lives.
#[derive(Clone, Debug)]
enum Flag {
    /// `StartupApproved\<sub>` under HKCU or HKLM, value named after the entry.
    Approved {
        machine: bool,
        sub: &'static str,
        value: String,
    },
    /// A Store app's startup task: `State` under its own key in HKCU.
    Store { key: String },
}

#[derive(Clone, Debug)]
struct Entry {
    id: String,
    name: String,
    command: String,
    /// The file to read the icon and publisher from.
    file: Option<PathBuf>,
    source: &'static str,
    all_users: bool,
    flag: Flag,
}

fn hive(machine: bool) -> RegKey {
    RegKey::predef(if machine {
        HKEY_LOCAL_MACHINE
    } else {
        HKEY_CURRENT_USER
    })
}

/// Expands `%ProgramFiles%` and friends.
pub fn expand(s: &str) -> String {
    use windows::core::HSTRING;
    use windows::Win32::System::Environment::ExpandEnvironmentStringsW;
    if !s.contains('%') {
        return s.to_string();
    }
    let src = HSTRING::from(s);
    let mut buf = vec![0u16; 2048];
    let n = unsafe { ExpandEnvironmentStringsW(&src, Some(&mut buf)) } as usize;
    if n == 0 || n > buf.len() {
        return s.to_string();
    }
    String::from_utf16_lossy(&buf[..n - 1])
}

/// The executable a command line starts: `"C:\a b\x.exe" --flag` → `C:\a b\x.exe`.
pub fn exe_of(command: &str) -> Option<PathBuf> {
    let c = expand(command.trim());
    let raw = if let Some(rest) = c.strip_prefix('"') {
        rest.split('"').next()?.to_string()
    } else if let Some(i) = c.to_lowercase().find(".exe") {
        c[..i + 4].to_string()
    } else {
        c.split_whitespace().next()?.to_string()
    };
    let p = PathBuf::from(raw.trim());
    if p.as_os_str().is_empty() {
        return None;
    }
    if p.is_absolute() {
        return Some(p);
    }
    // `rundll32.exe ...`: a bare name runs from System32.
    let sys = system_root().join("System32").join(&p);
    Some(if sys.exists() { sys } else { p })
}

fn run_entries(machine: bool, path: &str, sub: &'static str, tag: &str, out: &mut Vec<Entry>) {
    let Ok(key) = hive(machine).open_subkey(path) else {
        return;
    };
    for (name, _) in key.enum_values().flatten() {
        let Ok(command) = key.get_value::<String, _>(&name) else {
            continue;
        };
        if name.is_empty() || command.trim().is_empty() {
            continue;
        }
        out.push(Entry {
            id: format!("{tag}|{name}"),
            name: name.clone(),
            file: exe_of(&command),
            command,
            source: "registry",
            all_users: machine,
            flag: Flag::Approved {
                machine,
                sub,
                value: name,
            },
        });
    }
}

fn folder_entries(dir: &Path, machine: bool, tag: &str, out: &mut Vec<Entry>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let file = e.file_name().to_string_lossy().into_owned();
        if file.eq_ignore_ascii_case("desktop.ini") || !e.path().is_file() {
            continue;
        }
        let name = Path::new(&file)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| file.clone());
        out.push(Entry {
            id: format!("{tag}|{file}"),
            name,
            command: display(&e.path()),
            file: Some(e.path()),
            source: "folder",
            all_users: machine,
            flag: Flag::Approved {
                machine,
                sub: "StartupFolder",
                value: file,
            },
        });
    }
}

/// `Microsoft.WindowsTerminal_8wekyb3d8bbwe` → `WindowsTerminal`.
fn store_name(family: &str) -> String {
    let base = family.split('_').next().unwrap_or(family);
    base.rsplit('.').next().unwrap_or(base).to_string()
}

#[derive(Serialize, serde::Deserialize, Clone)]
struct StoreMeta {
    name: String,
    publisher: Option<String>,
    icon: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct AppxRow {
    name: String,
    family: String,
    location: Option<String>,
    display_name: Option<String>,
    publisher: Option<String>,
    logo: Option<String>,
}

const STORE_META_CACHE: &str = "startup-store.json";

/// [`store_meta`], remembered: a package's name and logo do not change, so only families not seen
/// before are asked about. A remembered family counts as installed while its folder under
/// `AppData\Local\Packages` exists (Windows removes it with the app).
fn store_meta_cached(families: &[String]) -> HashMap<String, StoreMeta> {
    let mut known: HashMap<String, StoreMeta> =
        crate::cache::load(STORE_META_CACHE).unwrap_or_default();
    let packages = crate::util::local_appdata().join("Packages");
    let missing: Vec<String> = families
        .iter()
        .filter(|f| !known.contains_key(*f) || !packages.join(f).is_dir())
        .cloned()
        .collect();
    if !missing.is_empty() {
        let fresh = store_meta(&missing);
        for f in &missing {
            known.remove(f);
        }
        known.extend(fresh);
        crate::cache::save(STORE_META_CACHE, &known);
    }
    families
        .iter()
        .filter(|f| packages.join(f).is_dir())
        .filter_map(|f| known.get(f).map(|m| (f.clone(), m.clone())))
        .collect()
}

/// Names, publishers and logos of the installed packages among `families`. A family missing from
/// the result is not installed any more: its startup task is a leftover.
fn store_meta(families: &[String]) -> HashMap<String, StoreMeta> {
    if families.is_empty() {
        return HashMap::new();
    }
    let list = families
        .iter()
        .map(|f| format!("'{}'", f.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(",");
    let script = format!(
        r#"$fams = @({list})
@(Get-AppxPackage | Where-Object {{ $fams -contains $_.PackageFamilyName }} | ForEach-Object {{
  $m = Get-AppxPackageManifest $_
  [pscustomobject]@{{
    Name = $_.Name; Family = $_.PackageFamilyName; Location = $_.InstallLocation;
    DisplayName = [string]$m.Package.Properties.DisplayName;
    Publisher = [string]$m.Package.Properties.PublisherDisplayName; Logo = [string]$m.Package.Properties.Logo
  }}
}}) | ConvertTo-Json -Compress"#
    );
    let Some(json) = apps::powershell(&script) else {
        return HashMap::new();
    };
    let json = json.trim();
    let rows: Vec<AppxRow> = if json.starts_with('[') {
        serde_json::from_str(json).unwrap_or_default()
    } else {
        serde_json::from_str::<AppxRow>(json)
            .map(|r| vec![r])
            .unwrap_or_default()
    };
    let resource = |s: &Option<String>| {
        s.clone()
            .filter(|s| !s.is_empty() && !s.starts_with("ms-resource:"))
    };
    rows.into_iter()
        .map(|r| {
            let icon = r
                .location
                .as_deref()
                .zip(r.logo.as_deref())
                .and_then(|(loc, logo)| apps::find_logo(Path::new(loc), logo))
                .and_then(|p| std::fs::read(p).ok())
                .map(|b| format!("data:image/png;base64,{}", apps::base64_encode(&b)));
            let meta = StoreMeta {
                name: resource(&r.display_name)
                    .unwrap_or_else(|| apps::pretty_package_name(&r.name)),
                publisher: resource(&r.publisher),
                icon,
            };
            (r.family, meta)
        })
        .collect()
}

fn store_entries(out: &mut Vec<Entry>) {
    let user = hive(false);
    let Ok(tasks) = user.open_subkey(STORE_TASKS) else {
        return;
    };
    for family in tasks.enum_keys().flatten() {
        let Ok(pkg) = tasks.open_subkey(&family) else {
            continue;
        };
        for task in pkg.enum_keys().flatten() {
            let Ok(t) = pkg.open_subkey(&task) else {
                continue;
            };
            if t.get_value::<u32, _>("State").is_err() {
                continue;
            }
            out.push(Entry {
                id: format!("store|{family}|{task}"),
                name: store_name(&family),
                command: family.clone(),
                file: None,
                source: "store",
                all_users: false,
                flag: Flag::Store {
                    key: format!(r"{STORE_TASKS}\{family}\{task}"),
                },
            });
        }
    }
}

fn entries() -> Vec<Entry> {
    let mut out = Vec::new();
    run_entries(false, RUN, "Run", "hkcu-run", &mut out);
    run_entries(true, RUN, "Run", "hklm-run", &mut out);
    run_entries(true, RUN32, "Run32", "hklm-run32", &mut out);
    folder_entries(
        &roaming_appdata().join(r"Microsoft\Windows\Start Menu\Programs\Startup"),
        false,
        "user-folder",
        &mut out,
    );
    folder_entries(
        &program_data().join(r"Microsoft\Windows\Start Menu\Programs\StartUp"),
        true,
        "common-folder",
        &mut out,
    );
    store_entries(&mut out);
    out
}

/// Approved flags: the first byte is even when the entry may run (02, 06), odd when switched off.
fn is_enabled(flag: &Flag) -> bool {
    match flag {
        Flag::Approved {
            machine,
            sub,
            value,
        } => hive(*machine)
            .open_subkey(format!(r"{APPROVED}\{sub}"))
            .and_then(|k| k.get_raw_value(value))
            .map(|v| v.bytes.first().is_none_or(|b| b & 1 == 0))
            .unwrap_or(true),
        // 2 = on, 4 = on by policy; 0 = off by default, 1 = off by the user, 3 = off by policy.
        Flag::Store { key } => hive(false)
            .open_subkey(key)
            .and_then(|k| k.get_value::<u32, _>("State"))
            .map(|s| s == 2 || s == 4)
            .unwrap_or(false),
    }
}

fn approved_bytes(enabled: bool) -> Vec<u8> {
    let mut b = vec![0u8; 12];
    if enabled {
        b[0] = 0x02;
    } else {
        b[0] = 0x03;
        // When it was switched off, as a FILETIME (100 ns ticks since 1601).
        let ticks = (util::unix_secs(std::time::SystemTime::now()) + 11_644_473_600) * 10_000_000;
        b[4..].copy_from_slice(&ticks.to_le_bytes());
    }
    b
}

const KEEP_DRIVERS: &[&str] = &[
    "realtek",
    "synaptics",
    "elan",
    "intel",
    "nvidia",
    "advanced micro devices",
    "amd",
    "dolby",
    "conexant",
    "waves",
    "logitech",
];
const KEEP_SECURITY: &[&str] = &[
    "huorong",
    "火绒",
    "kaspersky",
    "eset",
    "norton",
    "mcafee",
    "avast",
    "avg",
    "bitdefender",
    "360",
    "tencent pc manager",
    "电脑管家",
];
const KEEP_INPUT: &[&str] = &[
    "sogou",
    "搜狗",
    "pinyin",
    "拼音",
    "输入法",
    "ctfmon",
    "rime",
    "weasel",
];

/// Names (lowercase substrings of the entry name or file) that rarely need to start with Windows.
const ON_DEMAND: &[(&str, &str)] = &[
    ("steam", "游戏平台，玩游戏时再打开就行"),
    ("epicgames", "游戏平台，玩游戏时再打开就行"),
    ("wegame", "游戏平台，玩游戏时再打开就行"),
    ("battle.net", "游戏平台，玩游戏时再打开就行"),
    ("discord", "聊天软件，需要时再打开就行"),
    ("spotify", "音乐软件，需要时再打开就行"),
    ("qqmusic", "音乐软件，需要时再打开就行"),
    ("cloudmusic", "音乐软件，需要时再打开就行"),
    ("kugou", "音乐软件，需要时再打开就行"),
    ("kuwo", "音乐软件，需要时再打开就行"),
    ("thunder", "下载工具，需要下载时再打开就行"),
    ("迅雷", "下载工具，需要下载时再打开就行"),
    ("baidunetdisk", "网盘客户端，需要时再打开就行"),
    ("aliyundrive", "网盘客户端，需要时再打开就行"),
    ("quark", "网盘客户端，需要时再打开就行"),
    ("msedge", "Edge 的「启动加速」，开机就在后台加载浏览器"),
    (
        "microsoftedgeautolaunch",
        "Edge 的「启动加速」，开机就在后台加载浏览器",
    ),
    (
        "docker",
        "开发工具，需要时再打开就行，开机启动会额外占用不少内存",
    ),
    ("adobe", "Adobe 的后台服务，用 Adobe 软件时会自己启动"),
    ("ccxprocess", "Adobe 的后台服务，用 Adobe 软件时会自己启动"),
    (
        "creative cloud",
        "Adobe 的后台服务，用 Adobe 软件时会自己启动",
    ),
    (
        "itunes",
        "iTunes 的辅助程序，接 iPhone 时 iTunes 会自己处理",
    ),
    ("skype", "聊天软件，需要时再打开就行"),
    ("cortana", "Cortana 已基本停用"),
];

fn advice(name: &str, file: Option<&Path>, company: Option<&str>) -> Option<Advice> {
    let file_l = file.map(|f| display(f).to_lowercase()).unwrap_or_default();
    let stem = file
        .and_then(|f| f.file_stem())
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let name_l = name.to_lowercase();
    let company_l = company.unwrap_or("").to_lowercase();
    let has = |words: &[&str], s: &str| words.iter().any(|w| s.contains(w));
    let sys = display(&system_root()).to_lowercase();

    // Drivers install their helpers under System32\DriverStore; those are the vendor's, not Windows'.
    if !file_l.is_empty()
        && file_l.starts_with(&sys)
        && !file_l.contains(r"\driverstore\")
        && !stem.contains("rundll32")
    {
        return Some(Advice {
            kind: "keep",
            reason: "Windows 自带的组件",
        });
    }
    if company_l == "microsoft windows" {
        return Some(Advice {
            kind: "keep",
            reason: "Windows 自带的组件",
        });
    }
    if has(KEEP_SECURITY, &company_l) || has(KEEP_SECURITY, &name_l) {
        return Some(Advice {
            kind: "keep",
            reason: "安全软件，关掉后开机时不受保护",
        });
    }
    if has(KEEP_INPUT, &name_l) || has(KEEP_INPUT, &stem) || has(KEEP_INPUT, &company_l) {
        return Some(Advice {
            kind: "keep",
            reason: "输入法，关掉后可能要手动切换",
        });
    }
    let updater = ["update", "updater", "autoupdate"]
        .iter()
        .any(|w| stem.contains(w) || name_l.contains(w));
    if updater {
        return Some(Advice {
            kind: "disable",
            reason: "自动更新程序，关掉后软件打开时仍会自己检查更新",
        });
    }
    // Folder names match only from their start: "MSTeams_..." must not read as "steam".
    let dirs: Vec<String> = file
        .map(|f| {
            f.components()
                .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
                .collect()
        })
        .unwrap_or_default();
    if let Some((_, why)) = ON_DEMAND.iter().find(|(w, _)| {
        name_l.contains(w) || stem.contains(w) || dirs.iter().any(|d| d.starts_with(w))
    }) {
        return Some(Advice {
            kind: "disable",
            reason: why,
        });
    }
    if has(KEEP_DRIVERS, &company_l) {
        return Some(Advice {
            kind: "keep",
            reason: "硬件的配套程序，关掉后声音、触控板、显卡设置等可能不正常",
        });
    }
    None
}

/// Programs whose own name says nothing about what they run.
fn is_host(stem: &str) -> bool {
    matches!(
        stem,
        "rundll32" | "cmd" | "powershell" | "pwsh" | "wscript" | "cscript" | "explorer" | "conhost"
    )
}

pub fn list() -> Vec<StartupItem> {
    let all = entries();
    let files: Vec<String> = all
        .iter()
        .filter_map(|e| e.file.as_deref().map(display))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let families: Vec<String> = all
        .iter()
        .filter(|e| e.source == "store")
        .map(|e| e.command.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    // Two PowerShell runs at most (none when everything is remembered), side by side. Plain threads,
    // not the worker pool: this is a quick lookup and must not queue behind a scan.
    let (meta, store) = std::thread::scope(|s| {
        let store = s.spawn(|| store_meta_cached(&families));
        (filemeta::details(&files), store.join().unwrap_or_default())
    });
    let mut out: Vec<StartupItem> = all
        .into_iter()
        .filter_map(|e| {
            if e.source == "store" {
                // Not installed any more: the task is a leftover and never runs.
                let m = store.get(&e.command)?;
                return Some(StartupItem {
                    advice: advice(&m.name, None, m.publisher.as_deref()),
                    enabled: is_enabled(&e.flag),
                    id: e.id,
                    name: m.name.clone(),
                    command: e.command,
                    path: None,
                    publisher: m.publisher.clone(),
                    icon: m.icon.clone(),
                    source: e.source,
                    all_users: false,
                    missing: false,
                });
            }
            let key = e.file.as_deref().map(display);
            let d = key
                .as_ref()
                .and_then(|k| meta.get(k))
                .cloned()
                .unwrap_or_default();
            // A shortcut: what it opens is the program that matters.
            let program = match (&d.target, e.source) {
                (Some(t), "folder") => Some(PathBuf::from(t)),
                _ => e.file.clone(),
            };
            let stem = program
                .as_deref()
                .and_then(|p| p.file_stem())
                .map(|s| s.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            // Value names are often ids ("com.squirrel.slack.slack"); a description reads better,
            // unless the program is a generic host like rundll32.
            let name = match (&d.description, e.source) {
                (Some(desc), "registry") if !is_host(&stem) && desc.chars().count() <= 40 => {
                    desc.clone()
                }
                _ => e.name.clone(),
            };
            let missing = program
                .as_deref()
                .is_some_and(|p| p.is_absolute() && !p.exists());
            Some(StartupItem {
                advice: advice(&e.name, program.as_deref(), d.company.as_deref()),
                id: e.id,
                name,
                command: e.command,
                path: program.as_deref().map(display),
                publisher: d.company,
                icon: d.icon,
                source: e.source,
                all_users: e.all_users,
                enabled: is_enabled(&e.flag),
                missing,
            })
        })
        .collect();
    out.sort_by(|a, b| {
        b.enabled
            .cmp(&a.enabled)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}

/// Writes an HKLM value through an elevated reg.exe: the user sees the UAC prompt.
fn write_machine_flag(sub: &str, value: &str, bytes: &[u8]) -> Result<(), String> {
    if value.contains('"') {
        return Err("这一项的名字里有引号，请到任务管理器里修改".into());
    }
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let args = format!(r#"add "HKLM\{APPROVED}\{sub}" /v "{value}" /t REG_BINARY /d {hex} /f"#);
    let ps = format!(
        "$p = Start-Process -FilePath reg.exe -ArgumentList '{}' -Verb RunAs -Wait -WindowStyle Hidden -PassThru; exit $p.ExitCode",
        args.replace('\'', "''")
    );
    let status = util::command("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("没有获得管理员授权".into())
    }
}

/// Switches the entry `id` on or off. The id is looked up again here, so only entries that really
/// are startup entries can be touched.
pub fn set_enabled(id: &str, enabled: bool) -> Result<(), String> {
    let entry = entries()
        .into_iter()
        .find(|e| e.id == id)
        .ok_or("找不到这个启动项，可能已经被卸载")?;
    match &entry.flag {
        Flag::Approved {
            machine: false,
            sub,
            value,
        } => {
            let (key, _) = hive(false)
                .create_subkey(format!(r"{APPROVED}\{sub}"))
                .map_err(|e| e.to_string())?;
            key.set_raw_value(
                value,
                &RegValue {
                    bytes: approved_bytes(enabled),
                    vtype: REG_BINARY,
                },
            )
            .map_err(|e| e.to_string())?;
        }
        Flag::Approved {
            machine: true,
            sub,
            value,
        } => {
            write_machine_flag(sub, value, &approved_bytes(enabled))?;
        }
        Flag::Store { key } => {
            let (k, _) = hive(false).create_subkey(key).map_err(|e| e.to_string())?;
            k.set_value("State", &(if enabled { 2u32 } else { 1u32 }))
                .map_err(|e| e.to_string())?;
        }
    }
    if is_enabled(&entry.flag) == enabled {
        Ok(())
    } else {
        Err("设置没有生效".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executables_come_out_of_command_lines() {
        assert_eq!(
            exe_of(r#""C:\Program Files\App\app.exe" --minimized"#),
            Some(PathBuf::from(r"C:\Program Files\App\app.exe"))
        );
        assert_eq!(
            exe_of(r"C:\Program Files\App\app.exe /background"),
            Some(PathBuf::from(r"C:\Program Files\App\app.exe"))
        );
        let sys = exe_of("rundll32.exe shell32.dll,Something").unwrap();
        assert!(sys.is_absolute() && sys.ends_with("rundll32.exe"));
        assert!(expand("%SystemRoot%").to_lowercase().contains("windows"));
    }

    #[test]
    fn approved_flags_round_trip() {
        assert_eq!(approved_bytes(true)[0] & 1, 0);
        assert_eq!(approved_bytes(false)[0] & 1, 1);
        assert_eq!(approved_bytes(false).len(), 12);
    }

    #[test]
    fn advice_is_given_for_obvious_cases() {
        let a = advice(
            "Steam",
            Some(Path::new(r"C:\Program Files (x86)\Steam\steam.exe")),
            Some("Valve"),
        );
        assert_eq!(a.unwrap().kind, "disable");
        let a = advice(
            "Teams",
            Some(Path::new(
                r"C:\Users\me\AppData\Local\Microsoft\WindowsApps\MSTeams_8wekyb3d8bbwe\ms-teams.exe",
            )),
            Some("Microsoft"),
        );
        assert!(a.is_none(), "Teams is not Steam");
        let a = advice(
            "BaiduYunDetect",
            Some(Path::new(
                r"C:\Users\me\AppData\Roaming\baidu\BaiduNetdisk\YunDetectService.exe",
            )),
            None,
        );
        assert_eq!(a.unwrap().kind, "disable");
        let a = advice(
            "GoogleUpdater",
            Some(Path::new(r"C:\x\updater.exe")),
            Some("Google LLC"),
        );
        assert_eq!(a.unwrap().kind, "disable");
        let a = advice(
            "RtkAudUService",
            Some(Path::new(r"C:\Drivers\RtkAudUService64.exe")),
            Some("Realtek Semiconductor"),
        );
        assert_eq!(a.unwrap().kind, "keep");
        assert!(advice(
            "MyTool",
            Some(Path::new(r"C:\Tools\tool.exe")),
            Some("Someone")
        )
        .is_none());
        assert_eq!(
            store_name("Microsoft.WindowsTerminal_8wekyb3d8bbwe"),
            "WindowsTerminal"
        );
    }
}

#[cfg(test)]
mod real {
    /// Read-only: `cargo test --release real_startup -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_startup() {
        let started = std::time::Instant::now();
        for i in super::list() {
            println!(
                "{:<5} {:<30} {:<9} all={} missing={} {:?} | {:?} | {}",
                i.enabled,
                i.name.chars().take(30).collect::<String>(),
                i.source,
                i.all_users,
                i.missing,
                i.advice.map(|a| a.kind),
                i.publisher,
                i.path.unwrap_or_default()
            );
        }
        println!("took {:?}", started.elapsed());
    }
}

#[cfg(test)]
mod real_toggle {
    use super::*;

    /// Switches a throwaway HKCU Run entry off and on, then removes it:
    /// `cargo test --release real_toggle -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_toggle_round_trip() {
        let name = "DiskMateToggleTest";
        let id = format!("hkcu-run|{name}");
        let (run, _) = hive(false).create_subkey(RUN).unwrap();
        run.set_value(name, &r"C:\Windows\System32\notepad.exe".to_string())
            .unwrap();
        let state = || list().into_iter().find(|i| i.id == id).map(|i| i.enabled);
        assert_eq!(state(), Some(true), "a new entry starts enabled");
        set_enabled(&id, false).unwrap();
        assert_eq!(state(), Some(false), "switched off");
        set_enabled(&id, true).unwrap();
        assert_eq!(state(), Some(true), "switched back on");
        run.delete_value(name).unwrap();
        let _ = hive(false)
            .open_subkey_with_flags(format!(r"{APPROVED}\Run"), winreg::enums::KEY_ALL_ACCESS)
            .and_then(|k| k.delete_value(name));
        println!("toggle ok");
    }
}
