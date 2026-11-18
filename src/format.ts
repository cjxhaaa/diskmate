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

/** When something happened, to the minute: "今天 13:13", "昨天 09:05", "11-17 13:13", "2025-11-17 13:13". */
export function formatWhen(unixSecs: number, now = new Date()): string {
  const d = new Date(unixSecs * 1000);
  const p = (n: number) => String(n).padStart(2, "0");
  const time = `${p(d.getHours())}:${p(d.getMinutes())}`;
  const day = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const days = Math.round((day(now) - day(d)) / 86400000);
  if (days === 0) return `今天 ${time}`;
  if (days === 1) return `昨天 ${time}`;
  const date = `${d.getMonth() + 1}-${p(d.getDate())}`;
  return d.getFullYear() === now.getFullYear() ? `${date} ${time}` : `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${time}`;
}

/** How long before `now` (both Unix seconds) `unixSecs` was: "3 天前", "5 个月前", "2 年前". */
export function formatAgo(unixSecs: number, now = Date.now() / 1000): string {
  if (!unixSecs) return "—";
  const days = Math.max(0, (now - unixSecs) / 86400);
  if (days < 1) return "今天";
  if (days < 30) return `${Math.floor(days)} 天前`;
  if (days < 365) return `${Math.floor(days / 30)} 个月前`;
  const years = days / 365;
  return years < 10 ? `${years.toFixed(1).replace(/\.0$/, "")} 年前` : `${Math.floor(years)} 年前`;
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
