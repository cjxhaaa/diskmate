import { useEffect, useMemo, useState } from "react";
import { FolderOpen, TrendingDown, TrendingUp } from "lucide-react";
import { api, type FolderChange, type ScanDiff, type ScanSummary } from "../api";
import { CATEGORIES } from "../categories";
import { formatBytes } from "../format";
import { useStore } from "../store";

function ago(unixSecs: number): string {
  const mins = Math.floor((Date.now() / 1000 - unixSecs) / 60);
  if (mins < 1) return "刚刚";
  if (mins < 60) return `${mins} 分钟前`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours} 小时前`;
  const days = Math.floor(hours / 24);
  return days < 30 ? `${days} 天前` : `${Math.floor(days / 30)} 个月前`;
}

function stamp(unixSecs: number): string {
  const d = new Date(unixSecs * 1000);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getMonth() + 1}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

function signed(bytes: number): string {
  if (bytes === 0) return "0";
  return `${bytes > 0 ? "+" : "−"}${formatBytes(Math.abs(bytes))}`;
}

/** `C:\Users\me\xwechat_files` → `~\xwechat_files`: the home prefix is the same on every row. */
function short(path: string): string {
  return path.replace(/^[a-z]:\\users\\[^\\]+/i, "~");
}

const used = (s: ScanSummary) => s.total - s.free;

function Trend({ history, base, onPick }: { history: ScanSummary[]; base: number | null; onPick: (at: number) => void }) {
  const [hover, setHover] = useState<number | null>(null);
  const W = 720;
  const H = 180;
  const pad = { l: 8, r: 8, t: 16, b: 26 };
  const values = history.map(used);
  const lo = Math.min(...values);
  const hi = Math.max(...values);
  // Zoomed to the range the values actually move in, so a few GB of change is visible on a 500 GB disk.
  const span = Math.max(hi - lo, 1024 ** 3);
  const min = lo - span * 0.25;
  const max = hi + span * 0.25;
  const x = (i: number) => pad.l + (history.length === 1 ? (W - pad.l - pad.r) / 2 : (i / (history.length - 1)) * (W - pad.l - pad.r));
  const y = (v: number) => pad.t + (1 - (v - min) / (max - min)) * (H - pad.t - pad.b);
  const line = values.map((v, i) => `${i ? "L" : "M"}${x(i).toFixed(1)},${y(v).toFixed(1)}`).join(" ");
  const area = `${line} L${x(values.length - 1)},${H - pad.b} L${x(0)},${H - pad.b} Z`;
  const shown = hover ?? history.length - 1;

  return (
    <div className="card card-pad">
      <div className="card-title" style={{ justifyContent: "space-between" }}>
        <span>已用空间的变化</span>
        <span className="muted num" style={{ fontSize: 13, fontWeight: 500 }}>
          {stamp(history[shown].scannedAt)} · 已用 {formatBytes(values[shown])}
        </span>
      </div>
      <svg viewBox={`0 0 ${W} ${H}`} width="100%" style={{ display: "block", overflow: "visible" }} onMouseLeave={() => setHover(null)}>
        <defs>
          <linearGradient id="trend-fill" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0" stopColor="var(--accent)" stopOpacity="0.28" />
            <stop offset="1" stopColor="var(--accent)" stopOpacity="0" />
          </linearGradient>
        </defs>
        <line x1={pad.l} x2={W - pad.r} y1={H - pad.b} y2={H - pad.b} stroke="var(--border)" />
        <path d={area} fill="url(#trend-fill)" />
        <path d={line} fill="none" stroke="var(--accent)" strokeWidth={2.5} strokeLinejoin="round" />
        {history.map((s, i) => (
          <g key={s.scannedAt} onMouseEnter={() => setHover(i)} onClick={() => i < history.length - 1 && onPick(s.scannedAt)} style={{ cursor: i < history.length - 1 ? "pointer" : "default" }}>
            {/* Wide invisible hit area: the dot itself is too small to aim at. */}
            <rect x={x(i) - 14} y={pad.t} width={28} height={H - pad.t - pad.b} fill="transparent" />
            <circle
              cx={x(i)}
              cy={y(values[i])}
              r={s.scannedAt === base || i === shown ? 5.5 : 3.5}
              fill={s.scannedAt === base ? "var(--caution)" : "var(--bg-elev)"}
              stroke={s.scannedAt === base ? "var(--caution)" : "var(--accent)"}
              strokeWidth={2}
            />
          </g>
        ))}
        <text x={pad.l} y={H - 6} fontSize={11} fill="var(--text-3)">
          {stamp(history[0].scannedAt)}
        </text>
        <text x={W - pad.r} y={H - 6} fontSize={11} fill="var(--text-3)" textAnchor="end">
          {stamp(history[history.length - 1].scannedAt)}
        </text>
      </svg>
      <div className="faint" style={{ fontSize: 12.5, marginTop: 6 }}>
        点图上的某次扫描，下面就和那次比。橙色的点是当前的对比基准。
      </div>
    </div>
  );
}

function ChangeList({ title, icon, rows, tone }: { title: string; icon: React.ReactNode; rows: FolderChange[]; tone: "up" | "down" }) {
  if (!rows.length) return null;
  const max = Math.max(...rows.map((r) => Math.abs(r.delta)));
  return (
    <div className="card card-pad">
      <div className="card-title">
        {icon} {title}
      </div>
      {rows.map((r) => (
        <div key={r.path} className="change-row">
          <div style={{ minWidth: 0 }}>
            <div className="ellipsis" style={{ fontWeight: 600 }} title={r.path}>
              {short(r.path)}
            </div>
            <div className="faint num" style={{ fontSize: 12 }}>
              {r.before === null ? "原来不到 50 MB" : formatBytes(r.before)} → {r.after === null ? "不到 50 MB" : formatBytes(r.after)}
            </div>
          </div>
          <div className="bar" style={{ height: 6 }}>
            <span style={{ width: `${(Math.abs(r.delta) / max) * 100}%`, background: tone === "up" ? "#F97316" : "var(--safe)" }} />
          </div>
          <span className="num" style={{ fontWeight: 700, textAlign: "right", color: tone === "up" ? "#EA580C" : "var(--safe)" }}>
            {signed(r.delta)}
          </span>
          <button className="icon-btn" title="在资源管理器中打开" onClick={() => void api.revealPath(r.path)}>
            <FolderOpen size={15} />
          </button>
        </div>
      ))}
    </div>
  );
}

export function Changes() {
  const { root, scanCount, scanning, startScan } = useStore();
  const [history, setHistory] = useState<ScanSummary[] | null>(null);
  const [base, setBase] = useState<number | null>(null);
  const [diff, setDiff] = useState<ScanDiff | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setHistory(null);
    void api.scanHistory(root).then((h) => {
      setHistory(h);
      setBase(h.length >= 2 ? h[h.length - 2].scannedAt : null);
    });
  }, [root, scanCount]);

  const latest = history?.[history.length - 1] ?? null;

  useEffect(() => {
    if (!latest || base === null) return setDiff(null);
    setError(null);
    api
      .scanDiff(root, base, latest.scannedAt)
      .then(setDiff)
      .catch((e) => setError(String(e)));
  }, [root, base, latest]);

  const baseOptions = useMemo(() => (history ?? []).slice(0, -1).reverse(), [history]);

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1 className="page-title">空间变化</h1>
          <div className="page-sub">每次扫描都会记一笔，对比之后就知道空间是被谁慢慢吃掉的</div>
        </div>
        {baseOptions.length > 0 && (
          <label className="sort" style={{ marginLeft: 0 }}>
            和
            <select value={base ?? ""} onChange={(e) => setBase(Number(e.target.value))}>
              {baseOptions.map((s) => (
                <option key={s.scannedAt} value={s.scannedAt}>
                  {ago(s.scannedAt)}（{stamp(s.scannedAt)}）
                </option>
              ))}
            </select>
            的扫描比
          </label>
        )}
      </div>

      {history === null && <div className="skeleton" style={{ height: 220 }} />}

      {history !== null && history.length < 2 && (
        <div className="card empty">
          <div className="empty-icon">
            <TrendingUp size={30} />
          </div>
          <div>
            {history.length === 0 ? "还没有扫描记录" : `已经记下了 ${ago(history[0].scannedAt)}的扫描`}
            <div className="faint" style={{ fontSize: 13, marginTop: 4 }}>
              再扫描一次就能看到这段时间里哪些文件夹变大了。隔几天扫描一次，变化会更明显。
            </div>
          </div>
          <button className="btn btn-primary" disabled={!!scanning} onClick={() => startScan()}>
            {scanning ? "正在扫描…" : "现在扫描"}
          </button>
        </div>
      )}

      {history !== null && history.length >= 2 && (
        <div style={{ display: "grid", gap: 18 }}>
          <Trend history={history} base={base} onPick={setBase} />

          {error && <div className="card card-pad">{error}</div>}

          {diff && (
            <>
              <div className="card card-pad diff-hero">
                <div>
                  <div className="faint" style={{ fontSize: 12.5 }}>
                    {ago(diff.baseAt)} → {ago(diff.currentAt)}，已用空间
                  </div>
                  <div className="num" style={{ fontSize: 34, fontWeight: 750, color: diff.usedDelta > 0 ? "#EA580C" : "var(--safe)" }}>
                    {signed(diff.usedDelta)}
                  </div>
                </div>
                <div className="diff-cats">
                  {diff.categories.slice(0, 6).map((c) => (
                    <div key={c.id} className="diff-cat">
                      <span className="legend-dot" style={{ background: CATEGORIES[c.id]?.color ?? "#94A3B8" }} />
                      <span className="ellipsis">{CATEGORIES[c.id]?.label ?? c.id}</span>
                      <span className="num" style={{ fontWeight: 650, color: c.delta > 0 ? "#EA580C" : "var(--safe)" }}>
                        {signed(c.delta)}
                      </span>
                    </div>
                  ))}
                  {diff.categories.length === 0 && <span className="muted">各类别都没有明显变化</span>}
                </div>
              </div>

              {diff.grew.length === 0 && diff.shrank.length === 0 && (
                <div className="card card-pad muted">没有哪个文件夹变化超过 100 MB。</div>
              )}
              <ChangeList title="变大的文件夹" icon={<TrendingUp size={17} color="#EA580C" />} rows={diff.grew} tone="up" />
              <ChangeList title="变小的文件夹" icon={<TrendingDown size={17} color="var(--safe)" />} rows={diff.shrank} tone="down" />
            </>
          )}
        </div>
      )}
    </div>
  );
}
