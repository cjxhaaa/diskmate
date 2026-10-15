import { useState } from "react";
import { TriangleAlert } from "lucide-react";
import { formatBytes } from "../format";
import { Dialog } from "./ui";

interface Props {
  items: { name: string; size: number }[];
  onCancel: () => void;
  onConfirm: (permanent: boolean) => void;
  /** Extra line under the title, e.g. what "kept one copy" means. */
  note?: string;
}

/**
 * Confirms deleting files the user picked by hand. The Recycle Bin is the default, but it does not
 * free anything until it is emptied — which is the whole point here — so "delete for good" is one
 * tick away and says so.
 */
export function DeleteDialog({ items, onCancel, onConfirm, note }: Props) {
  const [permanent, setPermanent] = useState(false);
  const total = items.reduce((s, i) => s + i.size, 0);
  return (
    <Dialog onClose={onCancel}>
      <div className="dialog-body">
        <h2 className="dialog-title">
          删除 {items.length} 项，共 {formatBytes(total)}？
        </h2>
        {note && (
          <div className="muted" style={{ fontSize: 13.5 }}>
            {note}
          </div>
        )}
        <div className="dialog-list" style={{ maxHeight: 260, overflowY: "auto" }}>
          {items.map((i, n) => (
            <div key={n} className="dialog-list-row">
              <span className="ellipsis">{i.name}</span>
              <span className="num muted">{formatBytes(i.size)}</span>
            </div>
          ))}
        </div>
        <label className="perm-toggle">
          <input type="checkbox" className="check" checked={permanent} onChange={(e) => setPermanent(e.target.checked)} />
          <span>
            直接删除，不放进回收站
            <div className="faint" style={{ fontSize: 12.5 }}>
              放进回收站的文件还占着空间，要清空回收站后才真正腾出 {formatBytes(total)}
            </div>
          </span>
        </label>
        {permanent && (
          <div className="warn-box">
            <TriangleAlert size={18} style={{ flex: "none" }} />
            <div>直接删除后无法恢复，请确认这些文件你都不再需要。</div>
          </div>
        )}
      </div>
      <div className="dialog-actions">
        <button className="btn" onClick={onCancel}>
          取消
        </button>
        <button className={`btn ${permanent ? "btn-danger" : "btn-primary"}`} onClick={() => onConfirm(permanent)}>
          {permanent ? "直接删除" : "移到回收站"}
        </button>
      </div>
    </Dialog>
  );
}
