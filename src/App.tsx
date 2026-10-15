import { useEffect, useState } from "react";
import { Activity, AppWindow, Crosshair, FolderSearch, HardDrive, LayoutDashboard, LayoutGrid, Power, Sparkles, TrendingUp, Truck } from "lucide-react";
import { api } from "./api";
import { Dialog, Toasts, UsageBar } from "./components/ui";
import { formatBytes } from "./format";
import { Apps } from "./pages/Apps";
import { Cleanup } from "./pages/Cleanup";
import { Explorer } from "./pages/Explorer";
import { Finder } from "./pages/Finder";
import { Changes } from "./pages/Changes";
import { Files } from "./pages/Files";
import { Move } from "./pages/Move";
import { Overview } from "./pages/Overview";
import { Perf } from "./pages/Perf";
import { Startup } from "./pages/Startup";
import { StoreProvider, useStore } from "./store";

type Page = "overview" | "cleanup" | "apps" | "startup" | "files" | "move" | "explorer" | "changes" | "perf" | "finder";

const NAV: { id: Page; label: string; icon: typeof LayoutDashboard }[] = [
  { id: "overview", label: "概览", icon: LayoutDashboard },
  { id: "cleanup", label: "智能清理", icon: Sparkles },
  { id: "apps", label: "应用管理", icon: AppWindow },
  { id: "startup", label: "开机启动", icon: Power },
  { id: "files", label: "文件整理", icon: FolderSearch },
  { id: "move", label: "搬家与压缩", icon: Truck },
  { id: "explorer", label: "空间分析", icon: LayoutGrid },
  { id: "changes", label: "空间变化", icon: TrendingUp },
  { id: "perf", label: "性能", icon: Activity },
  { id: "finder", label: "窗口定位", icon: Crosshair },
];

const PAGES = new Set<string>(NAV.map((n) => n.id));

/** Asked the first time the window is closed: stay in the tray, or quit. Remembered. */
function CloseChoice({ onDone }: { onDone: () => void }) {
  const choose = async (tray: boolean) => {
    await api.setCloseToTray(tray);
    onDone();
    await api.closeMainWindow();
  };
  return (
    <Dialog onClose={onDone}>
      <div className="dialog-body">
        <h2 className="dialog-title">关闭窗口后，DiskMate 要…</h2>
        <div className="close-choices">
          <button className="close-choice" onClick={() => void choose(true)}>
            <b>保留在托盘</b>
            <span className="muted">窗口关掉、释放界面占用的内存，任务栏右下角留一个小图标，点它就能打开。正在进行的扫描和搬家会继续。</span>
          </button>
          <button className="close-choice" onClick={() => void choose(false)}>
            <b>直接退出</b>
            <span className="muted">完全退出，不在后台运行。</span>
          </button>
        </div>
        <div className="faint" style={{ fontSize: 12.5, marginTop: 10 }}>
          只问这一次。以后可以在托盘图标的右键菜单里改（「关闭窗口时保留在托盘」）。
        </div>
      </div>
      <div className="dialog-actions">
        <button className="btn" onClick={onDone}>
          取消
        </button>
      </div>
    </Dialog>
  );
}

function Shell() {
  const [page, setPage] = useState<Page>("overview");
  const [askClose, setAskClose] = useState(false);

  // The tray menu opens pages: through an event when the window is open, or as the page a
  // freshly built window should start on.
  useEffect(() => {
    const go = (p: string | null) => p && PAGES.has(p) && setPage(p as Page);
    void api.takePendingPage().then(go);
    const offs = [api.onNavigate(go), api.onCloseAsked(() => setAskClose(true))];
    return () => offs.forEach((p) => p.then((off) => off()));
  }, []);
  const { drives, root, analysis, scanning } = useStore();
  const drive = drives.find((d) => d.root === root);
  const safeBytes =
    analysis.status === "done" ? analysis.items.filter((i) => !i.guideOnly && i.risk === "safe").reduce((s, i) => s + i.size, 0) : 0;

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-logo">
            <HardDrive size={18} />
          </div>
          <div>
            <div className="brand-name">DiskMate</div>
            <div className="brand-sub">磁盘空间管家</div>
          </div>
        </div>
        {NAV.map(({ id, label, icon: Icon }) => (
          <button key={id} className={`nav-item ${page === id ? "active" : ""}`} onClick={() => setPage(id)}>
            <Icon size={18} />
            {label}
            {id === "cleanup" && safeBytes > 0 && <span className="nav-badge num">{formatBytes(safeBytes)}</span>}
            {id === "overview" && scanning && <span className="spinner" style={{ marginLeft: "auto", width: 14, height: 14, borderWidth: 2 }} />}
          </button>
        ))}
        {drive && (
          <div className="sidebar-foot">
            <div style={{ display: "flex", justifyContent: "space-between" }}>
              <span>
                {drive.root.replace("\\", "")} {drive.label}
              </span>
              <span className="num">{formatBytes(drive.free)} 可用</span>
            </div>
            <div className="mini-bar">
              <UsageBar used={drive.total - drive.free} total={drive.total} />
            </div>
            <div className="faint num">共 {formatBytes(drive.total)}</div>
          </div>
        )}
      </aside>
      <main className="main">
        {page === "overview" && <Overview onGoClean={() => setPage("cleanup")} />}
        {page === "cleanup" && <Cleanup />}
        {page === "apps" && <Apps />}
        {page === "explorer" && <Explorer />}
        {page === "files" && <Files />}
        {page === "move" && <Move />}
        {page === "changes" && <Changes />}
        {page === "startup" && <Startup />}
        {page === "perf" && <Perf />}
        {page === "finder" && <Finder />}
      </main>
      <Toasts />
      {askClose && <CloseChoice onDone={() => setAskClose(false)} />}
    </div>
  );
}

export default function App() {
  return (
    <StoreProvider>
      <Shell />
    </StoreProvider>
  );
}
