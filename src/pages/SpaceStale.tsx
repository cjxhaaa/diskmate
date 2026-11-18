import { useEffect, useState } from "react";
import { Info } from "lucide-react";
import { api, type Hit } from "../api";
import { HitList } from "../components/HitList";
import { formatBytes } from "../format";

interface Props {
  tabs: React.ReactNode;
  root: string;
  reload: number;
  onShow: (t: { path: string; isDir: boolean }) => void;
  onMenu: (h: Hit, x: number, y: number) => void;
}

const MB = 1024 ** 2;
const AGES: [number, string][] = [
  [180, "半年"],
  [365, "一年"],
  [730, "两年"],
  [1095, "三年"],
];
const SIZES: [number, string][] = [
  [100 * MB, "100 MB"],
  [500 * MB, "500 MB"],
  [1024 * MB, "1 GB"],
];

/**
 * Big folders in which nothing has changed for a long time: old projects, installers kept "just in
 * case", games and tools no longer used. Windows and installed programs are left out — they are
 * rarely written to however much they are used.
 */
export function SpaceStale({ tabs, root, reload, onShow, onMenu }: Props) {
  const [days, setDays] = useState(365);
  const [minSize, setMinSize] = useState(500 * MB);
  const [hits, setHits] = useState<Hit[] | null>(null);

  useEffect(() => {
    let alive = true;
    setHits(null);
    void api
      .staleFolders(root, days, minSize)
      .then((h) => alive && setHits(h))
      .catch(() => alive && setHits([]));
    return () => {
      alive = false;
    };
  }, [root, days, minSize, reload]);

  const total = hits?.reduce((s, h) => s + h.size, 0) ?? 0;
  const label = AGES.find(([d]) => d === days)?.[1];

  return (
    <>
    <div className="space-tabs-row">
      {tabs}
      <div className="filter-chips">
          <span className="chip-label">多久没改动</span>
          {AGES.map(([d, l]) => (
            <button key={d} className={`chip ${days === d ? "active" : ""}`} onClick={() => setDays(d)}>
              {l}
            </button>
          ))}
          <span className="chip-label" style={{ marginLeft: 10 }}>
            至少
          </span>
          {SIZES.map(([s, l]) => (
            <button key={s} className={`chip ${minSize === s ? "active" : ""}`} onClick={() => setMinSize(s)}>
              {l}
            </button>
          ))}
      </div>
    </div>
    <div className="card stale-card">
      <div className="side-head">
        <span>
          {hits ? (
            <>
              {hits.length} 个文件夹超过{label}没有改动，合计 <span className="num">{formatBytes(total)}</span>
            </>
          ) : (
            "正在查找…"
          )}
        </span>
      </div>
      <div className="stale-note">
        <Info size={14} />
        <span>
          按扫描时文件夹里最新一个文件的修改时间来算。程序和游戏可能天天在用却不改动文件，所以列在这里不代表没在用，删除前请先确认；Windows、Program Files 等系统文件夹不在其中。
        </span>
      </div>
      {hits ? (
        <HitList hits={hits} onShow={onShow} onMenu={onMenu} dates="full" empty={`没有超过${label}没改动、又至少这么大的文件夹`} />
      ) : (
        <div className="skeleton" style={{ height: 240 }} />
      )}
    </div>
    </>
  );
}
