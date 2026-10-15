import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import {
  api,
  type AppInfo,
  type AnalyzeProgress,
  type CleanItem,
  type CleanResult,
  type DownloadItem,
  type DriveInfo,
  type DupeGroup,
  type DupeProgress,
  type ScanProgress,
  type ScanSummary,
} from "./api";

/** The scan running right now, if any. Only one runs at a time: each one reads the whole disk. */
export interface Scanning {
  root: string;
  progress: ScanProgress | null;
  startedAt: number;
}

/**
 * A scan result. `fromHistory` ones were read back from a previous session: their totals are real,
 * but the folder tree behind them was not kept, so the space map and large files need a new scan.
 */
export type ShownSummary = ScanSummary & { fromHistory?: boolean };

export type AnalysisState =
  | { status: "idle" }
  | { status: "loading"; progress: AnalyzeProgress | null }
  | { status: "done"; items: CleanItem[]; at: number }
  | { status: "error"; message: string };

export type DupeState =
  | { status: "idle" }
  | { status: "running"; progress: DupeProgress | null; minSize: number }
  | { status: "done"; groups: DupeGroup[]; minSize: number; at: number };

interface Toast {
  id: number;
  text: string;
}

/** A scan this recent is shown at launch instead of scanning again. */
const REUSE_SCAN_SECS = 24 * 3600;

interface Store {
  drives: DriveInfo[];
  /** The drive on screen. Choosing one never starts a scan by itself. */
  root: string;
  setRoot: (root: string) => void;
  /** Finished scans, one per drive, keyed by uppercased root. */
  scans: Record<string, ShownSummary>;
  /** The finished scan of the drive on screen. */
  current: ShownSummary | null;
  scanning: Scanning | null;
  startScan: (root?: string) => void;
  cancelScan: () => void;
  /** Bumped whenever a scan of any drive finishes, so the history views know to re-read. */
  scanCount: number;
  analysis: AnalysisState;
  analyze: () => void;
  cleanup: (ids: string[]) => Promise<CleanResult[]>;
  refreshDrives: () => Promise<DriveInfo[]>;
  toasts: Toast[];
  toast: (text: string) => void;
  /** Installed apps; null until first loaded. Sizes fill in after the list arrives. */
  apps: AppInfo[] | null;
  appsLoading: boolean;
  appIcons: Record<string, string>;
  loadApps: () => void;
  dropApp: (id: string) => void;
  downloads: DownloadItem[] | null;
  downloadsLoading: boolean;
  loadDownloads: () => void;
  dropDownloads: (paths: string[]) => void;
  dupes: DupeState;
  findDupes: (minSize: number) => void;
  cancelDupes: () => void;
  dropDupeFiles: (paths: string[]) => void;
}

const Ctx = createContext<Store | null>(null);

export function useStore(): Store {
  const s = useContext(Ctx);
  if (!s) throw new Error("useStore outside StoreProvider");
  return s;
}

const key = (root: string) => root.toUpperCase();

export function StoreProvider({ children }: { children: ReactNode }) {
  const [drives, setDrives] = useState<DriveInfo[]>([]);
  const [root, setRoot] = useState("C:\\");
  const [scans, setScans] = useState<Record<string, ShownSummary>>({});
  const [scanning, setScanning] = useState<Scanning | null>(null);
  const [scanCount, setScanCount] = useState(0);
  const [analysis, setAnalysis] = useState<AnalysisState>({ status: "idle" });
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [apps, setApps] = useState<AppInfo[] | null>(null);
  const [appsLoading, setAppsLoading] = useState(false);
  const [appIcons, setAppIcons] = useState<Record<string, string>>({});
  const [downloads, setDownloads] = useState<DownloadItem[] | null>(null);
  const [downloadsLoading, setDownloadsLoading] = useState(false);
  const [dupes, setDupes] = useState<DupeState>({ status: "idle" });
  const analysisStarted = useRef(false);
  const startedOnce = useRef(false);
  const toastId = useRef(0);

  const toast = useCallback((text: string) => {
    const id = ++toastId.current;
    setToasts((t) => [...t, { id, text }]);
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), 3800);
  }, []);

  const refreshDrives = useCallback(async () => {
    const list = await api.listDrives();
    setDrives(list);
    return list;
  }, []);

  const startScan = useCallback(
    (target?: string) => {
      const r = target ?? root;
      setScanning({ root: r, progress: null, startedAt: Date.now() });
      api.startScan(r).catch((e) => {
        toast(`扫描失败：${e}`);
        setScanning(null);
      });
    },
    [root, toast],
  );

  const cancelScan = useCallback(() => {
    void api.cancelScan();
  }, []);

  const analyze = useCallback(() => {
    analysisStarted.current = true;
    setAnalysis({ status: "loading", progress: null });
    api
      .analyzeCleanup()
      .then((items) => setAnalysis({ status: "done", items, at: Date.now() }))
      .catch((e) => setAnalysis({ status: "error", message: String(e) }));
  }, []);

  const cleanup = useCallback(
    async (ids: string[]) => {
      const results = await api.runCleanup(ids);
      await refreshDrives();
      return results;
    },
    [refreshDrives],
  );

  const loadApps = useCallback(() => {
    setAppsLoading(true);
    api
      .listApps()
      .then(setApps)
      .catch((e) => toast(`读取应用列表失败：${e}`))
      .finally(() => setAppsLoading(false));
  }, [toast]);

  const dropApp = useCallback((id: string) => setApps((list) => list?.filter((a) => a.id !== id) ?? null), []);

  const loadDownloads = useCallback(() => {
    setDownloadsLoading(true);
    api
      .analyzeDownloads()
      .then(setDownloads)
      .catch((e) => toast(`读取下载文件夹失败：${e}`))
      .finally(() => setDownloadsLoading(false));
  }, [toast]);

  const dropDownloads = useCallback((paths: string[]) => {
    const gone = new Set(paths);
    setDownloads((list) => list?.filter((d) => !gone.has(d.path)) ?? null);
  }, []);

  const findDupes = useCallback((minSize: number) => {
    setDupes({ status: "running", progress: null, minSize });
    void api.findDuplicates(minSize);
  }, []);

  const cancelDupes = useCallback(() => {
    void api.cancelDuplicates();
  }, []);

  const dropDupeFiles = useCallback((paths: string[]) => {
    const gone = new Set(paths);
    setDupes((d) =>
      d.status !== "done"
        ? d
        : {
            ...d,
            groups: d.groups
              .map((g) => {
                const files = g.files.filter((f) => !gone.has(f.path));
                return { ...g, files, wasted: g.size * Math.max(files.length - 1, 0) };
              })
              .filter((g) => g.files.length > 1),
          },
    );
  }, []);

  // Events, wired once.
  useEffect(() => {
    const offs = [
      api.onScanProgress((p) => setScanning((s) => (s ? { ...s, progress: p } : s))),
      api.onScanDone((summary) => {
        setScans((all) => ({ ...all, [key(summary.root)]: summary }));
        setScanning(null);
        setScanCount((n) => n + 1);
        void refreshDrives();
        if (!analysisStarted.current) analyze();
      }),
      api.onAnalyzeProgress((p) => setAnalysis((a) => (a.status === "loading" ? { ...a, progress: p } : a))),
      api.onAppMeasured((m) =>
        setApps((list) => list?.map((a) => (a.id === m.id ? { ...a, ...m, measured: true } : a)) ?? null),
      ),
      api.onAppIcons((icons) => setAppIcons((old) => ({ ...old, ...icons }))),
      // The Store list was re-read in the background and changed; sizes already shown are kept.
      api.onAppsUpdated((fresh) =>
        setApps((list) => {
          const old = new Map((list ?? []).map((a) => [a.id, a]));
          return fresh.map((a) => {
            const o = old.get(a.id);
            return !a.measured && o?.measured ? { ...a, installSize: o.installSize, dataSize: o.dataSize, dataDirs: o.dataDirs, lastUsed: o.lastUsed, measured: true } : a;
          });
        }),
      ),
      api.onDupesProgress((p) => setDupes((d) => (d.status === "running" ? { ...d, progress: p } : d))),
      api.onDupesDone((groups) =>
        setDupes((d) => ({ status: "done", groups, minSize: d.status === "idle" ? 1024 * 1024 : d.minSize, at: Date.now() })),
      ),
    ];
    return () => offs.forEach((p) => p.then((off) => off()));
  }, [analyze, refreshDrives]);

  // First launch: show the system drive's last scan if it is recent, otherwise scan it once.
  // Every later scan is the user's call — a whole-disk read is not something to do behind their back.
  useEffect(() => {
    if (startedOnce.current) return;
    startedOnce.current = true;
    void Promise.all([refreshDrives(), api.scanSummaries()]).then(async ([list, done]) => {
      const sys = list.find((d) => d.isSystem) ?? list[0];
      if (done.length) setScans(Object.fromEntries(done.map((s) => [key(s.root), s])));
      if (!sys) return;
      setRoot(sys.root);
      if (done.some((s) => key(s.root) === key(sys.root))) return;
      const past = await api.scanHistory(sys.root).catch(() => [] as ScanSummary[]);
      const last = past[past.length - 1];
      if (last && Date.now() / 1000 - last.scannedAt < REUSE_SCAN_SECS) {
        setScans((all) => ({ ...all, [key(last.root)]: { ...last, fromHistory: true } }));
        analyze();
      } else {
        startScan(sys.root);
      }
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const current = scans[key(root)] ?? null;

  const value = useMemo<Store>(
    () => ({
      drives,
      root,
      setRoot,
      scans,
      current,
      scanning,
      startScan,
      cancelScan,
      scanCount,
      analysis,
      analyze,
      cleanup,
      refreshDrives,
      toasts,
      toast,
      apps,
      appsLoading,
      appIcons,
      loadApps,
      dropApp,
      downloads,
      downloadsLoading,
      loadDownloads,
      dropDownloads,
      dupes,
      findDupes,
      cancelDupes,
      dropDupeFiles,
    }),
    [
      drives, root, scans, current, scanning, startScan, cancelScan, scanCount, analysis, analyze, cleanup, refreshDrives,
      toasts, toast, apps, appsLoading, appIcons, loadApps, dropApp, downloads, downloadsLoading, loadDownloads,
      dropDownloads, dupes, findDupes, cancelDupes, dropDupeFiles,
    ],
  );
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}
