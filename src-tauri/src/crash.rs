//! Crash dumps. When DiskMate itself crashes — an access violation, a stack overflow, or a panic —
//! a minidump goes to `crashes\` in the data folder. With the release's .pdb (attached to every
//! GitHub release) it shows the stack and the line it died on, instead of a guess from "it closed
//! when I clicked delete".

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicIsize, AtomicPtr, AtomicU32, Ordering};
use std::sync::OnceLock;
use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Diagnostics::Debug::{
    MiniDumpScanMemory, MiniDumpWithIndirectlyReferencedMemory, MiniDumpWithThreadInfo,
    MiniDumpWithUnloadedModules, MiniDumpWriteDump, SetUnhandledExceptionFilter,
    EXCEPTION_POINTERS, MINIDUMP_EXCEPTION_INFORMATION, MINIDUMP_TYPE,
};
use windows::Win32::System::Threading::{
    CreateEventW, GetCurrentProcess, GetCurrentProcessId, GetCurrentThreadId, SetEvent,
    WaitForSingleObject, INFINITE,
};

/// Dumps kept; older ones are deleted as new ones arrive.
const KEEP: usize = 5;

static DIR: OnceLock<PathBuf> = OnceLock::new();

// The crashed thread hands its exception to a thread started beforehand, which writes the dump: a
// thread that overflowed its stack has no room left to do it itself.
static REQUEST: AtomicIsize = AtomicIsize::new(0);
static DONE: AtomicIsize = AtomicIsize::new(0);
static EXCEPTION: AtomicPtr<EXCEPTION_POINTERS> = AtomicPtr::new(std::ptr::null_mut());
static THREAD: AtomicU32 = AtomicU32::new(0);

/// Starts writing a dump on any crash from here on. Once per process.
pub fn init(data_dir: &Path) {
    if DIR.set(data_dir.join("crashes")).is_err() {
        return;
    }
    let (Ok(request), Ok(done)) = (unsafe { CreateEventW(None, false, false, None) }, unsafe {
        CreateEventW(None, false, false, None)
    }) else {
        return;
    };
    REQUEST.store(request.0 as isize, Ordering::SeqCst);
    DONE.store(done.0 as isize, Ordering::SeqCst);
    let started = std::thread::Builder::new()
        .name("diskmate-crash-dump".into())
        .spawn(|| loop {
            let request = HANDLE(REQUEST.load(Ordering::SeqCst) as *mut _);
            if unsafe { WaitForSingleObject(request, INFINITE) } != WAIT_OBJECT_0 {
                return;
            }
            let ptrs = EXCEPTION.load(Ordering::SeqCst);
            write_dump(Some((THREAD.load(Ordering::SeqCst), ptrs)));
            let _ = unsafe { SetEvent(HANDLE(DONE.load(Ordering::SeqCst) as *mut _)) };
        });
    if started.is_ok() {
        unsafe { SetUnhandledExceptionFilter(Some(on_crash)) };
    }
}

/// The last stop of an exception nothing handled. Writes the dump, then lets Windows carry on as
/// it would have: the crash still reaches Windows Error Reporting and the event log.
unsafe extern "system" fn on_crash(info: *const EXCEPTION_POINTERS) -> i32 {
    const EXCEPTION_CONTINUE_SEARCH: i32 = 0;
    let request = REQUEST.load(Ordering::SeqCst);
    if request == 0 {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    EXCEPTION.store(info as *mut _, Ordering::SeqCst);
    THREAD.store(unsafe { GetCurrentThreadId() }, Ordering::SeqCst);
    unsafe {
        let _ = SetEvent(HANDLE(request as *mut _));
        // Bounded: a dump that cannot be written must not keep the process from ending.
        WaitForSingleObject(HANDLE(DONE.load(Ordering::SeqCst) as *mut _), 30_000);
    }
    EXCEPTION_CONTINUE_SEARCH
}

/// Writes a dump of this process: of the crash when `exception` (thread id, exception) is given,
/// otherwise of where every thread is right now (used for panics, from the panic hook).
pub fn write_dump(exception: Option<(u32, *mut EXCEPTION_POINTERS)>) -> Option<PathBuf> {
    use std::os::windows::io::AsRawHandle;
    let dir = DIR.get()?;
    std::fs::create_dir_all(dir).ok()?;
    prune(dir, KEEP - 1);
    let path = dir.join(format!(
        "diskmate-{}-{}-{}.dmp",
        env!("CARGO_PKG_VERSION"),
        crate::util::unix_secs(std::time::SystemTime::now()),
        std::process::id()
    ));
    let file = std::fs::File::create(&path).ok()?;
    let info = exception.map(|(thread, ptrs)| MINIDUMP_EXCEPTION_INFORMATION {
        ThreadId: thread,
        ExceptionPointers: ptrs,
        ClientPointers: false.into(),
    });
    // Stacks, plus the memory they point at: enough to read locals, a few MB rather than the
    // whole process.
    let kind = MINIDUMP_TYPE(
        MiniDumpWithThreadInfo.0
            | MiniDumpWithUnloadedModules.0
            | MiniDumpWithIndirectlyReferencedMemory.0
            | MiniDumpScanMemory.0,
    );
    let written = unsafe {
        MiniDumpWriteDump(
            GetCurrentProcess(),
            GetCurrentProcessId(),
            HANDLE(file.as_raw_handle()),
            kind,
            info.as_ref().map(|i| i as *const _),
            None,
            None,
        )
    };
    drop(file);
    if written.is_err() {
        let _ = std::fs::remove_file(&path);
        return None;
    }
    Some(path)
}

/// Where the name of the last dump the user was told about is kept.
const SEEN: &str = "seen.txt";

/// The newest dump the user has not been told about yet: DiskMate offers it on the next launch,
/// since a crashed process can show nothing.
pub fn unseen() -> Option<PathBuf> {
    let dir = DIR.get()?;
    let newest = newest_dump(dir)?;
    let seen = std::fs::read_to_string(dir.join(SEEN)).unwrap_or_default();
    (seen.trim() != newest.file_name()?.to_string_lossy()).then_some(newest)
}

/// The user has been told about every dump there is now.
pub fn mark_seen() {
    let Some(dir) = DIR.get() else { return };
    if let Some(name) = newest_dump(dir).and_then(|p| p.file_name().map(|n| n.to_owned())) {
        let _ = std::fs::write(dir.join(SEEN), name.to_string_lossy().as_bytes());
    }
}

fn newest_dump(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "dmp"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .max_by_key(|(t, _)| *t)
        .map(|(_, p)| p)
}

/// Deletes all but the `keep` newest dumps in `dir`.
fn prune(dir: &Path, keep: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut dumps: Vec<(std::time::SystemTime, PathBuf)> = rd
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "dmp"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    dumps.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, old) in dumps.into_iter().skip(keep) {
        let _ = std::fs::remove_file(old);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Crashes a copy of this test binary for real and looks for the dump it left.
    #[test]
    fn a_crash_leaves_a_dump() {
        if let Some(dir) = std::env::var_os("DISKMATE_CRASH_CHILD") {
            use windows::Win32::System::Diagnostics::Debug::{
                SetErrorMode, SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX,
            };
            // No "has stopped working" window for this deliberate crash.
            unsafe { SetErrorMode(SEM_NOGPFAULTERRORBOX | SEM_FAILCRITICALERRORS) };
            init(Path::new(&dir));
            // An access violation, as a bad pointer in a shell extension would cause.
            unsafe { std::ptr::write_volatile(8 as *mut u8, 1) };
            unreachable!("the write above crashes the process");
        }
        let dir = tempfile::tempdir().unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "crash::tests::a_crash_leaves_a_dump",
                "--exact",
                "--test-threads=1",
            ])
            .env("DISKMATE_CRASH_CHILD", dir.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(!status.success(), "the child crashed");
        let dumps: Vec<PathBuf> = std::fs::read_dir(dir.path().join("crashes"))
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .collect();
        assert_eq!(dumps.len(), 1, "{dumps:?}");
        let bytes = std::fs::read(&dumps[0]).unwrap();
        assert!(bytes.starts_with(b"MDMP"), "a minidump");
        println!("dump: {} KB", bytes.len() / 1024);
        // The exception stream is what a debugger opens on to show the crashing line: it must be
        // there and name the access violation.
        let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let (streams, directory) = (u32_at(8) as usize, u32_at(12) as usize);
        const EXCEPTION_STREAM: u32 = 6;
        let exception = (0..streams)
            .map(|i| directory + i * 12)
            .find(|&e| u32_at(e) == EXCEPTION_STREAM)
            .map(|e| u32_at(e + 8) as usize)
            .expect("the dump records the exception");
        // MINIDUMP_EXCEPTION_STREAM: thread id, alignment, then the record, code first.
        assert_eq!(u32_at(exception + 8), 0xC000_0005, "access violation");
    }

    #[test]
    fn only_the_newest_dumps_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..7 {
            let f = dir.path().join(format!("d{i}.dmp"));
            std::fs::write(&f, b"x").unwrap();
            let t = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000 + i);
            std::fs::File::options()
                .write(true)
                .open(&f)
                .unwrap()
                .set_modified(t)
                .unwrap();
        }
        std::fs::write(dir.path().join("notes.txt"), b"keep me").unwrap();
        prune(dir.path(), 4);
        let mut left: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, ["d3.dmp", "d4.dmp", "d5.dmp", "d6.dmp", "notes.txt"]);
    }
}
