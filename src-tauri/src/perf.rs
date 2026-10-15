//! Live performance numbers, the way Task Manager's Performance tab shows them.
//!
//! Counters are read through PDH by their English names (`PdhAddEnglishCounterW`), so they work on
//! a Chinese Windows too, where the localized names differ. CPU uses `% Processor Utility` like
//! Task Manager does since Windows 10 — `% Processor Time` reads lower on CPUs that boost.

use crate::apps::powershell;
use serde::Serialize;
use std::collections::HashMap;
use windows::core::HSTRING;
use windows::Win32::System::Performance::*;

/// Doubles, not capped at 100 (utility and multi-core process time can go over).
const FMT: PDH_FMT = PDH_FMT(PDH_FMT_DOUBLE.0 | 0x8000);

struct Query(PDH_HQUERY);
// PDH handles are plain handles; every use is behind the state mutex.
unsafe impl Send for Query {}
impl Drop for Query {
    fn drop(&mut self) {
        unsafe { PdhCloseQuery(self.0) };
    }
}

#[derive(Clone, Copy)]
struct Counter(PDH_HCOUNTER);
unsafe impl Send for Counter {}

impl Query {
    fn open() -> Option<Self> {
        let mut q = PDH_HQUERY::default();
        (unsafe { PdhOpenQueryW(None, 0, &mut q) } == 0).then_some(Query(q))
    }
    fn add(&self, path: &str) -> Option<Counter> {
        let mut c = PDH_HCOUNTER::default();
        (unsafe { PdhAddEnglishCounterW(self.0, &HSTRING::from(path), 0, &mut c) } == 0)
            .then_some(Counter(c))
    }
    fn collect(&self) {
        unsafe { PdhCollectQueryData(self.0) };
    }
}

fn value(c: Option<Counter>) -> f64 {
    let Some(c) = c else { return 0.0 };
    let mut v = PDH_FMT_COUNTERVALUE::default();
    let ok = unsafe { PdhGetFormattedCounterValue(c.0, FMT, None, &mut v) } == 0;
    // CStatus 0 = valid, 1 = new data; anything else (first sample of a rate) reads as 0.
    if ok && v.CStatus <= 1 {
        unsafe { v.Anonymous.doubleValue }.max(0.0)
    } else {
        0.0
    }
}

/// Every instance of a wildcard counter, as (instance name, value).
fn array(c: Option<Counter>) -> Vec<(String, f64)> {
    let Some(c) = c else { return Vec::new() };
    let (mut size, mut count) = (0u32, 0u32);
    let r = unsafe { PdhGetFormattedCounterArrayW(c.0, FMT, &mut size, &mut count, None) };
    if r != PDH_MORE_DATA || size == 0 {
        return Vec::new();
    }
    // u64 words keep the item structs aligned.
    let mut buf = vec![0u64; (size as usize).div_ceil(8)];
    let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
    if unsafe { PdhGetFormattedCounterArrayW(c.0, FMT, &mut size, &mut count, Some(items)) } != 0 {
        return Vec::new();
    }
    (0..count as usize)
        .filter_map(|i| {
            let it = unsafe { &*items.add(i) };
            let name = unsafe { it.szName.to_string() }.ok()?;
            let v = if it.FmtValue.CStatus <= 1 {
                unsafe { it.FmtValue.Anonymous.doubleValue }.max(0.0)
            } else {
                0.0
            };
            Some((name, v))
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Machine-wide sample
// ---------------------------------------------------------------------------------------------

pub struct Monitor {
    q: Query,
    cpu: Option<Counter>,
    cpu_perf: Option<Counter>,
    cpu_freq: Option<Counter>,
    disk_idle: Option<Counter>,
    disk_read: Option<Counter>,
    disk_write: Option<Counter>,
    disk_resp: Option<Counter>,
    net_recv: Option<Counter>,
    net_sent: Option<Counter>,
    net_bw: Option<Counter>,
    gpu_engine: Option<Counter>,
    gpu_dedicated: Option<Counter>,
    gpu_shared: Option<Counter>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskSample {
    pub index: u32,
    /// `C: D:`
    pub letters: String,
    /// Percent of the time the disk was busy.
    pub active: f64,
    pub read: f64,
    pub write: f64,
    pub response_ms: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetSample {
    /// Matches `NetInfo::key`.
    pub key: String,
    pub name: String,
    /// Bytes per second.
    pub recv: f64,
    pub sent: f64,
    /// Link speed, bits per second.
    pub bandwidth: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuSample {
    /// `0x00000000_0x0000d1b3`, matches `GpuInfo::luid`.
    pub luid: String,
    pub util: f64,
    /// Busiest engines by type: `3D`, `Copy`, `VideoDecode`, ...
    pub engines: Vec<(String, f64)>,
    pub dedicated_used: u64,
    pub shared_used: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    pub cpu: f64,
    pub cores: Vec<f64>,
    pub cpu_mhz: f64,
    pub mem_total: u64,
    pub mem_used: u64,
    pub mem_cached: u64,
    pub commit_total: u64,
    pub commit_limit: u64,
    pub paged_pool: u64,
    pub nonpaged_pool: u64,
    pub processes: u32,
    pub threads: u32,
    pub handles: u32,
    pub uptime_secs: u64,
    pub disks: Vec<DiskSample>,
    pub nets: Vec<NetSample>,
    pub gpus: Vec<GpuSample>,
}

/// PDH spells adapter descriptions with `[]` for `()` and `_` for `#` and `/`.
pub fn pdh_key(desc: &str) -> String {
    desc.chars()
        .map(|c| match c {
            '(' => '[',
            ')' => ']',
            '#' | '/' | '\\' => '_',
            c => c,
        })
        .collect::<String>()
        .to_lowercase()
}

/// `pid_12_luid_0x00000000_0x0000D1B3_phys_0_eng_0_engtype_3D` → `0x00000000_0x0000d1b3`.
fn luid_of(instance: &str) -> Option<String> {
    let i = instance.find("luid_")? + 5;
    let rest = &instance[i..];
    let parts: Vec<&str> = rest.splitn(3, '_').take(2).collect();
    (parts.len() == 2).then(|| format!("{}_{}", parts[0], parts[1]).to_lowercase())
}

impl Monitor {
    pub fn new() -> Option<Self> {
        let q = Query::open()?;
        let m = Monitor {
            cpu: q.add(r"\Processor Information(*)\% Processor Utility"),
            cpu_perf: q.add(r"\Processor Information(_Total)\% Processor Performance"),
            cpu_freq: q.add(r"\Processor Information(_Total)\Processor Frequency"),
            disk_idle: q.add(r"\PhysicalDisk(*)\% Idle Time"),
            disk_read: q.add(r"\PhysicalDisk(*)\Disk Read Bytes/sec"),
            disk_write: q.add(r"\PhysicalDisk(*)\Disk Write Bytes/sec"),
            disk_resp: q.add(r"\PhysicalDisk(*)\Avg. Disk sec/Transfer"),
            net_recv: q.add(r"\Network Interface(*)\Bytes Received/sec"),
            net_sent: q.add(r"\Network Interface(*)\Bytes Sent/sec"),
            net_bw: q.add(r"\Network Interface(*)\Current Bandwidth"),
            gpu_engine: q.add(r"\GPU Engine(*)\Utilization Percentage"),
            gpu_dedicated: q.add(r"\GPU Adapter Memory(*)\Dedicated Usage"),
            gpu_shared: q.add(r"\GPU Adapter Memory(*)\Shared Usage"),
            q,
        };
        // Rates need two samples; take the first now so the next call has numbers.
        m.q.collect();
        Some(m)
    }

    pub fn sample(&mut self) -> Sample {
        use windows::Win32::System::ProcessStatus::{GetPerformanceInfo, PERFORMANCE_INFORMATION};
        use windows::Win32::System::SystemInformation::{
            GetTickCount64, GlobalMemoryStatusEx, MEMORYSTATUSEX,
        };
        self.q.collect();

        // CPU: "_Total" is the whole machine; "0,3" is core 3 of group 0.
        let mut total = 0.0;
        let mut cores: Vec<((u32, u32), f64)> = Vec::new();
        for (name, v) in array(self.cpu) {
            if name == "_Total" {
                total = v;
            } else if let Some((g, i)) = name.split_once(',') {
                if let (Ok(g), Ok(i)) = (g.parse(), i.parse()) {
                    cores.push(((g, i), v));
                }
            }
        }
        cores.sort_by_key(|(k, _)| *k);
        let cpu_mhz = value(self.cpu_freq) * value(self.cpu_perf) / 100.0;

        let mut mem = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        let _ = unsafe { GlobalMemoryStatusEx(&mut mem) };
        let mut pi = PERFORMANCE_INFORMATION {
            cb: std::mem::size_of::<PERFORMANCE_INFORMATION>() as u32,
            ..Default::default()
        };
        let _ = unsafe { GetPerformanceInfo(&mut pi, pi.cb) };
        let page = pi.PageSize as u64;

        // Disks: instances are "0 C: D:"; "_Total" is skipped.
        let read: HashMap<String, f64> = array(self.disk_read).into_iter().collect();
        let write: HashMap<String, f64> = array(self.disk_write).into_iter().collect();
        let resp: HashMap<String, f64> = array(self.disk_resp).into_iter().collect();
        let mut disks: Vec<DiskSample> = array(self.disk_idle)
            .into_iter()
            .filter(|(n, _)| n != "_Total")
            .filter_map(|(n, idle)| {
                let (index, letters) = n.split_once(' ').unwrap_or((&n, ""));
                Some(DiskSample {
                    index: index.parse().ok()?,
                    letters: letters.trim().to_string(),
                    active: (100.0 - idle).clamp(0.0, 100.0),
                    read: read.get(&n).copied().unwrap_or(0.0),
                    write: write.get(&n).copied().unwrap_or(0.0),
                    response_ms: resp.get(&n).copied().unwrap_or(0.0) * 1000.0,
                })
            })
            .collect();
        disks.sort_by_key(|d| d.index);

        // Network: only adapters with a link (a bandwidth) are of interest.
        let recv: HashMap<String, f64> = array(self.net_recv).into_iter().collect();
        let sent: HashMap<String, f64> = array(self.net_sent).into_iter().collect();
        let nets: Vec<NetSample> = array(self.net_bw)
            .into_iter()
            .filter(|(_, bw)| *bw > 0.0)
            .map(|(n, bw)| NetSample {
                key: pdh_key(&n),
                recv: recv.get(&n).copied().unwrap_or(0.0),
                sent: sent.get(&n).copied().unwrap_or(0.0),
                bandwidth: bw,
                name: n,
            })
            .collect();

        // GPU: like Task Manager, the busiest engine type is the adapter's utilization; each
        // engine type is the sum over the processes using it.
        let mut by_engine: HashMap<(String, String), f64> = HashMap::new();
        for (n, v) in array(self.gpu_engine) {
            let (Some(luid), Some(i)) = (luid_of(&n), n.find("engtype_")) else {
                continue;
            };
            let eng = n[i + 8..].to_string();
            if eng.is_empty() {
                continue;
            }
            *by_engine.entry((luid, eng)).or_default() += v;
        }
        let mut gpus: HashMap<String, GpuSample> = HashMap::new();
        fn slot<'a>(gpus: &'a mut HashMap<String, GpuSample>, luid: &str) -> &'a mut GpuSample {
            gpus.entry(luid.to_string()).or_insert_with(|| GpuSample {
                luid: luid.to_string(),
                util: 0.0,
                engines: Vec::new(),
                dedicated_used: 0,
                shared_used: 0,
            })
        }
        for ((luid, eng), v) in by_engine {
            let g = slot(&mut gpus, &luid);
            let v = v.min(100.0);
            g.util = g.util.max(v);
            g.engines.push((eng, v));
        }
        for (n, v) in array(self.gpu_dedicated) {
            if let Some(l) = luid_of(&n) {
                slot(&mut gpus, &l).dedicated_used += v as u64;
            }
        }
        for (n, v) in array(self.gpu_shared) {
            if let Some(l) = luid_of(&n) {
                slot(&mut gpus, &l).shared_used += v as u64;
            }
        }
        let mut gpus: Vec<GpuSample> = gpus.into_values().collect();
        for g in &mut gpus {
            g.engines.sort_by(|a, b| b.1.total_cmp(&a.1));
        }
        gpus.sort_by(|a, b| a.luid.cmp(&b.luid));

        Sample {
            cpu: total.min(100.0),
            cores: cores.into_iter().map(|(_, v)| v.min(100.0)).collect(),
            cpu_mhz,
            mem_total: mem.ullTotalPhys,
            mem_used: mem.ullTotalPhys - mem.ullAvailPhys,
            mem_cached: pi.SystemCache as u64 * page,
            commit_total: pi.CommitTotal as u64 * page,
            commit_limit: pi.CommitLimit as u64 * page,
            paged_pool: pi.KernelPaged as u64 * page,
            nonpaged_pool: pi.KernelNonpaged as u64 * page,
            processes: pi.ProcessCount,
            threads: pi.ThreadCount,
            handles: pi.HandleCount,
            uptime_secs: unsafe { GetTickCount64() } / 1000,
            disks,
            nets,
            gpus,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Processes, grouped by program
// ---------------------------------------------------------------------------------------------

pub struct ProcMonitor {
    q: Query,
    cpu: Option<Counter>,
    mem: Option<Counter>,
    pid: Option<Counter>,
    paths: HashMap<u32, Option<String>>,
    logical: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcGroup {
    /// Executable name without `.exe`, as PDH names the process.
    pub name: String,
    pub count: u32,
    /// Percent of the whole machine.
    pub cpu: f64,
    /// Private working set, as Task Manager's Memory column.
    pub memory: u64,
    pub path: Option<String>,
}

fn image_path(pid: u32) -> Option<String> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let r = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(h);
        r.ok()?;
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

impl ProcMonitor {
    pub fn new() -> Option<Self> {
        let q = Query::open()?;
        let m = ProcMonitor {
            cpu: q.add(r"\Process(*)\% Processor Time"),
            mem: q.add(r"\Process(*)\Working Set - Private"),
            pid: q.add(r"\Process(*)\ID Process"),
            paths: HashMap::new(),
            logical: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f64,
            q,
        };
        m.q.collect();
        Some(m)
    }

    pub fn sample(&mut self) -> Vec<ProcGroup> {
        self.q.collect();
        let cpu: HashMap<String, f64> = array(self.cpu).into_iter().collect();
        let mem: HashMap<String, f64> = array(self.mem).into_iter().collect();
        let mut groups: HashMap<String, (ProcGroup, u32)> = HashMap::new();
        for (inst, pid) in array(self.pid) {
            // "chrome#12" belongs to "chrome". The numbering shifts as processes come and go, but
            // within one name it no longer matters once the group is summed.
            let base = inst.split('#').next().unwrap_or(&inst).to_string();
            if base == "_Total" || base == "Idle" || pid == 0.0 {
                continue;
            }
            let g = groups.entry(base.clone()).or_insert_with(|| {
                (
                    ProcGroup { name: base.clone(), count: 0, cpu: 0.0, memory: 0, path: None },
                    pid as u32,
                )
            });
            g.0.count += 1;
            g.0.cpu += cpu.get(&inst).copied().unwrap_or(0.0) / self.logical;
            g.0.memory += mem.get(&inst).copied().unwrap_or(0.0) as u64;
        }
        let mut out: Vec<ProcGroup> = groups
            .into_values()
            .map(|(mut g, pid)| {
                g.path = self.paths.entry(pid).or_insert_with(|| image_path(pid)).clone();
                g.cpu = g.cpu.min(100.0);
                g
            })
            .collect();
        if self.paths.len() > 4000 {
            self.paths.clear();
        }
        out.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then(b.memory.cmp(&a.memory)));
        out
    }
}

/// Processes Windows needs; ending them crashes or locks the session.
const CRITICAL: &[&str] = &[
    "system", "registry", "smss", "csrss", "wininit", "winlogon", "services", "lsass", "lsaiso",
    "svchost", "dwm", "fontdrvhost", "memory compression", "secure system", "sihost", "explorer",
    "ctfmon", "spoolsv", "audiodg", "searchindexer", "msmpeng", "securityhealthservice",
];

/// Ends every process named `name` (no `.exe`). Returns how many ended.
pub fn end_group(name: &str) -> Result<u32, String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
    let lower = name.to_lowercase();
    if CRITICAL.contains(&lower.as_str()) {
        return Err(format!("{name} 是 Windows 运行必需的进程，不能结束"));
    }
    let me = std::process::id();
    let mut pids = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).map_err(|e| e.message())?;
        let mut e = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut ok = Process32FirstW(snap, &mut e).is_ok();
        while ok {
            let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
            let exe = String::from_utf16_lossy(&e.szExeFile[..len]).to_lowercase();
            let stem = exe.strip_suffix(".exe").unwrap_or(&exe);
            if stem == lower {
                pids.push(e.th32ProcessID);
            }
            ok = Process32NextW(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    if pids.contains(&me) {
        return Err("不能结束 DiskMate 自己".into());
    }
    if pids.is_empty() {
        return Err("这个程序已经退出了".into());
    }
    let mut ended = 0;
    for pid in &pids {
        unsafe {
            if let Ok(h) = OpenProcess(PROCESS_TERMINATE, false, *pid) {
                if TerminateProcess(h, 1).is_ok() {
                    ended += 1;
                }
                let _ = CloseHandle(h);
            }
        }
    }
    if ended == 0 {
        Err("没有权限结束它，它可能以管理员身份运行".into())
    } else {
        Ok(ended)
    }
}

// ---------------------------------------------------------------------------------------------
// Hardware description (read once)
// ---------------------------------------------------------------------------------------------

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct CpuInfo {
    pub name: String,
    pub base_mhz: u32,
    pub cores: u32,
    pub logical: u32,
    pub sockets: u32,
    pub l2_kb: u32,
    pub l3_kb: u32,
    pub virtualization: Option<bool>,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct MemInfo {
    pub speed_mhz: u32,
    pub slots_used: u32,
    pub slots_total: u32,
    /// `DIMM` | `SODIMM` | ""
    pub form: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DiskInfo {
    pub index: u32,
    pub model: String,
    /// `SSD` | `HDD` | ""
    pub media: String,
    /// `NVMe` | `SATA` | `USB` | ...
    pub bus: String,
    pub size: u64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NetInfo {
    /// Matches `NetSample::key`.
    pub key: String,
    pub description: String,
    /// What Windows calls it: `WLAN`, `以太网`.
    pub alias: String,
    pub ipv4: String,
    pub wireless: bool,
    /// A real network card, not a virtual one (Hyper-V, VPN, ...).
    pub physical: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GpuInfo {
    pub luid: String,
    pub name: String,
    pub dedicated_total: u64,
    pub shared_total: u64,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct HardwareInfo {
    pub cpu: CpuInfo,
    pub mem: MemInfo,
    pub disks: Vec<DiskInfo>,
    pub nets: Vec<NetInfo>,
    pub gpus: Vec<GpuInfo>,
}

fn gpus() -> Vec<GpuInfo> {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE};
    let Ok(f) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for i in 0.. {
        let Ok(a) = (unsafe { f.EnumAdapters1(i) }) else { break };
        let Ok(d) = (unsafe { a.GetDesc1() }) else { continue };
        if d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
            continue;
        }
        let len = d.Description.iter().position(|&c| c == 0).unwrap_or(d.Description.len());
        let luid = format!("0x{:08x}_0x{:08x}", d.AdapterLuid.HighPart as u32, d.AdapterLuid.LowPart);
        if out.iter().any(|g: &GpuInfo| g.luid == luid) {
            continue;
        }
        out.push(GpuInfo {
            luid,
            name: String::from_utf16_lossy(&d.Description[..len]),
            dedicated_total: d.DedicatedVideoMemory as u64,
            shared_total: d.SharedSystemMemory as u64,
        });
    }
    out
}

pub fn hardware() -> HardwareInfo {
    let script = r#"
$p = @(Get-CimInstance Win32_Processor)
$m = @(Get-CimInstance Win32_PhysicalMemory)
$a = Get-CimInstance Win32_PhysicalMemoryArray | Select-Object -First 1
$disks = @(Get-PhysicalDisk | ForEach-Object { @{ id = [string]$_.DeviceId; model = [string]$_.FriendlyName; media = [string]$_.MediaType; bus = [string]$_.BusType; size = [uint64]$_.Size } })
$nets = @(Get-NetAdapter | ForEach-Object {
  $ip = (Get-NetIPAddress -InterfaceIndex $_.ifIndex -AddressFamily IPv4 -ErrorAction SilentlyContinue | Select-Object -First 1).IPAddress
  @{ desc = [string]$_.InterfaceDescription; alias = [string]$_.Name; ip = [string]$ip; media = [string]$_.NdisPhysicalMedium; physical = [bool]$_.HardwareInterface }
})
@{
  cpu = @{ name = [string]$p[0].Name; cores = [int]($p | Measure-Object NumberOfCores -Sum).Sum; logical = [int]($p | Measure-Object NumberOfLogicalProcessors -Sum).Sum; mhz = [int]$p[0].MaxClockSpeed; l2 = [int]$p[0].L2CacheSize; l3 = [int]$p[0].L3CacheSize; virt = $p[0].VirtualizationFirmwareEnabled; sockets = $p.Count }
  mem = @{ speed = [int]($m | Measure-Object ConfiguredClockSpeed -Maximum).Maximum; used = $m.Count; slots = [int]$a.MemoryDevices; form = [int]$m[0].FormFactor }
  disks = $disks
  nets = $nets
} | ConvertTo-Json -Depth 4 -Compress"#;
    let v: serde_json::Value = powershell(script)
        .and_then(|s| serde_json::from_str(s.trim()).ok())
        .unwrap_or_default();
    let s = |v: &serde_json::Value| v.as_str().unwrap_or("").trim().to_string();
    let n = |v: &serde_json::Value| v.as_u64().unwrap_or(0) as u32;
    let list = |v: &serde_json::Value| -> Vec<serde_json::Value> {
        match v {
            serde_json::Value::Array(a) => a.clone(),
            serde_json::Value::Null => Vec::new(),
            other => vec![other.clone()],
        }
    };
    let c = &v["cpu"];
    let m = &v["mem"];
    HardwareInfo {
        cpu: CpuInfo {
            name: s(&c["name"]),
            base_mhz: n(&c["mhz"]),
            cores: n(&c["cores"]),
            logical: n(&c["logical"]),
            sockets: n(&c["sockets"]).max(1),
            l2_kb: n(&c["l2"]),
            l3_kb: n(&c["l3"]),
            virtualization: c["virt"].as_bool(),
        },
        mem: MemInfo {
            speed_mhz: n(&m["speed"]),
            slots_used: n(&m["used"]),
            slots_total: n(&m["slots"]),
            form: match n(&m["form"]) {
                8 => "DIMM",
                12 => "SODIMM",
                _ => "",
            }
            .into(),
        },
        disks: list(&v["disks"])
            .iter()
            .filter_map(|d| {
                Some(DiskInfo {
                    index: s(&d["id"]).parse().ok()?,
                    model: s(&d["model"]),
                    media: match s(&d["media"]).as_str() {
                        "SSD" | "4" => "SSD".into(),
                        "HDD" | "3" => "HDD".into(),
                        _ => String::new(),
                    },
                    bus: match s(&d["bus"]).as_str() {
                        "17" => "NVMe".into(),
                        "11" => "SATA".into(),
                        "7" => "USB".into(),
                        other => other.to_string(),
                    },
                    size: d["size"].as_u64().unwrap_or(0),
                })
            })
            .collect(),
        nets: list(&v["nets"])
            .iter()
            .map(|d| {
                let desc = s(&d["desc"]);
                let media = s(&d["media"]).to_lowercase();
                NetInfo {
                    key: pdh_key(&desc),
                    wireless: media.contains("802.11") || media == "9" || media.contains("wireless"),
                    description: desc,
                    alias: s(&d["alias"]),
                    ipv4: s(&d["ip"]),
                    physical: d["physical"].as_bool().unwrap_or(false),
                }
            })
            .collect(),
        gpus: gpus(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_instances_name_their_adapter() {
        assert_eq!(
            luid_of("pid_1234_luid_0x00000000_0x0000D1B3_phys_0_eng_0_engtype_3D").as_deref(),
            Some("0x00000000_0x0000d1b3")
        );
        assert_eq!(
            luid_of("luid_0x00000000_0x0000D1B3_phys_0").as_deref(),
            Some("0x00000000_0x0000d1b3")
        );
    }

    #[test]
    fn adapter_names_match_pdh_spelling() {
        assert_eq!(
            pdh_key("Intel(R) Wi-Fi 6 AX201 160MHz"),
            pdh_key("Intel[R] Wi-Fi 6 AX201 160MHz")
        );
        assert_eq!(pdh_key("Realtek PCIe GbE #2"), "realtek pcie gbe _2");
    }

    #[test]
    fn critical_processes_are_refused() {
        assert!(end_group("csrss").is_err());
        assert!(end_group("definitely-not-running-xyz").is_err());
    }
}

#[cfg(test)]
mod real {
    /// Read-only: `cargo test --release real_perf -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_perf() {
        let t = std::time::Instant::now();
        let hw = super::hardware();
        println!("hardware in {:?}", t.elapsed());
        println!("{}", serde_json::to_string_pretty(&hw).unwrap());
        let mut m = super::Monitor::new().unwrap();
        let mut p = super::ProcMonitor::new().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1000));
        let t = std::time::Instant::now();
        let s = m.sample();
        println!("sample in {:?}", t.elapsed());
        println!("{}", serde_json::to_string_pretty(&s).unwrap());
        let t = std::time::Instant::now();
        let procs = p.sample();
        println!("{} groups in {:?}", procs.len(), t.elapsed());
        for g in procs.iter().take(8) {
            println!("{:>5.1}% {:>8} MB x{} {} {:?}", g.cpu, g.memory / 1048576, g.count, g.name, g.path);
        }
    }
}
