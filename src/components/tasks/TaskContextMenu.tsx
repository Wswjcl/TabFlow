import { useEffect } from "react";
import { useTaskStore } from "@/stores";
import type { TrackedItem } from "@/stores/types";
import { Check, FolderKanban } from "lucide-react";

interface Props {
  x: number;
  y: number;
  item: TrackedItem;
  onClose: () => void;
}

/** Right-click menu on an item: toggle which tasks track this resource. */
export function TaskContextMenu({ x, y, item, onClose }: Props) {
  const tasks = useTaskStore((s) => s.tasks);
  const assignToTask = useTaskStore((s) => s.assignToTask);
  const unassignFromTask = useTaskStore((s) => s.unassignFromTask);
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

  // Keep the menu inside the viewport
  const left = Math.min(x, window.innerWidth - 200);
  const top = Math.min(y, window.innerHeight - 40 - tasks.length * 32);

  return (
    <>
      <div
        className="fixed inset-0 z-40"
        onClick={onClose}
        onContextMenu={(e) => {
          e.preventDefault();
          onClose();
        }}
      />
      <div
        className="fixed z-50 min-w-[180px] bg-popover border rounded-md shadow-xl py-1 text-sm"
        style={{ left, top: Math.max(8, top) }}
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
      </div>
    </>
  );
}
