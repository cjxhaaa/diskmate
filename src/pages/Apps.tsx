import { useEffect, useMemo, useState } from "react";
import {
  ArrowUpDown,
  ChevronDown,
  ChevronRight,
  CircleCheckBig,
  Clock,
  FolderOpen,
  Hourglass,
  Lightbulb,
  PackageX,
  RefreshCw,
  Search,
  ShieldAlert,
  TriangleAlert,
} from "lucide-react";
import { api, type AppInfo, type Detail, type UninstallResult } from "../api";
import { Dialog } from "../components/ui";
import { formatBytes, formatCount } from "../format";
import { useStore } from "../store";

const DAY = 86_400;
const STALE_DAYS = 90;
/** Apps smaller than this are not worth suggesting, however long unused. */
const SUGGEST_MIN_BYTES = 300 * 1024 * 1024;

type Filter = "all" | "suggest" | "stale" | "data" | "store" | "dev" | "system";
type Sort = "size" | "lastUsed" | "installed" | "name";

const FILTERS: { id: Filter; label: string }[] = [
  { id: "all", label: "全部" },
  { id: "suggest", label: "建议卸载" },
  { id: "stale", label: "3 个月没用" },
  { id: "data", label: "数据超过 1 GB" },
  { id: "dev", label: "开发工具" },
  { id: "store", label: "商店应用" },
  { id: "system", label: "运行库与驱动" },
];

const SORTS: { id: Sort; label: string }[] = [
  { id: "size", label: "占用空间" },
  { id: "lastUsed", label: "最久没用" },
  { id: "installed", label: "安装时间" },
  { id: "name", label: "名称" },
];

const TAG_LABEL: Record<string, { label: string; tone: string }> = {
  runtime: { label: "运行库", tone: "caution" },
  driver: { label: "驱动", tone: "caution" },
  dev: { label: "开发工具", tone: "neutral" },
  store: { label: "商店应用", tone: "neutral" },
};

const total = (a: AppInfo) => (a.installSize ?? (a.measured ? 0 : a.estimatedSize)) + (a.dataSize ?? 0);
const isProtected = (a: AppInfo) => a.tags.includes("runtime") || a.tags.includes("driver");

function daysSince(a: AppInfo): number | null {
  if (a.lastUsed) return (Date.now() / 1000 - a.lastUsed) / DAY;
  return null;
}

function isStale(a: AppInfo): boolean {
  const d = daysSince(a);
  if (d !== null) return d > STALE_DAYS;
  // Never seen in use: fall back to how long it has been installed.
  return !!a.installDate && (Date.now() - Date.parse(a.installDate)) / 1000 / DAY > STALE_DAYS;
}

function isSuggested(a: AppInfo): boolean {
  return a.measured && !isProtected(a) && isStale(a) && total(a) >= SUGGEST_MIN_BYTES;
}

function lastUsedText(a: AppInfo): string {
  const d = daysSince(a);
  if (d === null) return "没有记录";
  if (d < 1) return "今天";
  if (d < 2) return "昨天";
  if (d < 30) return `${Math.floor(d)} 天前`;
  if (d < 365) return `${Math.floor(d / 30)} 个月前`;
  return `${(d / 365).toFixed(1).replace(/\.0$/, "")} 年前`;
}

function hue(s: string): number {
  let h = 0;
  for (const c of s) h = (h * 31 + c.charCodeAt(0)) % 360;
  return h;
}

function AppIcon({ app, size = 36 }: { app: AppInfo; size?: number }) {
  const { appIcons } = useStore();
  const src = appIcons[app.id];
  if (src) {
    return <img src={src} width={size} height={size} style={{ borderRadius: 8, objectFit: "contain", flex: "none" }} alt="" />;
  }
  const h = hue(app.name);
  return (
    <div
      style={{
        width: size,
        height: size,
        borderRadius: 10,
        flex: "none",
        display: "grid",
        placeItems: "center",
        fontWeight: 700,
        color: "white",
        background: `linear-gradient(135deg, hsl(${h} 70% 60%), hsl(${(h + 40) % 360} 70% 48%))`,
      }}
    >
      {[...app.name][0]?.toUpperCase()}
    </div>
  );
}

function SizeCell({ app, max }: { app: AppInfo; max: number }) {
  if (!app.measured) return <div className="skeleton" style={{ height: 18, width: 90, marginLeft: "auto" }} />;
  const inst = app.installSize ?? 0;
  const data = app.dataSize ?? 0;
  const t = inst + data;
  return (
    <div style={{ textAlign: "right" }}>
      <div className="num" style={{ fontWeight: 650 }}>
        {formatBytes(t)}
      </div>
      <div className="bar" style={{ height: 5, width: 110, marginLeft: "auto", marginTop: 4 }} title={`程序 ${formatBytes(inst)} · 数据 ${formatBytes(data)}`}>
        <span style={{ width: `${(inst / (max || 1)) * 100}%`, background: "var(--accent)" }} />
        <span style={{ width: `${(data / (max || 1)) * 100}%`, background: "#EC4899" }} />
      </div>
    </div>
  );
}

function AppRow({ app, max, onUninstall }: { app: AppInfo; max: number; onUninstall: () => void }) {
  const [open, setOpen] = useState(false);
  const suggested = isSuggested(app);
  return (
    <div className="item" style={{ marginBottom: 6 }}>
      <div className="app-row" onClick={() => setOpen((o) => !o)}>
        <AppIcon app={app} />
        <div style={{ minWidth: 0 }}>
          <div className="item-title" style={{ flexWrap: "nowrap" }}>
            <span className="ellipsis" title={app.name}>
              {app.name}
            </span>
            {suggested && (
              <span className="badge review">
                <Lightbulb size={11} /> 建议卸载
              </span>
            )}
            {app.tags
              .filter((t) => TAG_LABEL[t])
              .map((t) => (
                <span key={t} className={`badge ${TAG_LABEL[t].tone}`}>
                  {TAG_LABEL[t].label}
                </span>
              ))}
          </div>
          <div className="faint ellipsis" style={{ fontSize: 12.5 }}>
            {[app.publisher, app.version && `v${app.version}`].filter(Boolean).join(" · ") || "—"}
          </div>
        </div>
        <SizeCell app={app} max={max} />
        <div className="muted" style={{ fontSize: 13, textAlign: "right" }} title="根据启动记录和数据文件夹的修改时间估算">
          {app.measured || app.lastUsed ? lastUsedText(app) : <div className="skeleton" style={{ height: 16, width: 60, marginLeft: "auto" }} />}
        </div>
        <div style={{ display: "flex", gap: 4, justifyContent: "flex-end" }}>
          <button
            className="btn"
            style={{ height: 32, padding: "0 12px" }}
            onClick={(e) => {
              e.stopPropagation();
              onUninstall();
            }}
          >
            卸载
          </button>
          <button className="icon-btn" onClick={(e) => (e.stopPropagation(), setOpen((o) => !o))}>
            {open ? <ChevronDown size={18} /> : <ChevronRight size={18} />}
          </button>
        </div>
      </div>
      {open && (
        <div className="item-details" style={{ paddingLeft: 66 }}>
          <div className="app-facts">
            <span>程序 {app.installSize !== null ? formatBytes(app.installSize) : app.estimatedSize ? `约 ${formatBytes(app.estimatedSize)}` : "未知"}</span>
            <span>数据 {formatBytes(app.dataSize ?? 0)}</span>
            <span>安装于 {app.installDate ?? "未知"}</span>
            <span>上次使用 {lastUsedText(app)}</span>
          </div>
          {app.installLocation && (
            <div className="detail-row">
              <div style={{ minWidth: 0 }}>
                <div>安装位置</div>
                <div className="detail-path">{app.installLocation}</div>
              </div>
              <span className="num faint">{app.installSize !== null ? formatBytes(app.installSize) : ""}</span>
              <button className="icon-btn" title="在资源管理器中打开" onClick={() => void api.revealPath(app.installLocation!)}>
                <FolderOpen size={16} />
              </button>
            </div>
          )}
          {app.dataDirs.map((d) => (
            <div key={d.path} className="detail-row">
              <div style={{ minWidth: 0 }}>
                <div>{d.label}</div>
                <div className="detail-path">{d.path}</div>
              </div>
              <span className="num faint">{formatBytes(d.size)}</span>
              <button className="icon-btn" title="在资源管理器中打开" onClick={() => void api.revealPath(d.path)}>
                <FolderOpen size={16} />
              </button>
            </div>
          ))}
          {app.dataDirs.length === 0 && <div className="faint" style={{ fontSize: 12.5, padding: "4px 0" }}>没有找到这个应用单独存放的数据文件夹</div>}
        </div>
      )}
    </div>
  );
}

type Flow =
  | { step: "confirm"; app: AppInfo }
  | { step: "waiting"; app: AppInfo }
  | { step: "done"; app: AppInfo; result: UninstallResult; picked: Set<string> }
  | { step: "cleaning"; app: AppInfo };

/** Leftovers outside the user folder (Program Files) need admin rights: point at them instead. */
const cleanable = (d: Detail) => /^[a-z]:\\users\\/i.test(d.path);

function UninstallFlow({ flow, setFlow }: { flow: Flow; setFlow: (f: Flow | null) => void }) {
  const { dropApp, toast, refreshDrives } = useStore();
  const app = flow.app;

  // Listening from the moment the dialog opens, not from when it starts waiting: a Store app can be
  // gone before a listener registered after the uninstall call would have been in place.
  useEffect(() => {
    const off = api.onAppUninstall((r) => {
      if (r.id !== app.id) return;
      if (r.removed) dropApp(app.id);
      setFlow({ step: "done", app, result: r, picked: new Set(r.leftovers.filter(cleanable).map((d) => d.path)) });
      void refreshDrives();
    });
    return () => void off.then((f) => f());
  }, [app, dropApp, setFlow, refreshDrives]);

  const close = () => (flow.step === "waiting" || flow.step === "cleaning" ? undefined : setFlow(null));

  return (
    <Dialog onClose={close}>
      {flow.step === "confirm" && (
        <>
          <div className="dialog-body">
            <div style={{ display: "flex", gap: 14, alignItems: "center", marginBottom: 10 }}>
              <AppIcon app={app} size={44} />
              <div>
                <h2 className="dialog-title" style={{ margin: 0 }}>
                  卸载 {app.name}？
                </h2>
                <div className="muted" style={{ fontSize: 13 }}>
                  程序 {formatBytes(app.installSize ?? app.estimatedSize)} · 数据 {formatBytes(app.dataSize ?? 0)}
                </div>
              </div>
            </div>
            <div className="muted" style={{ fontSize: 13.5 }}>
              {app.kind === "store"
                ? "商店应用会直接移除，连同它在 Packages 里的数据。"
                : "会打开这个应用自带的卸载程序，请在弹出的窗口里完成卸载。卸载完成后，DiskMate 会检查有没有留下的数据。"}
            </div>
            {isProtected(app) && (
              <div className="warn-box">
                <ShieldAlert size={18} style={{ flex: "none" }} />
                <div>这是{app.tags.includes("driver") ? "硬件驱动" : "运行库"}，其他软件或硬件可能依赖它。卸载后可能导致别的程序打不开或设备异常，除非确定不需要，否则不建议卸载。</div>
              </div>
            )}
          </div>
          <div className="dialog-actions">
            <button className="btn" onClick={() => setFlow(null)}>
              取消
            </button>
            <button
              className={`btn ${isProtected(app) ? "btn-danger" : "btn-primary"}`}
              onClick={() => {
                setFlow({ step: "waiting", app });
                api.uninstallApp(app.id).catch((e) => {
                  toast(`无法卸载：${e}`);
                  setFlow(null);
                });
              }}
            >
              开始卸载
            </button>
          </div>
        </>
      )}

      {flow.step === "waiting" && (
        <div className="dialog-body" style={{ textAlign: "center", padding: "34px 30px 26px" }}>
          <div className="pulse-ring" style={{ margin: "0 auto 16px" }}>
            <Hourglass size={24} />
          </div>
          <h2 className="dialog-title">正在卸载 {app.name}</h2>
          <div className="muted" style={{ fontSize: 13.5, marginBottom: 20 }}>
            {app.kind === "store" ? "正在移除…" : "请在卸载程序的窗口中完成操作。如果弹出了管理员授权，请点「是」。"}
          </div>
          {app.kind !== "store" && (
            <button className="btn" onClick={() => void api.stopUninstallWait(app.id)}>
              卸载窗口已经关了
            </button>
          )}
        </div>
      )}

      {flow.step === "done" && (
        <>
          <div className="dialog-body">
            {flow.result.removed ? (
              <>
                <div style={{ display: "flex", gap: 10, alignItems: "center" }}>
                  <CircleCheckBig size={22} color="var(--safe)" />
                  <h2 className="dialog-title" style={{ margin: 0 }}>
                    已卸载 {app.name}
                  </h2>
                </div>
                {flow.result.leftovers.length > 0 ? (
                  <>
                    <div className="muted" style={{ fontSize: 13.5, margin: "8px 0 4px" }}>
                      卸载程序留下了 {formatBytes(flow.result.leftovers.reduce((s, d) => s + d.size, 0))} 的文件。勾选的会被删除：
                    </div>
                    <div className="dialog-list">
                      {flow.result.leftovers.map((d) => (
                        <div key={d.path} className="dialog-list-row" style={{ alignItems: "center" }}>
                          <span style={{ display: "flex", gap: 10, alignItems: "center", minWidth: 0 }}>
                            <input
                              type="checkbox"
                              className="check"
                              disabled={!cleanable(d)}
                              checked={flow.picked.has(d.path)}
                              onChange={() => {
                                const picked = new Set(flow.picked);
                                if (picked.has(d.path)) picked.delete(d.path);
                                else picked.add(d.path);
                                setFlow({ ...flow, picked });
                              }}
                            />
                            <span style={{ minWidth: 0 }}>
                              <div>{d.label}</div>
                              <div className="detail-path">{d.path}</div>
                              {!cleanable(d) && <div className="faint" style={{ fontSize: 11.5 }}>需要管理员权限，请在资源管理器里手动删除</div>}
                            </span>
                          </span>
                          <span style={{ display: "flex", gap: 6, alignItems: "center" }}>
                            <span className="num muted">{formatBytes(d.size)}</span>
                            <button className="icon-btn" onClick={() => void api.revealPath(d.path)}>
                              <FolderOpen size={15} />
                            </button>
                          </span>
                        </div>
                      ))}
                    </div>
                  </>
                ) : (
                  <div className="muted" style={{ marginTop: 8 }}>
                    没有发现残留文件。
                  </div>
                )}
              </>
            ) : (
              <>
                <div style={{ display: "flex", gap: 10, alignItems: "center" }}>
                  <TriangleAlert size={22} color="var(--caution)" />
                  <h2 className="dialog-title" style={{ margin: 0 }}>
                    {app.name} 还在
                  </h2>
                </div>
                <div className="muted" style={{ marginTop: 8, fontSize: 13.5 }}>
                  {flow.result.error ?? "卸载程序已经关闭，但这个应用仍在已安装列表里，可能是卸载被取消了。"}
                </div>
              </>
            )}
          </div>
          <div className="dialog-actions">
            {flow.result.removed && flow.picked.size > 0 ? (
              <>
                <button className="btn" onClick={() => setFlow(null)}>
                  保留
                </button>
                <button
                  className="btn btn-primary"
                  onClick={async () => {
                    const picked = [...flow.picked];
                    setFlow({ step: "cleaning", app });
                    const out = await api.cleanAppLeftovers(app.id, picked);
                    toast(`已清理残留 ${formatBytes(out.freed)}${out.skippedFiles ? `，${formatCount(out.skippedFiles)} 个文件被占用已跳过` : ""}`);
                    void refreshDrives();
                    setFlow(null);
                  }}
                >
                  删除勾选的残留
                </button>
              </>
            ) : (
              <button className="btn btn-primary" onClick={() => setFlow(null)}>
                好的
              </button>
            )}
          </div>
        </>
      )}

      {flow.step === "cleaning" && (
        <div className="dialog-body" style={{ textAlign: "center", padding: 36 }}>
          <span className="spinner" style={{ display: "inline-block", width: 28, height: 28, color: "var(--accent)" }} />
          <div style={{ marginTop: 14 }}>正在清理残留…</div>
        </div>
      )}
    </Dialog>
  );
}

export function Apps() {
  const { apps, appsLoading, loadApps } = useStore();
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [sort, setSort] = useState<Sort>("size");
  const [flow, setFlow] = useState<Flow | null>(null);

  useEffect(() => {
    if (apps === null && !appsLoading) loadApps();
  }, [apps, appsLoading, loadApps]);

  const list = apps ?? [];
  const measuredCount = list.filter((a) => a.measured).length;
  const totalBytes = list.reduce((s, a) => s + total(a), 0);
  const dataBytes = list.reduce((s, a) => s + (a.dataSize ?? 0), 0);
  const suggested = list.filter(isSuggested);
  const suggestedBytes = suggested.reduce((s, a) => s + total(a), 0);

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    const filtered = list.filter((a) => {
      if (q && !`${a.name} ${a.publisher}`.toLowerCase().includes(q)) return false;
      switch (filter) {
        case "suggest":
          return isSuggested(a);
        case "stale":
          return isStale(a) && !isProtected(a);
        case "data":
          return (a.dataSize ?? 0) >= 1024 ** 3;
        case "store":
          return a.kind === "store";
        case "dev":
          return a.tags.includes("dev");
        case "system":
          return isProtected(a);
        default:
          return true;
      }
    });
    const by: Record<Sort, (a: AppInfo, b: AppInfo) => number> = {
      size: (a, b) => total(b) - total(a),
      lastUsed: (a, b) => (a.lastUsed ?? 0) - (b.lastUsed ?? 0),
      installed: (a, b) => (b.installDate ?? "").localeCompare(a.installDate ?? ""),
      name: (a, b) => a.name.localeCompare(b.name, "zh-CN"),
    };
    return [...filtered].sort(by[sort]);
  }, [list, query, filter, sort]);

  const max = Math.max(1, ...list.map(total));

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1 className="page-title">应用管理</h1>
          <div className="page-sub">看看每个应用占了多少空间、多久没用，不需要的可以直接卸载</div>
        </div>
        <button className="btn" disabled={appsLoading} onClick={loadApps}>
          <RefreshCw size={15} /> 刷新
        </button>
      </div>

      <div className="stat-row">
        <div className="card card-pad stat">
          <div className="faint" style={{ fontSize: 12.5 }}>已安装</div>
          <div className="stat-value num">{apps ? list.length : "—"}</div>
          <div className="muted" style={{ fontSize: 12.5 }}>
            共占用 {formatBytes(totalBytes)}
            {apps && measuredCount < list.length && `（已测 ${measuredCount}/${list.length}）`}
          </div>
        </div>
        <div className="card card-pad stat">
          <div className="faint" style={{ fontSize: 12.5 }}>应用数据</div>
          <div className="stat-value num" style={{ color: "#EC4899" }}>
            {formatBytes(dataBytes)}
          </div>
          <div className="muted" style={{ fontSize: 12.5 }}>
            聊天记录、缓存、下载等，常常比程序本身还大
          </div>
        </div>
        <button className="card card-pad stat stat-action" onClick={() => setFilter("suggest")}>
          <div className="faint" style={{ fontSize: 12.5, display: "flex", gap: 6, alignItems: "center" }}>
            <Lightbulb size={14} color="var(--caution)" /> 可以考虑卸载
          </div>
          <div className="stat-value num" style={{ color: "var(--accent-text)" }}>
            {suggested.length} 个
          </div>
          <div className="muted" style={{ fontSize: 12.5 }}>
            超过 3 个月没用，共 {formatBytes(suggestedBytes)} · 点击查看
          </div>
        </button>
      </div>

      <div className="toolbar">
        <div className="search">
          <Search size={16} color="var(--text-3)" />
          <input placeholder="搜索应用或发行商" value={query} onChange={(e) => setQuery(e.target.value)} />
        </div>
        <div className="filter-chips">
          {FILTERS.map((f) => (
            <button key={f.id} className={`chip ${filter === f.id ? "active" : ""}`} onClick={() => setFilter(f.id)}>
              {f.label}
            </button>
          ))}
        </div>
        <label className="sort">
          <ArrowUpDown size={14} />
          <select value={sort} onChange={(e) => setSort(e.target.value as Sort)}>
            {SORTS.map((s) => (
              <option key={s.id} value={s.id}>
                按{s.label}
              </option>
            ))}
          </select>
        </label>
      </div>

      <div className="app-head">
        <span />
        <span>应用</span>
        <span style={{ textAlign: "right" }}>占用</span>
        <span style={{ textAlign: "right", display: "flex", gap: 4, justifyContent: "flex-end", alignItems: "center" }}>
          <Clock size={12} /> 上次使用
        </span>
        <span />
      </div>

      {apps === null &&
        [0, 1, 2, 3, 4, 5].map((i) => <div key={i} className="skeleton" style={{ height: 62, marginBottom: 6, opacity: 1 - i * 0.13 }} />)}

      {apps !== null && shown.length === 0 && (
        <div className="card empty">
          <div className="empty-icon">
            <PackageX size={30} />
          </div>
          <div>没有符合条件的应用</div>
        </div>
      )}

      {shown.map((a) => (
        <AppRow key={a.id} app={a} max={max} onUninstall={() => setFlow({ step: "confirm", app: a })} />
      ))}

      {flow && <UninstallFlow flow={flow} setFlow={setFlow} />}
    </div>
  );
}
