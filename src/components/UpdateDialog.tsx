import { useEffect, useState, type ReactNode } from "react";
import { Check, Copy, Download, TriangleAlert } from "lucide-react";
import { api, type UpdateInfo, type UpdateProgress } from "../api";
import { formatBytes } from "../format";
import { useStore } from "../store";
import { Dialog } from "./ui";

type Phase = { kind: "ask" } | { kind: "downloading"; progress: UpdateProgress | null } | { kind: "installing" } | { kind: "error"; message: string };

/** `**bold**` inside a line of release notes. */
function inline(text: string): ReactNode[] {
  return text.split(/\*\*(.+?)\*\*/g).map((part, i) => (i % 2 ? <b key={i}>{part}</b> : part));
}

/**
 * The release notes, which are Markdown on the release page: headings and bullet points are all
 * they use, so those are drawn and everything else is shown as plain text.
 */
function Notes({ text }: { text: string }) {
  const lines = text
    .replace(/\r/g, "")
    .split("\n")
    .filter((l) => l.trim() && !/^完整更新日志/.test(l.trim()));
  if (!lines.length) return null;
  return (
    <div className="dialog-list update-notes">
      {lines.map((l, i) => {
        const heading = l.match(/^#{1,6}\s+(.*)/);
        if (heading) return <div key={i} className="update-notes-head">{inline(heading[1])}</div>;
        const bullet = l.match(/^\s*[-*]\s+(.*)/);
        if (bullet) return <div key={i} className="update-notes-item">{inline(bullet[1])}</div>;
        return <div key={i}>{inline(l)}</div>;
      })}
    </div>
  );
}

/**
 * Offers a newer release, opened from the sidebar's 「新版本」 note or the tray's 「检查更新…」. An installed copy downloads, verifies and installs it (DiskMate closes
 * and the installer opens the new version); a portable or Scoop copy is told how to update.
 */
export function UpdateDialog({ info, onClose, onSkip }: { info: UpdateInfo; onClose: () => void; onSkip: () => void }) {
  const { scanning } = useStore();
  const [phase, setPhase] = useState<Phase>({ kind: "ask" });
  const [copied, setCopied] = useState(false);
  const busy = phase.kind === "downloading" || phase.kind === "installing";

  useEffect(() => {
    const off = api.onUpdateProgress((p) => setPhase(p.installing ? { kind: "installing" } : { kind: "downloading", progress: p }));
    return () => void off.then((f) => f());
  }, []);

  const install = () => {
    setPhase({ kind: "downloading", progress: null });
    // On success DiskMate exits while the installer runs, so only a failure comes back.
    api.installUpdate().catch((e) => setPhase({ kind: "error", message: String(e) }));
  };
  const skip = () => {
    void api.skipUpdate(info.version);
    onSkip();
  };

  const p = phase.kind === "downloading" ? phase.progress : null;
  const pct = p?.total ? Math.min(100, (p.downloaded / p.total) * 100) : null;

  return (
    <Dialog onClose={busy ? () => {} : onClose}>
      <div className="dialog-body">
        <h2 className="dialog-title">DiskMate {info.version} 可以更新了</h2>
        <div className="muted" style={{ fontSize: 13 }}>
          当前版本 {info.current}
        </div>
        <Notes text={info.notes} />

        {info.kind === "installer" && phase.kind === "ask" && (
          <div className="faint" style={{ fontSize: 12.5 }}>
            更新时 DiskMate 会先关闭，装好后自动重新打开。
            {scanning ? "现在正在扫描，更新会中断它。" : "正在进行的搬家或清理会被中断，请等它们完成后再更新。"}
          </div>
        )}
        {phase.kind === "downloading" && (
          <div style={{ marginTop: 12 }}>
            <div className="muted num" style={{ fontSize: 13, marginBottom: 6 }}>
              正在下载{p ? `：${formatBytes(p.downloaded)}${p.total ? ` / ${formatBytes(p.total)}` : ""}` : "…"}
            </div>
            <div className="bar">
              <span style={{ width: `${pct ?? 4}%`, background: "var(--grad)" }} />
            </div>
          </div>
        )}
        {phase.kind === "installing" && (
          <div className="muted" style={{ marginTop: 12, fontSize: 13.5 }}>
            下载完成，签名校验通过。正在打开安装程序，DiskMate 会关闭，装好后自动重新打开…
          </div>
        )}
        {phase.kind === "error" && (
          <div className="warn-box" style={{ marginTop: 12 }}>
            <TriangleAlert size={18} style={{ flex: "none" }} />
            <div>{phase.message}</div>
          </div>
        )}

        {info.kind === "portable" && (
          <div className="muted" style={{ marginTop: 12, fontSize: 13.5 }}>
            你用的是便携版：下载新的压缩包，解压后替换原来的 DiskMate.exe 即可，设置和扫描记录都会保留。
          </div>
        )}
        {info.kind === "scoop" && (
          <>
            <div className="muted" style={{ marginTop: 12, fontSize: 13.5 }}>
              你是用 Scoop 安装的，在终端里运行下面的命令升级：
            </div>
            <div className="dialog-list" style={{ display: "flex", alignItems: "center", gap: 10, padding: "6px 6px 6px 14px" }}>
              <code style={{ flex: 1, userSelect: "text" }}>scoop update diskmate</code>
              <button
                className="btn"
                onClick={() =>
                  void navigator.clipboard.writeText("scoop update diskmate").then(
                    () => setCopied(true),
                    () => {},
                  )
                }
              >
                {copied ? <Check size={15} /> : <Copy size={15} />} {copied ? "已复制" : "复制"}
              </button>
            </div>
          </>
        )}
      </div>

      <div className="dialog-actions">
        {!busy && (
          <button className="btn btn-ghost" onClick={skip} style={{ marginRight: "auto" }}>
            跳过这个版本
          </button>
        )}
        {!busy && (
          <button className="btn" onClick={onClose}>
            {info.kind === "scoop" ? "知道了" : "稍后"}
          </button>
        )}
        {info.kind === "installer" && !busy && (
          <button className="btn btn-primary" onClick={install}>
            <Download size={15} /> {phase.kind === "error" ? "重试" : "立即更新"}
          </button>
        )}
        {info.kind === "portable" && (
          <button className="btn btn-primary" onClick={() => api.openReleases().catch(() => {})}>
            <Download size={15} /> 去下载
          </button>
        )}
      </div>
    </Dialog>
  );
}
