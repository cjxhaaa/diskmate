import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import {
  api,
  type AnalyzeProgress,
  type CleanItem,
  type CleanResult,
  type DriveInfo,
  type ScanProgress,
  type ScanSummary,
} from "./api";

/** The scan running right now, if any. Only one runs at a time: each one reads the whole disk. */
export interface Scanning {
  root: string;
  progress: ScanProgress | null;
  startedAt: number;
}

export type AnalysisState =
  | { status: "idle" }
  | { status: "loading"; progress: AnalyzeProgress | null }
  | { status: "done"; items: CleanItem[]; at: number }
  | { status: "error"; message: string };

interface Toast {
  id: number;
  text: string;
}

interface Store {
  drives: DriveInfo[];
  /** The drive on screen. Choosing one never starts a scan by itself. */
  root: string;
  setRoot: (root: string) => void;
  /** Finished scans, one per drive, keyed by uppercased root. */
  scans: Record<string, ScanSummary>;
  /** The finished scan of the drive on screen. */
  current: ScanSummary | null;
  scanning: Scanning | null;
  startScan: (root?: string) => void;
  cancelScan: () => void;
  analysis: AnalysisState;
  analyze: () => void;
  cleanup: (ids: string[]) => Promise<CleanResult[]>;
  refreshDrives: () => Promise<DriveInfo[]>;
  toasts: Toast[];
  toast: (text: string) => void;
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
  const [scans, setScans] = useState<Record<string, ScanSummary>>({});
  const [scanning, setScanning] = useState<Scanning | null>(null);
  const [analysis, setAnalysis] = useState<AnalysisState>({ status: "idle" });
  const [toasts, setToasts] = useState<Toast[]>([]);
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

  // Events, wired once.
  useEffect(() => {
    const offs = [
      api.onScanProgress((p) => setScanning((s) => (s ? { ...s, progress: p } : s))),
      api.onScanDone((summary) => {
        setScans((all) => ({ ...all, [key(summary.root)]: summary }));
        setScanning(null);
        void refreshDrives();
        if (!analysisStarted.current) analyze();
      }),
      api.onAnalyzeProgress((p) => setAnalysis((a) => (a.status === "loading" ? { ...a, progress: p } : a))),
    ];
    return () => offs.forEach((p) => p.then((off) => off()));
  }, [analyze, refreshDrives]);

  // First launch: scan the system drive once, unless this window already has its scan (a reload).
  // Every later scan is the user's call — a whole-disk read is not something to do behind their back.
  useEffect(() => {
    if (startedOnce.current) return;
    startedOnce.current = true;
    void Promise.all([refreshDrives(), api.scanSummaries()]).then(([list, done]) => {
      const sys = list.find((d) => d.isSystem) ?? list[0];
      if (done.length) setScans(Object.fromEntries(done.map((s) => [key(s.root), s])));
      if (!sys) return;
      setRoot(sys.root);
      if (!done.some((s) => key(s.root) === key(sys.root))) startScan(sys.root);
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
      analysis,
      analyze,
      cleanup,
      refreshDrives,
      toasts,
      toast,
    }),
    [drives, root, scans, current, scanning, startScan, cancelScan, analysis, analyze, cleanup, refreshDrives, toasts, toast],
  );
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}
