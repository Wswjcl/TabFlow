import { useEffect, useState } from "react";
import { useTaskStore, useWindowStore } from "@/stores";
import type { TrackedItem } from "@/stores/types";
import { Check, EyeOff, FolderKanban, Pencil, X } from "lucide-react";
import { NoteDialog } from "@/components/items/NoteDialog";

interface Props {
  x: number;
  y: number;
  item: TrackedItem;
  onClose: () => void;
}

/** Right-click menu on an item: toggle which tasks track this resource,
 *  annotate (rename) it, or stop tracking it entirely. */
export function TaskContextMenu({ x, y, item, onClose }: Props) {
  const tasks = useTaskStore((s) => s.tasks);
  const assignToTask = useTaskStore((s) => s.assignToTask);
  const unassignFromTask = useTaskStore((s) => s.unassignFromTask);
  const ignoreItem = useWindowStore((s) => s.ignoreItem);
  const setResourceNote = useWindowStore((s) => s.setResourceNote);
  const [noteOpen, setNoteOpen] = useState(false);
  const assigned = new Set(item.task_ids);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const toggle = (taskId: string) => {
    if (assigned.has(taskId)) {
      unassignFromTask(item.id, taskId);
    } else {
      assignToTask(item.id, taskId);
    }
    onClose();
  };

  // The dialog replaces the menu; Escape inside it closes everything.
  if (noteOpen) {
    return <NoteDialog item={item} onClose={onClose} />;
  }

  // Keep the menu inside the viewport
  const left = Math.min(x, window.innerWidth - 200);
  const top = Math.min(y, window.innerHeight - 40 - tasks.length * 32);

  return (
    <>
      {/* Click-catcher: closes the menu. stopPropagation is essential —
          without it every click bubbles into the parent ItemCard's
          onClick and focuses/jumps to the tracked window. */}
      <div
        className="fixed inset-0 z-40"
        onClick={(e) => {
          e.stopPropagation();
          onClose();
        }}
        onContextMenu={(e) => {
          e.preventDefault();
          e.stopPropagation();
          onClose();
        }}
      />
      <div
        className="fixed z-50 min-w-[180px] bg-popover border rounded-md shadow-xl py-1 text-sm"
        style={{ left, top: Math.max(8, top) }}
        onClick={(e) => e.stopPropagation()}
        onContextMenu={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-1.5 px-3 py-1.5 text-[10px] font-semibold text-muted-foreground/60 uppercase tracking-wider">
          <FolderKanban className="w-3 h-3" />
          追踪到 Task
        </div>
        {tasks.length === 0 && (
          <div className="px-3 py-2 text-xs text-muted-foreground">
            还没有 Task，在左侧栏「新建 Task」
          </div>
        )}
        {tasks.map((t) => (
          <button
            key={t.id}
            onClick={() => toggle(t.id)}
            className="flex items-center gap-2 w-full px-3 py-1.5 hover:bg-accent transition-colors text-left"
          >
            <span
              className="w-2.5 h-2.5 rounded-full shrink-0"
              style={{ backgroundColor: t.color }}
            />
            <span className="flex-1 truncate">{t.name}</span>
            {assigned.has(t.id) && (
              <Check className="w-3.5 h-3.5 text-green-500 shrink-0" />
            )}
          </button>
        ))}

        {/* Annotate: custom display name for this resource */}
        <div className="my-1 border-t border-border/60" />
        <button
          onClick={() => setNoteOpen(true)}
          className="flex items-center gap-2 w-full px-3 py-1.5 hover:bg-accent transition-colors text-left"
          title="给此页面起一个备注名，显示时替代原标题"
        >
          <Pencil className="w-3.5 h-3.5 text-muted-foreground shrink-0" />
          <span className="flex-1">{item.note ? "编辑备注" : "备注 / 重命名"}</span>
        </button>
        {item.note && (
          <button
            onClick={() => {
              setResourceNote(item.id, "");
              onClose();
            }}
            className="flex items-center gap-2 w-full px-3 py-1.5 hover:bg-accent transition-colors text-left"
          >
            <X className="w-3.5 h-3.5 text-muted-foreground shrink-0" />
            <span className="flex-1">清除备注</span>
          </button>
        )}

        {/* Ignore tracking: removes this resource from the app entirely */}
        <div className="my-1 border-t border-border/60" />
        <button
          onClick={() => {
            ignoreItem(item.id);
            onClose();
          }}
          className="flex items-center gap-2 w-full px-3 py-1.5 hover:bg-accent transition-colors text-left"
          title="不再追踪此页面（可在侧边栏「已忽略」中恢复）"
        >
          <EyeOff className="w-3.5 h-3.5 text-muted-foreground shrink-0" />
          <span className="flex-1">忽略追踪</span>
        </button>
      </div>
    </>
  );
}
