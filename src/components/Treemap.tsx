import { useLayoutEffect, useRef, useState } from "react";
import type { NodeView } from "../api";
import { formatBytes, percent } from "../format";

interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/**
 * Squarified treemap (Bruls, Huizing & van Wijk): lays items out in rows along the shorter side,
 * adding to a row while that keeps the worst aspect ratio from getting worse.
 */
function squarify(values: number[], box: Rect): Rect[] {
  const out: Rect[] = new Array(values.length);
  const total = values.reduce((a, b) => a + b, 0);
  if (total <= 0) return values.map(() => ({ x: box.x, y: box.y, w: 0, h: 0 }));
  const scale = (box.w * box.h) / total;
  const areas = values.map((v) => v * scale);
  let { x, y, w, h } = box;
  let i = 0;

  const worst = (row: number[], side: number) => {
    const s = row.reduce((a, b) => a + b, 0);
    const max = Math.max(...row);
    const min = Math.min(...row);
    return Math.max((side * side * max) / (s * s), (s * s) / (side * side * min));
  };

  while (i < areas.length) {
    const side = Math.min(w, h);
    const row = [areas[i]];
    let j = i + 1;
    while (j < areas.length && worst([...row, areas[j]], side) <= worst(row, side)) {
      row.push(areas[j]);
      j++;
    }
    const sum = row.reduce((a, b) => a + b, 0);
    if (w >= h) {
      const rowW = sum / h;
      let yy = y;
      row.forEach((a, k) => {
        const hh = a / rowW;
        out[i + k] = { x, y: yy, w: rowW, h: hh };
        yy += hh;
      });
      x += rowW;
      w -= rowW;
    } else {
      const rowH = sum / w;
      let xx = x;
      row.forEach((a, k) => {
        const ww = a / rowH;
        out[i + k] = { x: xx, y, w: ww, h: rowH };
        xx += ww;
      });
      y += rowH;
      h -= rowH;
    }
    i = j;
  }
  return out;
}

const PALETTE = ["#5B8CFF", "#7B4DFF", "#22B8CF", "#F59E0B", "#EC4899", "#10B981", "#6366F1", "#F97316", "#14B8A6", "#A855F7"];

export function colorFor(index: number, isFiles: boolean): string {
  if (isFiles) return "#94A3B8";
  return PALETTE[index % PALETTE.length];
}

interface Props {
  nodes: NodeView[];
  total: number;
  onOpen: (node: NodeView) => void;
}

export function Treemap({ nodes, total, onOpen }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const [box, setBox] = useState({ w: 0, h: 0 });

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const ro = new ResizeObserver(([e]) => setBox({ w: e.contentRect.width, h: e.contentRect.height }));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // Tiles too small to read are folded into one "其他" tile so the map stays legible.
  const minShare = 0.004;
  const shown = nodes.filter((n) => n.size / (total || 1) >= minShare).slice(0, 60);
  const rest = nodes.length - shown.length;
  const leftover = total - shown.reduce((s, n) => s + n.size, 0);
  const values = shown.map((n) => n.size).concat(rest > 0 && leftover > 0 ? [leftover] : []);
  const rects = squarify(values, { x: 0, y: 0, w: box.w, h: box.h });

  return (
    <div className="treemap" ref={ref}>
      {rects.map((r, i) => {
        const node = shown[i];
        const isRest = !node;
        const big = r.w > 90 && r.h > 46;
        const color = isRest ? "#64748B" : colorFor(i, node.isFiles);
        return (
          <div
            key={isRest ? "__rest" : node.path + node.name}
            className={`tile ${isRest || !node.hasChildren ? "leaf" : ""}`}
            style={{
              left: r.x,
              top: r.y,
              width: r.w,
              height: r.h,
              background: `linear-gradient(160deg, ${color}, color-mix(in srgb, ${color} 72%, black))`,
            }}
            title={isRest ? `其他 ${rest} 项 · ${formatBytes(leftover)}` : `${node.name}\n${formatBytes(node.size)} · ${percent(node.size, total)}`}
            onClick={() => node && node.hasChildren && !node.isFiles && onOpen(node)}
          >
            {big && (
              <>
                <div className="tile-name">{isRest ? `其他 ${rest} 项` : node.name}</div>
                <div className="tile-size num">
                  {formatBytes(isRest ? leftover : node.size)} · {percent(isRest ? leftover : node.size, total)}
                </div>
              </>
            )}
          </div>
        );
      })}
    </div>
  );
}
