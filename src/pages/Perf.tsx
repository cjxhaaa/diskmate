import { useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { Cpu, Gauge, HardDrive, MemoryStick, MonitorPlay, Network, X } from "lucide-react";
import { api, type FileDetails, type HardwareInfo, type PerfSample, type ProcGroup } from "../api";
import { Dialog, ProgramIcon } from "../components/ui";
import { formatBytes } from "../format";
import { useStore } from "../store";

/** Seconds of history the graphs show, like Task Manager. */
const HISTORY = 60;

type Kind = "cpu" | "mem" | "disk" | "net" | "gpu";
const COLOR: Record<Kind, string> = {
  cpu: "var(--res-cpu)",
  mem: "var(--res-mem)",
  disk: "var(--res-disk)",
  net: "var(--res-net)",
  gpu: "var(--res-gpu)",
};

const pct = (v: number) => `${Math.round(v)}%`;
const ghz = (mhz: number) => `${(mhz / 1000).toFixed(2)} GHz`;
/** Bytes per second → bits per second, the way network speeds are quoted. */
function bits(bytesPerSec: number): string {
  const b = bytesPerSec * 8;
  if (b < 1000) return `${Math.round(b)} bps`;
  if (b < 1e6) return `${(b / 1e3).toFixed(b < 1e4 ? 1 : 0)} Kbps`;
  if (b < 1e9) return `${(b / 1e6).toFixed(b < 1e7 ? 1 : 0)} Mbps`;
  return `${(b / 1e9).toFixed(1)} Gbps`;
}
const rate = (bytesPerSec: number) => `${formatBytes(bytesPerSec)}/秒`;
function uptime(secs: number): string {
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  return d ? `${d} 天 ${h} 小时` : h ? `${h} 小时 ${m} 分` : `${m} 分钟`;
}
/** 1, 2 or 5 × 10ⁿ at or above `v`, so an auto-scaled axis ends on a round number. */
function niceMax(v: number, floor: number): number {
  const x = Math.max(v * 1.1, floor);
  const p = 10 ** Math.floor(Math.log10(x));
  return [1, 2, 5, 10].map((m) => m * p).find((c) => c >= x) ?? 10 * p;
}

type Series = { name: string; values: number[]; dashed?: boolean };

/** Columns for the per-core grid: near square, and full rows when possible (20 → 5 × 4). */
function coreColumns(n: number): number {
  const start = Math.max(1, Math.ceil(Math.sqrt(n)));
  for (let c = start; c <= Math.min(n, start * 2); c++) if (n % c === 0) return c;
  return start;
}

/** A Task Manager style history graph: newest on the right, one value per second. */
function Chart({
  series,
  max,
  color,
  format,
  label,
  grow = 1,
}: {
  series: Series[];
  max: number;
  color: string;
  format: (v: number) => string;
  label: string;
  /** Share of the detail card's free height, against the other graphs in the same view. */
  grow?: number;
}) {
  const W = 600;
  const H = 200;
  const [hover, setHover] = useState<number | null>(null);
  const x = (i: number, n: number) => W - (n - 1 - i) * (W / (HISTORY - 1));
  const y = (v: number) => H - (Math.min(v, max) / max) * H;
  const line = (vals: number[]) => vals.map((v, i) => `${i ? "L" : "M"}${x(i, vals.length).toFixed(1)},${y(v).toFixed(1)}`).join("");
  const first = series[0].values;
  const area = first.length > 1 ? `${line(first)}L${W},${H}L${x(0, first.length)},${H}Z` : "";
  // `hover` is seconds ago.
  const at = (s: Series) => s.values[s.values.length - 1 - (hover ?? 0)];
  const hoverValid = hover !== null && first.length - 1 - hover >= 0;
  const left = hover !== null ? `${(1 - hover / (HISTORY - 1)) * 100}%` : "0";

  return (
    <div className="chart" style={{ flexGrow: grow }}>
      <div className="chart-top">
        <span>{label}</span>
        {series.length > 1 && (
          <span className="chart-legend">
            {series.map((s) => (
              <span key={s.name}>
                <svg width="18" height="8" aria-hidden>
                  <line x1="1" y1="4" x2="17" y2="4" stroke={color} strokeWidth="2" strokeDasharray={s.dashed ? "4 3" : undefined} />
                </svg>
                {s.name}
              </span>
            ))}
          </span>
        )}
        <span className="num">{format(max)}</span>
      </div>
      <div
        className="chart-plot"
        style={{ "--c": color } as CSSProperties}
        onMouseMove={(e) => {
          const r = e.currentTarget.getBoundingClientRect();
          const f = Math.min(1, Math.max(0, (e.clientX - r.left) / r.width));
          setHover(Math.round((1 - f) * (HISTORY - 1)));
        }}
        onMouseLeave={() => setHover(null)}
      >
        <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" width="100%" height="100%" aria-label={label}>
          {[1, 2, 3].map((i) => (
            <line key={`h${i}`} x1="0" x2={W} y1={(H / 4) * i} y2={(H / 4) * i} className="chart-grid" vectorEffect="non-scaling-stroke" />
          ))}
          {[1, 2, 3, 4, 5].map((i) => (
            <line key={`v${i}`} y1="0" y2={H} x1={(W / 6) * i} x2={(W / 6) * i} className="chart-grid" vectorEffect="non-scaling-stroke" />
          ))}
          {area && <path d={area} fill={color} opacity="0.13" />}
          {series.map((s) =>
            s.values.length > 1 ? (
              <path
                key={s.name}
                d={line(s.values)}
                fill="none"
                stroke={color}
                strokeWidth="2"
                strokeDasharray={s.dashed ? "5 4" : undefined}
                strokeLinejoin="round"
                vectorEffect="non-scaling-stroke"
              />
            ) : null,
          )}
        </svg>
        {hoverValid && (
          <>
            <div className="chart-cross" style={{ left }} />
            {series.map((s) => (
              <div key={s.name} className="chart-dot" style={{ left, top: `${(y(at(s)) / H) * 100}%` }} />
            ))}
            <div className={`chart-tip ${hover! < HISTORY / 2 ? "flip" : ""}`} style={{ left }}>
              <div className="faint">{hover === 0 ? "现在" : `${hover} 秒前`}</div>
              {series.map((s) => (
                <div key={s.name} className="num">
                  {series.length > 1 && <span className="faint">{s.name} </span>}
                  <b>{format(at(s))}</b>
                </div>
              ))}
            </div>
          </>
        )}
      </div>
      <div className="chart-bottom faint">
        <span>60 秒</span>
        <span>0</span>
      </div>
    </div>
  );
}

function Spark({ values, max, color, fill }: { values: number[]; max: number; color: string; fill?: boolean }) {
  const W = 64;
  const H = 40;
  const pts = values.map((v, i) => `${(W - (values.length - 1 - i) * (W / (HISTORY - 1))).toFixed(1)},${(H - (Math.min(v, max) / max) * (H - 2) - 1).toFixed(1)}`);
  return (
    <svg
      className="spark"
      width={W}
      height={H}
      viewBox={`0 0 ${W} ${H}`}
      preserveAspectRatio={fill ? "none" : undefined}
      style={{ borderColor: color }}
      aria-hidden
    >
      {pts.length > 1 && (
        <>
          <polygon points={`${pts.join(" ")} ${W},${H} ${pts[0].split(",")[0]},${H}`} fill={color} opacity="0.15" />
          <polyline points={pts.join(" ")} fill="none" stroke={color} strokeWidth="1.5" vectorEffect="non-scaling-stroke" />
        </>
      )}
    </svg>
  );
}

function Stat({ label, value, sub }: { label: string; value: string; sub?: string }) {
  return (
    <div className="perf-stat">
      <div className="faint">{label}</div>
      <div className="perf-stat-value num">{value}</div>
      {sub && <div className="faint num" style={{ fontSize: 11.5 }}>{sub}</div>}
    </div>
  );
}

function Specs({ rows }: { rows: [string, string | null | undefined][] }) {
  const shown = rows.filter((r): r is [string, string] => !!r[1]);
  if (!shown.length) return null;
  return (
    <dl className="perf-specs">
      {shown.map(([k, v]) => (
        <div key={k}>
          <dt className="faint">{k}</dt>
          <dd className="num">{v}</dd>
        </div>
      ))}
    </dl>
  );
}

type Resource = {
  id: string;
  kind: Kind;
  title: string;
  model: string;
  /** One line under the title in the list. */
  brief: (s: PerfSample) => string;
  /** The graph in the list, 0..spark max. */
  spark: (s: PerfSample) => number;
  sparkMax: number | "auto";
};

const VIRTUAL = /hyper-v|virtual|vethernet|tunnel|wintun|loopback|tap-|vpn|wan miniport|bluetooth|isatap|teredo/i;

function resources(s: PerfSample, hw: HardwareInfo | null): Resource[] {
  const out: Resource[] = [
    {
      id: "cpu",
      kind: "cpu",
      title: "CPU",
      model: hw?.cpu.name ?? "",
      brief: (x) => `${pct(x.cpu)}  ${x.cpuMhz ? ghz(x.cpuMhz) : ""}`,
      spark: (x) => x.cpu,
      sparkMax: 100,
    },
    {
      id: "mem",
      kind: "mem",
      title: "内存",
      model: hw ? `${formatBytes(s.memTotal)} ${hw.mem.form}` : formatBytes(s.memTotal),
      brief: (x) => `${formatBytes(x.memUsed)} / ${formatBytes(x.memTotal)} (${pct((x.memUsed / x.memTotal) * 100)})`,
      spark: (x) => (x.memUsed / x.memTotal) * 100,
      sparkMax: 100,
    },
  ];
  for (const d of s.disks) {
    const info = hw?.disks.find((h) => h.index === d.index);
    out.push({
      id: `disk:${d.index}`,
      kind: "disk",
      title: `磁盘 ${d.index}${d.letters ? ` (${d.letters})` : ""}`,
      model: info?.model ?? "",
      brief: (x) => {
        const cur = x.disks.find((y) => y.index === d.index);
        return `${[info?.media, info?.bus].filter(Boolean).join(" ")}  ${pct(cur?.active ?? 0)}`.trim();
      },
      spark: (x) => x.disks.find((y) => y.index === d.index)?.active ?? 0,
      sparkMax: 100,
    });
  }
  for (const n of s.nets) {
    const info = hw?.nets.find((h) => h.key === n.key);
    if (hw ? !info?.physical : VIRTUAL.test(n.name)) continue;
    out.push({
      id: `net:${n.key}`,
      kind: "net",
      title: info?.alias || (info?.wireless ? "Wi-Fi" : "网络"),
      model: info?.description ?? n.name,
      brief: (x) => {
        const cur = x.nets.find((y) => y.key === n.key);
        return `发 ${bits(cur?.sent ?? 0)}  收 ${bits(cur?.recv ?? 0)}`;
      },
      spark: (x) => {
        const cur = x.nets.find((y) => y.key === n.key);
        return (cur?.recv ?? 0) + (cur?.sent ?? 0);
      },
      sparkMax: "auto",
    });
  }
  const gpus = hw ? s.gpus.filter((g) => hw.gpus.some((h) => h.luid === g.luid)) : s.gpus.filter((g) => g.sharedUsed > 1 << 20);
  gpus.forEach((g, i) => {
    const info = hw?.gpus.find((h) => h.luid === g.luid);
    out.push({
      id: `gpu:${g.luid}`,
      kind: "gpu",
      title: `GPU ${i}`,
      model: info?.name ?? "",
      brief: (x) => `${pct(x.gpus.find((y) => y.luid === g.luid)?.util ?? 0)}`,
      spark: (x) => x.gpus.find((y) => y.luid === g.luid)?.util ?? 0,
      sparkMax: 100,
    });
  });
  return out;
}

const ICON: Record<Kind, typeof Cpu> = { cpu: Cpu, mem: MemoryStick, disk: HardDrive, net: Network, gpu: MonitorPlay };

function Detail({ r, history, hw }: { r: Resource; history: PerfSample[]; hw: HardwareInfo | null }) {
  const s = history[history.length - 1];
  const color = COLOR[r.kind];
  const [cpuView, setCpuView] = useState<"total" | "cores">("total");

  if (r.kind === "cpu") {
    const c = hw?.cpu;
    return (
      <>
        <div className="perf-view-switch">
          <div className="tabs">
            <button className={`tab ${cpuView === "total" ? "active" : ""}`} onClick={() => setCpuView("total")}>
              整体
            </button>
            <button className={`tab ${cpuView === "cores" ? "active" : ""}`} onClick={() => setCpuView("cores")}>
              每个逻辑处理器
            </button>
          </div>
        </div>
        {cpuView === "total" ? (
          <Chart label="利用率 %" series={[{ name: "CPU", values: history.map((h) => h.cpu) }]} max={100} color={color} format={pct} />
        ) : (
          <div className="perf-cores" style={{ gridTemplateColumns: `repeat(${coreColumns(s.cores.length)}, 1fr)` }}>
            {s.cores.map((v, i) => (
              <div key={i} className="perf-core" title={`处理器 ${i}：${pct(v)}`}>
                <Spark values={history.map((h) => h.cores[i] ?? 0)} max={100} color={color} fill />
                <span className="num faint">{pct(v)}</span>
              </div>
            ))}
          </div>
        )}
        <div className="perf-stats">
          <Stat label="利用率" value={pct(s.cpu)} />
          <Stat label="速度" value={s.cpuMhz ? ghz(s.cpuMhz) : "—"} />
          <Stat label="进程" value={s.processes.toLocaleString()} />
          <Stat label="线程" value={s.threads.toLocaleString()} />
          <Stat label="句柄" value={s.handles.toLocaleString()} />
          <Stat label="已开机" value={uptime(s.uptimeSecs)} />
        </div>
        <Specs
          rows={[
            ["基准速度", c?.baseMhz ? ghz(c.baseMhz) : null],
            ["插槽", c ? String(c.sockets) : null],
            ["内核", c?.cores ? String(c.cores) : null],
            ["逻辑处理器", c?.logical ? String(c.logical) : String(s.cores.length)],
            ["虚拟化", c?.virtualization == null ? null : c.virtualization ? "已启用" : "已禁用"],
            ["L2 缓存", c?.l2Kb ? formatBytes(c.l2Kb * 1024) : null],
            ["L3 缓存", c?.l3Kb ? formatBytes(c.l3Kb * 1024) : null],
          ]}
        />
      </>
    );
  }

  if (r.kind === "mem") {
    const m = hw?.mem;
    const usedPct = (s.memUsed / s.memTotal) * 100;
    return (
      <>
        <Chart
          label="内存使用量"
          series={[{ name: "内存", values: history.map((h) => h.memUsed) }]}
          max={s.memTotal}
          color={color}
          format={formatBytes}
        />
        <div className="perf-compose">
          <div className="faint" style={{ fontSize: 12, marginBottom: 6 }}>
            内存组合
          </div>
          <div className="bar thick">
            <span style={{ width: `${usedPct}%`, background: color }} />
          </div>
          <div className="perf-compose-legend faint">
            <span>使用中 {formatBytes(s.memUsed)}</span>
            <span>可用 {formatBytes(s.memTotal - s.memUsed)}</span>
          </div>
        </div>
        <div className="perf-stats">
          <Stat label="使用中" value={formatBytes(s.memUsed)} sub={pct(usedPct)} />
          <Stat label="可用" value={formatBytes(s.memTotal - s.memUsed)} />
          <Stat label="已提交" value={`${formatBytes(s.commitTotal)} / ${formatBytes(s.commitLimit)}`} sub="含页面文件" />
          <Stat label="系统文件缓存" value={formatBytes(s.memCached)} />
          <Stat label="分页缓冲池" value={formatBytes(s.pagedPool)} />
          <Stat label="非分页缓冲池" value={formatBytes(s.nonpagedPool)} />
        </div>
        <Specs
          rows={[
            ["速度", m?.speedMhz ? `${m.speedMhz} MHz` : null],
            ["已使用的插槽", m?.slotsTotal ? `${m.slotsUsed} / ${m.slotsTotal}` : null],
            ["外形规格", m?.form],
          ]}
        />
      </>
    );
  }

  if (r.kind === "disk") {
    const index = Number(r.id.split(":")[1]);
    const pick = (h: PerfSample) => h.disks.find((d) => d.index === index);
    const cur = pick(s);
    const info = hw?.disks.find((d) => d.index === index);
    const reads = history.map((h) => pick(h)?.read ?? 0);
    const writes = history.map((h) => pick(h)?.write ?? 0);
    return (
      <>
        <Chart label="活动时间 %" series={[{ name: "活动时间", values: history.map((h) => pick(h)?.active ?? 0) }]} max={100} color={color} format={pct} grow={3} />
        <Chart
          label="传输速率"
          series={[
            { name: "读取", values: reads },
            { name: "写入", values: writes, dashed: true },
          ]}
          max={niceMax(Math.max(...reads, ...writes), 1024 * 1024)}
          color={color}
          format={rate}
          grow={2}
        />
        <div className="perf-stats">
          <Stat label="活动时间" value={pct(cur?.active ?? 0)} />
          <Stat label="平均响应时间" value={`${(cur?.responseMs ?? 0).toFixed(1)} 毫秒`} />
          <Stat label="读取速度" value={rate(cur?.read ?? 0)} />
          <Stat label="写入速度" value={rate(cur?.write ?? 0)} />
        </div>
        <Specs
          rows={[
            ["容量", info?.size ? formatBytes(info.size) : null],
            ["类型", [info?.media, info?.bus].filter(Boolean).join(" · ") || null],
            ["分区", cur?.letters || null],
          ]}
        />
      </>
    );
  }

  if (r.kind === "net") {
    const key = r.id.slice(4);
    const pick = (h: PerfSample) => h.nets.find((n) => n.key === key);
    const cur = pick(s);
    const info = hw?.nets.find((n) => n.key === key);
    const recv = history.map((h) => pick(h)?.recv ?? 0);
    const sent = history.map((h) => pick(h)?.sent ?? 0);
    return (
      <>
        <Chart
          label="吞吐量"
          series={[
            { name: "接收", values: recv },
            { name: "发送", values: sent, dashed: true },
          ]}
          max={niceMax(Math.max(...recv, ...sent), 100_000 / 8)}
          color={color}
          format={bits}
        />
        <div className="perf-stats">
          <Stat label="接收" value={bits(cur?.recv ?? 0)} />
          <Stat label="发送" value={bits(cur?.sent ?? 0)} />
          <Stat label="连接速度" value={bits((cur?.bandwidth ?? 0) / 8)} />
        </div>
        <Specs
          rows={[
            ["适配器", info?.description ?? cur?.name],
            ["连接类型", info ? (info.wireless ? "无线" : "有线") : null],
            ["IPv4 地址", info?.ipv4 || null],
          ]}
        />
      </>
    );
  }

  const luid = r.id.slice(4);
  const pick = (h: PerfSample) => h.gpus.find((g) => g.luid === luid);
  const cur = pick(s);
  const info = hw?.gpus.find((g) => g.luid === luid);
  return (
    <>
      <Chart label="利用率 %" series={[{ name: "GPU", values: history.map((h) => pick(h)?.util ?? 0) }]} max={100} color={color} format={pct} />
      <div className="perf-engines">
        {(cur?.engines ?? []).slice(0, 4).map(([name, v]) => (
          <div key={name} className="perf-engine">
            <span>{ENGINE[name] ?? name}</span>
            <div className="bar">
              <span style={{ width: `${Math.max(1, v)}%`, background: color }} />
            </div>
            <span className="num">{pct(v)}</span>
          </div>
        ))}
      </div>
      <div className="perf-stats">
        <Stat label="利用率" value={pct(cur?.util ?? 0)} />
        <Stat
          label="专用 GPU 内存"
          value={formatBytes(cur?.dedicatedUsed ?? 0)}
          sub={info?.dedicatedTotal ? `共 ${formatBytes(info.dedicatedTotal)}` : undefined}
        />
        <Stat
          label="共享 GPU 内存"
          value={formatBytes(cur?.sharedUsed ?? 0)}
          sub={info?.sharedTotal ? `共 ${formatBytes(info.sharedTotal)}` : undefined}
        />
      </div>
    </>
  );
}

const ENGINE: Record<string, string> = {
  "3D": "3D",
  Copy: "复制",
  VideoDecode: "视频解码",
  VideoEncode: "视频编码",
  VideoProcessing: "视频处理",
  Compute_0: "计算",
  Compute_1: "计算 1",
  LegacyOverlay: "覆盖层",
  Security: "安全",
};

function Processes() {
  const { toast } = useStore();
  const [procs, setProcs] = useState<ProcGroup[] | null>(null);
  const [sort, setSort] = useState<"cpu" | "memory">("cpu");
  const [details, setDetails] = useState<Record<string, FileDetails>>({});
  const asked = useRef(new Set<string>());
  const [kill, setKill] = useState<ProcGroup | null>(null);
  const smooth = useRef(new Map<string, { cpu: number; memory: number }>());
  const [killing, setKilling] = useState(false);

  useEffect(() => {
    let alive = true;
    let running = false;
    const tick = () => {
      if (running || document.hidden) return;
      running = true;
      api
        .perfProcesses()
        .then((p) => {
          if (!alive) return;
          // Rows are ordered by a running average: the numbers shown stay live, but a program
          // only changes places when its use really changes, not on every 2-second blip.
          const next = new Map<string, { cpu: number; memory: number }>();
          for (const g of p) {
            const old = smooth.current.get(g.name);
            next.set(g.name, old ? { cpu: old.cpu * 0.7 + g.cpu * 0.3, memory: old.memory * 0.5 + g.memory * 0.5 } : { cpu: g.cpu, memory: g.memory });
          }
          smooth.current = next;
          setProcs(p);
        })
        .catch(() => undefined)
        .finally(() => (running = false));
    };
    tick();
    const t = setInterval(tick, 2000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, []);

  const top = useMemo(() => {
    const key = (p: ProcGroup) => smooth.current.get(p.name) ?? { cpu: p.cpu, memory: p.memory };
    return [...(procs ?? [])]
      .sort((a, b) => {
        const [x, y] = [key(a), key(b)];
        return sort === "cpu" ? y.cpu - x.cpu || y.memory - x.memory : y.memory - x.memory;
      })
      .slice(0, 12);
  }, [procs, sort]);

  // Icons and readable names for programs not looked up yet.
  useEffect(() => {
    const want = top.map((p) => p.path).filter((p): p is string => !!p && !asked.current.has(p));
    if (!want.length) return;
    want.forEach((p) => asked.current.add(p));
    void api.fileDetails(want).then((d) => setDetails((all) => ({ ...all, ...d })));
  }, [top]);

  const end = async (p: ProcGroup) => {
    setKilling(true);
    try {
      const n = await api.endProcess(p.name);
      toast(`已结束 ${n} 个进程`);
      setProcs((l) => l?.filter((x) => x.name !== p.name) ?? l);
    } catch (e) {
      toast(String(e));
    } finally {
      setKilling(false);
      setKill(null);
    }
  };

  const label = (p: ProcGroup) => {
    const d = p.path ? details[p.path] : undefined;
    return d?.description && d.description.length <= 40 ? d.description : p.name;
  };

  return (
    <section className="card perf-procs">
      <div className="perf-procs-head">
        <div>
          <div className="group-title">正在占用资源的程序</div>
          <div className="faint" style={{ fontSize: 12.5 }}>
            同一个程序的多个进程合在一起算，每 2 秒刷新
          </div>
        </div>
        <div className="filter-chips">
          <button className={`chip ${sort === "cpu" ? "active" : ""}`} onClick={() => setSort("cpu")}>
            按 CPU
          </button>
          <button className={`chip ${sort === "memory" ? "active" : ""}`} onClick={() => setSort("memory")}>
            按内存
          </button>
        </div>
      </div>
      <div className="proc-row proc-header faint">
        <span>名称</span>
        <span>CPU</span>
        <span>内存</span>
        <span />
      </div>
      {procs === null && [0, 1, 2, 3].map((i) => <div key={i} className="skeleton" style={{ height: 36, margin: "6px 0" }} />)}
      {top.map((p) => (
        <div key={p.name} className="proc-row">
          <span className="proc-name">
            <ProgramIcon src={p.path ? details[p.path]?.icon : null} name={label(p)} size={22} />
            <span className="proc-label" title={p.path ?? p.name}>
              {label(p)}
            </span>
            {p.count > 1 && <span className="faint num">({p.count})</span>}
          </span>
          <span className="num proc-cell" style={{ "--w": `${Math.min(100, p.cpu)}%` } as CSSProperties}>
            {p.cpu < 0.1 ? "0%" : `${p.cpu.toFixed(1)}%`}
          </span>
          <span className="num">{formatBytes(p.memory)}</span>
          <button className="icon-btn proc-end" title={`结束 ${label(p)}`} onClick={() => setKill(p)}>
            <X size={15} />
          </button>
        </div>
      ))}

      {kill && (
        <Dialog onClose={() => !killing && setKill(null)}>
          <div className="dialog-body">
            <h2 className="dialog-title">结束「{label(kill)}」？</h2>
            <div className="muted" style={{ fontSize: 13.5 }}>
              {kill.count > 1 ? `会结束它的全部 ${kill.count} 个进程。` : ""}
              没有保存的内容会丢失。如果程序没有反应，这样可以强制关掉它。
            </div>
          </div>
          <div className="dialog-actions">
            <button className="btn" disabled={killing} onClick={() => setKill(null)}>
              取消
            </button>
            <button className="btn btn-danger" disabled={killing} onClick={() => void end(kill)}>
              {killing && <span className="spinner" style={{ width: 14, height: 14, borderWidth: 2 }} />}
              结束任务
            </button>
          </div>
        </Dialog>
      )}
    </section>
  );
}

export function Perf() {
  const { toast } = useStore();
  const [history, setHistory] = useState<PerfSample[]>([]);
  const [hw, setHw] = useState<HardwareInfo | null>(null);
  const [selected, setSelected] = useState("cpu");

  useEffect(() => {
    let alive = true;
    let running = false;
    let failed = false;
    const tick = () => {
      if (running || document.hidden) return;
      running = true;
      api
        .perfSample()
        .then((s) => alive && setHistory((h) => [...h.slice(-(HISTORY - 1)), s]))
        .catch((e) => {
          if (!failed) toast(`读不到性能数据：${e}`);
          failed = true;
        })
        .finally(() => (running = false));
    };
    tick();
    const t = setInterval(tick, 1000);
    void api.perfHardware().then((h) => alive && setHw(h));
    // The hardware shown first can be the last run's; the re-read one follows.
    const offHw = api.onPerfHardware((h) => alive && setHw(h));
    return () => {
      alive = false;
      clearInterval(t);
      void offHw.then((off) => off());
      // Close the counters; reopening them on the next visit costs a few milliseconds.
      void api.perfStop();
    };
  }, [toast]);

  const latest = history[history.length - 1];
  const list = latest ? resources(latest, hw) : [];
  const current = list.find((r) => r.id === selected) ?? list[0];

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1 className="page-title">性能</h1>
          <div className="page-sub">CPU、内存、磁盘、网络和显卡的实时使用情况，每秒更新</div>
        </div>
      </div>

      {!latest ? (
        <div className="perf">
          <div>{[0, 1, 2, 3].map((i) => <div key={i} className="skeleton" style={{ height: 60, marginBottom: 8, borderRadius: 12 }} />)}</div>
          <div className="skeleton" style={{ height: 380, borderRadius: 16 }} />
        </div>
      ) : (
        <div className="perf">
          <nav className="perf-nav" aria-label="资源">
            {list.map((r) => {
              const vals = history.map((h) => r.spark(h));
              const max = r.sparkMax === "auto" ? niceMax(Math.max(...vals), 12_500) : r.sparkMax;
              return (
                <button key={r.id} className={`perf-tile ${current?.id === r.id ? "active" : ""}`} onClick={() => setSelected(r.id)}>
                  <Spark values={vals} max={max} color={COLOR[r.kind]} />
                  <span style={{ minWidth: 0 }}>
                    <span className="perf-tile-title">{r.title}</span>
                    <span className="perf-tile-brief num">{r.brief(latest)}</span>
                  </span>
                </button>
              );
            })}
          </nav>
          {current && (
            <div className="card perf-detail">
              <div className="perf-head">
                <h2>
                  {(() => {
                    const Icon = ICON[current.kind];
                    return <Icon size={20} color={COLOR[current.kind]} />;
                  })()}
                  {current.title}
                </h2>
                <span className="muted perf-model" title={current.model}>
                  {current.model || (hw ? "" : "正在读取硬件信息…")}
                </span>
              </div>
              <div className="perf-body">
                <Detail key={current.id} r={current} history={history} hw={hw} />
              </div>
            </div>
          )}
        </div>
      )}

      <Processes />

      {latest && history.length < 3 && (
        <div className="faint" style={{ fontSize: 12, marginTop: 10, display: "flex", gap: 6, alignItems: "center" }}>
          <Gauge size={13} /> 刚开始记录，图表会随时间填满
        </div>
      )}
    </div>
  );
}
