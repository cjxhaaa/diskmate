import { useEffect, useMemo, useState } from "react";
import { Check, ChevronDown, ChevronRight, CircleCheckBig, FolderOpen, Info, RefreshCw, ShieldCheck, Sparkles, TriangleAlert } from "lucide-react";
import { api, type CleanItem, type CleanResult } from "../api";
import { GROUP_ORDER, GROUPS, RISKS } from "../categories";
import { AnimatedNumber, Dialog } from "../components/ui";
import { formatBytes, formatCount, splitBytes } from "../format";
import { useStore } from "../store";

type Phase = { kind: "list" } | { kind: "confirm" } | { kind: "cleaning"; done: number; total: number; current: string } | { kind: "result"; results: CleanResult[]; freeBefore: number; freeAfter: number };

function ItemRow({
  item,
  selected,
  expanded,
  onToggle,
  onExpand,
}: {
  item: CleanItem;
  selected: boolean;
  expanded: boolean;
  onToggle: () => void;
  onExpand: () => void;
}) {
  const risk = RISKS[item.risk];
  return (
    <div className={`item ${selected ? "selected" : ""}`}>
      <div className="item-main" onClick={item.guideOnly ? onExpand : onToggle}>
        {item.guideOnly ? (
          <Info size={20} color="var(--text-3)" />
        ) : (
          <input type="checkbox" className="check" checked={selected} onChange={onToggle} onClick={(e) => e.stopPropagation()} />
        )}
        <div style={{ minWidth: 0 }}>
          <div className="item-title">
            {item.title}
            {!item.guideOnly && <span className={`badge ${risk.tone}`}>{risk.label}</span>}
            {item.needsAdmin && <span className="badge neutral">需要管理员</span>}
          </div>
          <div className="item-desc">{item.desc}</div>
          {item.note && (
            <div className="item-note">
              <TriangleAlert size={14} style={{ flex: "none", marginTop: 2 }} />
              {item.note}
            </div>
          )}
        </div>
        <div className="item-size num">{formatBytes(item.size)}</div>
        <button
          className="icon-btn"
          title={expanded ? "收起" : "查看包含哪些位置"}
          onClick={(e) => {
            e.stopPropagation();
            onExpand();
          }}
        >
          {expanded ? <ChevronDown size={18} /> : <ChevronRight size={18} />}
        </button>
      </div>
      {expanded && (
        <div className="item-details">
          {item.details.length === 0 && <div className="faint" style={{ fontSize: 13 }}>没有可以展示的位置</div>}
          {item.details.map((d) => (
            <div key={d.path + d.label} className="detail-row">
              <div style={{ minWidth: 0 }}>
                <div>{d.label}</div>
                <div className="detail-path">{d.path}</div>
              </div>
              <span className="num faint">{formatBytes(d.size)}</span>
              {d.path.includes("\\") ? (
                <button className="icon-btn" title="在资源管理器中打开" onClick={() => void api.revealPath(d.path)}>
                  <FolderOpen size={16} />
                </button>
              ) : (
                <span />
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function Analyzing() {
  const { analysis } = useStore();
  const p = analysis.status === "loading" ? analysis.progress : null;
  const pct = p ? (p.done / p.total) * 100 : 4;
  return (
    <div className="card card-pad">
      <div style={{ display: "flex", gap: 14, alignItems: "center", marginBottom: 14 }}>
        <div className="pulse-ring" style={{ width: 46, height: 46 }}>
          <Sparkles size={20} />
        </div>
        <div>
          <div style={{ fontWeight: 650 }}>正在查找可以清理的内容</div>
          <div className="muted" style={{ fontSize: 13 }}>
            {p ? `${p.current}（${p.done}/${p.total}）` : "检查临时文件、开发缓存、应用缓存…"}
          </div>
        </div>
      </div>
      <div className="bar">
        <span style={{ width: `${pct}%`, background: "var(--grad)" }} />
      </div>
      <div style={{ marginTop: 22, display: "grid", gap: 10 }}>
        {[0, 1, 2, 3].map((i) => (
          <div key={i} className="skeleton" style={{ height: 64, opacity: 1 - i * 0.2 }} />
        ))}
      </div>
    </div>
  );
}

function ConfirmDialog({ items, onCancel, onConfirm }: { items: CleanItem[]; onCancel: () => void; onConfirm: () => void }) {
  const total = items.reduce((s, i) => s + i.size, 0);
  const risky = items.filter((i) => i.risk !== "safe");
  const admin = items.some((i) => i.needsAdmin);
  const hibernation = items.some((i) => i.id === "hibernation");
  return (
    <Dialog onClose={onCancel}>
      <div className="dialog-body">
        <h2 className="dialog-title">清理 {items.length} 项，约 {formatBytes(total)}</h2>
        <div className="muted" style={{ fontSize: 13.5 }}>
          这些文件会被直接删除，不进回收站。正在被使用的文件会自动跳过。
        </div>
        <div className="dialog-list">
          {items.map((i) => (
            <div key={i.id} className="dialog-list-row">
              <span style={{ display: "flex", gap: 8, alignItems: "center" }}>
                {i.title}
                <span className={`badge ${RISKS[i.risk].tone}`}>{RISKS[i.risk].label}</span>
              </span>
              <span className="num muted">{formatBytes(i.size)}</span>
            </div>
          ))}
        </div>
        {risky.length > 0 && (
          <div className="warn-box">
            <TriangleAlert size={18} style={{ flex: "none" }} />
            <div>
              其中 {risky.map((i) => `「${i.title}」`).join("")} 不是纯缓存：删掉后需要重新下载、重新编译，或者无法恢复。确认你已经看过说明。
            </div>
          </div>
        )}
        {admin && (
          <div className="faint" style={{ fontSize: 12.5, marginTop: 10 }}>
            部分项目需要管理员权限，没有权限的文件会跳过。
            {hibernation && "关闭休眠时会弹出系统授权窗口，请点「是」。"}
          </div>
        )}
      </div>
      <div className="dialog-actions">
        <button className="btn" onClick={onCancel}>
          再想想
        </button>
        <button className={`btn ${risky.length ? "btn-danger" : "btn-primary"}`} onClick={onConfirm}>
          确认清理
        </button>
      </div>
    </Dialog>
  );
}

function Cleaning({ phase }: { phase: Extract<Phase, { kind: "cleaning" }> }) {
  const pct = phase.total ? (phase.done / phase.total) * 100 : 0;
  return (
    <div className="card card-pad" style={{ textAlign: "center", padding: "56px 32px" }}>
      <div className="pulse-ring" style={{ margin: "0 auto 18px", width: 70, height: 70 }}>
        <Sparkles size={30} />
      </div>
      <div style={{ fontSize: 18, fontWeight: 700 }}>正在清理…</div>
      <div className="muted" style={{ margin: "6px 0 22px" }}>
        {phase.current}（{Math.min(phase.done + 1, phase.total)}/{phase.total}）
      </div>
      <div className="bar" style={{ maxWidth: 420, margin: "0 auto" }}>
        <span style={{ width: `${Math.max(pct, 6)}%`, background: "var(--grad)" }} />
      </div>
      <div className="faint" style={{ fontSize: 12.5, marginTop: 14 }}>
        文件很多时可能需要一两分钟，请不要关闭程序
      </div>
    </div>
  );
}

function Result({ phase, onDone }: { phase: Extract<Phase, { kind: "result" }>; onDone: () => void }) {
  const freed = phase.results.reduce((s, r) => s + r.outcome.freed, 0);
  const skipped = phase.results.reduce((s, r) => s + r.outcome.skippedFiles, 0);
  const [, u] = splitBytes(freed);
  return (
    <div className="card">
      <div className="result-hero">
        <div className="result-check">
          <Check size={40} strokeWidth={3} />
        </div>
        <div className="muted">清理完成，释放了</div>
        <div className="release-big num" style={{ fontSize: 56, margin: "6px 0 10px" }}>
          <AnimatedNumber value={freed} format={(x) => splitBytes(x)[0]} />
          <small> {u}</small>
        </div>
        <div className="muted num">
          可用空间 {formatBytes(phase.freeBefore)} → <b style={{ color: "var(--safe)" }}>{formatBytes(phase.freeAfter)}</b>
        </div>
      </div>
      <div style={{ padding: "0 28px 8px" }}>
        {phase.results.map((r) => (
          <div key={r.id} className="dialog-list-row" style={{ borderTop: "1px solid var(--border)" }}>
            <span style={{ display: "flex", gap: 8, alignItems: "center" }}>
              {r.outcome.error ? <TriangleAlert size={16} color="var(--caution)" /> : <CircleCheckBig size={16} color="var(--safe)" />}
              {r.title}
              {r.outcome.error && <span className="faint" style={{ fontSize: 12.5 }}>{r.outcome.error}</span>}
            </span>
            <span className="num muted">{formatBytes(r.outcome.freed)}</span>
          </div>
        ))}
      </div>
      {skipped > 0 && (
        <div className="faint" style={{ padding: "8px 28px 0", fontSize: 12.5 }}>
          {formatCount(skipped)} 个文件正在被使用或没有权限，已跳过。关掉对应软件后再清理一次可以删得更干净。
        </div>
      )}
      <div style={{ display: "flex", justifyContent: "center", padding: "22px 0 28px" }}>
        <button className="btn btn-primary btn-lg" onClick={onDone}>
          完成
        </button>
      </div>
    </div>
  );
}

export function Cleanup() {
  const { analysis, analyze, cleanup, drives, root, toast } = useStore();
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [phase, setPhase] = useState<Phase>({ kind: "list" });

  useEffect(() => {
    if (analysis.status === "idle") analyze();
  }, [analysis.status, analyze]);

  // A fresh analysis resets the ticks to each item's default.
  const analyzedAt = analysis.status === "done" ? analysis.at : 0;
  useEffect(() => {
    if (analysis.status === "done") setSelected(new Set(analysis.items.filter((i) => i.selected && !i.guideOnly).map((i) => i.id)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [analyzedAt]);

  useEffect(() => {
    const off = api.onCleanProgress((p) => {
      const title = analysis.status === "done" ? analysis.items.find((i) => i.id === p.id)?.title ?? "" : "";
      setPhase((ph) => (ph.kind === "cleaning" ? { ...ph, done: p.done, total: p.total, current: title } : ph));
    });
    return () => void off.then((f) => f());
  }, [analysis]);

  const items = analysis.status === "done" ? analysis.items : [];
  const chosen = useMemo(() => items.filter((i) => selected.has(i.id)), [items, selected]);
  const chosenBytes = chosen.reduce((s, i) => s + i.size, 0);

  const toggle = (id: string) =>
    setSelected((s) => {
      const n = new Set(s);
      if (n.has(id)) n.delete(id);
      else n.add(id);
      return n;
    });
  const toggleExpand = (id: string) =>
    setExpanded((s) => {
      const n = new Set(s);
      if (n.has(id)) n.delete(id);
      else n.add(id);
      return n;
    });

  const run = async () => {
    const freeBefore = drives.find((d) => d.root === root)?.free ?? 0;
    const ids = chosen.map((i) => i.id);
    setPhase({ kind: "cleaning", done: 0, total: ids.length, current: chosen[0]?.title ?? "" });
    try {
      const results = await cleanup(ids);
      const list = await api.listDrives();
      const freeAfter = list.find((d) => d.root === root)?.free ?? freeBefore;
      setPhase({ kind: "result", results, freeBefore, freeAfter });
    } catch (e) {
      toast(`清理出错：${e}`);
      setPhase({ kind: "list" });
    }
  };

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1 className="page-title">智能清理</h1>
          <div className="page-sub">每一项都写明了是什么、删了有什么影响。默认只勾选安全的项目。</div>
        </div>
        {phase.kind === "list" && (
          <button className="btn" disabled={analysis.status === "loading"} onClick={analyze}>
            <RefreshCw size={15} /> 重新检查
          </button>
        )}
      </div>

      {phase.kind === "cleaning" && <Cleaning phase={phase} />}
      {phase.kind === "result" && (
        <Result
          phase={phase}
          onDone={() => {
            setPhase({ kind: "list" });
            analyze();
          }}
        />
      )}

      {(phase.kind === "list" || phase.kind === "confirm") && (
        <>
          {analysis.status === "loading" && <Analyzing />}
          {analysis.status === "error" && <div className="card card-pad">检查失败：{analysis.message}</div>}
          {analysis.status === "done" && items.length === 0 && (
            <div className="card empty">
              <div className="empty-icon">
                <ShieldCheck size={30} />
              </div>
              <div>很干净，没有找到值得清理的内容</div>
            </div>
          )}
          {analysis.status === "done" &&
            GROUP_ORDER.map((g) => {
              const groupItems = items.filter((i) => i.group === g);
              if (groupItems.length === 0) return null;
              const groupBytes = groupItems.reduce((s, i) => s + i.size, 0);
              return (
                <section key={g} className="group">
                  <div className="group-head">
                    <span className="group-title">{GROUPS[g].label}</span>
                    <span className="faint" style={{ fontSize: 13 }}>
                      {GROUPS[g].hint}
                    </span>
                    <span className="group-size num">{formatBytes(groupBytes)}</span>
                  </div>
                  {groupItems.map((item) => (
                    <ItemRow
                      key={item.id}
                      item={item}
                      selected={selected.has(item.id)}
                      expanded={expanded.has(item.id)}
                      onToggle={() => toggle(item.id)}
                      onExpand={() => toggleExpand(item.id)}
                    />
                  ))}
                </section>
              );
            })}

          {analysis.status === "done" && items.length > 0 && (
            <div className="footer-bar">
              <div className="footer-total">
                已选 {chosen.length} 项，可释放
                <b className="num">{formatBytes(chosenBytes)}</b>
              </div>
              <button
                className="btn btn-ghost"
                onClick={() => setSelected(new Set(items.filter((i) => i.risk === "safe" && !i.guideOnly).map((i) => i.id)))}
              >
                只选安全项
              </button>
              <div style={{ flex: 1 }} />
              <button className="btn btn-primary btn-lg" disabled={chosen.length === 0} onClick={() => setPhase({ kind: "confirm" })}>
                <Sparkles size={18} /> 立即清理
              </button>
            </div>
          )}
        </>
      )}

      {phase.kind === "confirm" && <ConfirmDialog items={chosen} onCancel={() => setPhase({ kind: "list" })} onConfirm={run} />}
    </div>
  );
}
