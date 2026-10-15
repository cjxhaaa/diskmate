import { useEffect, useState } from "react";
import { AppWindow, Copy, Crosshair, FolderOpen, MousePointerClick, ShieldHalf, SquareX, X } from "lucide-react";
import { api, type WindowPick } from "../api";
import { Dialog, ProgramIcon } from "../components/ui";
import { formatBytes } from "../format";

const dateTime = (secs: number) => new Date(secs * 1000).toLocaleString("zh-CN", { hour12: false });
import { useStore } from "../store";

/** Picks made this session, newest first, kept when switching pages. */
let history: WindowPick[] = [];

function Row({ label, value, mono, copy }: { label: string; value: string | null | undefined; mono?: boolean; copy?: boolean }) {
  const { toast } = useStore();
  if (!value) return null;
  return (
    <div className="finder-row">
      <span className="faint">{label}</span>
      <span className={mono ? "finder-mono" : ""} title={value}>
        {value}
      </span>
      {copy ? (
        <button
          className="icon-btn"
          title="复制"
          onClick={() => void navigator.clipboard.writeText(value).then(() => toast("已复制"), () => toast("复制失败"))}
        >
          <Copy size={14} />
        </button>
      ) : (
        <span />
      )}
    </div>
  );
}

export function Finder() {
  const { toast } = useStore();
  const [picking, setPicking] = useState(false);
  const [pick, setPick] = useState<WindowPick | null>(history[0] ?? null);
  const [confirmEnd, setConfirmEnd] = useState(false);

  // A pick started from the tray finished before this page existed.
  useEffect(() => {
    void api.lastPick().then((p) => {
      if (p && !history.some((h) => h.hwnd === p.hwnd && h.pid === p.pid)) {
        history = [p, ...history].slice(0, 6);
        setPick(p);
      }
    });
  }, []);

  const start = async () => {
    setPicking(true);
    try {
      const p = await api.pickWindow();
      if (p) {
        history = [p, ...history.filter((h) => !(h.hwnd === p.hwnd && h.pid === p.pid))].slice(0, 6);
        setPick(p);
      }
    } catch (e) {
      toast(String(e));
    } finally {
      setPicking(false);
    }
  };

  const end = async (p: WindowPick) => {
    setConfirmEnd(false);
    try {
      await api.endPid(p.pid);
      toast(`已结束 ${p.name}`);
    } catch (e) {
      toast(String(e));
    }
  };

  const name = pick ? (pick.description && pick.description.length <= 40 ? pick.description : pick.name) : "";

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1 className="page-title">窗口定位</h1>
          <div className="page-sub">点一下桌面上的任意窗口，看看它是哪个程序、文件放在哪里</div>
        </div>
      </div>

      <div className="card finder-start">
        <div className="finder-steps">
          {[
            ["点「选择窗口」", "DiskMate 会先最小化，让出屏幕"],
            ["把鼠标移到窗口上", "鼠标下的窗口会被框出来，并显示程序名"],
            ["单击选定", "这一下不会点到窗口里的东西；按 Esc 或右键取消"],
          ].map(([t, d], i) => (
            <div key={t} className="move-step">
              <span className="move-step-num">{i + 1}</span>
              <div>
                <b>{t}</b>
                {d}
              </div>
            </div>
          ))}
        </div>
        <button className="btn btn-primary btn-lg" disabled={picking} onClick={() => void start()}>
          {picking ? <span className="spinner" style={{ width: 16, height: 16, borderWidth: 2 }} /> : <Crosshair size={18} />}
          {picking ? "正在选择…按 Esc 取消" : "选择窗口"}
        </button>
      </div>

      {!pick && (
        <div className="card empty">
          <div className="empty-icon">
            <MousePointerClick size={30} />
          </div>
          <div>还没有选择窗口。也可以在托盘图标的右键菜单里选「查找窗口所属程序…」。</div>
        </div>
      )}

      {pick && (
        <div className="card finder-result">
          <div className="finder-head">
            <ProgramIcon src={pick.icon} name={name} size={48} />
            <div style={{ minWidth: 0 }}>
              <div className="finder-name">
                {name}
                {pick.elevated && (
                  <span className="badge caution" title="这个程序以管理员身份运行">
                    <ShieldHalf size={11} /> 管理员
                  </span>
                )}
                {pick.hosted && (
                  <span className="badge neutral" title="应用商店应用的窗口由 ApplicationFrameHost 托管，这里显示的是应用本身的进程">
                    应用商店应用
                  </span>
                )}
              </div>
              <div className="muted num" style={{ fontSize: 13 }}>
                {pick.name} · PID {pick.pid}
                {pick.company && ` · ${pick.company}`}
              </div>
            </div>
          </div>

          <div className="finder-actions">
            <button className="btn btn-primary" disabled={!pick.path} onClick={() => pick.path && void api.revealPath(pick.path)}>
              <FolderOpen size={16} /> 打开文件位置
            </button>
            <button
              className="btn"
              onClick={() =>
                void api.focusWindow(pick.hwnd).catch((e) => toast(String(e)))
              }
            >
              <AppWindow size={16} /> 切换到该窗口
            </button>
            <button className="btn btn-ghost" onClick={() => setConfirmEnd(true)}>
              <SquareX size={16} /> 结束进程
            </button>
          </div>

          <div className="finder-rows">
            <Row label="文件位置" value={pick.path ?? "读不到（系统保护的进程）"} mono copy={!!pick.path} />
            <Row label="命令行" value={pick.commandLine} mono copy />
            <Row label="窗口标题" value={pick.title || "（无标题）"} />
            <Row label="窗口类名" value={pick.className} mono />
            <Row label="启动时间" value={pick.startedAt ? dateTime(pick.startedAt) : null} />
            <Row label="内存（专用）" value={pick.memory != null ? formatBytes(pick.memory) : null} />
            <Row label="线程数" value={pick.threads ? String(pick.threads) : null} />
            <Row label="父进程" value={pick.parentPid ? `${pick.parentName ?? "?"} · PID ${pick.parentPid}` : null} />
          </div>
        </div>
      )}

      {history.length > 1 && (
        <section className="group" style={{ marginTop: 18 }}>
          <div className="group-head">
            <span className="group-title">最近选过的</span>
          </div>
          <div className="finder-history">
            {history.map((h) => (
              <button
                key={`${h.hwnd}-${h.pid}`}
                className={`finder-chip ${pick && h.hwnd === pick.hwnd && h.pid === pick.pid ? "active" : ""}`}
                onClick={() => setPick(h)}
              >
                <ProgramIcon src={h.icon} name={h.description ?? h.name} size={18} />
                <span>{h.description && h.description.length <= 24 ? h.description : h.name}</span>
              </button>
            ))}
          </div>
        </section>
      )}

      {confirmEnd && pick && (
        <Dialog onClose={() => setConfirmEnd(false)}>
          <div className="dialog-body">
            <h2 className="dialog-title">结束「{name}」？</h2>
            <div className="muted" style={{ fontSize: 13.5 }}>
              只结束这一个进程（PID {pick.pid}），没有保存的内容会丢失。
            </div>
          </div>
          <div className="dialog-actions">
            <button className="btn" onClick={() => setConfirmEnd(false)}>
              <X size={15} /> 取消
            </button>
            <button className="btn btn-danger" onClick={() => void end(pick)}>
              结束进程
            </button>
          </div>
        </Dialog>
      )}
    </div>
  );
}
