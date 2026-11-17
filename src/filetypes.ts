/**
 * The kinds of file the space map colors by, in the backend's order (src-tauri/src/filetype.rs).
 *
 * The eight hues are a validated categorical palette, in its fixed order (checked for color-blind
 * separation and in both themes); the order of the kinds below decides which kind gets which
 * slot, so the commonest kinds on a system drive get the most distinct colors. "System" and
 * "other" are neutral grays: they are the background against which the rest should stand out.
 * Tile labels pick dark or light ink per color, since three of the light hues are too pale for
 * white text.
 */
export interface Kind {
  label: string;
  /** Short, for tight spots. */
  short: string;
}

export const KINDS: Kind[] = [
  { label: "视频", short: "视频" },
  { label: "图片", short: "图片" },
  { label: "音乐", short: "音乐" },
  { label: "压缩包和安装包", short: "压缩包" },
  { label: "磁盘镜像和虚拟机", short: "磁盘镜像" },
  { label: "程序", short: "程序" },
  { label: "开发", short: "开发" },
  { label: "文档", short: "文档" },
  { label: "系统和日志", short: "系统" },
  { label: "其他", short: "其他" },
];

/** CSS variable holding kind `k`'s color (defined per theme in styles.css). */
export const kindColor = (k: number) => `var(--kind-${k})`;
/** Ink that reads on kind `k`'s color. */
export const kindInk = (k: number) => `var(--kind-ink-${k})`;

/** The kind with the most bytes; "other" for nothing at all. */
export function dominantKind(types: number[]): number {
  let best = KINDS.length - 1;
  let max = 0;
  types.forEach((v, i) => {
    if (v > max) {
      max = v;
      best = i;
    }
  });
  return best;
}

/** The kinds that make up `types`, biggest first, as [kind, bytes]. */
export function kindsOf(types: number[]): [number, number][] {
  return types
    .map((v, i) => [i, v] as [number, number])
    .filter(([, v]) => v > 0)
    .sort((a, b) => b[1] - a[1]);
}
