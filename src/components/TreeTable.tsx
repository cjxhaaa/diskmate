import { useEffect, useRef, useState } from "react";
import { ChevronRight, File, Folder, Layers, Lock } from "lucide-react";
import { api, type TreeItem } from "../api";
import { dominantKind, kindColor } from "../filetypes";
import { formatAgo, formatBytes, formatCount, percent } from "../format";
import { itemKey } from "./SpaceMap";

interface Props {
  /** The folder on screen, with its contents one level down. */
  root: TreeItem;
  hovered: string | null;
  selected: string | null;
  onHover: (item: TreeItem | null) => void;
  onOpen: (item: TreeItem) => void;
  onSelect: (item: TreeItem) => void;
  onMenu: (item: TreeItem, x: number, y: number) => void;
}

/** How many contents a folder opened in the list shows; the rest are in its leftovers. */
const LIMIT = 500;

/**
 * The folder on screen as a list beside the map, biggest first: folders open in place (their
 * contents load when opened), and hovering or picking a row marks its tile on the map and the other
 * way round. Double-click a folder to go into it on the map.
 */
export function TreeTable({ root, hovered, selected, onHover, onOpen, onSelect, onMenu }: Props) {
  const [open, setOpen] = useState<Record<string, TreeItem[] | "loading">>({});
  const rows = useRef<Record<string, HTMLDivElement | null>>({});

  // Another folder, or the same one after a change: opened folders start closed again.
  useEffect(() => setOpen({}), [root]);

  // Picked on the map: bring its row into view.
  useEffect(() => {
    if (selected) rows.current[selected]?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  const toggle = (item: TreeItem) => {
    const key = itemKey(item);
    if (open[key]) {
      setOpen(({ [key]: _, ...rest }) => rest);
      return;
    }
    setOpen((o) => ({ ...o, [key]: "loading" }));
    api
      .getTree(item.path, 1, 0, LIMIT)
      .then((t) => setOpen((o) => (o[key] ? { ...o, [key]: t.children ?? [] } : o)))
      .catch(() => setOpen(({ [key]: _, ...rest }) => rest));
  };

  const render = (items: TreeItem[], level: number): React.ReactNode[] =>
    items.flatMap((item) => {
      const key = itemKey(item);
      const kids = open[key];
      const canExpand = item.kind === "dir" && item.hasChildren && !item.denied;
      const row = (
        <div
          key={key}
          ref={(el) => {
            rows.current[key] = el;
          }}
          className={`tt-row ${hovered === key ? "hover" : ""} ${selected === key ? "selected" : ""}`}
          style={{ paddingLeft: 6 + level * 16 }}
          onMouseEnter={() => onHover(item)}
          onMouseLeave={() => onHover(null)}
          onClick={() => onSelect(item)}
          onDoubleClick={() => canExpand && onOpen(item)}
          onContextMenu={(e) => {
            e.preventDefault();
            onSelect(item);
            onMenu(item, e.clientX, e.clientY);
          }}
          title={item.kind === "dir" ? "双击在地图里打开" : undefined}
        >
          <button
            className={`tt-chevron ${kids ? "open" : ""}`}
            style={{ visibility: canExpand ? "visible" : "hidden" }}
            onClick={(e) => {
              e.stopPropagation();
              toggle(item);
            }}
            aria-label={kids ? "收起" : "展开"}
          >
            <ChevronRight size={14} />
          </button>
          <ItemIcon item={item} />
          <div className="tt-main">
            <div className="tt-name" title={item.path}>
              {item.name}
            </div>
            <div className="tt-meta faint">
              {item.denied
                ? "没有权限读取"
                : [
                    percent(item.size, root.size),
                    item.kind !== "file" && item.files > 0 ? `${formatCount(item.files)} 个文件` : null,
                    item.modified > 0 ? formatAgo(item.modified) : null,
                  ]
                    .filter(Boolean)
                    .join(" · ")}
            </div>
            <div className="tt-bar">
              <span style={{ width: percent(item.size, root.size), background: kindColor(dominantKind(item.types)) }} />
            </div>
          </div>
          <div className="tt-size num">{item.denied ? "—" : formatBytes(item.size)}</div>
        </div>
      );
      if (!kids) return [row];
      if (kids === "loading")
        return [
          row,
          <div key={key + ":loading"} className="tt-loading faint" style={{ paddingLeft: 30 + (level + 1) * 16 }}>
            正在读取…
          </div>,
        ];
      return [row, ...render(kids, level + 1)];
    });

  const items = root.children ?? [];
  return (
    <div className="tree-table" role="tree" onMouseLeave={() => onHover(null)}>
      {items.length ? render(items, 0) : <div className="empty">这个文件夹是空的</div>}
    </div>
  );
}

export function ItemIcon({ item }: { item: { kind: string; denied?: boolean; types: number[] } }) {
  const color = kindColor(dominantKind(item.types));
  if (item.denied) return <Lock size={15} className="tt-icon" color="var(--text-3)" />;
  if (item.kind === "dir") return <Folder size={15} className="tt-icon" color={color} fill={color} fillOpacity={0.3} />;
  if (item.kind === "rest") return <Layers size={15} className="tt-icon" color={color} />;
  return <File size={15} className="tt-icon" color={color} />;
}
