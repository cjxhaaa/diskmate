import { useEffect, useMemo, useState } from "react";
import { api, type ExtStat, type Hit } from "../api";
import { HitList } from "../components/HitList";
import { KINDS, kindColor, kindsOf } from "../filetypes";
import { formatBytes, formatCount, percent } from "../format";

interface Props {
  tabs: React.ReactNode;
  root: string;
  reload: number;
  onShow: (t: { path: string; isDir: boolean }) => void;
  onMenu: (h: Hit, x: number, y: number) => void;
}

/** Extensions listed at first; the rest are a click away. */
const EXTS_SHOWN = 40;

/**
 * The whole drive by kind of file, then by extension (as WizTree's file types view): what fills
 * the disk — videos, installers, build output — rather than where it is. Pick a kind or an
 * extension to see its biggest files.
 */
export function SpaceTypes({ tabs, root, reload, onShow, onMenu }: Props) {
  const [types, setTypes] = useState<number[] | null>(null);
  const [total, setTotal] = useState(0);
  const [exts, setExts] = useState<ExtStat[] | null>(null);
  const [kind, setKind] = useState<number | null>(null);
  const [ext, setExt] = useState<string | null>(null);
  const [all, setAll] = useState(false);
  const [files, setFiles] = useState<Hit[] | null>(null);

  useEffect(() => {
    void api.getTree(root, 0, 0, 1).then((t) => {
      setTypes(t.types);
      setTotal(t.size);
    });
    void api.extStats(root).then(setExts);
  }, [root, reload]);

  useEffect(() => {
    let alive = true;
    setFiles(null);
    void api
      .searchScan(root, { ext, kind: ext === null ? kind : null, only: "files" })
      .then((h) => alive && setFiles(h.slice(0, 100)))
      .catch(() => alive && setFiles([]));
    return () => {
      alive = false;
    };
  }, [root, ext, kind, reload]);

  const kinds = useMemo(() => (types ? kindsOf(types) : []), [types]);
  const shownExts = useMemo(() => {
    const list = (exts ?? []).filter((e) => kind === null || e.kind === kind);
    return all ? list : list.slice(0, EXTS_SHOWN);
  }, [exts, kind, all]);
  const extCount = (exts ?? []).filter((e) => kind === null || e.kind === kind).length;
  const extTotal = (exts ?? []).filter((e) => kind === null || e.kind === kind).reduce((s, e) => s + e.bytes, 0);

  const pickKind = (k: number | null) => {
    setKind(k);
    setExt(null);
    setAll(false);
  };

  const filesTitle = ext !== null ? `最大的 ${ext ? "." + ext : "无扩展名"} 文件` : kind !== null ? `最大的${KINDS[kind].label}文件` : "整个磁盘最大的文件";

  return (
    <>
    <div className="space-tabs-row">
      {tabs}
      <span className="faint" style={{ fontSize: 12.5 }}>
        整个磁盘按文件类型统计，点一类或一种扩展名，右边列出最大的文件
      </span>
    </div>
    <div className="space-body">
      <div className="card types-card">
        <div className="side-head">
          <span>按类型</span>
          <span className="faint num">{formatBytes(total)}</span>
        </div>
        {types ? (
          <>
            <div className="kind-bar" role="img" aria-label="各类文件占的比例">
              {kinds.map(([k, v]) => (
                <span key={k} style={{ flexGrow: v, background: kindColor(k) }} title={`${KINDS[k].label} ${formatBytes(v)}`} />
              ))}
            </div>
            <div className="kind-list">
              <button className={`kind-row ${kind === null ? "on" : ""}`} onClick={() => pickKind(null)}>
                <span className="swatch" style={{ background: "var(--text-3)" }} />
                <span>全部类型</span>
                <span className="num">{formatBytes(total)}</span>
                <span className="num faint">100%</span>
              </button>
              {kinds.map(([k, v]) => (
                <button key={k} className={`kind-row ${kind === k ? "on" : ""}`} onClick={() => pickKind(kind === k ? null : k)}>
                  <span className="swatch" style={{ background: kindColor(k) }} />
                  <span>{KINDS[k].label}</span>
                  <span className="num">{formatBytes(v)}</span>
                  <span className="num faint">{percent(v, total)}</span>
                </button>
              ))}
            </div>
          </>
        ) : (
          <div className="skeleton" style={{ height: 160 }} />
        )}

        <div className="side-head" style={{ marginTop: 18 }}>
          <span>按扩展名{kind !== null ? `（${KINDS[kind].label}）` : ""}</span>
          <span className="faint num">{extCount} 种</span>
        </div>
        {exts ? (
          <div className="ext-table">
            {shownExts.map((e) => (
              <button key={e.ext} className={`ext-row ${ext === e.ext ? "on" : ""}`} onClick={() => setExt(ext === e.ext ? null : e.ext)} title="点击列出这种文件里最大的">
                <span className="swatch" style={{ background: kindColor(e.kind) }} />
                <span className="ext-name">{e.ext ? `.${e.ext}` : "无扩展名"}</span>
                <span className="faint">{KINDS[e.kind].short}</span>
                <span className="num faint">{formatCount(e.files)} 个</span>
                <span className="num ext-size">{formatBytes(e.bytes)}</span>
                <span className="ext-bar">
                  <span style={{ width: percent(e.bytes, extTotal || 1), background: kindColor(e.kind) }} />
                </span>
              </button>
            ))}
            {!all && extCount > EXTS_SHOWN && (
              <button className="btn btn-sm" style={{ alignSelf: "center", marginTop: 8 }} onClick={() => setAll(true)}>
                显示全部 {extCount} 种
              </button>
            )}
          </div>
        ) : (
          <div className="skeleton" style={{ height: 240 }} />
        )}
      </div>

      <div className="card side-card">
        <div className="side-head">
          <span>{filesTitle}</span>
          {(ext !== null || kind !== null) && (
            <button className="btn btn-sm" onClick={() => pickKind(null)}>
              看全部
            </button>
          )}
        </div>
        {files ? (
          <HitList hits={files} onShow={onShow} onMenu={onMenu} empty="没有 4 MB 以上的这类文件（更小的文件不单独记录）" />
        ) : (
          <div className="skeleton" style={{ height: 200 }} />
        )}
      </div>
    </div>
    </>
  );
}
