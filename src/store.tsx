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

export type ScanState =
  | { status: "idle" }
  | { status: "scanning"; root: string; progress: ScanProgress | null; startedAt: number }
  | { status: "done"; summary: ScanSummary };

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
  root: string;
  setRoot: (root: string) => void;
  scan: ScanState;
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

export function StoreProvider({ children }: { children: ReactNode }) {
  const [drives, setDrives] = useState<DriveInfo[]>([]);
  const [root, setRoot] = useState("C:\\");
  const [scan, setScan] = useState<ScanState>({ status: "idle" });
  const [analysis, setAnalysis] = useState<AnalysisState>({ status: "idle" });
  const [toasts, setToasts] = useState<Toast[]>([]);
  const analysisStarted = useRef(false);
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
      setScan({ status: "scanning", root: r, progress: null, startedAt: Date.now() });
      api.startScan(r).catch((e) => {
        toast(`扫描失败：${e}`);
        setScan({ status: "idle" });
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
      api.onScanProgress((p) => setScan((s) => (s.status === "scanning" ? { ...s, progress: p } : s))),
      api.onScanDone((summary) => {
        setScan({ status: "done", summary });
        void refreshDrives();
        if (!analysisStarted.current) analyze();
      }),
      api.onAnalyzeProgress((p) => setAnalysis((a) => (a.status === "loading" ? { ...a, progress: p } : a))),
    ];
    return () => offs.forEach((p) => p.then((off) => off()));
  }, [analyze, refreshDrives]);

  // First launch: find the drives and start on the system one straight away.
  useEffect(() => {
    void refreshDrives().then((list) => {
      const sys = list.find((d) => d.isSystem) ?? list[0];
      if (sys) {
        setRoot(sys.root);
        startScan(sys.root);
      }
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const value = useMemo<Store>(
    () => ({ drives, root, setRoot, scan, startScan, cancelScan, analysis, analyze, cleanup, refreshDrives, toasts, toast }),
    [drives, root, scan, startScan, cancelScan, analysis, analyze, cleanup, refreshDrives, toasts, toast],
  );
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}
