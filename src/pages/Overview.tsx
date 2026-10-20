import { useEffect, useMemo, useState } from "react";
import { ArrowRight, CodeXml, Globe, MessageCircle, Monitor, RefreshCw, ScanSearch, Sparkles, Square, TriangleAlert } from "lucide-react";
import type { CleanItem, ScanSummary } from "../api";
import { CATEGORIES } from "../categories";
import { Donut } from "../components/Donut";
import { AnimatedNumber, DrivePicker, UsageBar } from "../components/ui";
import { formatBytes, formatCount, formatDuration, percent, splitBytes } from "../format";
import { useStore } from "../store";

const GROUP_ICON = { system: Monitor, dev: CodeXml, app: Globe, review: TriangleAlert, guide: MessageCircle } as const;

function Figure({ label, bytes, tone }: { label: string; bytes: number; tone?: "free" }) {
  const [n, u] = splitBytes(bytes);
  return (
    <div>
      <div className="figure-label">{label}</div>
      <div className={`figure-value num ${tone ?? ""}`}>
        {n}
        <small>{u}</small>
      </div>
    </div>
  );
}

function SegmentBar({ summary }: { summary: ScanSummary }) {
  return (
    <div className="bar thick">
      {summary.categories.map((c) => (
        <span key={c.id} title={`${CATEGORIES[c.id].label} ${formatBytes(c.bytes)}`} style={{ width: `${(c.bytes / summary.total) * 100}%`, background: CATEGORIES[c.id].color }} />
      ))}
    </div>
  );
}

function Breakdown({ summary }: { summary: ScanSummary }) {
  const [active, setActive] = useState<string | null>(null);
  const used = summary.total - summary.free;
  const top3 = summary.categories.slice(0, 3);
  const activeCat = summary.categories.find((c) => c.id === active);

  return (
    <div className="card card-pad">
      <div className="card-title">空间都用在哪了</div>
      <div className="donut-wrap">
        <Donut
          size={200}
          thickness={28}
          slices={summary.categories.map((c) => ({ key: c.id, value: c.bytes, color: CATEGORIES[c.id].color }))}
          active={active}
          onHover={setActive}
          center={
            <div style={{ textAlign: "center" }}>
              <div className="faint" style={{ fontSize: 12 }}>
                {activeCat ? CATEGORIES[activeCat.id].label : "已使用"}
              </div>
              <div className="num" style={{ fontSize: 26, fontWeight: 700 }}>
                {formatBytes(activeCat ? activeCat.bytes : used)}
              </div>
              <div className="faint num" style={{ fontSize: 12 }}>
                {percent(activeCat ? activeCat.bytes : used, summary.total)}
              </div>
            </div>
          }
        />
        <div className="legend" onMouseLeave={() => setActive(null)}>
          {summary.categories.map((c) => (
            <div key={c.id} className={`legend-row ${active === c.id ? "active" : ""}`} onMouseEnter={() => setActive(c.id)}>
              <span className="legend-dot" style={{ background: CATEGORIES[c.id].color }} />
              <span className="legend-label">{CATEGORIES[c.id].label}</span>
              <span className="legend-size num">{formatBytes(c.bytes)}</span>
              <span className="legend-pct num">{percent(c.bytes, summary.total)}</span>
            </div>
          ))}
        </div>
      </div>
      {/* Every text this box can show is laid out at once in the same cell and all but one hidden,
          so the box is always as tall as the longest. Swapping a single text in and out changed its
          height on every hover, which moved the page and with it the ring under the pointer — the
          shaking. */}
      <div className="hint-box hint-stack">
        <div className={active ? "hint-layer hidden" : "hint-layer"}>
          占用最多的是 <strong>{top3.map((c) => `${CATEGORIES[c.id].label}（${formatBytes(c.bytes)}）`).join("、")}</strong>。
          把鼠标移到图例上可以看每一类是什么。扫描了 {formatCount(summary.files)} 个文件，用时 {formatDuration(summary.elapsedMs)}
          {summary.deniedDirs > 0 && `，${formatCount(summary.deniedDirs)} 个文件夹没有权限读取`}。
        </div>
        {summary.categories.map((c) => (
          <div key={c.id} className={active === c.id ? "hint-layer" : "hint-layer hidden"}>
            <strong>{CATEGORIES[c.id].label}</strong>：{CATEGORIES[c.id].hint}
          </div>
        ))}
      </div>
    </div>
  );
}

function ReleaseCard({ onGoClean }: { onGoClean: () => void }) {
  const { analysis } = useStore();

  if (analysis.status !== "done") {
    const p = analysis.status === "loading" ? analysis.progress : null;
    return (
      <div className="card card-pad release-card">
        <div className="card-title">
          <Sparkles size={18} color="var(--accent)" /> 可以释放的空间
        </div>
        <div className="skeleton" style={{ height: 44, width: "60%" }} />
        <div className="muted" style={{ fontSize: 13 }}>
          {p ? `正在检查「${p.current}」（${p.done}/${p.total}）` : "正在查找可以清理的内容…"}
        </div>
        {[0, 1, 2].map((i) => (
          <div key={i} className="skeleton" style={{ height: 40 }} />
        ))}
      </div>
    );
  }

  const cleanable = analysis.items.filter((i) => !i.guideOnly);
  const safe = cleanable.filter((i) => i.risk === "safe").reduce((s, i) => s + i.size, 0);
  const all = cleanable.reduce((s, i) => s + i.size, 0);
  const top: CleanItem[] = [...analysis.items].sort((a, b) => b.size - a.size).slice(0, 4);

  return (
    <div className="card card-pad release-card">
      <div className="card-title">
        <Sparkles size={18} color="var(--accent)" /> 可以释放的空间
      </div>
      <div>
        <div className="release-big num">
          <AnimatedNumber value={safe} format={(n) => splitBytes(n)[0]} />
          <small> {splitBytes(safe)[1]}</small>
        </div>
        <div className="muted" style={{ marginTop: 8 }}>
          可以放心清理；连同需要确认的项目，最多可释放 <b className="num">{formatBytes(all)}</b>
        </div>
      </div>
      <button className="btn btn-primary btn-lg" onClick={onGoClean}>
        去清理 <ArrowRight size={18} />
      </button>
      <div>
        <div className="faint" style={{ fontSize: 12.5, marginBottom: 4 }}>
          最大的几项
        </div>
        {top.map((item) => {
          const Icon = GROUP_ICON[item.group];
          return (
            <div key={item.id} className="suggest">
              <div className="suggest-icon" style={{ background: "var(--accent-soft)", color: "var(--accent)" }}>
                <Icon size={17} />
              </div>
              <div style={{ minWidth: 0, flex: 1 }}>
                <div className="suggest-title">{item.title}</div>
                <div className="suggest-sub">{item.guideOnly ? "需要在应用里清理" : item.risk === "safe" ? "可直接清理" : "清理前请确认"}</div>
              </div>
              <div className="num" style={{ fontWeight: 650 }}>
                {formatBytes(item.size)}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function HeroAction({ onGoClean }: { onGoClean: () => void }) {
  const { analysis } = useStore();
  if (analysis.status !== "done") return null;
  const safe = analysis.items.filter((i) => !i.guideOnly && i.risk === "safe").reduce((s, i) => s + i.size, 0);
  return (
    <button className="btn btn-primary btn-lg" onClick={onGoClean} style={{ position: "relative" }}>
      <Sparkles size={18} /> 一键释放 {formatBytes(safe)}
    </button>
  );
}

function Scanning() {
  const { scanning, cancelScan } = useStore();
  if (!scanning) return null;
  const p = scanning.progress;
  return (
    <div className="card card-pad scan-state">
      <div className="pulse-ring">
        <ScanSearch size={26} />
      </div>
      <div style={{ minWidth: 0 }}>
        <div style={{ fontWeight: 650, fontSize: 16 }}>正在扫描 {scanning.root}</div>
        <div className="muted num" style={{ margin: "2px 0 6px" }}>
          已扫描 {formatCount(p?.files ?? 0)} 个文件 · {formatBytes(p?.bytes ?? 0)} · {formatCount(p?.dirs ?? 0)} 个文件夹
        </div>
        <div className="scan-path">{p?.current || "准备中…"}</div>
      </div>
      <button className="btn" onClick={cancelScan}>
        <Square size={14} /> 停止
      </button>
    </div>
  );
}

/** "刚刚" / "12 分钟前" / "今天 09:41" / "10-14 16:02". */
function scannedAgo(unixSecs: number): string {
  const d = new Date(unixSecs * 1000);
  const mins = Math.floor((Date.now() - d.getTime()) / 60000);
  if (mins < 1) return "刚刚";
  if (mins < 60) return `${mins} 分钟前`;
  const hm = `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
  if (new Date().toDateString() === d.toDateString()) return `今天 ${hm}`;
  return `${d.getMonth() + 1}-${d.getDate()} ${hm}`;
}

export function Overview({ onGoClean }: { onGoClean: () => void }) {
  const { drives, root, setRoot, current: summary, scanning, startScan } = useStore();
  const drive = drives.find((d) => d.root === root);
  const scanningHere = scanning?.root.toUpperCase() === root.toUpperCase();
  const scanningElsewhere = !!scanning && !scanningHere;
  const used = drive ? drive.total - drive.free : 0;
  const pctUsed = drive ? used / drive.total : 0;
  const driveName = root.replace("\\", "");
  // Re-render every half minute so "N 分钟前" keeps up.
  const [, setTick] = useState(0);
  useEffect(() => {
    const t = setInterval(() => setTick((n) => n + 1), 30_000);
    return () => clearInterval(t);
  }, []);

  const tip = useMemo(() => {
    if (!drive) return null;
    if (pctUsed > 0.9) return { tone: "review", text: "空间非常紧张，系统更新和软件运行可能出问题。" };
    if (pctUsed > 0.8) return { tone: "caution", text: "空间偏紧，建议清理一下。" };
    return { tone: "safe", text: "空间还算充裕。" };
  }, [drive, pctUsed]);

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1 className="page-title">概览</h1>
          <div className="page-sub">看看磁盘空间都去哪了，哪些可以腾出来</div>
        </div>
        {/* The status and the scan button are always there, at fixed widths, so switching drives
            never moves the drive buttons. */}
        <div className="overview-actions">
          {drives.length > 1 && <DrivePicker drives={drives} value={root} onChange={setRoot} />}
          <span className="faint scan-when" title={summary ? "扫描结果会一直保留，需要时再手动重新扫描" : undefined}>
            {scanningHere ? "正在扫描…" : summary ? `扫描于 ${scannedAgo(summary.scannedAt)}` : "还没扫描过"}
          </span>
          <button
            className="btn scan-btn"
            disabled={!!scanning}
            title={
              scanningElsewhere
                ? `正在扫描 ${scanning?.root}，完成后再试`
                : summary
                  ? `扫描于 ${scannedAgo(summary.scannedAt)}`
                  : undefined
            }
            onClick={() => startScan()}
          >
            <RefreshCw size={15} /> {summary ? "重新扫描" : "扫描"}
          </button>
        </div>
      </div>

      {drive && (
        <div className="card hero">
          <div>
            <div className="hero-figures">
              <Figure label="已使用" bytes={used} />
              <Figure label="可用" bytes={drive.free} tone="free" />
              <Figure label="总容量" bytes={drive.total} />
            </div>
            {summary ? <SegmentBar summary={summary} /> : <UsageBar used={used} total={drive.total} thick />}
            <div style={{ display: "flex", gap: 8, alignItems: "center", marginTop: 10, fontSize: 13 }}>
              <span className={`badge ${tip?.tone}`}>已用 {percent(used, drive.total)}</span>
              <span className="muted">{tip?.text}</span>
            </div>
          </div>
          <HeroAction onGoClean={onGoClean} />
        </div>
      )}

      {scanningHere && <Scanning />}

      {summary && !scanningHere && (
        <div className="grid-2">
          <Breakdown summary={summary} />
          <ReleaseCard onGoClean={onGoClean} />
        </div>
      )}

      {!summary && !scanningHere && (
        <div className="card empty">
          <div className="empty-icon">
            <ScanSearch size={30} />
          </div>
          <div>
            还没有扫描 {driveName}，扫描后可以看到空间都用在哪里
            <div className="faint" style={{ fontSize: 13, marginTop: 4 }}>
              {scanningElsewhere ? `正在扫描 ${scanning?.root}，完成后才能开始下一个` : "整盘扫描通常需要一到三分钟"}
            </div>
          </div>
          <button className="btn btn-primary btn-lg" disabled={scanningElsewhere} onClick={() => startScan()}>
            扫描 {driveName}
          </button>
        </div>
      )}

      {summary?.cancelled && !scanningHere && (
        <div className="muted" style={{ marginTop: 12, fontSize: 13 }}>
          扫描被中途停止，以上数字不完整。
        </div>
      )}
    </div>
  );
}
