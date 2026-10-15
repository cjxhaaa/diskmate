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
  recycleFiles: (paths: string[]) => call<Outcome>("recycle_files", { paths }),
  revealPath: (path: string) => call<void>("reveal_path", { path }),
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
