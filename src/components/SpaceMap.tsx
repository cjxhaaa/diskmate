import { memo, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { TreeItem } from "../api";
import { dominantKind, KINDS, kindColor, kindInk, kindsOf } from "../filetypes";
import { formatAgo, formatBytes, formatCount, percent } from "../format";

interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** A tree item's identity: a folder's leftovers share its path, so the kind is part of it. */
export const itemKey = (i: { kind: string; path: string }) => `${i.kind}:${i.path}`;

/**
 * Squarified treemap (Bruls, Huizing & van Wijk): lays items out in rows along the shorter side,
 * adding to a row while that keeps the worst aspect ratio from getting worse.
 */
export function squarify(values: number[], box: Rect): Rect[] {
  const out: Rect[] = new Array(values.length);
  const total = values.reduce((a, b) => a + b, 0);
  if (total <= 0 || box.w <= 0 || box.h <= 0) return values.map(() => ({ x: box.x, y: box.y, w: 0, h: 0 }));
  const scale = (box.w * box.h) / total;
  const areas = values.map((v) => v * scale);
  let { x, y, w, h } = box;
  let i = 0;

  const worst = (sum: number, max: number, min: number, side: number) =>
    Math.max((side * side * max) / (sum * sum), (sum * sum) / (side * side * min));

  while (i < areas.length) {
    const side = Math.min(w, h);
    let sum = areas[i];
    let max = areas[i];
    let min = areas[i];
    let j = i + 1;
    while (j < areas.length) {
      const a = areas[j];
      const next = worst(sum + a, Math.max(max, a), Math.min(min, a), side);
      if (next > worst(sum, max, min, side)) break;
      sum += a;
      max = Math.max(max, a);
      min = Math.min(min, a);
      j++;
    }
    if (w >= h) {
      const rowW = sum / h;
      let yy = y;
      for (let k = i; k < j; k++) {
        const hh = areas[k] / rowW;
        out[k] = { x, y: yy, w: rowW, h: hh };
        yy += hh;
      }
      x += rowW;
      w -= rowW;
    } else {
      const rowH = sum / w;
      let xx = x;
      for (let k = i; k < j; k++) {
        const ww = areas[k] / rowH;
        out[k] = { x: xx, y, w: ww, h: rowH };
        xx += ww;
      }
      y += rowH;
      h -= rowH;
    }
    i = j;
  }
  return out;
}

interface Tile extends Rect {
  item: TreeItem;
  depth: number;
  /** A folder drawn with its contents inside, under a header with its name. */
  frame: boolean;
  head: number;
}

/** Below this, a folder is drawn as one tile instead of with its contents. */
const NEST_MIN_W = 64;
const NEST_MIN_H = 48;
const HEAD = 20;
const PAD = 3;

function layout(item: TreeItem, box: Rect, depth: number, out: Tile[]) {
  const kids = item.children?.filter((c) => c.size > 0) ?? [];
  const nest = kids.length > 0 && (depth === 0 || (box.w >= NEST_MIN_W && box.h >= NEST_MIN_H));
  if (!nest) {
    out.push({ ...box, item, depth, frame: false, head: 0 });
    return;
  }
  let inner = box;
  if (depth > 0) {
    const head = box.h >= 64 ? HEAD : 0;
    out.push({ ...box, item, depth, frame: true, head });
    inner = { x: box.x + PAD, y: box.y + (head || PAD), w: box.w - PAD * 2, h: box.h - (head || PAD) - PAD };
  }
  const rects = squarify(
    kids.map((k) => k.size),
    inner,
  );
  kids.forEach((k, i) => {
    const r = rects[i];
    // Slivers would be noise; their bytes still show as part of the folder around them.
    if (r.w >= 3 && r.h >= 3) layout(k, r, depth + 1, out);
  });
}

interface Props {
  root: TreeItem;
  /** Key ([`itemKey`]) of the item hovered in the list beside the map. */
  hovered: string | null;
  selected: string | null;
  /** A kind picked in the legend: tiles of other kinds fade. */
  kind: number | null;
  onHover: (item: TreeItem | null) => void;
  onOpen: (item: TreeItem) => void;
  onSelect: (item: TreeItem) => void;
  onMenu: (item: TreeItem, x: number, y: number) => void;
}

/**
 * The space map: the folder on screen as nested rectangles, each sized by its bytes. Folders big
 * enough show what is inside them (two or three levels down) under a header with their name;
 * smaller ones are one tile, colored by the kind of file that fills most of them, with a thin
 * strip showing the mix. Big files are tiles of their own; a folder's small files and folders are
 * one hatched tile. Click a folder to go into it; right-click anything for what can be done.
 */
export function SpaceMap({ root, hovered, selected, kind, onHover, onOpen, onSelect, onMenu }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const tipRef = useRef<HTMLDivElement>(null);
  const [box, setBox] = useState({ w: 0, h: 0 });
  const [tip, setTip] = useState<TreeItem | null>(null);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const ro = new ResizeObserver(([e]) => setBox({ w: Math.floor(e.contentRect.width), h: Math.floor(e.contentRect.height) }));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const tiles = useMemo(() => {
    const out: Tile[] = [];
    if (box.w > 0 && box.h > 0) layout(root, { x: 0, y: 0, w: box.w, h: box.h }, 0, out);
    return out;
  }, [root, box.w, box.h]);

  // The tooltip follows the pointer without re-rendering the map: only its position changes.
  const place = (e: React.MouseEvent) => {
    const el = tipRef.current;
    const map = ref.current;
    if (!el || !map) return;
    const r = map.getBoundingClientRect();
    const x = e.clientX - r.left;
    const y = e.clientY - r.top;
    const flipX = x + el.offsetWidth + 18 > r.width;
    const flipY = y + el.offsetHeight + 18 > r.height;
    el.style.transform = `translate(${flipX ? x - el.offsetWidth - 12 : x + 14}px, ${flipY ? y - el.offsetHeight - 12 : y + 14}px)`;
  };

  const enter = (item: TreeItem | null) => {
    setTip(item);
    onHover(item);
  };

  return (
    <div
      className="spacemap"
      ref={ref}
      onMouseMove={place}
      onMouseLeave={() => enter(null)}
      role="img"
      aria-label={`${root.name} 的空间地图`}
    >
      <Tiles tiles={tiles} hovered={hovered} selected={selected} kind={kind} onEnter={enter} onOpen={onOpen} onSelect={onSelect} onMenu={onMenu} />
      <div className={`map-tip ${tip ? "show" : ""}`} ref={tipRef} aria-hidden>
        {tip && <TipBody item={tip} total={root.size} />}
      </div>
    </div>
  );
}

interface TilesProps {
  tiles: Tile[];
  hovered: string | null;
  selected: string | null;
  kind: number | null;
  onEnter: (item: TreeItem | null) => void;
  onOpen: (item: TreeItem) => void;
  onSelect: (item: TreeItem) => void;
  onMenu: (item: TreeItem, x: number, y: number) => void;
}

const Tiles = memo(function Tiles({ tiles, hovered, selected, kind, onEnter, onOpen, onSelect, onMenu }: TilesProps) {
  return (
    <>
      {tiles.map((t) => {
        const { item } = t;
        const key = itemKey(item);
        const dom = dominantKind(item.types);
        const isDir = item.kind === "dir";
        const canOpen = isDir && item.hasChildren && !item.denied;
        const cls = [
          "mt",
          t.frame ? "mt-frame" : `mt-leaf mt-${item.kind}`,
          t.frame && t.depth % 2 === 0 ? "mt-even" : "",
          item.denied ? "mt-denied" : "",
          hovered === key ? "mt-hover" : "",
          selected === key ? "mt-selected" : "",
          !t.frame && kind !== null && dom !== kind ? "mt-faded" : "",
          canOpen ? "mt-open" : "",
        ].join(" ");
        const style: React.CSSProperties = {
          left: t.x + 1,
          top: t.y + 1,
          width: Math.max(0, t.w - 2),
          height: Math.max(0, t.h - 2),
        };
        if (!t.frame && !item.denied) {
          style.background = kindColor(dom);
          style.color = kindInk(dom);
        }
        const label = t.frame ? t.head > 0 && t.w > 56 : t.w > 54 && t.h > 20;
        const sizeToo = t.frame ? t.w > 150 : t.w > 70 && t.h > 38;
        return (
          <div
            key={key}
            className={cls}
            style={style}
            onMouseEnter={() => onEnter(item)}
            onClick={(e) => {
              e.stopPropagation();
              if (canOpen) onOpen(item);
              else onSelect(item);
            }}
            onContextMenu={(e) => {
              e.preventDefault();
              e.stopPropagation();
              onSelect(item);
              onMenu(item, e.clientX, e.clientY);
            }}
          >
            {label && (
              <div className={t.frame ? "mt-head" : "mt-label"} style={t.frame ? { height: t.head } : undefined}>
                <span className="mt-name">{item.denied ? `🔒 ${item.name}` : item.name}</span>
                {sizeToo && <span className="mt-size num">{item.denied ? "无权限" : formatBytes(item.size)}</span>}
              </div>
            )}
            {!t.frame && isDir && !item.denied && t.h > 30 && t.w > 30 && <Mix types={item.types} size={item.size} />}
          </div>
        );
      })}
    </>
  );
});

/** A thin strip along a folder tile's bottom: the kinds of file in it, by bytes. */
function Mix({ types, size }: { types: number[]; size: number }) {
  const parts = kindsOf(types).filter(([, v]) => v / (size || 1) > 0.04);
  if (parts.length < 2) return null;
  return (
    <div className="mt-mix">
      {parts.map(([k, v]) => (
        <span key={k} style={{ flexGrow: v, background: kindColor(k) }} />
      ))}
    </div>
  );
}

/** What the tooltip says about an item: where it is, how big, what is in it, when it changed. */
export function TipBody({ item, total }: { item: TreeItem; total: number }) {
  const parts = kindsOf(item.types).slice(0, 4);
  const kindName = item.kind === "dir" ? "文件夹" : item.kind === "file" ? KINDS[dominantKind(item.types)].label : "合在一起的小项";
  return (
    <>
      <div className="tip-name">{item.name}</div>
      <div className="tip-path">{item.path}</div>
      <div className="tip-line num">
        <b>{item.denied ? "无权限读取" : formatBytes(item.size)}</b>
        {!item.denied && <span className="faint"> · 占这里的 {percent(item.size, total)}</span>}
      </div>
      <div className="tip-line faint">
        {kindName}
        {item.kind !== "file" && item.files > 0 && ` · ${formatCount(item.files)} 个文件`}
        {item.modified > 0 && ` · ${item.kind === "file" ? "修改于" : "最近修改"} ${formatAgo(item.modified)}`}
      </div>
      {item.kind !== "file" && parts.length > 0 && (
        <div className="tip-kinds">
          {parts.map(([k, v]) => (
            <div key={k} className="tip-kind">
              <span className="swatch" style={{ background: kindColor(k) }} />
              <span>{KINDS[k].label}</span>
              <span className="num faint">{percent(v, item.size)}</span>
            </div>
          ))}
        </div>
      )}
    </>
  );
}
