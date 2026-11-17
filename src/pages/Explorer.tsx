import { useCallback, useEffect, useMemo, useState } from "react";
import { ArrowLeft, FolderOpen, ScanSearch, Search, SlidersHorizontal, X, Zap } from "lucide-react";
import { api, type Hit, type TreeItem } from "../api";
import { HitList } from "../components/HitList";
import { openEntry, useItemActions, type ActionTarget } from "../components/ItemActions";
import { itemKey, SpaceMap } from "../components/SpaceMap";
import { TreeTable } from "../components/TreeTable";
import { DrivePicker } from "../components/ui";
import { KINDS, kindColor, kindsOf } from "../filetypes";
import { formatAgo, formatBytes, formatCount, formatDate, percent } from "../format";
import { useStore } from "../store";
import { SpaceStale } from "./SpaceStale";
import { SpaceTypes } from "./SpaceTypes";

type Tab = "map" | "types" | "stale";

const GB = 1024 ** 3;
const MB = 1024 ** 2;

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

/** The folder a path is in; a drive root stays itself. */
export function parentOf(path: string): string {
  const i = path.replace(/\\$/, "").lastIndexOf("\\");
  if (i < 0) return path;
  const p = path.slice(0, i);
  return /^[A-Za-z]:$/.test(p) ? p + "\\" : p;
}

/** A tree item or search hit as something the right-click menu can act on. */
export const targetOf = (i: TreeItem | Hit): ActionTarget =>
  "kind" in i
    ? { path: i.path, name: i.name, size: i.size, isDir: i.kind === "dir", lumped: i.kind === "rest" }
    : { path: i.path, name: i.name, size: i.size, isDir: i.isDir };

export function Explorer() {
  const { current, scanning, root: driveRoot, setRoot, drives, startScan, restoreSaved } = useStore();
  const [canFast, setCanFast] = useState(false);
  useEffect(() => {
    let alive = true;
    void api
      .canFastScan(driveRoot)
      .then((v) => alive && setCanFast(v))
      .catch(() => alive && setCanFast(false));
    return () => {
      alive = false;
    };
  }, [driveRoot]);
  // The same drive as on the overview: picking one here picks it there too.
  const picker = drives.length > 1 && <DrivePicker drives={drives} value={driveRoot} onChange={setRoot} />;
  const [restoring, setRestoring] = useState(false);
  const [tab, setTab] = useState<Tab>("map");
  const [path, setPath] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [reload, setReload] = useState(0);

  // Only totals from a past session on screen: load the tree saved with that scan.
  const fromHistory = current?.fromHistory ? current.root : null;
  useEffect(() => {
    if (!fromHistory) return;
    let alive = true;
    setRestoring(true);
    void restoreSaved(fromHistory).finally(() => alive && setRestoring(false));
    return () => {
      alive = false;
    };
  }, [fromHistory, restoreSaved]);

  const root = current && !current.fromHistory ? current.root : null;
  const scanningHere = scanning?.root.toUpperCase() === driveRoot.toUpperCase();

  // A new scan (or another drive) starts the map over from the top.
  useEffect(() => {
    setPath(root);
    setSelected(null);
  }, [root, current?.scannedAt]);

  const changed = useCallback(() => setReload((n) => n + 1), []);

  /** Shows a found folder or file on the map: a folder opened, a file picked in its folder. */
  const show = useCallback((t: { path: string; isDir: boolean }) => {
    if (t.isDir) {
      setPath(t.path);
      setSelected(null);
    } else {
      setPath(parentOf(t.path));
      setSelected(itemKey({ kind: "file", path: t.path }));
    }
    setTab("map");
  }, []);

  // "Open on the map": folders anywhere; files from the other tabs (on the map they already are).
  const actions = useItemActions(changed, (t) => (!t.lumped && (t.isDir || tab !== "map") ? [openEntry(() => show(t), t.isDir)] : []));

  if (!root || !path) {
    return (
      <div className="page">
        <div className="page-head">
          <div>
            <h1 className="page-title">空间分析</h1>
            <div className="page-sub">用色块大小直观地看每个文件夹、每类文件占了多少</div>
          </div>
          {picker}
        </div>
        <div className="card empty">
          <div className="empty-icon">
            <ScanSearch size={30} />
          </div>
          {restoring ? (
            <div>正在读取上次的扫描结果…</div>
          ) : scanningHere ? (
            <div>
              正在扫描，已经看过 {formatCount(scanning?.progress?.files ?? 0)} 个文件…
              <div className="faint" style={{ fontSize: 13, marginTop: 4 }}>
                扫描完成后这里会显示空间地图
              </div>
            </div>
          ) : (
            <>
              <div>
                {current?.fromHistory ? "上次的扫描没有保存空间地图（旧版本扫描的），需要重新扫描一次" : `需要先扫描一次 ${driveRoot.replace("\\", "")}`}
                {scanning && (
                  <div className="faint" style={{ fontSize: 13, marginTop: 4 }}>
                    正在扫描 {scanning.root}，完成后才能开始下一个
                  </div>
                )}
              </div>
              <div style={{ display: "flex", gap: 10, justifyContent: "center" }}>
                <button className="btn btn-primary" disabled={!!scanning} onClick={() => startScan()}>
                  开始扫描
                </button>
                {canFast && (
                  <button className="btn" disabled={!!scanning} title="直接读取 NTFS 的主文件表，通常几秒就能扫完；需要管理员权限，Windows 会先弹窗询问" onClick={() => startScan(undefined, true)}>
                    <Zap size={15} /> 快速扫描（需管理员）
                  </button>
                )}
              </div>
            </>
          )}
        </div>
      </div>
    );
  }

  return (
    <div className="page space-page">
      <div className="page-head">
        <div>
          <h1 className="page-title">空间分析</h1>
          <div className="page-sub">
            扫描于 {formatDate(current!.scannedAt)}（{formatAgo(current!.scannedAt)}）· 点击色块进入文件夹，右键查看能做什么
          </div>
        </div>
        <div className="overview-actions">
          {picker}
          <div className="tabs" role="tablist">
          {(
            [
              ["map", "空间地图"],
              ["types", "文件类型"],
              ["stale", "很久没动"],
            ] as const
          ).map(([id, label]) => (
            <button key={id} role="tab" aria-selected={tab === id} className={`tab ${tab === id ? "active" : ""}`} onClick={() => setTab(id)}>
              {label}
            </button>
          ))}
          </div>
        </div>
      </div>

      {tab === "map" && (
        <MapTab
          root={root}
          path={path}
          setPath={(p) => {
            setPath(p);
            setSelected(null);
          }}
          selected={selected}
          setSelected={setSelected}
          reload={reload}
          onMenu={actions.open}
          onShow={show}
        />
      )}
      {tab === "types" && <SpaceTypes root={root} reload={reload} onShow={show} onMenu={(h, x, y) => actions.open(targetOf(h), x, y)} />}
      {tab === "stale" && <SpaceStale root={root} reload={reload} onShow={show} onMenu={(h, x, y) => actions.open(targetOf(h), x, y)} />}
      {actions.element}
    </div>
  );
}

interface MapTabProps {
  root: string;
  path: string;
  setPath: (p: string) => void;
  selected: string | null;
  setSelected: (k: string | null) => void;
  reload: number;
  onMenu: (t: ActionTarget, x: number, y: number) => void;
  onShow: (t: { path: string; isDir: boolean }) => void;
}

const AGES: [number, string][] = [
  [0, "修改时间不限"],
  [180, "半年没改过"],
  [365, "一年没改过"],
  [730, "两年没改过"],
];
const SIZES: [number, string][] = [
  [0, "大小不限"],
  [100 * MB, "100 MB 以上"],
  [GB, "1 GB 以上"],
  [10 * GB, "10 GB 以上"],
];

function MapTab({ root, path, setPath, selected, setSelected, reload, onMenu, onShow }: MapTabProps) {
  const [map, setMap] = useState<TreeItem | null>(null);
  const [list, setList] = useState<TreeItem | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [hovered, setHovered] = useState<TreeItem | null>(null);
  const [legendKind, setLegendKind] = useState<number | null>(null);
  const [text, setText] = useState("");
  const [showFilters, setShowFilters] = useState(false);
  const [kind, setKind] = useState<number | null>(null);
  const [minSize, setMinSize] = useState(0);
  const [age, setAge] = useState(0);
  const [hits, setHits] = useState<Hit[] | null>(null);

  useEffect(() => {
    let alive = true;
    setError(null);
    setMap(null);
    setList(null);
    // The map: three levels, leaving out what would be too small to see.
    api
      .getTree(path, 3, 0.0005, 60)
      .then((t) => alive && setMap(t))
      .catch((e) => alive && setError(String(e)));
    // The list: everything in this folder, one level.
    api
      .getTree(path, 1, 0, 500)
      .then((t) => alive && setList(t))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [path, reload]);

  const filtering = text.trim() !== "" || kind !== null || minSize > 0 || age > 0;
  useEffect(() => {
    if (!filtering) {
      setHits(null);
      return;
    }
    let alive = true;
    const t = setTimeout(() => {
      void api
        .searchScan(root, {
          text: text.trim(),
          kind,
          minSize,
          olderThan: age ? Math.floor(Date.now() / 1000) - age * 86400 : null,
          under: path,
        })
        .then((h) => alive && setHits(h))
        .catch(() => alive && setHits([]));
    }, 180);
    return () => {
      alive = false;
      clearTimeout(t);
    };
  }, [filtering, text, kind, minSize, age, path, root, reload]);

  const clearFilters = () => {
    setText("");
    setKind(null);
    setMinSize(0);
    setAge(0);
  };

  const crumbs = crumbsOf(root, path);
  const up = useCallback(() => {
    if (crumbs.length > 1) setPath(crumbs[crumbs.length - 2].path);
  }, [crumbs, setPath]);

  // Backspace or the mouse's back button goes up a level, as in File Explorer.
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement;
      if (t.closest("input, textarea, select, [contenteditable]")) return;
      if (e.key === "Backspace" || (e.altKey && e.key === "ArrowLeft")) {
        e.preventDefault();
        up();
      }
    };
    const mouse = (e: MouseEvent) => {
      if (e.button === 3) {
        e.preventDefault();
        up();
      }
    };
    window.addEventListener("keydown", key);
    window.addEventListener("mouseup", mouse);
    return () => {
      window.removeEventListener("keydown", key);
      window.removeEventListener("mouseup", mouse);
    };
  }, [up]);

  const open = useCallback(
    (item: TreeItem) => {
      if (item.kind === "dir" && item.hasChildren && !item.denied) setPath(item.path);
    },
    [setPath],
  );
  const select = useCallback((item: TreeItem) => setSelected(itemKey(item)), [setSelected]);
  const menu = useCallback((item: TreeItem, x: number, y: number) => onMenu(targetOf(item), x, y), [onMenu]);
  const hover = useCallback((item: TreeItem | null) => setHovered(item), []);

  const legend = useMemo(() => (map ? kindsOf(map.types) : []), [map]);
  const filterCount = (kind !== null ? 1 : 0) + (minSize ? 1 : 0) + (age ? 1 : 0);
  const here = crumbs[crumbs.length - 1].label;
  const shown = hovered ?? map;

  return (
    <>
      <div className="map-toolbar">
        <button className="icon-btn" disabled={crumbs.length <= 1} title="上一级（Backspace）" onClick={up}>
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
        <div className="map-tools">
          <label className="search-box">
            <Search size={15} />
            <input value={text} onChange={(e) => setText(e.target.value)} placeholder={`在 ${here} 里找名称`} />
            {text && (
              <button className="icon-btn tiny" onClick={() => setText("")} aria-label="清空">
                <X size={14} />
              </button>
            )}
          </label>
          <button className={`btn btn-sm ${showFilters || filterCount ? "btn-on" : ""}`} onClick={() => setShowFilters((v) => !v)}>
            <SlidersHorizontal size={14} /> 筛选{filterCount ? ` ${filterCount}` : ""}
          </button>
          <button className="icon-btn" title="在资源管理器中打开这个文件夹" onClick={() => void api.revealPath(path)}>
            <FolderOpen size={17} />
          </button>
        </div>
      </div>
      {showFilters && (
        <div className="map-filters">
          <select value={kind ?? ""} onChange={(e) => setKind(e.target.value === "" ? null : Number(e.target.value))}>
            <option value="">所有类型</option>
            {KINDS.map((k, i) => (
              <option key={i} value={i}>
                只看{k.label}
              </option>
            ))}
          </select>
          <select value={minSize} onChange={(e) => setMinSize(Number(e.target.value))}>
            {SIZES.map(([v, l]) => (
              <option key={v} value={v}>
                {l}
              </option>
            ))}
          </select>
          <select value={age} onChange={(e) => setAge(Number(e.target.value))}>
            {AGES.map(([v, l]) => (
              <option key={v} value={v}>
                {l}
              </option>
            ))}
          </select>
          {filtering && (
            <button className="btn btn-sm" onClick={clearFilters}>
              清除筛选
            </button>
          )}
          <span className="faint" style={{ fontSize: 12.5 }}>
            只能找到 1 MB 以上的文件夹和 4 MB 以上的文件
          </span>
        </div>
      )}

      <div className="space-body">
        <div className="card map-card">
          {map ? (
            map.children?.length ? (
              <SpaceMap root={map} hovered={hovered ? itemKey(hovered) : null} selected={selected} kind={legendKind} onHover={hover} onOpen={open} onSelect={select} onMenu={menu} />
            ) : (
              <div className="empty">这个文件夹是空的</div>
            )
          ) : error ? (
            <div className="empty">{error}</div>
          ) : (
            <div className="skeleton" style={{ flex: 1 }} />
          )}
          <div className="map-foot">
            <div className="map-legend" onMouseLeave={() => setLegendKind(null)}>
              {legend.map(([k, v]) => (
                <button
                  key={k}
                  className={`legend-chip ${kind === k ? "on" : ""}`}
                  onMouseEnter={() => setLegendKind(k)}
                  onClick={() => {
                    setKind(kind === k ? null : k);
                    setShowFilters(true);
                  }}
                  title={`点击列出这里最大的${KINDS[k].label}文件`}
                >
                  <span className="swatch" style={{ background: kindColor(k) }} />
                  {KINDS[k].short}
                  <span className="num faint">{percent(v, map!.size)}</span>
                </button>
              ))}
            </div>
            {shown && (
              <div className="map-status faint" title={shown.path}>
                <span className="num">{formatBytes(shown.size)}</span>
                {hovered && map && <span className="num"> · {percent(hovered.size, map.size)}</span>}
                {shown.kind !== "file" && shown.files > 0 && <span> · {formatCount(shown.files)} 个文件</span>}
                {shown.modified > 0 && <span> · {formatAgo(shown.modified)}修改</span>}
                <span className="map-status-path"> · {shown.kind === "rest" ? `${shown.path} 里的${shown.name}` : shown.path}</span>
              </div>
            )}
          </div>
        </div>

        <div className="card side-card">
          {filtering ? (
            <>
              <div className="side-head">
                <span>
                  找到 <b className="num">{hits?.length ?? "…"}</b> 项{hits && hits.length >= 300 ? "（只列最大的 300 项）" : ""}
                </span>
                <button className="btn btn-sm" onClick={clearFilters}>
                  清除
                </button>
              </div>
              {hits ? (
                <HitList hits={hits} onShow={onShow} onMenu={(h, x, y) => onMenu(targetOf(h), x, y)} empty="这里没有符合条件的文件夹或文件" />
              ) : (
                <div className="skeleton" style={{ height: 200 }} />
              )}
            </>
          ) : (
            <>
              <div className="side-head">
                <span>
                  {here} <span className="faint num">· {list ? formatBytes(list.size) : "…"}</span>
                </span>
              </div>
              {list ? (
                <TreeTable root={list} hovered={hovered ? itemKey(hovered) : null} selected={selected} onHover={hover} onOpen={open} onSelect={select} onMenu={menu} />
              ) : (
                <div className="skeleton" style={{ height: 200 }} />
              )}
            </>
          )}
        </div>
      </div>
    </>
  );
}
