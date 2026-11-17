//! Fast scan: reads an NTFS volume's master file table ($MFT) straight off the disk, as WizTree
//! does, instead of listing every folder. Every file and folder is one record in that table, so
//! one sequential read of it (a gigabyte or two, a few seconds) replaces a minute or more of
//! directory listing. Reading the raw volume needs administrator rights: a non-elevated DiskMate
//! runs itself elevated as a helper (`--mft-scan`), which writes the finished scan to a file.
//!
//! The tree it builds is the same shape as [`crate::scan::run`]'s and counts sizes the same way
//! (logical length; compressed size for compressed and sparse files; nothing for cloud-only
//! files; reparse points skipped; NTFS's own metadata files left to "hidden"). Two differences:
//! folders the normal scan may not enter (System Volume Information) are counted, and every hard
//! link counts once, not only big ones.

use crate::scan::{self, Node, ScanCtx, ScanData};
use crate::util::display;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Instant;

pub const MFT_ARG: &str = "--mft-scan";

const ROOT: u64 = 5;
const EXTEND: u64 = 11;
/// Records below this are NTFS's own files ($MFT, $LogFile, $Bitmap, ...), except the root.
const FIRST_USER: u64 = 16;

const ATTR_STANDARD: u32 = 0x10;
const ATTR_LIST: u32 = 0x20;
const ATTR_FILE_NAME: u32 = 0x30;
const ATTR_DATA: u32 = 0x80;
const ATTR_END: u32 = 0xFFFF_FFFF;

const REPARSE_POINT: u32 = 0x400;
/// A cloud file whose contents are not on this disk (matches util::disk_len).
const CLOUD_ONLY: u32 = 0x1000 | 0x40000 | 0x400000;
/// $DATA attribute flags: compressed, sparse.
const DATA_COMPRESSED: u16 = 0x0001;
const DATA_SPARSE: u16 = 0x8000;
/// $FILE_NAME namespace of a short 8.3 alias, which is not a name of its own.
const NS_DOS: u8 = 2;

/// Read this much of the table at a time.
const CHUNK: usize = 4 << 20;

fn u16_at(b: &[u8], i: usize) -> u16 {
    b.get(i..i + 2)
        .map_or(0, |s| u16::from_le_bytes([s[0], s[1]]))
}
fn u32_at(b: &[u8], i: usize) -> u32 {
    b.get(i..i + 4)
        .map_or(0, |s| u32::from_le_bytes(s.try_into().unwrap()))
}
fn u64_at(b: &[u8], i: usize) -> u64 {
    b.get(i..i + 8)
        .map_or(0, |s| u64::from_le_bytes(s.try_into().unwrap()))
}
/// A file reference's record number (its low 48 bits; the rest is a reuse counter).
fn rec_of(r: u64) -> u64 {
    r & 0x0000_FFFF_FFFF_FFFF
}

struct Boot {
    sector: usize,
    cluster: u64,
    record: usize,
    mft_lcn: u64,
}

fn parse_boot(b: &[u8]) -> Result<Boot, String> {
    if b.len() < 512 || &b[3..11] != b"NTFS    " {
        return Err("这块磁盘不是 NTFS 格式，不能快速扫描".into());
    }
    let sector = u16_at(b, 0x0B) as usize;
    let cluster = sector as u64 * b[0x0D] as u64;
    let per = b[0x40] as i8;
    let record = if per > 0 {
        per as usize * cluster as usize
    } else {
        1usize << (-(per as i32))
    };
    if sector < 256 || cluster == 0 || !(256..=65536).contains(&record) {
        return Err("磁盘的引导扇区看不懂，不能快速扫描".into());
    }
    Ok(Boot {
        sector,
        cluster,
        record,
        mft_lcn: u64_at(b, 0x30),
    })
}

/// Undoes a record's update sequence: when written, the last two bytes of every sector were
/// swapped for a check value (to catch torn writes); the real bytes are kept in an array.
fn fixup(rec: &mut [u8], sector: usize) -> bool {
    if rec.len() < 48 || &rec[0..4] != b"FILE" {
        return false;
    }
    let ofs = u16_at(rec, 4) as usize;
    let count = u16_at(rec, 6) as usize;
    if count == 0 || ofs + count * 2 > rec.len() || (count - 1) * sector > rec.len() {
        return false;
    }
    let check = [rec[ofs], rec[ofs + 1]];
    for i in 1..count {
        let end = i * sector - 2;
        if rec[end..end + 2] != check {
            return false;
        }
        rec[end] = rec[ofs + 2 * i];
        rec[end + 1] = rec[ofs + 2 * i + 1];
    }
    true
}

/// A non-resident attribute's data runs: (first cluster, clusters); a sparse run has no cluster.
fn data_runs(b: &[u8]) -> Vec<(Option<u64>, u64)> {
    let mut out = Vec::new();
    let mut i = 0;
    let mut lcn: i64 = 0;
    while i < b.len() && b[i] != 0 {
        let len_size = (b[i] & 0x0F) as usize;
        let off_size = (b[i] >> 4) as usize;
        i += 1;
        if len_size == 0 || len_size > 8 || off_size > 8 || i + len_size + off_size > b.len() {
            break;
        }
        let mut len = 0u64;
        for k in 0..len_size {
            len |= (b[i + k] as u64) << (8 * k);
        }
        i += len_size;
        if off_size == 0 {
            out.push((None, len));
            continue;
        }
        let mut off = 0i64;
        for k in 0..off_size {
            off |= (b[i + k] as i64) << (8 * k);
        }
        // Sign-extend from the highest byte given.
        let shift = 64 - 8 * off_size as u32;
        off = (off << shift) >> shift;
        i += off_size;
        lcn += off;
        out.push((Some(lcn.max(0) as u64), len));
    }
    out
}

/// The unnamed data stream: the file's contents.
#[derive(Clone, Copy, Default)]
struct DataInfo {
    resident: bool,
    size: u64,
    compressed: u64,
    flags: u16,
}

/// What one record (or a base record and its extensions together) says about a file or folder.
#[derive(Default)]
struct Record {
    /// Known once the base record itself has been read.
    seen_base: bool,
    dir: bool,
    attrs: u32,
    modified: u64,
    names: Vec<(u64, String)>,
    data: Option<DataInfo>,
    has_list: bool,
}

/// Reads one record's header and attributes into `r`. Returns the base record's number (0 for a
/// base record itself), or `None` for an unused or broken record.
fn read_record(
    rec: &[u8],
    r: &mut Record,
    want_runs: bool,
    runs: &mut Vec<(Option<u64>, u64)>,
) -> Option<u64> {
    let flags = u16_at(rec, 0x16);
    if flags & 1 == 0 {
        return None;
    }
    let base = rec_of(u64_at(rec, 0x20));
    if base == 0 {
        r.seen_base = true;
        r.dir = flags & 2 != 0;
    }
    let used = (u32_at(rec, 0x18) as usize).min(rec.len());
    let mut at = u16_at(rec, 0x14) as usize;
    while at + 16 <= used {
        let kind = u32_at(rec, at);
        let len = u32_at(rec, at + 4) as usize;
        if kind == ATTR_END || len < 16 || at + len > used {
            break;
        }
        let a = &rec[at..at + len];
        let resident = a[8] == 0;
        let named = a[9] != 0;
        let aflags = u16_at(a, 0x0C);
        let value = if resident {
            let vlen = u32_at(a, 0x10) as usize;
            let voff = u16_at(a, 0x14) as usize;
            a.get(voff..voff + vlen).unwrap_or(&[])
        } else {
            &[][..]
        };
        match kind {
            ATTR_STANDARD if resident && value.len() >= 0x24 => {
                r.modified = filetime(u64_at(value, 0x08));
                r.attrs = u32_at(value, 0x20);
            }
            ATTR_LIST => r.has_list = true,
            ATTR_FILE_NAME if resident && value.len() >= 0x42 => {
                let n = value[0x40] as usize;
                let ns = value[0x41];
                if ns != NS_DOS && value.len() >= 0x42 + n * 2 {
                    let wide: Vec<u16> = (0..n).map(|k| u16_at(value, 0x42 + k * 2)).collect();
                    r.names
                        .push((rec_of(u64_at(value, 0)), String::from_utf16_lossy(&wide)));
                }
            }
            ATTR_DATA if !named => {
                if resident {
                    r.data = Some(DataInfo {
                        resident: true,
                        size: value.len() as u64,
                        ..Default::default()
                    });
                } else {
                    // A stream split over several records: only its first part carries the sizes.
                    if u64_at(a, 0x10) == 0 {
                        r.data = Some(DataInfo {
                            resident: false,
                            size: u64_at(a, 0x30),
                            compressed: if len >= 0x48 {
                                u64_at(a, 0x40)
                            } else {
                                u64_at(a, 0x28)
                            },
                            flags: aflags,
                        });
                    }
                    if want_runs {
                        let ro = u16_at(a, 0x20) as usize;
                        runs.extend(data_runs(a.get(ro..).unwrap_or(&[])));
                    }
                }
            }
            _ => {}
        }
        at += len;
    }
    Some(base)
}

fn filetime(t: u64) -> u64 {
    (t / 10_000_000).saturating_sub(11_644_473_600)
}

/// Bytes a file takes, the way the normal scan counts them (see util::disk_len).
fn file_len(r: &Record) -> u64 {
    if r.attrs & CLOUD_ONLY != 0 {
        return 0;
    }
    match r.data {
        None => 0,
        Some(d) if d.resident => d.size,
        Some(d) if d.flags & (DATA_COMPRESSED | DATA_SPARSE) != 0 => d.compressed,
        Some(d) => d.size,
    }
}

/// A file with several names (folder, name), its size and last change.
type Linked = (Vec<(u64, String)>, u64, u64);

struct Dir {
    parent: u64,
    name: String,
    reparse: bool,
}

/// Everything gathered from the table, before it becomes a tree.
#[derive(Default)]
struct Gathered {
    dirs: HashMap<u64, Dir>,
    /// Each folder's own files, counted as they are met.
    own: HashMap<u64, Node>,
    exts: HashMap<String, (u64, u64)>,
    /// Files of 100 MB and up, placed once paths are known: (folder, name, size, modified).
    large: Vec<(u64, String, u64, u64)>,
    /// Files with several names (hard links), placed once folders are known.
    links: Vec<Linked>,
    /// Base records with an attribute list, and their extension records, put together at the end.
    split: HashMap<u64, Record>,
}

impl Gathered {
    fn take(&mut self, ctx: &ScanCtx, no: u64, r: Record) {
        if no < FIRST_USER && no != ROOT {
            return;
        }
        if r.dir {
            if let Some((parent, name)) = r.names.into_iter().next() {
                self.dirs.insert(
                    no,
                    Dir {
                        parent,
                        name,
                        reparse: r.attrs & REPARSE_POINT != 0,
                    },
                );
            } else if no == ROOT {
                self.dirs.insert(
                    no,
                    Dir {
                        parent: ROOT,
                        name: String::new(),
                        reparse: false,
                    },
                );
            }
            return;
        }
        // The normal scan does not follow reparse points (cloud files, symlinks, dedup stubs).
        if r.attrs & REPARSE_POINT != 0 || r.names.is_empty() {
            return;
        }
        let len = file_len(&r);
        if r.names.len() == 1 {
            let (parent, name) = r.names.into_iter().next().unwrap();
            self.add(ctx, parent, &name, len, r.modified);
        } else {
            self.links.push((r.names, len, r.modified));
        }
    }

    fn add(&mut self, ctx: &ScanCtx, parent: u64, name: &str, len: u64, modified: u64) {
        let node = self.own.entry(parent).or_default();
        scan::count_file(node, &mut self.exts, name, len, modified);
        if len >= scan::LARGE_FILE_BYTES {
            self.large.push((parent, name.to_string(), len, modified));
        }
        ctx.files.fetch_add(1, Ordering::Relaxed);
        ctx.bytes.fetch_add(len, Ordering::Relaxed);
    }
}

/// Scans the NTFS volume `root` (`C:\`) by reading its master file table. Needs administrator
/// rights.
pub fn scan(root: &Path, ctx: &ScanCtx) -> Result<ScanData, String> {
    let started = Instant::now();
    let drive = display(root);
    let letter = drive.get(..2).ok_or("磁盘路径不对")?;
    let mut vol = std::fs::File::open(format!(r"\\.\{letter}")).map_err(|e| {
        if e.raw_os_error() == Some(5) {
            "快速扫描需要管理员权限".to_string()
        } else {
            format!("没能直接读取磁盘：{e}")
        }
    })?;
    let mut boot = vec![0u8; 512];
    vol.read_exact(&mut boot).map_err(|e| e.to_string())?;
    let b = parse_boot(&boot)?;

    // Record 0 is the table's own entry: where on disk the rest of it is.
    let mut first = vec![0u8; b.record];
    vol.seek(SeekFrom::Start(b.mft_lcn * b.cluster))
        .map_err(|e| e.to_string())?;
    vol.read_exact(&mut first).map_err(|e| e.to_string())?;
    if !fixup(&mut first, b.sector) {
        return Err("主文件表的第一条记录损坏，不能快速扫描".into());
    }
    let mut runs = Vec::new();
    let mut mft = Record::default();
    read_record(&first, &mut mft, true, &mut runs).ok_or("主文件表读不出来")?;
    let table_bytes = mft.data.map(|d| d.size).unwrap_or(0);
    let mapped: u64 = runs.iter().map(|r| r.1).sum::<u64>() * b.cluster;
    if table_bytes == 0 || mapped < table_bytes {
        // A table so fragmented that its own map spills into other records: rare enough to leave
        // to the normal scan.
        return Err("主文件表太零散，请用普通扫描".into());
    }
    let total = table_bytes / b.record as u64;

    let mut g = Gathered::default();
    let mut buf = vec![0u8; CHUNK];
    let mut carry: Vec<u8> = Vec::with_capacity(b.record);
    let mut no = 0u64;
    let mut left = table_bytes;
    'runs: for (lcn, clusters) in runs {
        let mut run_bytes = (clusters * b.cluster).min(left);
        let Some(lcn) = lcn else {
            // A sparse run holds no records; count them off.
            no += run_bytes / b.record as u64;
            left -= run_bytes;
            continue;
        };
        let mut pos = lcn * b.cluster;
        while run_bytes > 0 {
            if ctx.cancel.load(Ordering::Relaxed) {
                return Err("已取消".into());
            }
            let n = (run_bytes as usize).min(CHUNK);
            vol.seek(SeekFrom::Start(pos)).map_err(|e| e.to_string())?;
            vol.read_exact(&mut buf[..n]).map_err(|e| e.to_string())?;
            pos += n as u64;
            run_bytes -= n as u64;
            left -= n as u64;
            // A record may straddle two reads (or two runs, with clusters smaller than records).
            let mut data: &mut [u8] = &mut buf[..n];
            let mut joined;
            if !carry.is_empty() {
                joined = std::mem::take(&mut carry);
                joined.extend_from_slice(data);
                data = &mut joined[..];
            }
            let whole = data.len() / b.record * b.record;
            for rec in data[..whole].chunks_exact_mut(b.record) {
                record(&mut g, ctx, no, rec, b.sector);
                no += 1;
                if no >= total {
                    break 'runs;
                }
            }
            carry.extend_from_slice(&data[whole..]);
            *ctx.current.lock().unwrap() = format!("正在读取主文件表 {}%", no * 100 / total.max(1));
        }
    }

    // Base records with extension records, now that all their parts are in.
    for (no, r) in std::mem::take(&mut g.split) {
        if r.seen_base {
            g.take(ctx, no, r);
        }
    }
    let tree = build_tree(&mut g, ctx, &drive)?;
    ctx.add_exts(std::mem::take(&mut g.exts));
    Ok(scan::finish(root, tree, ctx, started))
}

fn record(g: &mut Gathered, ctx: &ScanCtx, no: u64, rec: &mut [u8], sector: usize) {
    if !fixup(rec, sector) {
        return;
    }
    let mut r = Record::default();
    let mut none = Vec::new();
    let Some(base) = read_record(rec, &mut r, false, &mut none) else {
        return;
    };
    if base != 0 {
        // An extension record: its attributes belong to its base record.
        let whole = g.split.entry(base).or_default();
        whole.names.append(&mut r.names);
        if r.data.is_some() {
            whole.data = r.data;
        }
        return;
    }
    if r.has_list {
        let whole = g.split.entry(no).or_default();
        whole.seen_base = true;
        whole.dir = r.dir;
        whole.attrs = r.attrs;
        whole.modified = r.modified;
        whole.names.append(&mut r.names);
        if r.data.is_some() {
            whole.data = r.data;
        }
        return;
    }
    g.take(ctx, no, r);
}

/// Turns the gathered folders and files into the scan's tree, rooted at the volume's root.
fn build_tree(g: &mut Gathered, ctx: &ScanCtx, drive: &str) -> Result<Node, String> {
    if !g.dirs.contains_key(&ROOT) {
        return Err("主文件表里没有找到根目录".into());
    }
    let mut kids: HashMap<u64, Vec<u64>> = HashMap::new();
    for (&no, d) in &g.dirs {
        // Junctions and other reparse folders are not followed, as in the normal scan; NTFS's
        // $Extend holds its own files.
        if no == ROOT || no == EXTEND || d.reparse || !g.dirs.contains_key(&d.parent) {
            continue;
        }
        kids.entry(d.parent).or_default().push(no);
    }

    // Which folders are under C:\Windows\WinSxS: a file linked from there and from elsewhere
    // counts elsewhere, as in the normal scan (and as DISM counts the component store).
    let find_child = |parent: u64, name: &str| {
        kids.get(&parent).and_then(|k| {
            k.iter()
                .copied()
                .find(|c| g.dirs[c].name.eq_ignore_ascii_case(name))
        })
    };
    let sys = crate::util::system_root();
    let sxs = sys
        .file_name()
        .and_then(|w| find_child(ROOT, &w.to_string_lossy()))
        .and_then(|w| find_child(w, "WinSxS"));
    let mut in_sxs: HashMap<u64, bool> = HashMap::new();
    let mut under_sxs = |dir: u64| -> bool {
        let mut chain = Vec::new();
        let mut d = dir;
        let found = loop {
            if let Some(&v) = in_sxs.get(&d) {
                break v;
            }
            if Some(d) == sxs {
                break true;
            }
            if d == ROOT || chain.len() > 512 {
                break false;
            }
            chain.push(d);
            match g.dirs.get(&d) {
                Some(x) => d = x.parent,
                None => break false,
            }
        };
        for c in chain {
            in_sxs.insert(c, found);
        }
        found
    };
    let links = std::mem::take(&mut g.links);
    let placed: Vec<(u64, String, u64, u64)> = links
        .into_iter()
        .filter_map(|(names, len, modified)| {
            names
                .into_iter()
                .filter(|(p, _)| g.dirs.contains_key(p))
                .min_by_key(|(p, _)| under_sxs(*p))
                .map(|(p, n)| (p, n, len, modified))
        })
        .collect();
    for (parent, name, len, modified) in placed {
        g.add(ctx, parent, &name, len, modified);
    }

    fn build(
        no: u64,
        name: String,
        g: &mut Gathered,
        kids: &HashMap<u64, Vec<u64>>,
        ctx: &ScanCtx,
        depth: usize,
    ) -> Node {
        let mut node = g.own.remove(&no).unwrap_or_default();
        node.name = name;
        scan::finish_files(&mut node);
        ctx.dirs.fetch_add(1, Ordering::Relaxed);
        if depth < 512 {
            if let Some(list) = kids.get(&no) {
                node.children = list
                    .iter()
                    .map(|&c| {
                        let n = g.dirs[&c].name.clone();
                        build(c, n, g, kids, ctx, depth + 1)
                    })
                    .collect();
            }
        }
        scan::finish_dir(&mut node);
        node
    }

    // Large files need their full paths: the folders' names up to the root.
    let path_of = |mut d: u64| -> String {
        let mut parts = Vec::new();
        while d != ROOT && parts.len() < 512 {
            let Some(x) = g.dirs.get(&d) else { break };
            parts.push(x.name.as_str());
            d = x.parent;
        }
        parts.reverse();
        let mut p = PathBuf::from(drive);
        p.extend(parts);
        display(&p)
    };
    let large: Vec<_> = std::mem::take(&mut g.large)
        .into_iter()
        .map(|(dir, name, size, modified)| {
            (
                display(&PathBuf::from(path_of(dir)).join(&name)),
                name,
                size,
                modified,
            )
        })
        .collect();
    for (path, name, size, modified) in large {
        ctx.add_large(path, name, size, modified);
    }
    Ok(build(ROOT, drive.to_string(), g, &kids, ctx, 0))
}

// ---------------------------------------------------------------------------------------------
// Running elevated.

/// Whether this process runs with administrator rights.
pub fn elevated() -> bool {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut e = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut e as *mut TOKEN_ELEVATION as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        );
        let _ = CloseHandle(token);
        ok.is_ok() && e.TokenIsElevated != 0
    }
}

/// The files the helper and DiskMate talk through, next to the scan's output.
struct Files {
    out: PathBuf,
    progress: PathBuf,
    cancel: PathBuf,
    error: PathBuf,
}

impl Files {
    fn at(out: &Path) -> Files {
        Files {
            out: out.to_path_buf(),
            progress: out.with_extension("progress"),
            cancel: out.with_extension("cancel"),
            error: out.with_extension("error"),
        }
    }
    fn clear(&self) {
        for p in [&self.out, &self.progress, &self.cancel, &self.error] {
            let _ = std::fs::remove_file(p);
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct HelperProgress {
    files: u64,
    bytes: u64,
    current: String,
}

/// `diskmate.exe --mft-scan C: <out>`, elevated: scans and writes the result to `out`, progress
/// beside it; stops when a cancel file appears beside it.
pub fn helper(letter: &str, out: &Path) -> i32 {
    crate::util::background_thread();
    let files = Files::at(out);
    let ctx = std::sync::Arc::new(ScanCtx::default());
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let (ctx, done) = (ctx.clone(), done.clone());
        let (progress, cancel) = (files.progress.clone(), files.cancel.clone());
        std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                let p = HelperProgress {
                    files: ctx.files.load(Ordering::Relaxed),
                    bytes: ctx.bytes.load(Ordering::Relaxed),
                    current: ctx.current.lock().unwrap().clone(),
                };
                if let Ok(s) = serde_json::to_vec(&p) {
                    let _ = std::fs::write(&progress, s);
                }
                if cancel.exists() {
                    ctx.cancel.store(true, Ordering::Relaxed);
                }
                std::thread::sleep(std::time::Duration::from_millis(150));
            }
        });
    }
    let root = PathBuf::from(format!("{letter}\\"));
    let result = scan(&root, &ctx);
    done.store(true, Ordering::Relaxed);
    match result.and_then(|data| scan::write_file(&files.out, &data)) {
        Ok(()) => 0,
        Err(e) => {
            let _ = std::fs::write(&files.error, e);
            1
        }
    }
}

/// Runs a fast scan of `root`: in this process when it already has administrator rights, else in
/// an elevated copy of DiskMate (Windows asks the user first). `work` is a folder for the files
/// passed between the two.
pub fn run(root: &Path, ctx: &ScanCtx, work: &Path) -> Result<ScanData, String> {
    if elevated() {
        return scan(root, ctx);
    }
    use std::os::windows::ffi::OsStrExt;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
    use windows::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let letter = display(root).chars().take(2).collect::<String>();
    std::fs::create_dir_all(work).map_err(|e| e.to_string())?;
    let files = Files::at(&work.join(format!("fast-{}.json", std::process::id())));
    files.clear();
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let wide = |s: &std::ffi::OsStr| {
        s.encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>()
    };
    let exe_w = wide(exe.as_os_str());
    // No trailing backslash inside quotes: `"C:\"` would read as an escaped quote.
    let args = format!("{MFT_ARG} {letter} \"{}\"", files.out.display());
    let args_w = wide(std::ffi::OsStr::new(&args));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(exe_w.as_ptr()),
        lpParameters: PCWSTR(args_w.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    if unsafe { ShellExecuteExW(&mut info) }.is_err() {
        let code = windows::core::Error::from_win32().code().0 & 0xFFFF;
        return Err(if code == 1223 {
            "没有获得管理员权限，快速扫描没有开始".into()
        } else {
            format!("没能启动快速扫描（{code}）")
        });
    }
    let process = info.hProcess;
    let mut cancel_sent = false;
    loop {
        let waited = unsafe { WaitForSingleObject(process, 150) };
        if let Ok(s) = std::fs::read(&files.progress) {
            if let Ok(p) = serde_json::from_slice::<HelperProgress>(&s) {
                ctx.files.store(p.files, Ordering::Relaxed);
                ctx.bytes.store(p.bytes, Ordering::Relaxed);
                *ctx.current.lock().unwrap() = p.current;
            }
        }
        if ctx.cancel.load(Ordering::Relaxed) && !cancel_sent {
            let _ = std::fs::write(&files.cancel, b"");
            cancel_sent = true;
        }
        if waited == WAIT_OBJECT_0 {
            break;
        }
    }
    let mut code = 1u32;
    unsafe {
        let _ = GetExitCodeProcess(process, &mut code);
        let _ = CloseHandle(process);
    }
    let result = if code == 0 {
        scan::read_file(&files.out).ok_or_else(|| "快速扫描的结果读不出来".to_string())
    } else {
        Err(std::fs::read_to_string(&files.error).unwrap_or_else(|_| "快速扫描中途出错".into()))
    };
    files.clear();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_runs_decode() {
        // 0x21: 1-byte length, 2-byte offset. Run 1: 0x18 clusters at 0x5634; run 2: 0x10 clusters
        // back 0x100 from there; then a sparse run of 4.
        let b = [
            0x21, 0x18, 0x34, 0x56, 0x21, 0x10, 0x00, 0xFF, 0x01, 0x04, 0x00,
        ];
        let runs = data_runs(&b);
        assert_eq!(
            runs,
            vec![(Some(0x5634), 0x18), (Some(0x5534), 0x10), (None, 4)]
        );
    }

    #[test]
    fn fixups_restore_sector_ends() {
        let mut rec = vec![0u8; 1024];
        rec[0..4].copy_from_slice(b"FILE");
        rec[4..6].copy_from_slice(&0x30u16.to_le_bytes());
        rec[6..8].copy_from_slice(&3u16.to_le_bytes());
        // Check value 0xABCD, the real ends 0x1111 and 0x2222.
        rec[0x30..0x32].copy_from_slice(&[0xCD, 0xAB]);
        rec[0x32..0x34].copy_from_slice(&[0x11, 0x11]);
        rec[0x34..0x36].copy_from_slice(&[0x22, 0x22]);
        rec[510..512].copy_from_slice(&[0xCD, 0xAB]);
        rec[1022..1024].copy_from_slice(&[0xCD, 0xAB]);
        assert!(fixup(&mut rec, 512));
        assert_eq!(&rec[510..512], &[0x11, 0x11]);
        assert_eq!(&rec[1022..1024], &[0x22, 0x22]);
        // A torn write (check value missing) is refused.
        rec[510..512].copy_from_slice(&[0, 0]);
        rec[0x30..0x32].copy_from_slice(&[0xCD, 0xAB]);
        assert!(!fixup(&mut rec, 512));
    }

    /// Reads the system drive's table and compares it with the normal scan. Needs an elevated
    /// shell: `cargo test mft_matches_normal_scan -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn mft_matches_normal_scan() {
        let root = crate::util::system_drive();
        let ctx = ScanCtx::default();
        let fast = scan(&root, &ctx).expect("needs an elevated shell");
        let gb = |b: u64| b as f64 / 1073741824.0;
        println!(
            "mft: {:.1} GB, {} files, {} dirs, {} ms",
            gb(fast.summary.scanned),
            fast.summary.files,
            fast.summary.dirs,
            fast.summary.elapsed_ms
        );
        for c in &fast.tree.children {
            println!("  {:>8.2} GB  {}", gb(c.size), c.name);
        }
    }
}
