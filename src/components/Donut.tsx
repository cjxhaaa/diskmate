import { useEffect, useState } from "react";

export interface Slice {
  key: string;
  value: number;
  color: string;
}

interface Props {
  slices: Slice[];
  size?: number;
  thickness?: number;
  active?: string | null;
  onHover?: (key: string | null) => void;
  center?: React.ReactNode;
}

/** A ring chart. Slices grow in on first paint; the hovered one lifts out a little. */
export function Donut({ slices, size = 220, thickness = 30, active, onHover, center }: Props) {
  const [grown, setGrown] = useState(false);
  useEffect(() => {
    const t = requestAnimationFrame(() => setGrown(true));
    return () => cancelAnimationFrame(t);
  }, []);

  const total = slices.reduce((s, x) => s + x.value, 0) || 1;
  const r = (size - thickness) / 2 - 6;
  const c = 2 * Math.PI * r;
  const gap = slices.length > 1 ? 2.5 : 0;
  let offset = 0;

  return (
    <div style={{ position: "relative", width: size, height: size }}>
      {/* Leaving clears the highlight only when the pointer leaves the whole ring: per-slice leave
          events fired across every gap between slices and blinked the highlight off and on. */}
      <svg
        width={size}
        height={size}
        viewBox={`0 0 ${size} ${size}`}
        style={{ transform: "rotate(-90deg)" }}
        onMouseLeave={() => onHover?.(null)}
      >
        <circle cx={size / 2} cy={size / 2} r={r} fill="none" stroke="var(--bg-soft)" strokeWidth={thickness} />
        {slices.map((s) => {
          const len = (s.value / total) * c;
          const dash = Math.max(len - gap, 0.5);
          // Highlighting never changes a slice's geometry. It used to widen the hovered slice, and
          // a slice that grows under the pointer moves the edge the pointer is on — part of what
          // made the chart shake.
          const el = (
            <circle
              key={s.key}
              cx={size / 2}
              cy={size / 2}
              r={r}
              fill="none"
              stroke={s.color}
              strokeWidth={thickness}
              strokeDasharray={`${grown ? dash : 0} ${c}`}
              strokeDashoffset={-offset}
              style={{
                transition: "stroke-dasharray 0.9s cubic-bezier(0.2,0.8,0.2,1), opacity 0.15s",
                opacity: active && active !== s.key ? 0.3 : 1,
                cursor: "pointer",
              }}
              onMouseEnter={() => onHover?.(s.key)}
            />
          );
          offset += len;
          return el;
        })}
      </svg>
      <div style={{ position: "absolute", inset: 0, display: "grid", placeItems: "center", pointerEvents: "none" }}>
        {center}
      </div>
    </div>
  );
}
