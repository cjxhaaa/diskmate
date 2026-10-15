import { useCallback, useEffect, useState } from "react";
import { FolderOpen, Info, Power, RefreshCw, ShieldCheck, ShieldHalf, Sparkles, TriangleAlert } from "lucide-react";
import { api, type StartupItem } from "../api";
import { Dialog, ProgramIcon, Switch } from "../components/ui";
import { useStore } from "../store";

type Filter = "all" | "on" | "off";

const SOURCE: Record<StartupItem["source"], string> = {
  registry: "注册表",
  folder: "启动文件夹",
  store: "应用商店应用",
};

const GROUPS: { key: "disable" | "none" | "keep"; title: string; hint: string }[] = [
  { key: "disable", title: "可以关掉", hint: "不需要开机就运行，用的时候再打开即可" },
  { key: "none", title: "其他", hint: "按自己的习惯决定" },
  { key: "keep", title: "建议保留", hint: "系统组件、驱动和安全软件" },
];

const groupOf = (i: StartupItem) => i.advice?.kind ?? "none";

export function Startup() {
  const { toast } = useStore();
  const [list, setList] = useState<StartupItem[] | null>(null);
  const [filter, setFilter] = useState<Filter>("all");
  const [busy, setBusy] = useState<Set<string>>(new Set());
  const [bulk, setBulk] = useState<StartupItem[] | null>(null);

  const load = useCallback(() => {
    setList(null);
    api
      .listStartup()
      .then(setList)
      .catch((e) => toast(`读取失败：${e}`));
  }, [toast]);
  useEffect(load, [load]);

  const setOne = async (item: StartupItem, enabled: boolean) => {
    setBusy((b) => new Set(b).add(item.id));
    try {
      await api.setStartupEnabled(item.id, enabled);
      setList((l) => l?.map((i) => (i.id === item.id ? { ...i, enabled } : i)) ?? l);
      return true;
    } catch (e) {
      toast(`「${item.name}」没有改成：${e}`);
      return false;
    } finally {
      setBusy((b) => {
        const n = new Set(b);
        n.delete(item.id);
        return n;
      });
    }
  };

  const toggle = async (item: StartupItem, enabled: boolean) => {
    if (item.allUsers) toast("这一项对所有用户生效，请在弹出的窗口里点「是」");
    if (await setOne(item, enabled)) toast(enabled ? `已允许「${item.name}」开机启动` : `「${item.name}」下次开机不会再自动运行`);
  };

  const runBulk = async (items: StartupItem[]) => {
    setBulk(null);
    let done = 0;
    for (const i of items) if (await setOne(i, false)) done++;
    toast(`已关掉 ${done} 项，下次开机生效`);
  };

  const all = list ?? [];
  const enabled = all.filter((i) => i.enabled);
  const suggested = enabled.filter((i) => i.advice?.kind === "disable");
  const shown = all.filter((i) => (filter === "on" ? i.enabled : filter === "off" ? !i.enabled : true));

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1 className="page-title">开机启动</h1>
          <div className="page-sub">开机时自动运行的程序。关掉不常用的，开机更快，后台也更清爽</div>
        </div>
        <button className="icon-btn" title="重新读取" onClick={load}>
          <RefreshCw size={16} />
        </button>
      </div>

      <div className="startup-summary">
        <div className="card startup-stat">
          <Power size={18} color="var(--accent)" />
          <div>
            <div className="startup-stat-num num">{list ? enabled.length : "–"}</div>
            <div className="faint">个程序会开机启动</div>
          </div>
        </div>
        <div className="card startup-stat">
          <Sparkles size={18} color="var(--caution)" />
          <div>
            <div className="startup-stat-num num">{list ? suggested.length : "–"}</div>
            <div className="faint">个可以关掉</div>
          </div>
          {suggested.length > 0 && (
            <button className="btn btn-primary" style={{ marginLeft: "auto" }} onClick={() => setBulk(suggested)}>
              全部关掉
            </button>
          )}
        </div>
        <div className="card startup-stat">
          <Info size={18} color="var(--text-3)" />
          <div className="muted" style={{ fontSize: 12.5, lineHeight: 1.5 }}>
            关掉不会删除任何东西，随时可以再打开。和任务管理器「启动应用」里的开关是同一个设置。
          </div>
        </div>
      </div>

      <div className="move-list-head">
        <div className="filter-chips">
          {(
            [
              ["all", `全部 ${all.length}`],
              ["on", `开机启动 ${enabled.length}`],
              ["off", `已关掉 ${all.length - enabled.length}`],
            ] as const
          ).map(([k, label]) => (
            <button key={k} className={`chip ${filter === k ? "active" : ""}`} onClick={() => setFilter(k)}>
              {label}
            </button>
          ))}
        </div>
      </div>

      {list === null && [0, 1, 2, 3].map((i) => <div key={i} className="skeleton" style={{ height: 72, marginBottom: 8, borderRadius: 14 }} />)}

      {list !== null &&
        GROUPS.map((g) => {
          const rows = shown.filter((i) => groupOf(i) === g.key);
          if (!rows.length) return null;
          return (
            <section key={g.key} className="group">
              <div className="group-head">
                <span className="group-title">{g.title}</span>
                <span className="faint" style={{ fontSize: 13 }}>
                  {g.hint}
                </span>
              </div>
              {rows.map((i) => (
                <div key={i.id} className={`item startup-row ${i.enabled ? "" : "is-off"}`}>
                  <ProgramIcon src={i.icon} name={i.name} />
                  <div style={{ minWidth: 0 }}>
                    <div className="item-title">
                      {i.name}
                      {i.allUsers && (
                        <span className="badge neutral" title="对这台电脑的所有用户生效，修改时需要管理员授权">
                          <ShieldHalf size={11} /> 所有用户
                        </span>
                      )}
                      {i.missing && (
                        <span className="badge review" title="指向的程序已经不在了，这一项开机时什么也不会做">
                          <TriangleAlert size={11} /> 程序已不存在
                        </span>
                      )}
                    </div>
                    <div className="item-desc">
                      {[i.publisher, SOURCE[i.source]].filter(Boolean).join(" · ")}
                      {i.advice && (
                        <span className={`startup-advice ${i.advice.kind}`}>
                          {i.advice.kind === "keep" ? <ShieldCheck size={12} /> : <Sparkles size={12} />}
                          {i.advice.reason}
                        </span>
                      )}
                    </div>
                    {i.source !== "store" && (
                      <div className="detail-path" title={i.command}>
                        {i.path ?? i.command}
                      </div>
                    )}
                  </div>
                  {i.path && !i.missing ? (
                    <button className="icon-btn" title="打开所在位置" onClick={() => void api.revealPath(i.path!)}>
                      <FolderOpen size={16} />
                    </button>
                  ) : (
                    <span />
                  )}
                  <div className="startup-state">
                    <span className={i.enabled ? "" : "faint"}>{i.enabled ? "开机启动" : "已关掉"}</span>
                    <Switch
                      on={i.enabled}
                      busy={busy.has(i.id)}
                      label={i.enabled ? `不让「${i.name}」开机启动` : `让「${i.name}」开机启动`}
                      onChange={(on) => void toggle(i, on)}
                    />
                  </div>
                </div>
              ))}
            </section>
          );
        })}

      {list !== null && shown.length === 0 && (
        <div className="card empty">
          <div className="empty-icon">
            <Power size={30} />
          </div>
          <div>{filter === "off" ? "没有关掉的启动项" : "没有找到启动项"}</div>
        </div>
      )}

      {bulk && (
        <Dialog onClose={() => setBulk(null)}>
          <div className="dialog-body">
            <h2 className="dialog-title">关掉这 {bulk.length} 个启动项？</h2>
            <div className="dialog-list">
              {bulk.map((i) => (
                <div key={i.id} className="dialog-list-row" style={{ alignItems: "center", justifyContent: "flex-start" }}>
                  <ProgramIcon src={i.icon} name={i.name} size={24} />
                  <span style={{ minWidth: 0, flex: 1 }}>
                    {i.name}
                    <span className="faint" style={{ display: "block", fontSize: 12 }}>
                      {i.advice?.reason}
                    </span>
                  </span>
                </div>
              ))}
            </div>
            <div className="muted" style={{ fontSize: 13 }}>
              下次开机时它们不会再自动运行，程序本身不受影响，需要时照常打开。
              {bulk.some((i) => i.allUsers) && " 其中对所有用户生效的项会逐个弹出管理员授权。"}
            </div>
          </div>
          <div className="dialog-actions">
            <button className="btn" onClick={() => setBulk(null)}>
              取消
            </button>
            <button className="btn btn-primary" onClick={() => void runBulk(bulk)}>
              全部关掉
            </button>
          </div>
        </Dialog>
      )}
    </div>
  );
}
