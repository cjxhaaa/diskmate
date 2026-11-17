import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { Copy, FolderInput, FolderOpen, HardDriveUpload, LoaderCircle, Trash2 } from "lucide-react";
import { api, type MoveOutcome, type PathActions } from "../api";
import { failNote, formatBytes } from "../format";
import { useStore } from "../store";
import { DeleteDialog } from "./DeleteDialog";
import { MoveDialog } from "./MoveDialog";

export interface MenuEntry {
  label: string;
  icon?: ReactNode;
  onClick?: () => void;
  disabled?: boolean;
  danger?: boolean;
}

/** A menu at the pointer: closes on a pick, a click elsewhere, Esc, or scrolling. */
export function ContextMenu({ x, y, title, entries, note, onClose }: { x: number; y: number; title: string; entries: MenuEntry[]; note?: ReactNode; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x, y });

  // Kept inside the window: flipped up or left when it would run off the edge.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const w = el.offsetWidth;
    const h = el.offsetHeight;
    setPos({
      x: x + w + 8 > window.innerWidth ? Math.max(8, x - w) : x,
      y: y + h + 8 > window.innerHeight ? Math.max(8, y - h) : y,
    });
  }, [x, y, entries.length, note]);

  useEffect(() => {
    // A right-click elsewhere closes this one when its own menu event comes (which may open the
    // next menu); its button press alone does not, since that can arrive after the menu opened.
    const away = (e: MouseEvent) => {
      if (e.type === "mousedown" && e.button === 2) return;
      if (!ref.current?.contains(e.target as Node)) onClose();
    };
    const key = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("mousedown", away, true);
    window.addEventListener("contextmenu", away, true);
    window.addEventListener("keydown", key);
    window.addEventListener("wheel", onClose, { passive: true });
    window.addEventListener("blur", onClose);
    return () => {
      window.removeEventListener("mousedown", away, true);
      window.removeEventListener("contextmenu", away, true);
      window.removeEventListener("keydown", key);
      window.removeEventListener("wheel", onClose);
      window.removeEventListener("blur", onClose);
    };
  }, [onClose]);

  return (
    <div className="ctx-menu" ref={ref} style={{ left: pos.x, top: pos.y }} role="menu" onContextMenu={(e) => e.preventDefault()}>
      <div className="ctx-title" title={title}>
        {title}
      </div>
      {entries.map((e) => (
        <button
          key={e.label}
          role="menuitem"
          className={`ctx-item ${e.danger ? "danger" : ""}`}
          disabled={e.disabled}
          onClick={() => {
            onClose();
            e.onClick?.();
          }}
        >
          {e.icon}
          <span>{e.label}</span>
        </button>
      ))}
      {note && <div className="ctx-note">{note}</div>}
    </div>
  );
}

/** Something on the map, in a list of found folders or files, that can be acted on. */
export interface ActionTarget {
  path: string;
  name: string;
  size: number;
  isDir: boolean;
  /** A folder's leftovers: only its folder can be shown. */
  lumped?: boolean;
}

/**
 * The right-click menu for folders and files from a scan, with what it leads to: show in File
 * Explorer, copy the path, move to another drive, delete. Moving and deleting follow the same rules
 * as the large files page (only the user's own files, never app data or system files): the menu
 * asks the backend first, and says why when it cannot. What leaves is taken out of the stored scan,
 * and `onChanged` tells the page to read it again.
 */
export function useItemActions(onChanged: () => void, extra?: (t: ActionTarget) => MenuEntry[]) {
  const { toast, refreshDrives, drives } = useStore();
  const [menu, setMenu] = useState<{ target: ActionTarget; x: number; y: number; actions: PathActions | null } | null>(null);
  const [deleting, setDeleting] = useState<ActionTarget | null>(null);
  const [moving, setMoving] = useState<ActionTarget | null>(null);

  const open = (target: ActionTarget, x: number, y: number) => {
    setMenu({ target, x, y, actions: null });
    if (!target.lumped)
      void api
        .pathActions(target.path)
        .then((actions) => setMenu((m) => (m && m.target.path === target.path ? { ...m, actions } : m)))
        .catch(() => setMenu((m) => (m ? { ...m, actions: { allowed: false, reason: "没能确认这一项能不能处理" } } : m)));
  };

  const gone = (paths: string[]) => {
    if (!paths.length) return;
    void api.forgetPaths(paths).then(onChanged);
  };

  const remove = async (target: ActionTarget, permanent: boolean) => {
    setDeleting(null);
    try {
      const out = await api.deleteFiles([target.path], permanent);
      toast(`${permanent ? "已删除" : "已移到回收站"}：${formatBytes(out.freed)}${failNote(out)}`);
      if (!out.skippedFiles) gone([target.path]);
      void refreshDrives();
    } catch (e) {
      toast(`删除失败：${String(e)}`);
    }
  };

  const moved = (out: MoveOutcome, drive: string) => {
    setMoving(null);
    toast(`已移到 ${drive.replace("\\", "")}：${formatBytes(out.freed)}${failNote(out)}`);
    gone(out.moved);
    void refreshDrives();
  };

  let element: ReactNode = null;
  if (menu) {
    const { target, actions } = menu;
    const checking = !target.lumped && !actions;
    const allowed = !!actions?.allowed;
    const otherDrive = drives.some((d) => d.root.toUpperCase() !== target.path.slice(0, 3).toUpperCase());
    const entries: MenuEntry[] = [
      ...(extra?.(target) ?? []),
      {
        label: target.isDir ? "在资源管理器中打开" : "在资源管理器中显示",
        icon: <FolderOpen size={15} />,
        onClick: () => void api.revealPath(target.path).catch((e) => toast(String(e))),
      },
      {
        label: "复制路径",
        icon: <Copy size={15} />,
        onClick: () =>
          void navigator.clipboard.writeText(target.path).then(
            () => toast("已复制路径"),
            () => toast("没能复制"),
          ),
      },
    ];
    if (!target.lumped) {
      entries.push(
        {
          label: "移到其他盘…",
          icon: <HardDriveUpload size={15} />,
          disabled: !allowed || !otherDrive,
          onClick: () => setMoving(target),
        },
        {
          label: target.isDir ? "删除文件夹…" : "删除…",
          icon: <Trash2 size={15} />,
          disabled: !allowed,
          danger: true,
          onClick: () => setDeleting(target),
        },
      );
    }
    const note = checking ? (
      <span className="faint">
        <LoaderCircle size={12} className="spin" /> 正在确认能不能删除或移动…
      </span>
    ) : target.lumped ? (
      "这里是多个小文件和小文件夹合在一起，打开所在文件夹后可以逐个处理。"
    ) : !allowed ? (
      actions?.reason
    ) : !otherDrive ? (
      "只有一块磁盘，没有地方可以移过去。"
    ) : null;
    element = <ContextMenu x={menu.x} y={menu.y} title={target.name} entries={entries} note={note} onClose={() => setMenu(null)} />;
  }

  return {
    open,
    element: (
      <>
        {element}
        {deleting && (
          <DeleteDialog
            items={[{ name: deleting.name, size: deleting.size }]}
            note={deleting.isDir ? "整个文件夹连同里面的所有文件一起删除。" : undefined}
            onCancel={() => setDeleting(null)}
            onConfirm={(permanent) => void remove(deleting, permanent)}
          />
        )}
        {moving && <MoveDialog items={[{ path: moving.path, name: moving.name, size: moving.size }]} onCancel={() => setMoving(null)} onDone={moved} />}
      </>
    ),
  };
}

/** The "go into this folder on the map" entry the map and lists add to the menu. */
export const openEntry = (onClick: () => void, isDir: boolean): MenuEntry => ({
  label: isDir ? "在空间地图里打开" : "在空间地图里查看",
  icon: <FolderInput size={15} />,
  onClick,
});
