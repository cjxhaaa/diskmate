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
