//! Path and size helpers shared by the scanner, the rules and the cleaner.

use rayon::prelude::*;
use std::fs::Metadata;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const FILE_ATTRIBUTE_OFFLINE: u32 = 0x1000;
const FILE_ATTRIBUTE_RECALL_ON_OPEN: u32 = 0x40000;
const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x400000;

/// The `\\?\` form of an absolute path.
///
/// Every file operation goes through this. Without it Windows caps paths at 260 characters, and
/// Android/Node build output nests deep enough to blow past that — which is exactly the
/// "Filename too long" that leaves a half-deleted folder behind.
pub fn long_path(p: &Path) -> PathBuf {
    let s = p.as_os_str().to_string_lossy();
    if s.starts_with(r"\\?\") {
        return p.to_path_buf();
    }
    if let Some(rest) = s.strip_prefix(r"\\") {
        return PathBuf::from(format!(r"\\?\UNC\{rest}"));
    }
    PathBuf::from(format!(r"\\?\{}", s.replace('/', "\\")))
}

/// A path as the user should see it: no `\\?\` prefix.
pub fn display(p: &Path) -> String {
    let s = p.to_string_lossy();
    s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
}

/// Symlinks and junctions. Followed, they double-count (`Application Data` points back into
/// AppData) or loop forever.
pub fn is_link(md: &Metadata) -> bool {
    md.file_type().is_symlink() || md.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

const FILE_ATTRIBUTE_SPARSE_FILE: u32 = 0x200;
const FILE_ATTRIBUTE_COMPRESSED: u32 = 0x800;

/// Bytes the file actually takes on this disk, which is not always its length:
///
/// - a OneDrive "online only" placeholder reports its full size but stores nothing locally;
/// - a sparse file (virtual disks, VM images) or an NTFS-compressed one stores less than its length.
///
/// Only those two cases cost an extra system call, so the common file stays as cheap as the
/// directory listing it came from.
pub fn on_disk_len_at(path: &Path, md: &Metadata) -> u64 {
    let attrs = md.file_attributes();
    let cloud = FILE_ATTRIBUTE_OFFLINE
        | FILE_ATTRIBUTE_RECALL_ON_OPEN
        | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS;
    if attrs & cloud != 0 {
        return 0;
    }
    if attrs & (FILE_ATTRIBUTE_SPARSE_FILE | FILE_ATTRIBUTE_COMPRESSED) != 0 {
        if let Some(n) = compressed_size(path) {
            return n;
        }
    }
    md.len()
}

/// [`on_disk_len_at`] when the path is not at hand; only the cloud-placeholder case is handled.
pub fn on_disk_len(md: &Metadata) -> u64 {
    let attrs = md.file_attributes();
    let cloud = FILE_ATTRIBUTE_OFFLINE
        | FILE_ATTRIBUTE_RECALL_ON_OPEN
        | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS;
    if attrs & cloud != 0 {
        0
    } else {
        md.len()
    }
}

fn compressed_size(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetCompressedFileSizeW;
    let wide: Vec<u16> = long_path(path)
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut high = 0u32;
    let low = unsafe { GetCompressedFileSizeW(PCWSTR(wide.as_ptr()), Some(&mut high)) };
    // INVALID_FILE_SIZE with a zero high word is an error; fall back to the plain length.
    if low == u32::MAX && high == 0 {
        return None;
    }
    Some(((high as u64) << 32) | low as u64)
}

/// (volume serial, file index) of a file with more than one hard link, or None for an ordinary
/// file. Two paths with the same identity are the same bytes on disk.
pub fn hard_link_identity(path: &Path) -> Option<(u32, u64)> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };
    const FILE_READ_ATTRIBUTES: u32 = 0x80;
    let file = std::fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(0x7)
        .open(long_path(path))
        .ok()?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }.ok()?;
    (info.nNumberOfLinks > 1).then_some({
        (
            info.dwVolumeSerialNumber,
            ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64,
        )
    })
}

fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

pub fn home() -> PathBuf {
    env_path("USERPROFILE").unwrap_or_else(|| PathBuf::from(r"C:\Users\Default"))
}

pub fn local_appdata() -> PathBuf {
    env_path("LOCALAPPDATA").unwrap_or_else(|| home().join(r"AppData\Local"))
}

pub fn roaming_appdata() -> PathBuf {
    env_path("APPDATA").unwrap_or_else(|| home().join(r"AppData\Roaming"))
}

pub fn program_data() -> PathBuf {
    env_path("ProgramData").unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"))
}

pub fn system_root() -> PathBuf {
    env_path("SystemRoot").unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
}

pub fn system_drive() -> PathBuf {
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    PathBuf::from(format!(r"{drive}\"))
}

#[derive(Default, Clone, Copy)]
pub struct Size {
    pub bytes: u64,
    pub files: u64,
}

impl std::ops::Add for Size {
    type Output = Size;
    fn add(self, o: Size) -> Size {
        Size {
            bytes: self.bytes + o.bytes,
            files: self.files + o.files,
        }
    }
}

/// Total size of everything under `path`, in parallel. With `cutoff`, only files last modified
/// before it count — the same filter the cleaner applies, so the number shown is the number freed.
pub fn measure(path: &Path, cutoff: Option<SystemTime>) -> Size {
    let Ok(rd) = std::fs::read_dir(long_path(path)) else {
        return Size::default();
    };
    let mut own = Size::default();
    let mut dirs = Vec::new();
    for entry in rd.flatten() {
        let Ok(md) = entry.metadata() else { continue };
        if is_link(&md) {
            continue;
        }
        if md.is_dir() {
            dirs.push(entry.path());
            continue;
        }
        if let Some(cutoff) = cutoff {
            if md.modified().map(|m| m > cutoff).unwrap_or(true) {
                continue;
            }
        }
        own.bytes += on_disk_len_at(&entry.path(), &md);
        own.files += 1;
    }
    own + dirs
        .par_iter()
        .map(|d| measure(d, cutoff))
        .reduce(Size::default, |a, b| a + b)
}

/// Size of one file, read through the directory listing so it works on files another process has
/// locked (pagefile.sys, hiberfil.sys).
pub fn file_len(path: &Path) -> Option<u64> {
    let parent = path.parent()?;
    let name = path.file_name()?;
    std::fs::read_dir(long_path(parent))
        .ok()?
        .flatten()
        .find(|e| e.file_name().eq_ignore_ascii_case(name))
        .and_then(|e| e.metadata().ok())
        .map(|md| md.len())
}

/// Newest modification time among the direct children of `dir`, skipping `skip` names.
pub fn newest_mtime(dir: &Path, skip: &[&str]) -> Option<SystemTime> {
    std::fs::read_dir(long_path(dir))
        .ok()?
        .flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().to_lowercase();
            !skip.iter().any(|s| *s == n)
        })
        .filter_map(|e| e.metadata().ok()?.modified().ok())
        .max()
}

pub fn days_ago(days: u64) -> SystemTime {
    SystemTime::now() - std::time::Duration::from_secs(days * 86_400)
}

pub fn unix_secs(t: SystemTime) -> u64 {
    t.duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Runs a command without flashing a console window, below normal priority: PowerShell queries,
/// robocopy and the like are DiskMate's chores and should give way to whatever the user is doing.
pub fn command(program: &str) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
    let mut cmd = std::process::Command::new(program);
    cmd.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);
    cmd
}

/// Puts the calling thread in background mode: lowest CPU priority, and low disk and memory
/// priority too, so a scan yields to other programs instead of competing with them.
pub fn background_thread() {
    use windows::Win32::System::Threading::{
        GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN,
    };
    let _ = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN) };
}

/// Worker threads for scans and measuring. Walking directories stops getting faster at about
/// four threads (NTFS serializes much of it); beyond that the extra threads only burn CPU —
/// measured on a 20-thread i5: a full C: scan took 97–136 CPU-seconds on 20 threads and
/// 68–75 on 4, in about the same time.
/// Every running process as (pid, lowercase exe name), from a Toolhelp snapshot: a few
/// milliseconds, where running tasklist.exe took the better part of a second.
pub fn processes() -> Vec<(u32, String)> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    let mut out = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return out;
        };
        let mut e = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut ok = Process32FirstW(snap, &mut e).is_ok();
        while ok {
            let len = e
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(e.szExeFile.len());
            out.push((
                e.th32ProcessID,
                String::from_utf16_lossy(&e.szExeFile[..len]).to_lowercase(),
            ));
            ok = Process32NextW(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    out
}

/// A second, smaller pool for work nobody is waiting on (measuring every installed app after the
/// list is shown), so it never holds up what the user just asked for in the main pool.
pub fn passive_pool() -> &'static rayon::ThreadPool {
    static POOL: std::sync::OnceLock<rayon::ThreadPool> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .thread_name(|i| format!("diskmate-passive-{i}"))
            .start_handler(|_| background_thread())
            .build()
            .expect("thread pool")
    })
}

pub fn init_worker_pool() {
    let logical = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let _ = rayon::ThreadPoolBuilder::new()
        .num_threads((logical / 4).clamp(2, 6))
        .thread_name(|i| format!("diskmate-worker-{i}"))
        .start_handler(|_| background_thread())
        .build_global();
}
