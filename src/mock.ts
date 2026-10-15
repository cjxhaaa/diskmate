// Stand-in for the Rust side when the UI runs in a plain browser (vite dev without Tauri). The
// numbers are shaped like a real developer's C: drive so layouts get exercised with real-ish data.

type Handler = (payload: unknown) => void;

const GB = 1024 ** 3;
const MB = 1024 ** 2;
const listeners = new Map<string, Set<Handler>>();

function emit(event: string, payload: unknown) {
  listeners.get(event)?.forEach((h) => h(payload));
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

const total = 477 * GB;
let free = 78 * GB;

const categories = [
  { id: "projects", bytes: 96 * GB },
  { id: "appdata", bytes: 71 * GB },
  { id: "dev", bytes: 48 * GB },
  { id: "vm", bytes: 43 * GB },
  { id: "system", bytes: 38 * GB },
  { id: "programs", bytes: 34 * GB },
  { id: "personal", bytes: 33 * GB },
  { id: "hidden", bytes: 21 * GB },
  { id: "temp", bytes: 3 * GB },
  { id: "other", bytes: 1.2 * GB },
  { id: "recycle", bytes: 0.4 * GB },
];

const tree: Record<string, [string, number, boolean?][]> = {
  "C:\\": [
    ["Users", 290 * GB],
    ["pagefile.sys + hiberfil.sys", 43 * GB, true],
    ["Windows", 33 * GB],
    ["Program Files", 21 * GB],
    ["Program Files (x86)", 9 * GB],
    ["ProgramData", 3.6 * GB],
    ["$Recycle.Bin", 0.4 * GB],
  ],
  "C:\\Users": [["40501", 288 * GB], ["Public", 1.2 * GB]],
  "C:\\Users\\40501": [
    ["AppData", 92 * GB],
    ["chorus", 61 * GB],
    [".android", 13 * GB],
    ["Documents", 12 * GB],
    ["slime", 6.1 * GB],
    [".gradle", 5.4 * GB],
    ["Mag", 4.8 * GB],
    ["xwechat_files", 3.1 * GB],
    ["Downloads", 2.6 * GB],
    [".cargo", 2.2 * GB],
    [".bun", 0.2 * GB],
    ["Desktop", 0.6 * GB],
  ],
  "C:\\Users\\40501\\AppData": [["Local", 61 * GB], ["Roaming", 30 * GB], ["LocalLow", 0.3 * GB]],
  "C:\\Users\\40501\\AppData\\Local": [
    ["Packages", 13 * GB],
    ["Google", 12 * GB],
    ["Android", 12 * GB],
    ["JetBrains", 6.7 * GB],
    ["Microsoft", 2.5 * GB],
    ["Temp", 2.4 * GB],
    ["npm-cache", 0.1 * GB],
  ],
};

function children(path: string) {
  const rows = tree[path] ?? [
    ["src", 1.2 * GB],
    ["build", 0.8 * GB],
    ["node_modules", 0.5 * GB],
    ["docs", 0.05 * GB],
  ];
  return rows.map(([name, size, isFiles]) => ({
    name,
    path: isFiles ? path : `${path.replace(/\\$/, "")}\\${name}`,
    size,
    files: Math.round(size / (3 * MB)),
    hasChildren: !isFiles,
    denied: false,
    isFiles: !!isFiles,
  }));
}

const largeFiles = [
  ["C:\\pagefile.sys", 30.2 * GB, 1791900000],
  ["C:\\hiberfil.sys", 12.7 * GB, 1791900000],
  ["C:\\Users\\40501\\.android\\avd\\pixel8_api36.avd\\userdata-qemu.img.qcow2", 3.6 * GB, 1791880000],
  ["C:\\Users\\40501\\AppData\\Roaming\\Claude\\vm_bundles\\claudevm.bundle\\rootfs.img", 8.1 * GB, 1791800000],
  ["C:\\Users\\40501\\Downloads\\android-studio-2025.3.3-windows.exe", 1.3 * GB, 1786000000],
  ["C:\\Users\\40501\\Videos\\录屏 2026-09-14.mp4", 612 * MB, 1789300000],
  ["C:\\Users\\40501\\vibe-ai-betaRelease-1.5.0-21-e108f854.aab", 152 * MB, 1782000000],
  ["C:\\Users\\40501\\gradle-8.11.1-bin.zip", 133 * MB, 1770000000],
].map(([path, size, modified]) => ({
  path: path as string,
  name: (path as string).split("\\").pop()!,
  size: size as number,
  modified: modified as number,
}));

const items = [
  { id: "user-temp", group: "system", title: "临时文件", desc: "程序运行时留下的临时文件。只清理 1 天前的，正在使用的会自动跳过。", risk: "safe", size: 1.1 * GB, files: 18342, selected: true, details: [["用户临时文件夹", "C:\\Users\\40501\\AppData\\Local\\Temp", 1.0 * GB], ["系统临时文件夹", "C:\\Windows\\Temp", 0.1 * GB]] },
  { id: "windows-update", group: "system", title: "Windows 更新缓存", desc: "更新安装完成后留下的下载包，Windows 不会再用到。", risk: "safe", size: 0.9 * GB, files: 210, selected: true, needsAdmin: true, details: [["SoftwareDistribution\\Download", "C:\\Windows\\SoftwareDistribution\\Download", 0.9 * GB]] },
  { id: "recycle-bin", group: "system", title: "回收站", desc: "已经删除、放在回收站里的文件。清空后无法再恢复。", risk: "caution", size: 0.4 * GB, files: 32, selected: false, details: [] },
  { id: "package-caches", group: "dev", title: "包管理器下载缓存", desc: "npm / bun / pip / yarn / Go 下载过的安装包。下次安装时按需重新下载。", risk: "safe", size: 2.3 * GB, files: 51000, selected: true, details: [["npm", "C:\\Users\\40501\\AppData\\Local\\npm-cache", 1.9 * GB], ["pip", "C:\\Users\\40501\\AppData\\Local\\pip\\cache", 0.12 * GB]] },
  { id: "gradle-caches", group: "dev", title: "Gradle 依赖缓存", desc: "所有 Android / Java 项目下载过的依赖。删除后每个项目第一次编译都要重新下载，比较慢。", risk: "caution", size: 5.3 * GB, files: 88000, selected: false, details: [] },
  { id: "android-images", group: "dev", title: "没被模拟器使用的 Android 系统镜像", desc: "SDK 里装着、但没有任何模拟器在用的系统镜像。需要时可以在 SDK Manager 里重新下载。", risk: "safe", size: 2.7 * GB, files: 12, selected: true, details: [["android-28 / google_apis / x86_64", "C:\\Users\\40501\\AppData\\Local\\Android\\Sdk\\system-images\\android-28\\google_apis\\x86_64", 2.7 * GB]] },
  { id: "avd-snapshots", group: "dev", title: "模拟器快速启动快照", desc: "Android 模拟器保存的开机快照。删除后模拟器下次会冷启动，慢几十秒，数据不受影响。", risk: "caution", size: 4.1 * GB, files: 6, selected: false, details: [] },
  { id: "ide-old-versions", group: "dev", title: "旧版本 IDE 缓存", desc: "已升级的 JetBrains IDE / Android Studio 旧版本留下的缓存和日志。设置保存在另一个目录，不受影响。", risk: "caution", size: 1.2 * GB, files: 4000, selected: false, details: [] },
  { id: "stale-build-output", group: "dev", title: "久未改动项目的构建产物", desc: "超过 30 天没改动的项目里的 node_modules、target、build 等目录。源代码不动；需要时重新 install / build 即可恢复。", risk: "caution", size: 9.6 * GB, files: 240000, selected: false, details: [["slime › node_modules", "C:\\Users\\40501\\slime\\node_modules", 0.4 * GB], ["slime › src-tauri\\target", "C:\\Users\\40501\\slime\\src-tauri\\target", 5.6 * GB], ["xiuxian › node_modules", "C:\\Users\\40501\\xiuxian\\node_modules", 0.6 * GB]] },
  { id: "chrome-cache", group: "app", title: "Chrome 缓存", desc: "网页图片、脚本等缓存。不会删除书签、密码、历史记录和登录状态，网页第一次打开会稍慢。", risk: "safe", size: 2.2 * GB, files: 30000, selected: true, note: "浏览器开着时部分缓存被占用会跳过，关闭浏览器后清理更彻底。", details: [] },
  { id: "git-worktrees", group: "review", title: "闲置的 Git worktree", desc: "没有未提交改动、3 天没动过的 worktree（比如 Claude Code 为每个会话建的）。用 git worktree remove 删除，分支和提交都保留，需要时可以重新检出。", risk: "review", size: 6.4 * GB, files: 120000, selected: false, details: [["highlight-upload-api-da1d0d · claude/highlight-upload-api-da1d0d", "C:\\Users\\40501\\chorus\\.claude\\worktrees\\highlight-upload-api-da1d0d", 0.7 * GB]] },
  { id: "hibernation", group: "review", title: "关闭休眠（删除 hiberfil.sys）", desc: "休眠文件大小和内存差不多。关闭后电脑不能「休眠」，开机的「快速启动」也会失效，睡眠不受影响。会弹出管理员授权。", risk: "review", size: 12.7 * GB, files: 1, selected: false, needsAdmin: true, details: [] },
  { id: "wechat", group: "guide", title: "微信", desc: "聊天里收发的图片、视频和文件。直接删文件夹会让聊天记录里的文件打不开，请在 微信 › 设置 › 存储空间 里按聊天清理。", risk: "review", size: 14.6 * GB, files: 0, selected: false, guideOnly: true, details: [["WeChat Files", "C:\\Users\\40501\\Documents\\WeChat Files", 11 * GB], ["微信数据", "C:\\Users\\40501\\AppData\\Roaming\\Tencent\\xwechat", 2.4 * GB]] },
  { id: "qqmusic", group: "guide", title: "QQ 音乐", desc: "下载的歌曲和播放缓存。请在 QQ 音乐 › 设置 › 下载与缓存 里清理缓存，或删除不听的下载歌曲。", risk: "review", size: 11 * GB, files: 0, selected: false, guideOnly: true, details: [["QQMusic", "C:\\Users\\40501\\AppData\\Roaming\\Tencent\\QQMusic", 11 * GB]] },
  { id: "pagefile", group: "guide", title: "虚拟内存 pagefile.sys", desc: "Windows 按内存大小自动分配。内存充足时可以在 系统 › 高级系统设置 › 性能 › 虚拟内存 里改成较小的固定大小；不建议关闭。", risk: "review", size: 30.2 * GB, files: 1, selected: false, guideOnly: true, details: [] },
].map((i) => ({
  guideOnly: false,
  needsAdmin: false,
  note: null,
  ...i,
  details: (i.details as [string, string, number][]).map(([label, path, size]) => ({ label, path, size })),
}));

const DAY = 86_400;
const now = () => Math.floor(Date.now() / 1000);
const apps = [
  ["微信", "Tencent", "4.1.2", [], 0.85, 29.3, 0, "2025-03-02", [["聊天文件", "C:\\Users\\40501\\xwechat_files", 15], ["聊天文件", "C:\\Users\\40501\\Documents\\WeChat Files", 10.8]]],
  ["Claude", "Anthropic", "1.2.0", ["store"], 0.68, 20.5, 0, null, [["应用数据", "C:\\Users\\40501\\AppData\\Local\\Packages\\Claude_pzs8sxrjxfjjc", 10.6]]],
  ["Android Studio", "Google LLC", "2025.3.3", ["dev"], 3.06, 17.9, 2, "2025-11-20", [["Android SDK", "C:\\Users\\40501\\AppData\\Local\\Android\\Sdk", 12]]],
  ["QQ音乐", "Tencent", "21.3", [], 0.2, 10.8, 41, "2024-06-11", [["下载与缓存", "C:\\Users\\40501\\AppData\\Roaming\\Tencent\\QQMusic", 10.8]]],
  ["CLion 2024.1.4", "JetBrains s.r.o.", "241.18034", ["dev"], 4.47, 4.65, 430, "2024-07-01", [["Local 数据", "C:\\Users\\40501\\AppData\\Local\\JetBrains\\CLion2024.1", 4.2]]],
  ["夸克", "Quark", "6.2", ["user"], 4.52, 0.08, 128, "2025-01-15", []],
  ["GoLand 2024.1.4", "JetBrains s.r.o.", "241.18034", ["dev"], 2.21, 1.97, 455, "2024-07-01", []],
  ["Microsoft Visual C++ 2015-2022 Redistributable (x64)", "Microsoft Corporation", "14.40", ["runtime"], 0.03, 0, null, "2024-05-20", []],
  ["NVIDIA 图形驱动程序 560.94", "NVIDIA Corporation", "560.94", ["driver"], 1.1, 0, null, "2025-02-10", []],
  ["斗鱼直播", "Douyu", "5.6", [], 0.37, 0.55, 410, "2024-03-02", []],
  ["Microsoft Teams", "Microsoft", "25.1", ["store"], 0.35, 0.93, 445, null, []],
  ["Visual Studio Code", "Microsoft Corporation", "1.105", ["dev", "user"], 4.25, 1.59, 0, "2024-04-02", []],
] as const;

const appList = apps.map(([name, publisher, version, tags, inst, data, ago, date, dirs], i) => ({
  id: `mock-${i}`,
  name,
  version,
  publisher,
  kind: (tags as readonly string[]).includes("store") ? "store" : "win32",
  installDate: date,
  installLocation: `C:\\Program Files\\${name}`,
  estimatedSize: inst * GB,
  installSize: null as number | null,
  dataSize: null as number | null,
  dataDirs: (dirs as readonly (readonly [string, string, number])[]).map(([label, path, size]) => ({ label, path, size: size * GB })),
  lastUsed: ago === null ? null : now() - (ago as number) * DAY,
  tags: [...tags],
  measured: false,
  _inst: inst * GB,
  _data: data * GB,
}));

let mockStatusChecks = 0;

const startupItems = [
  { id: "hkcu-run|Steam", name: "Steam", command: String.raw`"C:\Program Files (x86)\Steam\steam.exe" -silent`, path: String.raw`C:\Program Files (x86)\Steam\steam.exe`, publisher: "Valve Corporation", icon: null, source: "registry", allUsers: false, enabled: true, missing: false, advice: { kind: "disable", reason: "游戏平台，玩游戏时再打开就行" } },
  { id: "hkcu-run|MicrosoftEdgeAutoLaunch", name: "Microsoft Edge", command: String.raw`"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe" --no-startup-window`, path: String.raw`C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe`, publisher: "Microsoft Corporation", icon: null, source: "registry", allUsers: false, enabled: true, missing: false, advice: { kind: "disable", reason: "Edge 的「启动加速」，开机就在后台加载浏览器" } },
  { id: "hkcu-run|QuarkUpdater", name: "QuarkUpdater (x64)", command: String.raw`C:\Users\40501\AppData\Local\QuarkUpdater\updater.exe --wake`, path: String.raw`C:\Users\40501\AppData\Local\QuarkUpdater\updater.exe`, publisher: "Quark", icon: null, source: "registry", allUsers: false, enabled: true, missing: false, advice: { kind: "disable", reason: "自动更新程序，关掉后软件打开时仍会自己检查更新" } },
  { id: "hkcu-run|Slack", name: "Slack", command: String.raw`C:\Users\40501\AppData\Local\slack\slack.exe --process-start-args --startup`, path: String.raw`C:\Users\40501\AppData\Local\slack\slack.exe`, publisher: "Slack Technologies Inc.", icon: null, source: "registry", allUsers: false, enabled: true, missing: false, advice: null },
  { id: "hkcu-run|OneDrive", name: "Microsoft OneDrive", command: String.raw`"C:\Users\40501\AppData\Local\Microsoft\OneDrive\OneDrive.exe" /background`, path: String.raw`C:\Users\40501\AppData\Local\Microsoft\OneDrive\OneDrive.exe`, publisher: "Microsoft Corporation", icon: null, source: "registry", allUsers: false, enabled: true, missing: false, advice: null },
  { id: "store|Claude_pzs8sxrjxfjjc|ClaudeStartup", name: "Claude", command: "Claude_pzs8sxrjxfjjc", path: null, publisher: "Anthropic, PBC", icon: null, source: "store", allUsers: false, enabled: false, missing: false, advice: null },
  { id: "user-folder|OldTool.lnk", name: "OldTool", command: String.raw`C:\Users\40501\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup\OldTool.lnk`, path: String.raw`C:\Tools\OldTool\oldtool.exe`, publisher: null, icon: null, source: "folder", allUsers: false, enabled: true, missing: true, advice: null },
  { id: "hklm-run|RtkAudUService", name: "Realtek HD Audio Universal Service", command: String.raw`"C:\WINDOWS\System32\DriverStore\FileRepository\realtekservice.inf_amd64\RtkAudUService64.exe" -background`, path: String.raw`C:\WINDOWS\System32\DriverStore\FileRepository\realtekservice.inf_amd64\RtkAudUService64.exe`, publisher: "Realtek Semiconductor", icon: null, source: "registry", allUsers: true, enabled: true, missing: false, advice: { kind: "keep", reason: "硬件的配套程序，关掉后声音、触控板、显卡设置等可能不正常" } },
  { id: "hklm-run|SecurityHealth", name: "Windows Security notification icon", command: String.raw`%windir%\system32\SecurityHealthSystray.exe`, path: String.raw`C:\WINDOWS\system32\SecurityHealthSystray.exe`, publisher: "Microsoft Corporation", icon: null, source: "registry", allUsers: true, enabled: true, missing: false, advice: { kind: "keep", reason: "Windows 自带的组件" } },
  { id: "hklm-run|MuseHub", name: "MuseHub", command: String.raw`"C:\Program Files\MuseHub\current\MuseHub.exe" --autostart`, path: String.raw`C:\Program Files\MuseHub\current\MuseHub.exe`, publisher: "MuseHub", icon: null, source: "registry", allUsers: true, enabled: false, missing: false, advice: null },
];

/** A random walk per metric, so the mock graphs move like real ones. */
const walk: Record<string, number> = {};
const step = (key: string, lo: number, hi: number, jump: number) => {
  const v = walk[key] ?? (lo + hi) / 2;
  walk[key] = Math.min(hi, Math.max(lo, v + (Math.random() - 0.5) * jump));
  return walk[key];
};

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (cmd === "list_apps") {
    await sleep(500);
    void (async () => {
      for (const a of appList) {
        await sleep(120);
        emit("app-measured", { id: a.id, installSize: a._inst, dataSize: a._data, dataDirs: a.dataDirs, lastUsed: a.lastUsed });
      }
    })();
    return appList.map(({ _inst, _data, ...rest }) => rest) as T;
  }
  if (cmd === "uninstall_app") {
    const id = String(args?.id);
    void (async () => {
      await sleep(3000);
      emit("app-uninstall", {
        id,
        removed: true,
        error: null,
        leftovers: [
          { label: "Roaming 数据", path: "C:\\Users\\40501\\AppData\\Roaming\\Example", size: 0.6 * GB },
          { label: "安装目录", path: "C:\\Program Files\\Example", size: 0.05 * GB },
        ],
      });
    })();
    return undefined as T;
  }
  if (cmd === "stop_uninstall_wait") return undefined as T;
  if (cmd === "clean_app_leftovers") return { freed: 0.6 * GB, removedFiles: 120, skippedFiles: 0, error: null } as T;
  switch (cmd) {
    case "list_drives":
      return [
        { root: "C:\\", label: "系统", total, free, isSystem: true },
        { root: "D:\\", label: "数据", total: 931 * GB, free: 512 * GB, isSystem: false },
      ] as T;
    case "start_scan": {
      void (async () => {
        const steps = 30;
        for (let i = 1; i <= steps; i++) {
          await sleep(90);
          emit("scan-progress", {
            files: i * 41_000,
            dirs: i * 5_200,
            bytes: (i / steps) * (total - free - 21 * GB),
            current: `C:\\Users\\40501\\AppData\\Local\\Packages\\item-${i}\\cache`,
          });
        }
        emit("scan-done", {
          root: args?.root ?? "C:\\",
          total,
          free,
          scanned: total - free - 21 * GB,
          files: 1_230_000,
          dirs: 156_000,
          deniedDirs: 214,
          elapsedMs: 74_000,
          cancelled: false,
          scannedAt: Math.floor(Date.now() / 1000),
          categories,
        });
      })();
      return undefined as T;
    }
    case "cancel_scan":
      return undefined as T;
    case "scan_summaries":
      return [] as T;
    case "get_children":
      await sleep(60);
      return children(String(args?.path)) as T;
    case "large_files":
      return largeFiles as T;
    case "analyze_cleanup": {
      for (let i = 1; i <= items.length; i++) {
        await sleep(110);
        emit("analyze-progress", { done: i, total: items.length, current: items[i - 1].title });
      }
      return items as T;
    }
    case "run_cleanup": {
      const ids = (args?.ids as string[]) ?? [];
      const out = [];
      for (let i = 0; i < ids.length; i++) {
        emit("clean-progress", { id: ids[i], done: i, total: ids.length });
        await sleep(500);
        const item = items.find((x) => x.id === ids[i])!;
        const freed = Math.round(item.size * 0.96);
        free += freed;
        out.push({ id: item.id, title: item.title, outcome: { freed, removedFiles: item.files, skippedFiles: 12, error: null } });
      }
      return out as T;
    }
    case "delete_files":
    case "delete_duplicates":
      await sleep(400);
      return { freed: 1.3 * GB, removedFiles: 3, skippedFiles: 0, error: null } as T;
    case "scan_history": {
      const t = now();
      return [
        [14, 371], [10, 380], [7, 386], [4, 392], [2, 395], [0, 399],
      ].map(([d, usedGb]) => ({
        root: "C:\\", total, free: total - usedGb * GB, scanned: usedGb * GB, files: 1, dirs: 1, deniedDirs: 0,
        elapsedMs: 1, cancelled: false, scannedAt: t - d * DAY - 3600, categories,
      })) as T;
    }
    case "scan_diff":
      await sleep(150);
      return {
        baseAt: args?.baseAt, currentAt: args?.currentAt, usedDelta: 7.2 * GB,
        categories: [
          { id: "personal", before: 30 * GB, after: 33 * GB, delta: 3 * GB },
          { id: "dev", before: 46 * GB, after: 48 * GB, delta: 2.1 * GB },
          { id: "appdata", before: 69.8 * GB, after: 71 * GB, delta: 1.2 * GB },
          { id: "temp", before: 3.4 * GB, after: 3 * GB, delta: -0.4 * GB },
        ],
        grew: [
          { path: "C:\\Users\\40501\\xwechat_files\\cjxh123_6e82\\msg\\video", before: 4.1 * GB, after: 6.2 * GB, delta: 2.1 * GB },
          { path: "C:\\Users\\40501\\.gradle\\caches\\modules-2", before: 3.2 * GB, after: 4.4 * GB, delta: 1.2 * GB },
          { path: "C:\\Users\\40501\\Downloads", before: 35.1 * GB, after: 36.3 * GB, delta: 1.2 * GB },
          { path: "C:\\Users\\40501\\AppData\\Local\\Packages\\Claude_pzs8sxrjxfjjc", before: 9.9 * GB, after: 10.6 * GB, delta: 0.7 * GB },
          { path: "C:\\Users\\40501\\chorus\\apps\\android\\app\\build", before: null, after: 0.4 * GB, delta: 0.4 * GB },
        ],
        shrank: [{ path: "C:\\Users\\40501\\AppData\\Local\\Temp", before: 2.5 * GB, after: 2.1 * GB, delta: -0.4 * GB }],
      } as T;
    case "analyze_downloads": {
      await sleep(600);
      const d = (name: string, size: number, group: string, kind: string, reason: string | null, daysAgo: number, isDir = false) => ({
        path: `C:\\Users\\40501\\Downloads\\${name}`, name, size: size * GB, modified: now() - daysAgo * DAY, isDir, kind, group, reason,
      });
      return [
        d("Fooocus_win64_2-5-0", 12.2, "stale", "folder", "468 天没动过", 468, true),
        d("Win11_25H2_Chinese_Simplified_x64_v2.iso", 7.96, "stale", "image", "147 天没动过", 147),
        d("ComfyUI_windows_portable_nvidia", 6.18, "stale", "folder", "468 天没动过", 468, true),
        d("android-studio-panda3-patch1-windows.exe", 1.31, "installed", "installer", "已安装：Android Studio", 40),
        d("skywork-desktop_1.0.1_x64_en-US.msi", 1.18, "installed", "installer", "已安装：skywork-desktop", 210),
        d("Feishu-win32_x64-7.63.7-signed.exe", 0.34, "installed", "installer", "已安装：Feishu", 180),
        d("Godot_v4.7-stable_win64.exe.zip", 0.08, "extracted", "archive", "已解压到「Godot_v4.7-stable_win64.exe」", 20),
        d("PixelToPath-v1.0.0_Windows.zip", 0.04, "extracted", "archive", "已解压到「PixelToPath-v1.0.0_Windows」", 60),
        d("NVIDIA_Broadcast_v2.2.1.58338310.exe", 0.18, "other", "installer", null, 30),
        d("yt-dlp-gui.exe", 0.08, "other", "other", null, 12),
      ] as T;
    }
    case "find_duplicates":
      void (async () => {
        for (let i = 1; i <= 10; i++) {
          await sleep(120);
          emit("dupes-progress", { phase: i < 5 ? "walk" : "compare", files: i * 500, hashedBytes: (i - 4) * 0.2 * GB, toHashBytes: 1.7 * GB });
        }
        const g = (name: string, size: number, paths: [string, number][]) => ({
          id: paths[0][0], name, size: size * GB, wasted: size * GB * (paths.length - 1),
          files: paths.map(([path, d]) => ({ path, modified: now() - d * DAY })),
        });
        emit("dupes-done", [
          g("chrome.dll", 0.24, [["C:\\Users\\40501\\crawl_reddit\\chrome-win64\\chrome.dll", 420], ["C:\\Users\\40501\\Downloads\\chrome-win64\\chrome-win64\\chrome.dll", 421]]),
          g("GMT20250320-030326_Recording_640x360.mp4", 0.04, [
            ["C:\\Users\\40501\\Downloads\\GMT20250320-030326_Recording_640x360.mp4", 200],
            ["C:\\Users\\40501\\Downloads\\GMT20250320-030326_Recording_640x360 (1).mp4", 199],
            ["C:\\Users\\40501\\Downloads\\GMT20250320-030326_Recording_640x360 (2).mp4", 198],
          ]),
          g("e14a_f8df.wav", 0.02, [["C:\\Users\\40501\\Downloads\\e14a_f8df.wav", 240], ["C:\\Users\\40501\\xwechat_files\\cjxh123_6e82\\msg\\file\\2025-02\\e14a_f8df.wav", 241]]),
        ]);
      })();
      return undefined as T;
    case "cancel_duplicates":
      return undefined as T;
    case "migration_status":
    case "migration_candidates": {
      const full = cmd === "migration_candidates";
      if (!full) mockStatusChecks++;
      await sleep(full ? 2500 : 150);
      const c = (id: string, title: string, desc: string, path: string, size: number, method: string, group: string, blockers: string[] = [], movedTo: string | null = null) => ({
        id, title, desc, path, method, group, blockers, movedTo,
        size: full || movedTo ? size * GB : 0,
        measured: full || !!movedTo,
      });
      // WeChat counts as closed from the second status check on, so 重新检查 can be tried.
      const wechat = mockStatusChecks < 2 ? ["微信"] : [];
      const project = "用户目录下的文件夹，通常是代码项目。搬走后原路径照常可用；IDE、终端里打开着的话要先关掉。";
      return [
        c(String.raw`folder:c:\users\40501\slime`, "slime", project, String.raw`C:\Users\40501\slime`, 30.95, "junction", "project"),
        c(String.raw`folder:c:\users\40501\diskmate`, "diskmate", project, String.raw`C:\Users\40501\diskmate`, 6.2, "junction", "project"),
        c("wechat", "微信聊天文件", "微信 4.x 的聊天图片、视频和文件。搬走后微信照常读写，但搬之前必须退出微信。", String.raw`C:\Users\40501\xwechat_files`, 15.01, "junction", "app", wechat),
        c("qqmusic", "QQ 音乐", "下载的歌曲和缓存。搬之前要退出 QQ 音乐。", String.raw`C:\Users\40501\AppData\Roaming\Tencent\QQMusic`, 10.76, "junction", "app"),
        c("android-sdk", "Android SDK", "SDK、模拟器系统镜像和构建工具。项目里写的 SDK 路径不用改。", String.raw`C:\Users\40501\AppData\Local\Android\Sdk`, 8.64, "junction", "dev", ["adb"]),
        c("downloads", "下载", "Windows 会把「下载」文件夹指向新位置，浏览器下载的文件以后也直接存到那里。", String.raw`C:\Users\40501\Downloads`, 3.18, "knownFolder", "system"),
        c("documents", "文档", "「文档」文件夹整体搬走。微信默认把聊天文件存在「文档」里，会跟着一起搬。", String.raw`C:\Users\40501\Documents`, 1.4, "knownFolder", "system", wechat),
        c("gradle", "Gradle 缓存", "所有 Android / Java 项目下载过的依赖。", String.raw`C:\Users\40501\.gradle`, 5.46, "junction", "dev", [], String.raw`D:\Users\40501\.gradle`),
      ] as T;
    }
    case "migrate_folder": {
      const id = String(args?.id);
      void (async () => {
        for (const [phase, frac] of [["copy", 0.2], ["copy", 0.6], ["copy", 1], ["verify", 1], ["switch", 1], ["cleanup", 1]] as const) {
          await sleep(500);
          emit("migrate-progress", { id, phase, done: frac * 10 * GB, total: 10 * GB });
        }
        // QQ 音乐 fails, so the result view's failure row can be seen in the preview.
        const ok = id !== "qqmusic";
        emit("migrate-done", { id, ok, error: ok ? null : "有程序正在使用这个文件夹，请先关闭用到它的程序（编辑器、终端等）" });
      })();
      return undefined as T;
    }
    case "list_startup":
      await sleep(1200);
      return startupItems.map((i) => ({ ...i })) as T;
    case "set_startup_enabled": {
      await sleep(String(args?.id).startsWith("hklm") ? 1200 : 200);
      const item = startupItems.find((i) => i.id === args?.id);
      if (item) item.enabled = Boolean(args?.enabled);
      return undefined as T;
    }
    case "perf_sample": {
      const cores = Array.from({ length: 20 }, (_, i) => step(`core${i}`, 0, 100, 30));
      return {
        cpu: step("cpu", 3, 70, 14),
        cores,
        cpuMhz: step("mhz", 2400, 4600, 400),
        memTotal: 31.7 * GB,
        memUsed: step("mem", 18 * GB, 29 * GB, 0.4 * GB),
        memCached: 1.7 * GB,
        commitTotal: 52.9 * GB,
        commitLimit: 56.7 * GB,
        pagedPool: 1.04 * GB,
        nonpagedPool: 2.38 * GB,
        processes: 485,
        threads: 8180,
        handles: 234878,
        uptimeSecs: 46493 + Math.floor(performance.now() / 1000),
        disks: [
          { index: 0, letters: "C:", active: step("d0", 0, 60, 25), read: step("d0r", 0, 80e6, 30e6), write: step("d0w", 0, 40e6, 20e6), responseMs: step("d0ms", 0.1, 3, 1) },
          { index: 1, letters: "D:", active: step("d1", 0, 8, 4), read: step("d1r", 0, 2e6, 1e6), write: 0, responseMs: 0.2 },
        ],
        nets: [
          { key: "realtek pcie gbe family controller", name: "Realtek PCIe GbE Family Controller", recv: step("nr", 0, 4e6, 2e6), sent: step("ns", 0, 6e5, 3e5), bandwidth: 1e9 },
          { key: "hyper-v virtual ethernet adapter", name: "Hyper-V Virtual Ethernet Adapter", recv: 0, sent: 0, bandwidth: 1e10 },
        ],
        gpus: [
          { luid: "0x00000000_0x0000f7e2", util: step("gpu", 0, 60, 20), engines: [["3D", walk.gpu], ["VideoDecode", step("vd", 0, 20, 8)], ["Copy", 1]], dedicatedUsed: 0, sharedUsed: step("gm", 3.5 * GB, 4.5 * GB, 0.1 * GB) },
        ],
      } as T;
    }
    case "perf_processes": {
      const p = (name: string, cpu: number, memory: number, count = 1, path: string | null = null) => ({ name, cpu: step(`p-${name}`, 0, cpu, cpu / 2), memory: memory * 1048576, count, path });
      return [
        p("chrome", 18, 3420, 32, String.raw`C:\Program Files\Google\Chrome\Application\chrome.exe`),
        p("Code", 9, 1890, 14, String.raw`C:\Users\40501\AppData\Local\Programs\Microsoft VS Code\Code.exe`),
        p("vmmemWSL", 6, 2550),
        p("Weixin", 4, 820, 6, String.raw`C:\Program Files\Tencent\Weixin\Weixin.exe`),
        p("dwm", 3, 52),
        p("slack", 2, 640, 5, String.raw`C:\Users\40501\AppData\Local\slack\slack.exe`),
        p("System", 1.5, 0),
        p("explorer", 1, 210, 1, String.raw`C:\Windows\explorer.exe`),
        p("diskmate", 0.6, 48, 1),
        p("svchost", 0.8, 960, 88),
      ] as T;
    }
    case "perf_hardware":
      await sleep(1500);
      return {
        cpu: { name: "13th Gen Intel(R) Core(TM) i5-13500", baseMhz: 2500, cores: 14, logical: 20, sockets: 1, l2Kb: 11776, l3Kb: 24576, virtualization: false },
        mem: { speedMhz: 3200, slotsUsed: 2, slotsTotal: 2, form: "DIMM" },
        disks: [
          { index: 0, model: "UMIS RPEYJ512MKN1QWQ", media: "SSD", bus: "NVMe", size: 512 * GB },
          { index: 1, model: "UMIS RPEYJ512MKN1QWQ", media: "SSD", bus: "NVMe", size: 512 * GB },
        ],
        nets: [
          { key: "realtek pcie gbe family controller", description: "Realtek PCIe GbE Family Controller", alias: "以太网", ipv4: "192.168.50.176", wireless: false, physical: true },
          { key: "hyper-v virtual ethernet adapter", description: "Hyper-V Virtual Ethernet Adapter", alias: "vEthernet (WSL)", ipv4: "172.23.112.1", wireless: false, physical: false },
        ],
        gpus: [{ luid: "0x00000000_0x0000f7e2", name: "Intel(R) UHD Graphics 770", dedicatedTotal: 128 * 1048576, sharedTotal: 15.8 * GB }],
      } as T;
    case "perf_stop":
      return undefined as T;
    case "file_details":
      return {} as T;
    case "end_process":
      await sleep(300);
      if (args?.name === "System") throw "System 是 Windows 运行必需的进程，不能结束";
      return 3 as T;
    case "list_vdisks":
      return [
        { path: String.raw`D:\WSL\Ubuntu\ext4.vhdx`, size: 330 * GB, owner: "WSL · Ubuntu", kind: "wsl", inUse: true },
        { path: String.raw`C:\Users\40501\AppData\Roaming\Claude\vm_bundles\claudevm.bundle\rootfs.vhdx`, size: 7.82 * GB, owner: "Claude 桌面应用", kind: "claude", inUse: false },
      ] as T;
    case "shutdown_wsl":
      return undefined as T;
    case "compact_vdisk":
      await sleep(1500);
      return { before: 330 * GB, after: 214 * GB } as T;
    case "reveal_path":
      return undefined as T;
  }
  throw new Error(`mock: unknown command ${cmd}`);
}

function listen(event: string, handler: Handler): Promise<() => void> {
  if (!listeners.has(event)) listeners.set(event, new Set());
  listeners.get(event)!.add(handler);
  return Promise.resolve(() => listeners.get(event)?.delete(handler));
}

export const mock = { invoke, listen };
