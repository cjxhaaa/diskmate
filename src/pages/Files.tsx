import { useEffect, useMemo, useState } from "react";
import {
  Archive,
  ChevronDown,
  ChevronRight,
  Copy,
  Disc,
  Download,
  File,
  FileText,
  Film,
  Folder,
  FolderOpen,
  Lock,
  PackageCheck,
  RefreshCw,
  Search,
  Square,
  Trash,
  Wand,
} from "lucide-react";
import { api, type DownloadGroup, type DownloadKind, type DupeGroup } from "../api";
import { DeleteDialog } from "../components/DeleteDialog";
import { formatBytes, formatCount, formatDate } from "../format";
import { useStore } from "../store";
import { LargeFilesPanel } from "./LargeFiles";

type Tab = "downloads" | "dupes" | "large";

const KIND_ICON: Record<DownloadKind, typeof File> = {
  installer: Download,
  archive: Archive,
  image: Disc,
  video: Film,
  document: FileText,
  folder: Folder,
  other: File,
};
const KIND_TINT: Record<DownloadKind, string> = {
  installer: "#3FA9F5",
  archive: "#F59E0B",
  image: "#7B4DFF",
  video: "#EC4899",
  document: "#10B981",
  folder: "#5B8CFF",
  other: "#64748B",
};

const GROUPS: { id: DownloadGroup; title: string; hint: string; preselect: boolean }[] = [
  { id: "installed", title: "已经装过的软件安装包", hint: "软件已经装好了，安装包一般用不上了", preselect: true },
  { id: "extracted", title: "已经解压过的压缩包", hint: "旁边就有解压出来的同名文件夹", preselect: true },
  { id: "stale", title: "很久没动过的大文件", hint: "超过 3 个月没修改、大于 100 MB，请确认后再删", preselect: false },
  { id: "other", title: "其他文件", hint: "按大小排列，自己看着处理", preselect: false },
];

function FileIcon({ kind }: { kind: DownloadKind }) {
  const Icon = KIND_ICON[kind];
  return (
    <div className="file-icon" style={{ background: `color-mix(in srgb, ${KIND_TINT[kind]} 16%, transparent)`, color: KIND_TINT[kind] }}>
      <Icon size={16} />
    </div>
  );
}

function DownloadsPanel() {
  const { downloads, downloadsLoading, loadDownloads, dropDownloads, toast, refreshDrives } = useStore();
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [openOther, setOpenOther] = useState(false);
  const [confirm, setConfirm] = useState(false);

  useEffect(() => {
    if (downloads === null && !downloadsLoading) loadDownloads();
  }, [downloads, downloadsLoading, loadDownloads]);

  // Ticks the groups that are almost certainly safe each time a fresh analysis arrives.
  useEffect(() => {
    if (!downloads) return;
    const pre = new Set(GROUPS.filter((g) => g.preselect).map((g) => g.id));
    setSelected(new Set(downloads.filter((d) => pre.has(d.group)).map((d) => d.path)));
  }, [downloads]);

  const items = downloads ?? [];
  const total = items.reduce((s, i) => s + i.size, 0);
  const chosen = items.filter((i) => selected.has(i.path));
  const toggle = (p: string) =>
    setSelected((s) => {
      const n = new Set(s);
      if (n.has(p)) n.delete(p);
      else n.add(p);
      return n;
    });

  const remove = async (permanent: boolean) => {
    setConfirm(false);
    const paths = chosen.map((c) => c.path);
    const out = await api.deleteFiles(paths, permanent);
    toast(`${permanent ? "已删除" : "已移到回收站"}：${formatBytes(out.freed)}${out.skippedFiles ? `，${out.skippedFiles} 项失败` : ""}`);
    dropDownloads(paths);
    setSelected(new Set());
    void refreshDrives();
  };

  if (downloads === null) {
    return (
      <div className="card card-pad">
        <div className="muted" style={{ marginBottom: 12 }}>
          正在整理下载文件夹…
        </div>
        {[0, 1, 2].map((i) => (
          <div key={i} className="skeleton" style={{ height: 48, marginBottom: 8 }} />
        ))}
      </div>
    );
  }

  return (
    <>
      <div className="panel-head">
        <div className="muted" style={{ fontSize: 13 }}>
          下载文件夹共 <b className="num">{formatBytes(total)}</b>，{formatCount(items.length)} 项
        </div>
        <div style={{ display: "flex", gap: 8 }}>
          <button className="btn" onClick={() => void api.revealPath(items[0]?.path.replace(/\\[^\\]+$/, "") ?? "")} disabled={!items.length}>
            <FolderOpen size={15} /> 打开文件夹
          </button>
          <button className="btn" disabled={downloadsLoading} onClick={loadDownloads}>
            <RefreshCw size={15} /> 重新整理
          </button>
        </div>
      </div>

      {GROUPS.map((g) => {
        const all = items.filter((i) => i.group === g.id);
        if (!all.length) return null;
        const isOther = g.id === "other";
        const rows = isOther && !openOther ? [] : isOther ? all.slice(0, 80) : all;
        const groupBytes = all.reduce((s, i) => s + i.size, 0);
        const allPicked = all.every((i) => selected.has(i.path));
        return (
          <section key={g.id} className="group">
            <div className="group-head" style={{ alignItems: "center" }}>
              {!isOther && (
                <input
                  type="checkbox"
                  className="check"
                  checked={allPicked}
                  onChange={() =>
                    setSelected((s) => {
                      const n = new Set(s);
                      all.forEach((i) => (allPicked ? n.delete(i.path) : n.add(i.path)));
                      return n;
                    })
                  }
                />
              )}
              <span className="group-title">{g.title}</span>
              <span className="faint" style={{ fontSize: 13 }}>
                {g.hint}
              </span>
              <span className="group-size num">
                {all.length} 项 · {formatBytes(groupBytes)}
              </span>
              {isOther && (
                <button className="icon-btn" onClick={() => setOpenOther((o) => !o)}>
                  {openOther ? <ChevronDown size={18} /> : <ChevronRight size={18} />}
                </button>
              )}
            </div>
            {rows.length > 0 && (
              <div className="card" style={{ overflow: "hidden" }}>
                {rows.map((i) => (
                  <div key={i.path} className={`file-row ${selected.has(i.path) ? "picked" : ""}`} onClick={() => toggle(i.path)}>
                    <input type="checkbox" className="check" checked={selected.has(i.path)} onChange={() => toggle(i.path)} onClick={(e) => e.stopPropagation()} />
                    <FileIcon kind={i.kind} />
                    <div style={{ minWidth: 0 }}>
                      <div className="ellipsis" style={{ fontWeight: 600 }} title={i.name}>
                        {i.name}
                      </div>
                      <div className="faint ellipsis" style={{ fontSize: 12 }}>
                        {i.reason ?? (i.isDir ? "文件夹" : "")}
                      </div>
                    </div>
                    <span className="muted num" style={{ fontSize: 13 }}>
                      {formatDate(i.modified)}
                    </span>
                    <span className="num" style={{ fontWeight: 650, textAlign: "right" }}>
                      {formatBytes(i.size)}
                    </span>
                    <button
                      className="icon-btn"
                      title="在资源管理器中显示"
                      onClick={(e) => {
                        e.stopPropagation();
                        void api.revealPath(i.path);
                      }}
                    >
                      <FolderOpen size={16} />
                    </button>
                  </div>
                ))}
                {isOther && all.length > rows.length && (
                  <div className="faint" style={{ padding: "10px 16px", fontSize: 12.5 }}>
                    只显示最大的 {rows.length} 项，其余 {all.length - rows.length} 项都很小
                  </div>
                )}
              </div>
            )}
          </section>
        );
      })}

      {chosen.length > 0 && (
        <div className="footer-bar">
          <div className="footer-total">
            已选 {chosen.length} 项，共<b className="num">{formatBytes(chosen.reduce((s, i) => s + i.size, 0))}</b>
          </div>
          <div style={{ flex: 1 }} />
          <button className="btn btn-ghost" onClick={() => setSelected(new Set())}>
            取消选择
          </button>
          <button className="btn btn-primary btn-lg" onClick={() => setConfirm(true)}>
            <Trash size={17} /> 删除
          </button>
        </div>
      )}

      {confirm && <DeleteDialog items={chosen} onCancel={() => setConfirm(false)} onConfirm={remove} />}
    </>
  );
}

/** Chat apps keep received files outside AppData; deleting one breaks it in the chat history. */
const isProtected = (path: string) => /\\(xwechat_files|wechat files|tencent files)\\/i.test(path);
const copySuffix = /[ _-]?\(\d+\)(\.[^.\\]+)?$/;
const inDownloads = (path: string) => /\\downloads\\/i.test(path);

/**
 * The copy of each group to keep: a protected one (it is referenced from somewhere), then one
 * without a "(1)" suffix, then one outside Downloads, then the oldest.
 */
function keeperOf(g: DupeGroup): string {
  const score = (path: string, modified: number) => [
    isProtected(path) ? 0 : 1,
    copySuffix.test(path) ? 1 : 0,
    inDownloads(path) ? 1 : 0,
    modified,
  ];
  const best = [...g.files].sort((a, b) => {
    const sa = score(a.path, a.modified);
    const sb = score(b.path, b.modified);
    for (let i = 0; i < sa.length; i++) if (sa[i] !== sb[i]) return sa[i] - sb[i];
    return 0;
  })[0];
  return best.path;
}

const MIN_SIZES = [
  { bytes: 1024 ** 2, label: "1 MB 以上" },
  { bytes: 10 * 1024 ** 2, label: "10 MB 以上" },
  { bytes: 100 * 1024 ** 2, label: "100 MB 以上" },
];

function DupesPanel() {
  const { dupes, findDupes, cancelDupes, dropDupeFiles, toast, refreshDrives } = useStore();
  const [minSize, setMinSize] = useState(MIN_SIZES[0].bytes);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [confirm, setConfirm] = useState(false);
  const groups = dupes.status === "done" ? dupes.groups : [];

  const smartPick = () => {
    const pick = new Set<string>();
    for (const g of groups) {
      const keep = keeperOf(g);
      g.files.forEach((f) => f.path !== keep && !isProtected(f.path) && pick.add(f.path));
    }
    setSelected(pick);
  };

  // A finished search starts with the smart selection, so one click deletes the obvious copies.
  const doneAt = dupes.status === "done" ? dupes.at : 0;
  useEffect(() => {
    if (doneAt) smartPick();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [doneAt]);

  const chosen = useMemo(() => groups.flatMap((g) => g.files.filter((f) => selected.has(f.path)).map((f) => ({ ...f, size: g.size, name: f.path }))), [groups, selected]);
  const wasted = groups.reduce((s, g) => s + g.wasted, 0);

  const toggle = (g: DupeGroup, path: string) => {
    // Never every copy: the last unticked one stays unticked. Checked here, not inside the state
    // updater — updaters can run twice, and so would the toast.
    if (!selected.has(path) && !g.files.some((f) => f.path !== path && !selected.has(f.path))) {
      toast("每组至少要留一份");
      return;
    }
    setSelected((s) => {
      const n = new Set(s);
      if (n.has(path)) n.delete(path);
      else n.add(path);
      return n;
    });
  };

  const remove = async (permanent: boolean) => {
    setConfirm(false);
    const paths = chosen.map((c) => c.path);
    try {
      const out = await api.deleteDuplicates(paths, permanent);
      toast(`${permanent ? "已删除" : "已移到回收站"} ${out.removedFiles} 个重复文件，${formatBytes(out.freed)}`);
      dropDupeFiles(paths);
      setSelected(new Set());
      void refreshDrives();
    } catch (e) {
      toast(String(e));
    }
  };

  return (
    <>
      <div className="panel-head">
        <div className="muted" style={{ fontSize: 13 }}>
          在用户文件夹里找内容完全相同的文件。跳过 AppData、开发工具目录和代码仓库，那里的重复是正常的。
        </div>
        <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
          <div className="filter-chips">
            {MIN_SIZES.map((m) => (
              <button key={m.bytes} className={`chip ${minSize === m.bytes ? "active" : ""}`} onClick={() => setMinSize(m.bytes)}>
                {m.label}
              </button>
            ))}
          </div>
          {dupes.status === "running" ? (
            <button className="btn" onClick={cancelDupes}>
              <Square size={14} /> 停止
            </button>
          ) : (
            <button className="btn btn-primary" onClick={() => findDupes(minSize)}>
              <Search size={15} /> {dupes.status === "done" ? "重新查找" : "开始查找"}
            </button>
          )}
        </div>
      </div>

      {dupes.status === "idle" && (
        <div className="card empty">
          <div className="empty-icon">
            <Copy size={30} />
          </div>
          <div>
            重复下载、到处复制的文件会悄悄占掉不少空间
            <div className="faint" style={{ fontSize: 13, marginTop: 4 }}>
              先按大小筛，再比对文件头尾，最后才完整比对内容，通常一分钟内完成
            </div>
          </div>
          <button className="btn btn-primary btn-lg" onClick={() => findDupes(minSize)}>
            开始查找
          </button>
        </div>
      )}

      {dupes.status === "running" && (
        <div className="card card-pad">
          <div style={{ display: "flex", gap: 14, alignItems: "center", marginBottom: 14 }}>
            <div className="pulse-ring" style={{ width: 46, height: 46 }}>
              <Copy size={20} />
            </div>
            <div>
              <div style={{ fontWeight: 650 }}>{dupes.progress?.phase === "compare" ? "正在比对内容" : "正在查找文件"}</div>
              <div className="muted num" style={{ fontSize: 13 }}>
                {dupes.progress?.phase === "compare"
                  ? `已比对 ${formatBytes(dupes.progress.hashedBytes)} / ${formatBytes(dupes.progress.toHashBytes)}`
                  : `已找到 ${formatCount(dupes.progress?.files ?? 0)} 个候选文件`}
              </div>
            </div>
          </div>
          <div className="bar">
            <span
              style={{
                width: `${dupes.progress?.phase === "compare" && dupes.progress.toHashBytes ? Math.max(4, (dupes.progress.hashedBytes / dupes.progress.toHashBytes) * 100) : 6}%`,
                background: "var(--grad)",
              }}
            />
          </div>
        </div>
      )}

      {dupes.status === "done" && groups.length === 0 && (
        <div className="card empty">
          <div className="empty-icon">
            <PackageCheck size={30} />
          </div>
          <div>没有找到重复文件</div>
        </div>
      )}

      {dupes.status === "done" && groups.length > 0 && (
        <>
          <div className="dupe-summary">
            <span>
              找到 <b className="num">{groups.length}</b> 组重复，删掉多余的副本可以腾出 <b className="num">{formatBytes(wasted)}</b>
            </span>
            <button className="btn" onClick={smartPick}>
              <Wand size={15} /> 智能选择
            </button>
            <button className="btn btn-ghost" onClick={() => setSelected(new Set())}>
              全部取消
            </button>
          </div>
          {groups.map((g) => (
            <div key={g.id} className="item" style={{ marginBottom: 8 }}>
              <div className="dupe-head">
                <span className="ellipsis" style={{ fontWeight: 650 }} title={g.name}>
                  {g.name}
                </span>
                <span className="muted num" style={{ fontSize: 13 }}>
                  {formatBytes(g.size)} × {g.files.length}
                </span>
                <span className="num" style={{ fontWeight: 650, color: "var(--accent-text)" }}>
                  可省 {formatBytes(g.wasted)}
                </span>
              </div>
              {g.files.map((f) => {
                const locked = isProtected(f.path);
                return (
                  <div key={f.path} className={`dupe-file ${selected.has(f.path) ? "picked" : ""}`} onClick={() => !locked && toggle(g, f.path)}>
                    {locked ? (
                      <span title="微信 / QQ 的聊天文件：删掉后聊天记录里的这个文件会打不开" style={{ display: "grid", placeItems: "center", width: 20 }}>
                        <Lock size={14} color="var(--text-3)" />
                      </span>
                    ) : (
                      <input type="checkbox" className="check" checked={selected.has(f.path)} onChange={() => toggle(g, f.path)} onClick={(e) => e.stopPropagation()} />
                    )}
                    <span className="detail-path ellipsis" style={{ fontSize: 12.5, color: "var(--text-2)" }} title={f.path}>
                      {f.path}
                    </span>
                    <span className="faint num" style={{ fontSize: 12 }}>
                      {formatDate(f.modified)}
                    </span>
                    <span>{!selected.has(f.path) && <span className="badge safe">保留</span>}</span>
                    <button
                      className="icon-btn"
                      onClick={(e) => {
                        e.stopPropagation();
                        void api.revealPath(f.path);
                      }}
                    >
                      <FolderOpen size={15} />
                    </button>
                  </div>
                );
              })}
            </div>
          ))}
        </>
      )}

      {chosen.length > 0 && (
        <div className="footer-bar">
          <div className="footer-total">
            已选 {chosen.length} 个副本，共<b className="num">{formatBytes(chosen.reduce((s, c) => s + c.size, 0))}</b>
          </div>
          <div style={{ flex: 1 }} />
          <button className="btn btn-primary btn-lg" onClick={() => setConfirm(true)}>
            <Trash size={17} /> 删除选中的副本
          </button>
        </div>
      )}

      {confirm && (
        <DeleteDialog
          items={chosen.map((c) => ({ name: c.path, size: c.size }))}
          note="每组都至少保留了一份，被保留的那份不受影响。"
          onCancel={() => setConfirm(false)}
          onConfirm={remove}
        />
      )}
    </>
  );
}

const TABS: { id: Tab; label: string }[] = [
  { id: "downloads", label: "下载文件夹" },
  { id: "dupes", label: "重复文件" },
  { id: "large", label: "大文件" },
];

export function Files() {
  const [tab, setTab] = useState<Tab>("downloads");
  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1 className="page-title">文件整理</h1>
          <div className="page-sub">找出可以删的安装包、压缩包、重复文件和大文件</div>
        </div>
        <div className="tabs">
          {TABS.map((t) => (
            <button key={t.id} className={`tab ${tab === t.id ? "active" : ""}`} onClick={() => setTab(t.id)}>
              {t.label}
            </button>
          ))}
        </div>
      </div>
      {tab === "downloads" && <DownloadsPanel />}
      {tab === "dupes" && <DupesPanel />}
      {tab === "large" && <LargeFilesPanel />}
    </div>
  );
}

