import { useEffect, useRef, useState, type ReactNode } from "react";
import { CircleCheck } from "lucide-react";
import { useStore } from "../store";

/** Counts up to `value` instead of jumping, so a freed-space figure reads as something happening. */
export function AnimatedNumber({ value, format }: { value: number; format: (n: number) => string }) {
  const [shown, setShown] = useState(value);
  const from = useRef(value);
  useEffect(() => {
    const start = performance.now();
    const a = from.current;
    const dur = 700;
    let raf = 0;
    const tick = (t: number) => {
      const k = Math.min(1, (t - start) / dur);
      const eased = 1 - Math.pow(1 - k, 3);
      setShown(a + (value - a) * eased);
      if (k < 1) raf = requestAnimationFrame(tick);
      else from.current = value;
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [value]);
  return <>{format(shown)}</>;
}

export function Dialog({ children, onClose }: { children: ReactNode; onClose: () => void }) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  return (
    <div className="overlay" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="dialog">{children}</div>
    </div>
  );
}

export function Toasts() {
  const { toasts } = useStore();
  return (
    <div className="toasts">
      {toasts.map((t) => (
        <div key={t.id} className="toast">
          <CircleCheck size={18} />
          {t.text}
        </div>
      ))}
    </div>
  );
}

export function UsageBar({ used, total, thick }: { used: number; total: number; thick?: boolean }) {
  const pct = total > 0 ? (used / total) * 100 : 0;
  const color = pct > 90 ? "linear-gradient(90deg,#ff8a65,#e5484d)" : pct > 75 ? "linear-gradient(90deg,#fbbf24,#f59e0b)" : "var(--grad)";
  return (
    <div className={`bar ${thick ? "thick" : ""}`}>
      <span style={{ width: `${pct}%`, background: color }} />
    </div>
  );
}

function hue(s: string): number {
  let h = 0;
  for (const c of s) h = (h * 31 + c.charCodeAt(0)) % 360;
  return h;
}

/** A program's icon, or its initial on a colored tile when there is none. */
export function ProgramIcon({ src, name, size = 36 }: { src: string | null | undefined; name: string; size?: number }) {
  if (src) {
    return <img src={src} width={size} height={size} style={{ borderRadius: 8, objectFit: "contain", flex: "none" }} alt="" />;
  }
  const h = hue(name);
  return (
    <div
      style={{
        width: size,
        height: size,
        borderRadius: Math.round(size * 0.28),
        flex: "none",
        display: "grid",
        placeItems: "center",
        fontWeight: 700,
        fontSize: Math.round(size * 0.42),
        color: "white",
        background: `linear-gradient(135deg, hsl(${h} 70% 60%), hsl(${(h + 40) % 360} 70% 48%))`,
      }}
    >
      {[...name][0]?.toUpperCase()}
    </div>
  );
}

export function Switch({
  on,
  busy,
  disabled,
  label,
  onChange,
}: {
  on: boolean;
  busy?: boolean;
  disabled?: boolean;
  label: string;
  onChange: (on: boolean) => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      aria-label={label}
      title={label}
      className={`switch ${on ? "on" : ""} ${busy ? "busy" : ""}`}
      disabled={disabled || busy}
      onClick={(e) => {
        e.stopPropagation();
        onChange(!on);
      }}
    >
      <span className="switch-knob">{busy && <span className="spinner" />}</span>
    </button>
  );
}
