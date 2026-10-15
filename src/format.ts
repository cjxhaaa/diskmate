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

export function formatDuration(ms: number): string {
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s} 秒`;
  return `${Math.floor(s / 60)} 分 ${s % 60} 秒`;
}
