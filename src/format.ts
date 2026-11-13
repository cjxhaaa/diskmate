const UNITS = ["B", "KB", "MB", "GB", "TB"];

/** 1536 → "1.5 KB". Two significant decimals under 10, one under 100, none above. */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  let i = 0;
  let v = bytes;
  while (v >= 1024 && i < UNITS.length - 1) {
    v /= 1024;
    i++;
  }
  const digits = i === 0 ? 0 : v < 10 ? 2 : v < 100 ? 1 : 0;
  return `${v.toFixed(digits)} ${UNITS[i]}`;
}

/** Splits "12.3 GB" into its number and unit, for big headline figures. */
export function splitBytes(bytes: number): [string, string] {
  const [n, u] = formatBytes(bytes).split(" ");
  return [n, u];
}

export function formatCount(n: number): string {
  return n.toLocaleString("zh-CN");
}

export function percent(part: number, whole: number): string {
  if (whole <= 0) return "0%";
  const p = (part / whole) * 100;
  return `${p < 1 ? p.toFixed(1) : Math.round(p)}%`;
}

export function formatDate(unixSecs: number): string {
  if (!unixSecs) return "—";
  const d = new Date(unixSecs * 1000);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

/** 65_000 → "1 分 5 秒"; `rough` drops the seconds past a minute, for estimates. */
export function formatDuration(ms: number, rough = false): string {
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s} 秒`;
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (h > 0) return `${h} 小时 ${m} 分`;
  return rough ? `${m} 分钟` : `${m} 分 ${s % 60} 秒`;
}

/** "，2 项失败：<why>" after a delete or move that did not all go through; "" when all did. */
export function failNote(out: { skippedFiles: number; error: string | null }): string {
  if (!out.skippedFiles) return "";
  return `，${out.skippedFiles} 项失败${out.error ? `：${out.error}` : ""}`;
}
