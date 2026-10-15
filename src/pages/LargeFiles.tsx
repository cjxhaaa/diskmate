import { useEffect, useMemo, useState } from "react";
import { Archive, Disc, Download, File, FileBox, Film, FolderOpen, Lock, Trash } from "lucide-react";
import { api, type LargeFile } from "../api";
import { DeleteDialog } from "../components/DeleteDialog";
import { formatBytes, formatDate } from "../format";
import { useStore } from "../store";

type Kind = "all" | "video" | "archive" | "image" | "installer" | "other";

const KINDS: { id: Kind; label: string }[] = [
  { id: "all", label: "全部" },
  { id: "video", label: "视频" },
  { id: "archive", label: "压缩包" },
  { id: "installer", label: "安装包" },
  { id: "image", label: "镜像 / 虚拟磁盘" },
  { id: "other", label: "其他" },
];

const EXT: Record<Exclude<Kind, "all" | "other">, string[]> = {
  video: ["mp4", "mkv", "mov", "avi", "wmv", "flv", "webm"],
  archive: ["zip", "rar", "7z", "tar", "gz", "tgz", "xz", "bz2"],
  installer: ["exe", "msi", "apk", "aab", "dmg", "pkg", "appx", "msix"],
  image: ["iso", "img", "vhd", "vhdx", "vmdk", "qcow2", "wim"],
};

function kindOf(name: string): Kind {
  const ext = name.split(".").pop()?.toLowerCase() ?? "";
  for (const [k, list] of Object.entries(EXT)) if (list.includes(ext)) return k as Kind;
  return "other";
}

const ICON = { video: Film, archive: Archive, installer: Download, image: Disc, other: File, all: FileBox } as const;
const TINT: Record<Kind, string> = { video: "#EC4899", archive: "#F59E0B", installer: "#3FA9F5", image: "#7B4DFF", other: "#64748B", all: "#64748B" };

/** System-owned files (swap, hibernation) and anything outside the user folder can't be recycled here. */
function protectedReason(f: LargeFile): string | null {
  const lower = f.path.toLowerCase();
  if (/^[a-z]:\\(pagefile|hiberfil|swapfile)\.sys$/.test(lower)) return "系统文件，请在「智能清理」里处理";
  if (!/^[a-z]:\\users\\/.test(lower)) return "不在用户目录里，请在对应软件中处理";
  if (lower.includes("\\appdata\\")) return "软件的数据文件，请在对应软件中清理";
  // .android, .gradle, .cargo...: a tool's own data (an emulator disk, a cache index). Deleting one
  // file out of it breaks the tool rather than freeing space cleanly.
  if (/\\\.[^\\]+\\/.test(lower)) return "开发工具的数据文件，请在对应工具中处理";
  return null;
}

/** The large-files tab of the Files page: everything over 100 MB the last scan saw. */
export function LargeFilesPanel() {
  const { current, toast, refreshDrives, startScan, scanning } = useStore();
  const [files, setFiles] = useState<LargeFile[] | null>(null);
  const [kind, setKind] = useState<Kind>("all");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [confirm, setConfirm] = useState(false);
  // A result read back from history has no file list behind it.
  const scanDone = !!current && !current.fromHistory;

  useEffect(() => {
    setFiles(null);
    setSelected(new Set());
    if (current && !current.fromHistory) void api.largeFiles(current.root).then(setFiles);
  }, [current?.root, current?.scannedAt, current?.fromHistory]);

  const shown = useMemo(() => (files ?? []).filter((f) => kind === "all" || kindOf(f.name) === kind), [files, kind]);
  const chosen = (files ?? []).filter((f) => selected.has(f.path));
  const chosenBytes = chosen.reduce((s, f) => s + f.size, 0);

  const remove = async (permanent: boolean) => {
    setConfirm(false);
    const out = await api.deleteFiles(
      chosen.map((f) => f.path),
      permanent,
    );
    toast(`${permanent ? "已删除" : "已移到回收站"}：${formatBytes(out.freed)}${out.skippedFiles ? `，${out.skippedFiles} 个失败` : ""}`);
    setFiles((list) => list?.filter((f) => !selected.has(f.path)) ?? null);
    setSelected(new Set());
    void refreshDrives();
  };

  return (
    <>
      <div className="panel-head">
        <div className="muted" style={{ fontSize: 13 }}>
          超过 100 MB 的文件
        </div>
        <div className="filter-chips">
          {KINDS.map((k) => (
            <button key={k.id} className={`chip ${kind === k.id ? "active" : ""}`} onClick={() => setKind(k.id)}>
              {k.label}
            </button>
          ))}
        </div>
      </div>

      {!scanDone && (
        <div className="card empty">
          <div className="empty-icon">
            <FileBox size={30} />
          </div>
          <div>{current?.fromHistory ? "概览里的数字来自上次扫描，列出大文件需要重新扫描一次" : "扫描完成后会在这里列出大文件"}</div>
          {current?.fromHistory && (
            <button className="btn btn-primary" disabled={!!scanning} onClick={() => startScan()}>
              重新扫描
            </button>
          )}
        </div>
      )}

      {scanDone && (
        <div className="card" style={{ overflow: "hidden" }}>
          <table className="table">
            <thead>
              <tr>
                <th style={{ width: 44 }} />
                <th>文件</th>
                <th style={{ width: 120 }}>修改时间</th>
                <th style={{ width: 110, textAlign: "right" }}>大小</th>
                <th style={{ width: 52 }} />
              </tr>
            </thead>
            <tbody>
              {files === null &&
                [0, 1, 2, 3].map((i) => (
                  <tr key={i}>
                    <td colSpan={5}>
                      <div className="skeleton" style={{ height: 34 }} />
                    </td>
                  </tr>
                ))}
              {files !== null && shown.length === 0 && (
                <tr>
                  <td colSpan={5} className="faint" style={{ textAlign: "center", padding: 40 }}>
                    没有这一类的大文件
                  </td>
                </tr>
              )}
              {shown.map((f) => {
                const k = kindOf(f.name);
                const Icon = ICON[k];
                const blocked = protectedReason(f);
                return (
                  <tr key={f.path}>
                    <td>
                      {blocked ? (
                        <span title={blocked} style={{ display: "grid", placeItems: "center" }}>
                          <Lock size={15} color="var(--text-3)" />
                        </span>
                      ) : (
                        <input
                          type="checkbox"
                          className="check"
                          checked={selected.has(f.path)}
                          onChange={() =>
                            setSelected((s) => {
                              const n = new Set(s);
                              if (n.has(f.path)) n.delete(f.path);
                              else n.add(f.path);
                              return n;
                            })
                          }
                        />
                      )}
                    </td>
                    <td>
                      <div className="file-name">
                        <div className="file-icon" style={{ background: `color-mix(in srgb, ${TINT[k]} 16%, transparent)`, color: TINT[k] }}>
                          <Icon size={16} />
                        </div>
                        <div style={{ minWidth: 0 }}>
                          <div>{f.name}</div>
                          <div className="file-path" title={f.path}>
                            {f.path}
                          </div>
                        </div>
                      </div>
                    </td>
                    <td className="muted num">{formatDate(f.modified)}</td>
                    <td className="num" style={{ textAlign: "right", fontWeight: 650 }}>
                      {formatBytes(f.size)}
                    </td>
                    <td>
                      <button className="icon-btn" title="在资源管理器中显示" onClick={() => void api.revealPath(f.path)}>
                        <FolderOpen size={16} />
                      </button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}

      {chosen.length > 0 && (
        <div className="footer-bar">
          <div className="footer-total">
            已选 {chosen.length} 个文件，共<b className="num">{formatBytes(chosenBytes)}</b>
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
