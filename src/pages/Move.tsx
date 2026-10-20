import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import {
  ArrowRight,
  Box,
  Circle,
  CircleCheck,
  CircleX,
  Container,
  FolderOpen,
  FolderSymlink,
  HardDrive,
  Info,
  RefreshCw,
  Shrink,
  TriangleAlert,
  Truck,
  Undo2,
} from "lucide-react";
import { api, type DriveInfo, type MoveCandidate, type MoveProgress, type VDisk } from "../api";
import { Dialog } from "../components/ui";
import { formatBytes } from "../format";
import { useStore } from "../store";

type Tab = "move" | "moved" | "vdisk";

const GB = 1024 ** 3;

/** `C:\Users\me\.gradle` on `D:\` → `D:\Users\me\.gradle`, as the backend does it. */
const targetFor = (path: string, drive: string) => drive + path.slice(3);
const letter = (root: string | null | undefined) => (root ?? "").replace("\\", "");
/** Room a move needs on the target: the backend wants 5% over the size plus 1 GB to spare. */
const roomFor = (bytes: number[]) => bytes.reduce((s, b) => s + b * 1.05, 0) + (bytes.length ? GB : 0);

const GROUPS: { key: MoveCandidate["group"]; title: string; hint: string }[] = [
  { key: "app", title: "应用数据", hint: "聊天文件、下载的歌曲这类越用越多的数据" },
  { key: "dev", title: "开发工具", hint: "SDK、模拟器和依赖缓存，搬走后工具照常使用" },
  { key: "project", title: "用户目录下的大文件夹", hint: "通常是代码项目，只列出 1 GB 以上的" },
  { key: "system", title: "系统文件夹", hint: "Windows 会直接把位置改到新磁盘，原位置不留链接" },
];

/** What a move (or a restore) does, in the words the progress view uses, and where each phase is. */
const STEPS = (knownFolder: boolean, restore: boolean) =>
  restore ? ["复制回 C 盘", "核对", "删除新位置的副本"] : ["复制", "核对", knownFolder ? "改位置" : "换成链接", "删除原件"];
const stepOf = (phase: MoveProgress["phase"], restore: boolean) =>
  restore ? { copy: 0, verify: 1, switch: 1, cleanup: 2 }[phase] : { copy: 0, verify: 1, switch: 2, cleanup: 3 }[phase];

type RunItem = {
  candidate: MoveCandidate;
  state: "waiting" | "running" | "done" | "failed";
  progress: MoveProgress | null;
  error: string | null;
};
/** A queue of moves (or one restore) worked through one at a time. */
type Run = { restore: boolean; drive: string | null; items: RunItem[] };

function DriveTile({
  title,
  drive,
  change,
  tone,
  children,
}: {
  title: string;
  drive: DriveInfo | undefined;
  /** Bytes leaving (negative) or arriving (positive). */
  change: number;
  tone: "out" | "in";
  children?: ReactNode;
}) {
  if (!drive) return <div className="move-drive" />;
  const used = drive.total - drive.free;
  const pct = (b: number) => `${Math.max(0, Math.min(100, (b / drive.total) * 100))}%`;
  const after = drive.free - change;
  return (
    <div className="move-drive">
      <div className="move-drive-head">
        <span className="faint">{title}</span>
        {children ?? (
          <span className="move-drive-letter">
            <HardDrive size={16} /> {letter(drive.root)}
          </span>
        )}
      </div>
      <div className="bar thick" style={{ margin: "12px 0 10px" }}>
        {tone === "out" ? (
          <>
            <span className="seg-used" style={{ width: pct(used + change) }} />
            <span className="seg-out" style={{ width: pct(-change) }} />
          </>
        ) : (
          <>
            <span className="seg-used" style={{ width: pct(used) }} />
            <span className="seg-in" style={{ width: pct(change) }} />
          </>
        )}
      </div>
      <div className="move-drive-free num">
        可用 <b>{formatBytes(drive.free)}</b>
        {change !== 0 && (
          <>
            <ArrowRight size={13} />
            <b className={tone === "out" ? "gain" : ""}>{formatBytes(Math.max(0, after))}</b>
          </>
        )}
        <span className="faint"> / 共 {formatBytes(drive.total)}</span>
      </div>
    </div>
  );
}

function MovePanel({ view, onMovedCount }: { view: "move" | "moved"; onMovedCount: (n: number) => void }) {
  const { drives, toast, refreshDrives } = useStore();
  const system = drives.find((d) => d.isSystem);
  const targets = useMemo(() => drives.filter((d) => !d.isSystem).sort((a, b) => b.free - a.free), [drives]);
  const [drive, setDrive] = useState<string | null>(null);
  const [list, setList] = useState<MoveCandidate[] | null>(null);
  const [measuring, setMeasuring] = useState(false);
  const [checking, setChecking] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [confirm, setConfirm] = useState<{ restore: MoveCandidate | null } | null>(null);
  const [run, setRunState] = useState<Run | null>(null);
  // The event handlers are registered once; they read the run through this.
  const runRef = useRef<Run | null>(null);
  const setRun = (r: Run | null) => {
    runRef.current = r;
    setRunState(r);
  };

  useEffect(() => {
    if (!drive && targets.length) setDrive(targets[0].root);
  }, [targets, drive]);

  // The quick status (no measuring) shows the list at once; the measured one fills in the sizes.
  const load = useCallback(() => {
    setMeasuring(true);
    let measured = false;
    api
      .migrationStatus()
      .then((l) => {
        if (measured) return;
        // Re-measuring: keep the sizes already known until the new ones arrive.
        setList((prev) => {
          const old = new Map((prev ?? []).map((c) => [c.id, c]));
          return l.map((c) => {
            const o = old.get(c.id);
            return !c.measured && o?.measured ? { ...c, size: o.size, measured: true } : c;
          });
        });
      })
      .catch(() => undefined);
    api
      .migrationCandidates()
      .then((l) => {
        measured = true;
        setList(l);
      })
      .catch((e) => toast(`读取失败：${e}`))
      .finally(() => setMeasuring(false));
  }, [toast]);
  useEffect(load, [load]);

  /** Re-checks which programs are running without measuring again. */
  const recheck = async () => {
    setChecking(true);
    try {
      const fresh = await api.migrationStatus();
      const byId = new Map(fresh.map((c) => [c.id, c]));
      setList((l) => l?.map((c) => ({ ...c, blockers: byId.get(c.id)?.blockers ?? c.blockers })) ?? l);
      const still = fresh.filter((c) => c.blockers.length > 0).flatMap((c) => c.blockers);
      toast(still.length ? `还在运行：${[...new Set(still)].join("、")}` : "都已关闭，可以搬了");
    } finally {
      setChecking(false);
    }
  };

  const startNext = (r: Run) => {
    const i = r.items.findIndex((x) => x.state === "waiting");
    if (i < 0) {
      setRun(r);
      return;
    }
    const items = r.items.map((x, j) => (j === i ? { ...x, state: "running" as const } : x));
    setRun({ ...r, items });
    const id = items[i].candidate.id;
    api.migrateFolder(id, r.drive, r.restore).catch((e) => finishRef.current(id, false, String(e)));
  };
  const finish = (id: string, ok: boolean, error: string | null) => {
    const r = runRef.current;
    const i = r?.items.findIndex((x) => x.candidate.id === id && x.state === "running") ?? -1;
    if (!r || i < 0) return;
    void refreshDrives();
    startNext({ ...r, items: r.items.map((x, j) => (j === i ? { ...x, state: ok ? "done" : "failed", error } : x)) });
  };
  const finishRef = useRef(finish);
  finishRef.current = finish;

  useEffect(() => {
    const offs = [
      api.onMigrateProgress((p) => {
        const r = runRef.current;
        if (!r) return;
        setRun({ ...r, items: r.items.map((x) => (x.candidate.id === p.id && x.state === "running" ? { ...x, progress: p } : x)) });
      }),
      api.onMigrateDone((d) => finishRef.current(d.id, d.ok, d.error)),
    ];
    return () => offs.forEach((p) => p.then((f) => f()));
  }, []);

  const targetDrive = drives.find((d) => d.root === drive);
  const all = list ?? [];
  const pending = all.filter((c) => !c.movedTo);
  const moved = all.filter((c) => c.movedTo);
  useEffect(() => {
    if (list !== null) onMovedCount(moved.length);
  }, [list, moved.length, onMovedCount]);
  const chosen = pending.filter((c) => selected.has(c.id) && c.measured && c.blockers.length === 0);
  const chosenBytes = chosen.reduce((s, c) => s + c.size, 0);
  const free = targetDrive?.free ?? 0;
  const fits = roomFor(chosen.map((c) => c.size)) <= free;

  /** Why a row cannot be ticked, or null when it can. */
  const blockedReason = (c: MoveCandidate): string | null => {
    if (c.blockers.length) return "blocked";
    if (!c.measured) return "measuring";
    if (!selected.has(c.id) && roomFor([...chosen.map((x) => x.size), c.size]) > free) return "space";
    return null;
  };

  const toggle = (c: MoveCandidate) => {
    if (blockedReason(c) && !selected.has(c.id)) return;
    setSelected((s) => {
      const n = new Set(s);
      if (n.has(c.id)) n.delete(c.id);
      else n.add(c.id);
      return n;
    });
  };

  const begin = (restore: MoveCandidate | null) => {
    setConfirm(null);
    const items = (restore ? [restore] : chosen).map((candidate) => ({ candidate, state: "waiting" as const, progress: null, error: null }));
    startNext({ restore: !!restore, drive, items });
  };

  const closeRun = () => {
    const r = runRef.current;
    const done = r?.items.filter((x) => x.state === "done").length ?? 0;
    setRun(null);
    setSelected(new Set());
    if (done) load();
  };

  if (!targets.length && view === "move") {
    return (
      <div className="card empty">
        <div className="empty-icon">
          <HardDrive size={30} />
        </div>
        <div>只找到一块磁盘。插上移动硬盘或加一块硬盘后，就可以把大文件夹搬过去。</div>
      </div>
    );
  }

  const row = (c: MoveCandidate) => {
    const reason = blockedReason(c);
    const on = selected.has(c.id) && !reason;
    const knownFolder = c.method === "knownFolder";
    return (
      <div
        key={c.id}
        className={`item move-pick ${on ? "selected" : ""} ${reason && !on ? "is-off" : ""} ${reason === "blocked" ? "has-action" : ""}`}
        onClick={() => toggle(c)}
      >
        <input type="checkbox" className="check" checked={on} disabled={!!reason && !on} onChange={() => toggle(c)} onClick={(e) => e.stopPropagation()} />
        <div className="move-icon">{knownFolder ? <FolderOpen size={18} /> : <FolderSymlink size={18} />}</div>
        <div style={{ minWidth: 0 }}>
          <div className="item-title">
            {c.title}
            {reason === "blocked" && (
              <span className="badge caution">
                <TriangleAlert size={11} /> 先退出 {c.blockers.join("、")}
              </span>
            )}
            {reason === "space" && <span className="badge review">{letter(drive)} 剩余空间不够</span>}
          </div>
          <div className="item-desc">{c.desc}</div>
          <div className="move-route" title={`${c.path} → ${targetFor(c.path, drive ?? "D:\\")}`}>
            <span>{c.path}</span>
            <ArrowRight size={12} style={{ flex: "none" }} />
            <span className={on ? "to" : ""}>{targetFor(c.path, drive ?? "D:\\")}</span>
          </div>
        </div>
        <div className="item-size num">{c.measured ? formatBytes(c.size) : <span className="skeleton size-skel" />}</div>
        {reason === "blocked" && (
          <button
            className="btn"
            disabled={checking}
            onClick={(e) => {
              e.stopPropagation();
              void recheck();
            }}
          >
            {checking ? <span className="spinner" style={{ width: 14, height: 14, borderWidth: 2 }} /> : <RefreshCw size={14} />}
            已退出，重新检查
          </button>
        )}
      </div>
    );
  };

  const movedView =
    list === null ? (
      [0, 1].map((i) => <div key={i} className="skeleton" style={{ height: 76, marginBottom: 8, borderRadius: 14 }} />)
    ) : moved.length ? (
      <>
        <div className="move-list-head">
          <span className="muted">
            {moved.length} 个文件夹在其他盘上，原路径照常可用；搬回 C 盘会先复制回来、核对无误再删掉新位置的副本
          </span>
        </div>
        <section className="group">
            {moved.map((c) => (
              <div key={c.id} className="item move-pick moved has-action">
                <span />
                <div className="move-icon" style={{ background: "var(--safe-soft)", color: "var(--safe)" }}>
                  <CircleCheck size={18} />
                </div>
                <div style={{ minWidth: 0 }}>
                  <div className="item-title">
                    {c.title}
                    <span className="badge safe">已搬到 {letter(c.movedTo?.slice(0, 3))}</span>
                  </div>
                  <div className="move-route" title={`${c.path} → ${c.movedTo}`}>
                    <span>{c.path}</span>
                    <ArrowRight size={12} style={{ flex: "none" }} />
                    <span className="to">{c.movedTo}</span>
                  </div>
                </div>
                <div className="item-size num">{formatBytes(c.size)}</div>
                <button className="btn" onClick={() => setConfirm({ restore: c })}>
                  <Undo2 size={15} /> 搬回 C 盘
                </button>
              </div>
            ))}
        </section>
      </>
    ) : (
      <div className="card empty">
        <div className="empty-icon">
          <FolderSymlink size={30} />
        </div>
        <div>还没有搬走的文件夹。在「搬到其他盘」里搬过的都会列在这里，随时可以搬回 C 盘。</div>
      </div>
    );

  return (
    <>
      {view === "moved" ? (
        movedView
      ) : (
        <>
          <div className="card move-hero">
            <div className="move-hero-drives">
              <DriveTile title="从" drive={system} change={-chosenBytes} tone="out" />
              <div className="move-hero-arrow">
                <div className="move-hero-arrow-icon">
                  <Truck size={18} />
                </div>
                <span className="num">{chosenBytes ? formatBytes(chosenBytes) : "勾选下方"}</span>
              </div>
              <DriveTile title="搬到" drive={targetDrive} change={chosenBytes} tone="in">
                {targets.length > 1 ? (
                  <div className="drive-tabs">
                    {targets.map((d) => (
                      <button key={d.root} className={`drive-tab ${drive === d.root ? "active" : ""}`} onClick={() => setDrive(d.root)}>
                        <HardDrive size={14} />
                        {letter(d.root)}
                      </button>
                    ))}
                  </div>
                ) : undefined}
              </DriveTile>
            </div>
            <div className="move-steps">
              {[
                ["完整复制", `先把整个文件夹复制到 ${letter(drive)}`],
                ["逐个核对", "文件数和大小一一对上才继续"],
                ["原位置换成链接", "软件照旧按原路径读写，不用改设置"],
                ["删除 C 盘原件", "这一步之前出错都会自动撤销"],
              ].map(([t, d], i) => (
                <div key={t} className="move-step">
                  <span className="move-step-num">{i + 1}</span>
                  <div>
                    <b>{t}</b>
                    {d}
                  </div>
                </div>
              ))}
            </div>
          </div>

          <div className="move-list-head">
            <span className="muted">
              {list === null ? "正在读取…" : measuring ? "正在测量各个文件夹的大小，大约需要十几秒…" : `找到 ${pending.length} 个可以搬的文件夹`}
            </span>
            <button className="btn btn-ghost" onClick={load} disabled={measuring}>
              {measuring ? <span className="spinner" style={{ width: 14, height: 14, borderWidth: 2 }} /> : <RefreshCw size={14} />}
              重新测量
            </button>
          </div>

          {list === null &&
            [0, 1, 2].map((i) => <div key={i} className="skeleton" style={{ height: 76, marginBottom: 8, borderRadius: 14 }} />)}

          {list !== null &&
            GROUPS.map((g) => {
              const rows = pending.filter((c) => c.group === g.key);
              const shown = g.key === "project" ? rows.filter((c) => c.measured) : rows;
              const looking = g.key === "project" && rows.some((c) => !c.measured);
              if (!shown.length && !looking) return null;
              const groupBytes = shown.reduce((s, c) => s + c.size, 0);
              return (
                <section key={g.key} className="group">
                  <div className="group-head">
                    <span className="group-title">{g.title}</span>
                    <span className="faint" style={{ fontSize: 13 }}>
                      {g.hint}
                    </span>
                    {groupBytes > 0 && <span className="group-size num">{formatBytes(groupBytes)}</span>}
                  </div>
                  {shown.map(row)}
                  {looking && (
                    <div className="move-looking">
                      <span className="spinner" style={{ width: 14, height: 14, borderWidth: 2 }} />
                      正在查找 1 GB 以上的文件夹…
                    </div>
                  )}
                </section>
              );
            })}

          {list !== null && !measuring && pending.length === 0 && (
            <div className="card empty">
              <div className="empty-icon">
                <CircleCheck size={30} />
              </div>
              <div>没有找到值得搬的大文件夹</div>
            </div>
          )}


          {list !== null && pending.length > 0 && (
            <div className="footer-bar">
              <div className="footer-total">
                {chosen.length ? (
                  <>
                    已选 {chosen.length} 项，C 盘将腾出
                    <b className="num">{formatBytes(chosenBytes)}</b>
                  </>
                ) : (
                  "勾选要搬的文件夹"
                )}
              </div>
              {chosen.length > 0 && (
                <button className="btn btn-ghost" onClick={() => setSelected(new Set())}>
                  清空
                </button>
              )}
              <div style={{ flex: 1 }} />
              {!fits && <span className="badge review">{letter(drive)} 放不下这么多</span>}
              <button className="btn btn-primary btn-lg" disabled={!chosen.length || !fits} onClick={() => setConfirm({ restore: null })}>
                <Truck size={18} /> 搬到 {letter(drive)}
              </button>
            </div>
          )}
        </>
      )}

      {confirm && (
        <ConfirmMove
          restore={confirm.restore}
          items={confirm.restore ? [confirm.restore] : chosen}
          drive={drive}
          system={system}
          target={targetDrive}
          onCancel={() => setConfirm(null)}
          onStart={() => begin(confirm.restore)}
        />
      )}

      {run && <RunDialog run={run} onClose={closeRun} />}
    </>
  );
}

function ConfirmMove({
  restore,
  items,
  drive,
  system,
  target,
  onCancel,
  onStart,
}: {
  restore: MoveCandidate | null;
  items: MoveCandidate[];
  drive: string | null;
  system: DriveInfo | undefined;
  target: DriveInfo | undefined;
  onCancel: () => void;
  onStart: () => void;
}) {
  const total = items.reduce((s, c) => s + c.size, 0);
  const anyKnown = items.some((c) => c.method === "knownFolder");
  const anyLink = items.some((c) => c.method === "junction");
  const blockers = [...new Set(items.flatMap((c) => c.blockers))];
  return (
    <Dialog onClose={onCancel}>
      <div className="dialog-body">
        <h2 className="dialog-title">
          {restore ? `把「${restore.title}」搬回 C 盘？` : `把 ${items.length} 个文件夹搬到 ${letter(drive)}？`}
        </h2>
        <div className="dialog-list">
          {items.map((c) => (
            <div key={c.id} className="dialog-list-row">
              <span style={{ minWidth: 0 }}>
                {c.title}
                <span className="detail-path" style={{ display: "block" }}>
                  {restore ? `${c.movedTo} → ${c.path}` : `${c.path} → ${targetFor(c.path, drive ?? "D:\\")}`}
                </span>
              </span>
              <span className="num" style={{ flex: "none" }}>
                {formatBytes(c.size)}
              </span>
            </div>
          ))}
        </div>
        {!restore && system && target && (
          <div className="move-confirm-sum num">
            <div>
              C 盘可用 {formatBytes(system.free)} <ArrowRight size={12} /> <b className="gain">{formatBytes(system.free + total)}</b>
            </div>
            <div>
              {letter(drive)} 可用 {formatBytes(target.free)} <ArrowRight size={12} /> <b>{formatBytes(target.free - total)}</b>
            </div>
          </div>
        )}
        <div className="muted" style={{ fontSize: 13, marginTop: 10 }}>
          {restore
            ? "会把文件复制回 C 盘原来的位置，核对无误后删除新位置上的副本。"
            : [
                anyLink && "搬完后原位置会留下一个链接，软件按原路径照常读写。",
                anyKnown && "「下载」「文档」这类系统文件夹会直接改到新位置。",
              ]
                .filter(Boolean)
                .join("")}
          文件多的话每项需要几分钟，期间不要打开里面的文件。
        </div>
        {blockers.length > 0 && (
          <div className="warn-box">
            <TriangleAlert size={18} style={{ flex: "none" }} />
            <div>开始前请先退出 {blockers.join("、")}。</div>
          </div>
        )}
        <div className="warn-box info">
          <Info size={18} style={{ flex: "none" }} />
          <div>复制或核对出问题时会自动撤销，原来的文件不会动；只有核对无误后，才会删除原件。</div>
        </div>
      </div>
      <div className="dialog-actions">
        <button className="btn" onClick={onCancel}>
          取消
        </button>
        <button className="btn btn-primary" onClick={onStart}>
          {restore ? "搬回 C 盘" : `开始搬（${formatBytes(total)}）`}
        </button>
      </div>
    </Dialog>
  );
}

function RunDialog({ run, onClose }: { run: Run; onClose: () => void }) {
  const finished = run.items.every((x) => x.state === "done" || x.state === "failed");
  const done = run.items.filter((x) => x.state === "done");
  const failed = run.items.filter((x) => x.state === "failed");
  const total = run.items.reduce((s, x) => s + x.candidate.size, 0);
  const progressed = run.items.reduce(
    (s, x) =>
      s +
      (x.state === "done" || x.state === "failed"
        ? x.candidate.size
        : x.state === "running" && x.progress
          ? (x.progress.done / Math.max(x.progress.total, 1)) * x.candidate.size
          : 0),
    0,
  );
  const doneBytes = done.reduce((s, x) => s + x.candidate.size, 0);
  const where = run.restore ? "C 盘" : letter(run.drive);
  const index = run.items.findIndex((x) => x.state === "running");

  const title = !finished
    ? run.restore
      ? `正在搬回 C 盘`
      : `正在搬到 ${where}`
    : failed.length === 0
      ? run.restore
        ? "已经搬回 C 盘"
        : "搬好了"
      : done.length === 0
        ? "没有搬成功"
        : `搬好 ${done.length} 项，${failed.length} 项没有完成`;

  return (
    <Dialog onClose={() => (finished ? onClose() : undefined)}>
      <div className="dialog-body">
        <div className="run-head">
          <div className={`run-head-icon ${finished ? (failed.length ? "warn" : "ok") : ""}`}>
            {!finished ? (
              <span className="spinner" />
            ) : failed.length ? (
              <TriangleAlert size={22} />
            ) : (
              <CircleCheck size={22} />
            )}
          </div>
          <div>
            <h2 className="dialog-title" style={{ margin: 0 }}>
              {title}
            </h2>
            <div className="muted num" style={{ fontSize: 13 }}>
              {!finished
                ? run.items.length > 1
                  ? `第 ${index + 1} / ${run.items.length} 项`
                  : `「${run.items[0].candidate.title}」· ${formatBytes(total)}`
                : run.restore
                  ? "原位置已经恢复成普通文件夹"
                  : done.length
                    ? `C 盘腾出了 ${formatBytes(doneBytes)}`
                    : "原来的文件没有动过"}
            </div>
          </div>
        </div>

        {!finished && (
          <div className="bar" style={{ margin: "16px 0 4px" }}>
            <span style={{ width: `${Math.max(3, (progressed / Math.max(total, 1)) * 100)}%`, background: "var(--grad)" }} />
          </div>
        )}

        <div className="run-list">
          {run.items.map((x) => {
            const steps = STEPS(x.candidate.method === "knownFolder", run.restore);
            const at = x.progress ? stepOf(x.progress.phase, run.restore) : 0;
            const pct = x.progress?.phase === "copy" ? Math.floor((x.progress.done / Math.max(x.progress.total, 1)) * 100) : null;
            return (
              <div key={x.candidate.id} className={`run-item ${x.state}`}>
                <div className="run-item-head">
                  {x.state === "waiting" && <Circle size={18} className="faint" />}
                  {x.state === "running" && <span className="spinner" style={{ width: 16, height: 16, borderWidth: 2, color: "var(--accent)" }} />}
                  {x.state === "done" && <CircleCheck size={18} color="var(--safe)" />}
                  {x.state === "failed" && <CircleX size={18} color="var(--review)" />}
                  <span className="run-item-title">{x.candidate.title}</span>
                  <span className="num faint" style={{ fontSize: 12.5 }}>
                    {x.state === "waiting" && `等待中 · ${formatBytes(x.candidate.size)}`}
                    {x.state === "running" && (pct !== null ? `${formatBytes(x.progress!.done)} / ${formatBytes(x.progress!.total)}` : formatBytes(x.candidate.size))}
                    {x.state === "done" && (run.restore ? "已搬回" : `已搬走 ${formatBytes(x.candidate.size)}`)}
                    {x.state === "failed" && "没有完成，原文件未动"}
                  </span>
                </div>
                {x.state === "running" && (
                  <div className="run-phases">
                    {steps.map((s, i) => (
                      <span key={s} className={`run-phase ${i < at ? "past" : i === at ? "on" : ""}`}>
                        {i < at ? <CircleCheck size={12} /> : <span className="run-phase-dot" />}
                        {s}
                        {i === at && pct !== null && ` ${pct}%`}
                      </span>
                    ))}
                  </div>
                )}
                {x.state === "failed" && x.error && <div className="run-error">{x.error}</div>}
              </div>
            );
          })}
        </div>

        {!finished && (
          <div className="faint" style={{ fontSize: 12.5, textAlign: "center", marginTop: 4 }}>
            请不要关闭 DiskMate，也不要打开正在搬的文件夹
          </div>
        )}
      </div>
      {finished && (
        <div className="dialog-actions">
          <button className="btn btn-primary" onClick={onClose}>
            完成
          </button>
        </div>
      )}
    </Dialog>
  );
}

const KIND_ICON = { wsl: Box, docker: Container, claude: Box, other: HardDrive } as const;

function VDiskPanel() {
  const { toast, refreshDrives } = useStore();
  const [list, setList] = useState<VDisk[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<VDisk | null>(null);
  const [results, setResults] = useState<Record<string, { before: number; after: number }>>({});

  const load = useCallback(() => {
    void api.listVdisks().then(setList);
  }, []);
  useEffect(load, [load]);

  const compact = async (d: VDisk) => {
    setConfirm(null);
    setBusy(d.path);
    try {
      const r = await api.compactVdisk(d.path);
      setResults((all) => ({ ...all, [d.path]: r }));
      toast(r.after < r.before ? `压缩完成，腾出 ${formatBytes(r.before - r.after)}` : "压缩完成，这个磁盘里没有可以腾出的空间");
      void refreshDrives();
    } catch (e) {
      toast(`没有完成：${e}`);
    } finally {
      setBusy(null);
      load();
    }
  };

  return (
    <>
      <div className="card card-pad" style={{ marginBottom: 14 }}>
        <div style={{ display: "flex", gap: 12 }}>
          <Info size={18} color="var(--accent)" style={{ flex: "none", marginTop: 2 }} />
          <div className="muted" style={{ fontSize: 13.5 }}>
            WSL、Docker 和虚拟机用的 .vhdx 虚拟磁盘只会越用越大：里面删掉了文件，外面的文件也不会变小。压缩会把这些空出来的部分还给磁盘。
            压缩时虚拟磁盘不能在使用，并且需要管理员授权。
          </div>
        </div>
      </div>

      {list === null && <div className="skeleton" style={{ height: 120 }} />}
      {list !== null && list.length === 0 && (
        <div className="card empty">
          <div className="empty-icon">
            <Shrink size={30} />
          </div>
          <div>没有找到虚拟磁盘</div>
        </div>
      )}

      {list?.map((d) => {
        const Icon = KIND_ICON[d.kind];
        const r = results[d.path];
        return (
          <div key={d.path} className="item move-row">
            <div className="move-icon">
              <Icon size={18} />
            </div>
            <div style={{ minWidth: 0 }}>
              <div className="item-title">
                {d.owner}
                <span className="badge neutral">{d.path.slice(0, 2)} 盘</span>
                {d.inUse && d.kind !== "wsl" && <span className="badge caution">正在使用</span>}
                {d.inUse && d.kind === "wsl" && <span className="badge neutral">WSL 正在运行</span>}
              </div>
              <div className="item-desc">
                {d.kind === "wsl"
                  ? "会先在 Linux 里整理出空闲空间（fstrim），然后关闭 WSL 再压缩。WSL 里正在运行的程序会被停止。"
                  : d.kind === "docker"
                    ? "请先退出 Docker Desktop。"
                    : d.kind === "claude"
                      ? "Claude 桌面应用的虚拟机磁盘。请先退出 Claude 桌面应用。"
                      : "请先关闭用到这个虚拟磁盘的程序。"}
              </div>
              <div className="detail-path">{d.path}</div>
              {r && (
                <div className="num" style={{ fontSize: 12.5, marginTop: 4, color: "var(--safe)" }}>
                  上次压缩：{formatBytes(r.before)} → {formatBytes(r.after)}
                </div>
              )}
            </div>
            <div className="item-size num">{formatBytes(d.size)}</div>
            <button className="btn" disabled={!!busy || (d.inUse && d.kind !== "wsl")} onClick={() => setConfirm(d)}>
              {busy === d.path ? <span className="spinner" style={{ width: 14, height: 14, borderWidth: 2 }} /> : <Shrink size={15} />}
              {busy === d.path ? "压缩中" : "压缩"}
            </button>
          </div>
        );
      })}

      {confirm && (
        <Dialog onClose={() => setConfirm(null)}>
          <div className="dialog-body">
            <h2 className="dialog-title">压缩 {confirm.owner} 的虚拟磁盘？</h2>
            <div className="muted" style={{ fontSize: 13.5 }}>
              {formatBytes(confirm.size)} · {confirm.path}
            </div>
            {confirm.kind === "wsl" && (
              <div className="warn-box">
                <TriangleAlert size={18} style={{ flex: "none" }} />
                <div>会关闭所有 WSL 发行版，里面正在运行的程序（开发服务器、数据库等）都会停止。请先保存好工作。</div>
              </div>
            )}
            <div className="faint" style={{ fontSize: 12.5, marginTop: 12 }}>
              会弹出管理员授权，请点「是」。磁盘越大耗时越久，几百 GB 的磁盘可能要十几分钟，期间不要启动用到它的程序。
            </div>
          </div>
          <div className="dialog-actions">
            <button className="btn" onClick={() => setConfirm(null)}>
              取消
            </button>
            <button className="btn btn-primary" onClick={() => void compact(confirm)}>
              开始压缩
            </button>
          </div>
        </Dialog>
      )}
    </>
  );
}

export function Move() {
  const [tab, setTab] = useState<Tab>("move");
  const [movedCount, setMovedCount] = useState(0);
  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1 className="page-title">搬家与压缩</h1>
          <div className="page-sub">把大文件夹搬到别的磁盘，把只涨不缩的虚拟磁盘压回去</div>
        </div>
        <div className="tabs">
          <button className={`tab ${tab === "move" ? "active" : ""}`} onClick={() => setTab("move")}>
            搬到其他盘
          </button>
          <button className={`tab ${tab === "moved" ? "active" : ""}`} onClick={() => setTab("moved")}>
            已经搬走的
            {movedCount > 0 && <span className="tab-count num">{movedCount}</span>}
          </button>
          <button className={`tab ${tab === "vdisk" ? "active" : ""}`} onClick={() => setTab("vdisk")}>
            压缩虚拟磁盘
          </button>
        </div>
      </div>
      {tab === "vdisk" ? <VDiskPanel /> : <MovePanel view={tab} onMovedCount={setMovedCount} />}
    </div>
  );
}
