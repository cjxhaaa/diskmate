import { useState } from "react";
import { HardDrive, LoaderCircle } from "lucide-react";
import { api, type MoveOutcome } from "../api";
import { formatBytes } from "../format";
import { useStore } from "../store";
import { Dialog } from "./ui";

interface Props {
  items: { path: string; name: string; size: number }[];
  onCancel: () => void;
  onDone: (out: MoveOutcome, drive: string) => void;
}

/** Room kept free on the target drive beyond what is moved, matching the backend's check. */
const SPARE = 512 * 1024 * 1024;

/**
 * Moves files the user picked by hand to another drive, to the same path there — the other answer
 * to "C: is full" when a file is still wanted. Nothing is left behind at the old place.
 */
export function MoveDialog({ items, onCancel, onDone }: Props) {
  const { drives } = useStore();
  const from = items[0]?.path.slice(0, 3).toUpperCase() ?? "";
  const targets = drives.filter((d) => d.root.toUpperCase() !== from);
  const total = items.reduce((s, i) => s + i.size, 0);
  const fits = (free: number) => free >= total + SPARE;
  const [drive, setDrive] = useState<string | null>(targets.find((d) => fits(d.free))?.root ?? null);
  const [busy, setBusy] = useState(false);

  const run = async () => {
    if (!drive) return;
    setBusy(true);
    let out: MoveOutcome;
    try {
      out = await api.moveFiles(
        items.map((i) => i.path),
        drive,
      );
    } catch (e) {
      out = { freed: 0, removedFiles: 0, skippedFiles: items.length, error: String(e), moved: [] };
    }
    onDone(out, drive);
  };

  const example = items[0] && drive ? drive + items[0].path.slice(3) : null;

  return (
    <Dialog onClose={busy ? () => {} : onCancel}>
      <div className="dialog-body">
        <h2 className="dialog-title">
          把 {items.length} 项（{formatBytes(total)}）移到其他盘？
        </h2>
        <div className="muted" style={{ fontSize: 13.5 }}>
          移到另一块盘上的同一路径，原位置不再保留。复制完核对无误后才会删除原文件。
        </div>
        <div className="faint" style={{ fontSize: 12.5, marginTop: 6 }}>
          想把整个文件夹搬走、以后的新文件也直接存过去（比如整个「下载」文件夹），请用「搬家与压缩」。
        </div>
        {targets.length === 0 ? (
          <div className="warn-box" style={{ marginTop: 14 }}>
            <div>没有找到其他本地磁盘。</div>
          </div>
        ) : (
          <div className="dialog-list">
            {targets.map((d) => {
              const ok = fits(d.free);
              return (
                <label key={d.root} className="dialog-list-row" style={{ cursor: ok ? "pointer" : "not-allowed", opacity: ok ? 1 : 0.55 }}>
                  <span style={{ display: "flex", gap: 10, alignItems: "center" }}>
                    <input type="radio" name="move-drive" disabled={!ok || busy} checked={drive === d.root} onChange={() => setDrive(d.root)} />
                    <HardDrive size={15} />
                    {d.root.replace("\\", "")} {d.label}
                  </span>
                  <span className="num muted">{ok ? `可用 ${formatBytes(d.free)}` : `空间不够（可用 ${formatBytes(d.free)}）`}</span>
                </label>
              );
            })}
          </div>
        )}
        {example && (
          <div className="faint ellipsis" style={{ fontSize: 12.5 }} title={example}>
            例如：{example}
          </div>
        )}
        {busy && (
          <div className="muted" style={{ display: "flex", gap: 8, alignItems: "center", marginTop: 12, fontSize: 13.5 }}>
            <LoaderCircle size={16} style={{ animation: "spin 1s linear infinite" }} />
            正在移动，文件大时需要几分钟，请不要关闭 DiskMate…
          </div>
        )}
      </div>
      <div className="dialog-actions">
        <button className="btn" onClick={onCancel} disabled={busy}>
          取消
        </button>
        <button className="btn btn-primary" onClick={run} disabled={!drive || busy}>
          {busy ? "正在移动…" : "移过去"}
        </button>
      </div>
    </Dialog>
  );
}
