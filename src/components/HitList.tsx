import type { ReactNode } from "react";
import type { Hit } from "../api";
import { dominantKind, KINDS } from "../filetypes";
import { formatAgo, formatBytes, formatCount, formatDate } from "../format";
import { ItemIcon } from "./TreeTable";

interface Props {
  hits: Hit[];
  /** Click a row: show it on the map. */
  onShow: (hit: Hit) => void;
  onMenu: (hit: Hit, x: number, y: number) => void;
  /** How a row says when it last changed. */
  dates?: "ago" | "full";
  empty?: ReactNode;
}

/** Folders and files found in a scan, biggest first, each with where it is. */
export function HitList({ hits, onShow, onMenu, dates = "ago", empty }: Props) {
  if (!hits.length) return <div className="empty">{empty ?? "没有找到"}</div>;
  return (
    <div className="hit-list">
      {hits.map((h) => (
        <div
          key={h.path}
          className="hit-row"
          onClick={() => onShow(h)}
          onContextMenu={(e) => {
            e.preventDefault();
            onMenu(h, e.clientX, e.clientY);
          }}
          title="点击在空间地图里查看，右键查看更多操作"
        >
          <ItemIcon item={{ kind: h.isDir ? "dir" : "file", types: h.types }} />
          <div className="hit-main">
            <div className="hit-name">{h.name}</div>
            <div className="hit-path faint">{h.path}</div>
          </div>
          <div className="hit-meta faint">
            {h.isDir ? `${formatCount(h.files)} 个文件` : KINDS[dominantKind(h.types)].short}
            {h.modified > 0 && <div>{dates === "full" ? `${formatDate(h.modified)} · ${formatAgo(h.modified)}` : formatAgo(h.modified)}</div>}
          </div>
          <div className="hit-size num">{formatBytes(h.size)}</div>
        </div>
      ))}
    </div>
  );
}
