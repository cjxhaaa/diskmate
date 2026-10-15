import { useCallback, useEffect, useRef, useState } from "react";
import { Box, Container, FolderOpen, FolderSymlink, HardDrive, Info, RefreshCw, Shrink, TriangleAlert, Undo2 } from "lucide-react";
import { api, type MoveCandidate, type MoveProgress, type VDisk } from "../api";
import { Dialog } from "../components/ui";
import { formatBytes } from "../format";
import { useStore } from "../store";

type Tab = "move" | "vdisk";

const PHASE_LABEL: Record<MoveProgress["phase"], string> = {
  copy: "正在复制",
  verify: "正在逐个核对文件",
  switch: "正在切换到新位置",
  cleanup: "正在删除原来的文件",
};

/** `C:\Users\me\.gradle` on `D:\` → `D:\Users\me\.gradle`, as the backend does it. */
const targetFor = (path: string, drive: string) => drive + path.slice(3);

type Job = { candidate: MoveCandidate; restore: boolean; progress: MoveProgress | null };

function MovePanel() {
  const { drives, toast, refreshDrives } = useStore();
  const targets = drives.filter((d) => !d.isSystem);
  const [drive, setDrive] = useState<string | null>(null);
  const [list, setList] = useState<MoveCandidate[] | null>(null);
  const [confirm, setConfirm] = useState<{ candidate: MoveCandidate; restore: boolean } | null>(null);
  const [job, setJob] = useState<Job | null>(null);
  // Read by the event handlers, which are registered once and would otherwise see a stale job.
  const jobRef = useRef<Job | null>(null);
  jobRef.current = job;

  useEffect(() => {
    if (!drive && targets.length) setDrive(targets.sort((a, b) => b.free - a.free)[0].root);
  }, [targets, drive]);

  const load = useCallback(() => {
    setList(null);
    api.migrationCandidates().then(setList).catch((e) => toast(`读取失败：${e}`));
  }, [toast]);

  useEffect(load, [load]);

  useEffect(() => {
    const offs = [
      api.onMigrateProgress((p) => setJob((j) => (j && j.candidate.id === p.id ? { ...j, progress: p } : j))),
      api.onMigrateDone((d) => {
        const j = jobRef.current;
        if (!j || j.candidate.id !== d.id) return;
        // Outside any state updater: updaters can run twice, and the toast would too.
        toast(d.ok ? `「${j.candidate.title}」已${j.restore ? "搬回 C 盘" : "搬走"}` : `没有完成：${d.error}`);
        setJob(null);
        void refreshDrives();
        load();
      }),
    ];
    return () => offs.forEach((p) => p.then((f) => f()));
  }, [toast, refreshDrives, load]);

  const targetDrive = drives.find((d) => d.root === drive);

  if (!targets.length) {
    return (
      <div className="card empty">
        <div className="empty-icon">
          <HardDrive size={30} />
        </div>
        <div>只找到一块磁盘。插上移动硬盘或加一块硬盘后，就可以把大文件夹搬过去。</div>
      </div>
    );
  }

  const pending = (list ?? []).filter((c) => !c.movedTo);
  const moved = (list ?? []).filter((c) => c.movedTo);

  return (
    <>
      <div className="panel-head">
        <div className="muted" style={{ fontSize: 13, maxWidth: 560 }}>
          先完整复制到新磁盘、逐个核对，确认无误后，原位置换成一个指向新位置的链接，软件照原来的路径访问，不用改任何设置。
        </div>
        <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
          <span className="muted" style={{ fontSize: 13 }}>
            搬到
          </span>
          <div className="drive-tabs">
            {targets.map((d) => (
              <button key={d.root} className={`drive-tab ${drive === d.root ? "active" : ""}`} onClick={() => setDrive(d.root)}>
                <HardDrive size={15} />
                {d.root.replace("\\", "")} 剩 {formatBytes(d.free)}
              </button>
            ))}
          </div>
          <button className="icon-btn" title="重新测量" onClick={load}>
            <RefreshCw size={16} />
          </button>
        </div>
      </div>

      {list === null && (
        <div className="card card-pad">
          <div className="muted" style={{ marginBottom: 12 }}>
            正在测量各个文件夹的大小…
          </div>
          {[0, 1, 2, 3].map((i) => (
            <div key={i} className="skeleton" style={{ height: 58, marginBottom: 8 }} />
          ))}
        </div>
      )}

      {pending.map((c) => (
        <div key={c.id} className="item move-row">
          <div className="move-icon">{c.method === "knownFolder" ? <FolderOpen size={18} /> : <FolderSymlink size={18} />}</div>
          <div style={{ minWidth: 0 }}>
            <div className="item-title">
              {c.title}
              {c.method === "knownFolder" && <span className="badge neutral">系统文件夹</span>}
              {c.blockers.length > 0 && (
                <span className="badge caution">
                  <TriangleAlert size={11} /> 请先关闭 {c.blockers.join("、")}
                </span>
              )}
            </div>
            <div className="item-desc">{c.desc}</div>
            <div className="detail-path">{c.path}</div>
          </div>
          <div className="item-size num">{formatBytes(c.size)}</div>
          <button
            className="btn"
            disabled={!drive || c.blockers.length > 0 || !!job || (targetDrive ? targetDrive.free < c.size * 1.05 + 1024 ** 3 : true)}
            title={targetDrive && targetDrive.free < c.size * 1.05 + 1024 ** 3 ? "目标磁盘空间不够" : undefined}
            onClick={() => setConfirm({ candidate: c, restore: false })}
          >
            搬到 {drive?.replace("\\", "")}
          </button>
        </div>
      ))}

      {moved.length > 0 && (
        <section className="group" style={{ marginTop: 18 }}>
          <div className="group-head">
            <span className="group-title">已经搬走的</span>
            <span className="faint" style={{ fontSize: 13 }}>
              原位置是指向新位置的链接，删除新位置的文件就等于删除原文件
            </span>
          </div>
          {moved.map((c) => (
            <div key={c.id} className="item move-row">
              <div className="move-icon" style={{ background: "var(--safe-soft)", color: "var(--safe)" }}>
                <FolderSymlink size={18} />
              </div>
              <div style={{ minWidth: 0 }}>
                <div className="item-title">{c.title}</div>
                <div className="detail-path">
                  {c.path} → {c.movedTo}
                </div>
              </div>
              <div className="item-size num">{formatBytes(c.size)}</div>
              <button className="btn" disabled={!!job} onClick={() => setConfirm({ candidate: c, restore: true })}>
                <Undo2 size={15} /> 搬回 C 盘
              </button>
            </div>
          ))}
        </section>
      )}

      {confirm && (
        <Dialog onClose={() => setConfirm(null)}>
          <div className="dialog-body">
            <h2 className="dialog-title">
              {confirm.restore ? `把「${confirm.candidate.title}」搬回 C 盘？` : `把「${confirm.candidate.title}」搬到 ${drive?.replace("\\", "")}？`}
            </h2>
            <div className="dialog-list">
              <div className="dialog-list-row">
                <span>大小</span>
                <span className="num">{formatBytes(confirm.candidate.size)}</span>
              </div>
              <div className="dialog-list-row">
                <span>从</span>
                <span className="detail-path">{confirm.restore ? confirm.candidate.movedTo : confirm.candidate.path}</span>
              </div>
              <div className="dialog-list-row">
                <span>到</span>
                <span className="detail-path">{confirm.restore ? confirm.candidate.path : targetFor(confirm.candidate.path, drive ?? "D:\\")}</span>
              </div>
            </div>
            <div className="muted" style={{ fontSize: 13 }}>
              {confirm.candidate.method === "knownFolder"
                ? "Windows 会把这个文件夹的位置改到新磁盘，资源管理器和各个软件以后都直接用新位置。"
                : "复制完成并核对无误后，原位置会换成一个链接，软件按原路径照常读写。"}
              文件多的话需要几分钟，期间不要打开里面的文件。
            </div>
            <div className="warn-box">
              <Info size={18} style={{ flex: "none" }} />
              <div>复制或核对出问题时会自动撤销，原来的文件不会动。只有核对无误之后，才会删除 C 盘上的原件。</div>
            </div>
          </div>
          <div className="dialog-actions">
            <button className="btn" onClick={() => setConfirm(null)}>
              取消
            </button>
            <button
              className="btn btn-primary"
              onClick={() => {
                const { candidate, restore } = confirm;
                setConfirm(null);
                setJob({ candidate, restore, progress: null });
                api.migrateFolder(candidate.id, drive, restore).catch((e) => {
                  toast(String(e));
                  setJob(null);
                });
              }}
            >
              {confirm.restore ? "搬回来" : "开始搬"}
            </button>
          </div>
        </Dialog>
      )}

      {job && (
        <Dialog onClose={() => undefined}>
          <div className="dialog-body" style={{ textAlign: "center", padding: "32px 30px 30px" }}>
            <div className="pulse-ring" style={{ margin: "0 auto 16px" }}>
              <FolderSymlink size={24} />
            </div>
            <h2 className="dialog-title">
              {job.restore ? "正在搬回" : "正在搬"}「{job.candidate.title}」
            </h2>
            <div className="muted num" style={{ marginBottom: 16 }}>
              {job.progress ? PHASE_LABEL[job.progress.phase] : "准备中"}
              {job.progress?.phase === "copy" && ` ${formatBytes(job.progress.done)} / ${formatBytes(job.progress.total)}`}
            </div>
            <div className="bar">
              <span
                style={{
                  width: `${job.progress ? Math.max(4, (job.progress.done / Math.max(job.progress.total, 1)) * 100) : 3}%`,
                  background: "var(--grad)",
                }}
              />
            </div>
            <div className="faint" style={{ fontSize: 12.5, marginTop: 12 }}>
              请不要关闭 DiskMate
            </div>
          </div>
        </Dialog>
      )}
    </>
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
          <button className={`tab ${tab === "vdisk" ? "active" : ""}`} onClick={() => setTab("vdisk")}>
            压缩虚拟磁盘
          </button>
        </div>
      </div>
      {tab === "move" ? <MovePanel /> : <VDiskPanel />}
    </div>
  );
}
