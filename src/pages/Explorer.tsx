import { useEffect, useState } from "react";
import { ArrowLeft, File, Folder, FolderOpen, Lock, ScanSearch } from "lucide-react";
import { api, type NodeView } from "../api";
import { colorFor, Treemap } from "../components/Treemap";
import { formatBytes, formatCount, percent } from "../format";
import { useStore } from "../store";

function crumbsOf(root: string, path: string): { label: string; path: string }[] {
  const out = [{ label: root.replace(/\\$/, ""), path: root }];
  const rest = path.slice(root.length).split("\\").filter(Boolean);
  let acc = root.replace(/\\$/, "");
  for (const part of rest) {
    acc += "\\" + part;
    out.push({ label: part, path: acc });
  }
  return out;
}

export function Explorer() {
  const { scan, startScan } = useStore();
  const root = scan.status === "done" ? scan.summary.root : null;
  const [path, setPath] = useState<string | null>(null);
  const [nodes, setNodes] = useState<NodeView[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (root) setPath(root);
  }, [root]);

  useEffect(() => {
    if (!path) return;
    setNodes(null);
    setError(null);
    api
      .getChildren(path)
      .then(setNodes)
      .catch((e) => setError(String(e)));
  }, [path]);

  if (!root) {
    return (
      <div className="page">
        <div className="page-head">
          <div>
            <h1 className="page-title">空间分析</h1>
            <div className="page-sub">用色块大小直观地看每个文件夹占了多少</div>
          </div>
        </div>
        <div className="card empty">
          <div className="empty-icon">
            <ScanSearch size={30} />
          </div>
          {scan.status === "scanning" ? (
            <div>
              正在扫描，已经看过 {formatCount(scan.progress?.files ?? 0)} 个文件…
              <div className="faint" style={{ fontSize: 13, marginTop: 4 }}>扫描完成后这里会显示空间地图</div>
            </div>
          ) : (
            <>
              <div>需要先扫描一次磁盘</div>
              <button className="btn btn-primary" onClick={() => startScan()}>
                开始扫描
              </button>
            </>
          )}
        </div>
      </div>
    );
  }

  const total = nodes?.reduce((s, n) => s + n.size, 0) ?? 0;
  const crumbs = path ? crumbsOf(root, path) : [];
  const open = (n: NodeView) => n.hasChildren && !n.isFiles && !n.denied && setPath(n.path);

  return (
    <div className="page" style={{ display: "flex", flexDirection: "column" }}>
      <div className="page-head">
        <div>
          <h1 className="page-title">空间分析</h1>
          <div className="page-sub">点击色块进入文件夹，越大的块占的空间越多</div>
        </div>
        {path && (
          <button className="btn" onClick={() => void api.revealPath(path)}>
            <FolderOpen size={15} /> 在资源管理器中打开
          </button>
        )}
      </div>

      <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 14 }}>
        <button className="icon-btn" disabled={crumbs.length <= 1} title="上一级" onClick={() => crumbs.length > 1 && setPath(crumbs[crumbs.length - 2].path)}>
          <ArrowLeft size={18} />
        </button>
        <div className="crumbs">
          {crumbs.map((c, i) => (
            <span key={c.path} style={{ display: "flex", alignItems: "center" }}>
              {i > 0 && <span className="faint">›</span>}
              <button className={`crumb ${i === crumbs.length - 1 ? "last" : ""}`} onClick={() => setPath(c.path)}>
                {c.label}
              </button>
            </span>
          ))}
        </div>
        <div style={{ marginLeft: "auto" }} className="muted num">
          {formatBytes(total)}
        </div>
      </div>

      <div className="explorer" style={{ flex: 1 }}>
        <div className="card" style={{ padding: 8 }}>
          {nodes ? (
            nodes.length ? (
              <Treemap nodes={nodes} total={total} onOpen={open} />
            ) : (
              <div className="empty">这个文件夹是空的</div>
            )
          ) : error ? (
            <div className="empty">{error}</div>
          ) : (
            <div className="skeleton" style={{ height: "100%", minHeight: 520 }} />
          )}
        </div>
        <div className="card" style={{ padding: 10, overflowY: "auto", maxHeight: "calc(100vh - 230px)" }}>
          {nodes?.map((n, i) => (
            <div key={n.path + n.name} className="list-row" onClick={() => open(n)} style={{ cursor: n.hasChildren && !n.isFiles ? "pointer" : "default" }}>
              <div className="list-name">
                {n.denied ? <Lock size={15} color="var(--text-3)" /> : n.isFiles ? <File size={15} color="var(--text-3)" /> : <Folder size={15} color={colorFor(i, false)} fill={colorFor(i, false)} fillOpacity={0.25} />}
                <span title={n.name}>{n.name}</span>
              </div>
              <div className="num" style={{ fontWeight: 600, fontSize: 13 }}>
                {n.denied ? <span className="faint">无权限</span> : formatBytes(n.size)}
              </div>
              <div className="bar">
                <span style={{ width: percent(n.size, total), background: colorFor(i, n.isFiles) }} />
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
