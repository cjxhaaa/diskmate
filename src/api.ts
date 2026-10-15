// Everything the UI asks of the Rust side. Outside Tauri (a plain browser preview) the calls are
// answered by the mock in ./mock.ts, so the UI can be worked on without a build.

import type { UnlistenFn } from "@tauri-apps/api/event";
import { mock } from "./mock";

export interface DriveInfo {
  root: string;
  label: string;
  total: number;
  free: number;
  isSystem: boolean;
}

export interface ScanProgress {
  files: number;
  dirs: number;
  bytes: number;
  current: string;
}

export interface CategoryStat {
  id: CategoryId;
  bytes: number;
}

export interface ScanSummary {
  root: string;
  total: number;
  free: number;
  scanned: number;
  files: number;
  dirs: number;
  deniedDirs: number;
  elapsedMs: number;
  cancelled: boolean;
  scannedAt: number;
  categories: CategoryStat[];
}

export interface NodeView {
  name: string;
  path: string;
  size: number;
  files: number;
  hasChildren: boolean;
  denied: boolean;
  isFiles: boolean;
}

export interface LargeFile {
  path: string;
  name: string;
  size: number;
  modified: number;
}

export type Risk = "safe" | "caution" | "review";
export type Group = "system" | "dev" | "app" | "review" | "guide";

export interface Detail {
  label: string;
  path: string;
  size: number;
}

export interface CleanItem {
  id: string;
  group: Group;
  title: string;
  desc: string;
  risk: Risk;
  size: number;
  files: number;
  selected: boolean;
  guideOnly: boolean;
  needsAdmin: boolean;
  note: string | null;
  details: Detail[];
}

export interface Outcome {
  freed: number;
  removedFiles: number;
  skippedFiles: number;
  error: string | null;
}

export interface CleanResult {
  id: string;
  title: string;
  outcome: Outcome;
}

export interface AnalyzeProgress {
  done: number;
  total: number;
  current: string;
}

export interface CleanProgress {
  id: string;
  done: number;
  total: number;
}

export type AppTag = "runtime" | "driver" | "dev" | "store" | "user";

export interface AppInfo {
  id: string;
  name: string;
  version: string;
  publisher: string;
  kind: "win32" | "store";
  installDate: string | null;
  installLocation: string | null;
  estimatedSize: number;
  installSize: number | null;
  dataSize: number | null;
  dataDirs: Detail[];
  lastUsed: number | null;
  tags: AppTag[];
  measured: boolean;
}

export interface AppMeasured {
  id: string;
  installSize: number | null;
  dataSize: number;
  dataDirs: Detail[];
  lastUsed: number | null;
}

export interface UninstallResult {
  id: string;
  removed: boolean;
  error: string | null;
  leftovers: Detail[];
}

export interface FolderChange {
  path: string;
  before: number | null;
  after: number | null;
  delta: number;
}

export interface ScanDiff {
  baseAt: number;
  currentAt: number;
  usedDelta: number;
  categories: { id: CategoryId; before: number; after: number; delta: number }[];
  grew: FolderChange[];
  shrank: FolderChange[];
}

export type DownloadKind = "installer" | "archive" | "image" | "video" | "document" | "folder" | "other";
export type DownloadGroup = "installed" | "extracted" | "stale" | "other";

export interface DownloadItem {
  path: string;
  name: string;
  size: number;
  modified: number;
  isDir: boolean;
  kind: DownloadKind;
  group: DownloadGroup;
  reason: string | null;
}

export interface DupeGroup {
  id: string;
  name: string;
  size: number;
  files: { path: string; modified: number }[];
  wasted: number;
}

export interface DupeProgress {
  phase: "walk" | "compare";
  files: number;
  hashedBytes: number;
  toHashBytes: number;
}

export interface MoveCandidate {
  id: string;
  title: string;
  desc: string;
  path: string;
  size: number;
  method: "junction" | "knownFolder";
  group: "app" | "dev" | "system" | "project";
  blockers: string[];
  movedTo: string | null;
  /** False when `size` is not known yet (the quick status check does not measure). */
  measured: boolean;
}

export interface MoveProgress {
  id: string;
  phase: "copy" | "verify" | "switch" | "cleanup";
  done: number;
  total: number;
}

export interface MoveDone {
  id: string;
  ok: boolean;
  error: string | null;
}

export interface StartupItem {
  id: string;
  name: string;
  command: string;
  path: string | null;
  publisher: string | null;
  icon: string | null;
  source: "registry" | "folder" | "store";
  /** Registered for every user: changing it needs administrator rights. */
  allUsers: boolean;
  enabled: boolean;
  /** The program file is gone, so the entry does nothing. */
  missing: boolean;
  advice: { kind: "disable" | "keep"; reason: string } | null;
}

export interface PerfSample {
  cpu: number;
  cores: number[];
  cpuMhz: number;
  memTotal: number;
  memUsed: number;
  memCached: number;
  commitTotal: number;
  commitLimit: number;
  pagedPool: number;
  nonpagedPool: number;
  processes: number;
  threads: number;
  handles: number;
  uptimeSecs: number;
  disks: { index: number; letters: string; active: number; read: number; write: number; responseMs: number }[];
  /** Bytes per second; bandwidth in bits per second. */
  nets: { key: string; name: string; recv: number; sent: number; bandwidth: number }[];
  gpus: { luid: string; util: number; engines: [string, number][]; dedicatedUsed: number; sharedUsed: number }[];
}

export interface HardwareInfo {
  cpu: { name: string; baseMhz: number; cores: number; logical: number; sockets: number; l2Kb: number; l3Kb: number; virtualization: boolean | null };
  mem: { speedMhz: number; slotsUsed: number; slotsTotal: number; form: string };
  disks: { index: number; model: string; media: string; bus: string; size: number }[];
  nets: { key: string; description: string; alias: string; ipv4: string; wireless: boolean; physical: boolean }[];
  gpus: { luid: string; name: string; dedicatedTotal: number; sharedTotal: number }[];
}

export interface ProcGroup {
  name: string;
  count: number;
  /** Percent of the whole machine. */
  cpu: number;
  memory: number;
  path: string | null;
}

export interface FileDetails {
  icon: string | null;
  description: string | null;
  company: string | null;
  target: string | null;
}

export interface VDisk {
  path: string;
  size: number;
  owner: string;
  kind: "wsl" | "docker" | "claude" | "other";
  inUse: boolean;
}

export type CategoryId =
  | "system"
  | "programs"
  | "vm"
  | "dev"
  | "appdata"
  | "temp"
  | "personal"
  | "projects"
  | "recycle"
  | "other"
  | "hidden";

const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!inTauri) return mock.invoke<T>(cmd, args);
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

async function on<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  if (!inTauri) return mock.listen(event, handler as (p: unknown) => void);
  const { listen } = await import("@tauri-apps/api/event");
  return listen<T>(event, (e) => handler(e.payload));
}

export const api = {
  listDrives: () => call<DriveInfo[]>("list_drives"),
  startScan: (root: string) => call<void>("start_scan", { root }),
  cancelScan: () => call<void>("cancel_scan"),
  getChildren: (path: string) => call<NodeView[]>("get_children", { path }),
  largeFiles: (root: string) => call<LargeFile[]>("large_files", { root }),
  scanSummaries: () => call<ScanSummary[]>("scan_summaries"),
  analyzeCleanup: () => call<CleanItem[]>("analyze_cleanup"),
  runCleanup: (ids: string[]) => call<CleanResult[]>("run_cleanup", { ids }),
  deleteFiles: (paths: string[], permanent: boolean) => call<Outcome>("delete_files", { paths, permanent }),
  scanHistory: (root: string) => call<ScanSummary[]>("scan_history", { root }),
  scanDiff: (root: string, baseAt: number, currentAt: number) => call<ScanDiff>("scan_diff", { root, baseAt, currentAt }),
  analyzeDownloads: () => call<DownloadItem[]>("analyze_downloads"),
  findDuplicates: (minSize: number) => call<void>("find_duplicates", { minSize }),
  cancelDuplicates: () => call<void>("cancel_duplicates"),
  deleteDuplicates: (paths: string[], permanent: boolean) => call<Outcome>("delete_duplicates", { paths, permanent }),
  migrationCandidates: () => call<MoveCandidate[]>("migration_candidates"),
  migrationStatus: () => call<MoveCandidate[]>("migration_status"),
  migrateFolder: (id: string, driveRoot: string | null, restore: boolean) => call<void>("migrate_folder", { id, driveRoot, restore }),
  onMigrateProgress: (h: (p: MoveProgress) => void) => on("migrate-progress", h),
  onMigrateDone: (h: (d: MoveDone) => void) => on("migrate-done", h),
  listVdisks: () => call<VDisk[]>("list_vdisks"),
  shutdownWsl: () => call<void>("shutdown_wsl"),
  compactVdisk: (path: string) => call<{ before: number; after: number }>("compact_vdisk", { path }),
  onDupesProgress: (h: (p: DupeProgress) => void) => on("dupes-progress", h),
  onDupesDone: (h: (g: DupeGroup[]) => void) => on("dupes-done", h),
  revealPath: (path: string) => call<void>("reveal_path", { path }),
  listStartup: () => call<StartupItem[]>("list_startup"),
  setStartupEnabled: (id: string, enabled: boolean) => call<void>("set_startup_enabled", { id, enabled }),
  perfSample: () => call<PerfSample>("perf_sample"),
  perfProcesses: () => call<ProcGroup[]>("perf_processes"),
  perfHardware: () => call<HardwareInfo>("perf_hardware"),
  fileDetails: (paths: string[]) => call<Record<string, FileDetails>>("file_details", { paths }),
  endProcess: (name: string) => call<number>("end_process", { name }),
  perfStop: () => call<void>("perf_stop"),
  takePendingPage: () => call<string | null>("take_pending_page"),
  setCloseToTray: (on: boolean) => call<void>("set_close_to_tray", { on }),
  closeMainWindow: () => call<void>("close_main_window"),
  onNavigate: (h: (page: string) => void) => on("navigate", h),
  onCloseAsked: (h: () => void) => on("close-asked", h),
  listApps: () => call<AppInfo[]>("list_apps"),
  uninstallApp: (id: string) => call<void>("uninstall_app", { id }),
  stopUninstallWait: (id: string) => call<void>("stop_uninstall_wait", { id }),
  cleanAppLeftovers: (id: string, paths: string[]) => call<Outcome>("clean_app_leftovers", { id, paths }),
  onAppMeasured: (h: (m: AppMeasured) => void) => on("app-measured", h),
  onAppIcons: (h: (icons: Record<string, string>) => void) => on("app-icons", h),
  onAppUninstall: (h: (r: UninstallResult) => void) => on("app-uninstall", h),
  onScanProgress: (h: (p: ScanProgress) => void) => on("scan-progress", h),
  onScanDone: (h: (s: ScanSummary) => void) => on("scan-done", h),
  onAnalyzeProgress: (h: (p: AnalyzeProgress) => void) => on("analyze-progress", h),
  onCleanProgress: (h: (p: CleanProgress) => void) => on("clean-progress", h),
};
