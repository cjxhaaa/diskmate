import { useState } from "react";
import { AppWindow, FolderSearch, HardDrive, LayoutDashboard, LayoutGrid, Sparkles, TrendingUp, Truck } from "lucide-react";
import { Toasts, UsageBar } from "./components/ui";
import { formatBytes } from "./format";
import { Apps } from "./pages/Apps";
import { Cleanup } from "./pages/Cleanup";
import { Explorer } from "./pages/Explorer";
import { Changes } from "./pages/Changes";
import { Files } from "./pages/Files";
import { Move } from "./pages/Move";
import { Overview } from "./pages/Overview";
import { StoreProvider, useStore } from "./store";

type Page = "overview" | "cleanup" | "apps" | "files" | "move" | "explorer" | "changes";

const NAV: { id: Page; label: string; icon: typeof LayoutDashboard }[] = [
  { id: "overview", label: "概览", icon: LayoutDashboard },
  { id: "cleanup", label: "智能清理", icon: Sparkles },
  { id: "apps", label: "应用管理", icon: AppWindow },
  { id: "files", label: "文件整理", icon: FolderSearch },
  { id: "move", label: "搬家与压缩", icon: Truck },
  { id: "explorer", label: "空间分析", icon: LayoutGrid },
  { id: "changes", label: "空间变化", icon: TrendingUp },
];

function Shell() {
  const [page, setPage] = useState<Page>("overview");
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
      </main>
      <Toasts />
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
