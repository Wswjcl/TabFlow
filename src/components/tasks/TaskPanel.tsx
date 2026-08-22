import { useState } from "react";
import { useTaskStore } from "@/stores";
import { Button } from "@/components/ui/button";
import { Dialog, DialogHeader, DialogTitle, DialogClose } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Plus, Trash2 } from "lucide-react";
import { TASK_COLORS } from "@/stores/types";

export function TaskPanel() {
  const { tasks, selectedTaskId, selectTask, createTask, deleteTask } =
    useTaskStore();
  const [dialogOpen, setDialogOpen] = useState(false);
  const [newName, setNewName] = useState("");
  const [newColor, setNewColor] = useState(TASK_COLORS[0]);

  const handleCreate = async () => {
    if (!newName.trim()) return;
    await createTask(newName.trim(), newColor);
    setNewName("");
    setNewColor(TASK_COLORS[0]);
    setDialogOpen(false);
  };

  return (
    <div className="space-y-0.5">
      {tasks.map((task) => (
        <div
          key={task.id}
          role="button"
          tabIndex={0}
          onClick={() => selectTask(selectedTaskId === task.id ? null : task.id)}
          onKeyDown={(e) => {
            if (e.key === "Enter" || e.key === " ") {
              e.preventDefault();
              selectTask(selectedTaskId === task.id ? null : task.id);
            }
          }}
          className={`
            group flex items-center gap-2.5 w-full px-2 py-1.5 rounded-md text-sm
            transition-all duration-150 cursor-pointer
            ${
              selectedTaskId === task.id
                ? "bg-accent text-accent-foreground font-medium shadow-sm"
                : "text-foreground/60 hover:bg-accent/60 hover:text-foreground"
            }
          `}
        >
          {/* Color dot */}
          <span
            className="w-2.5 h-2.5 rounded-full shrink-0"
            style={{ backgroundColor: task.color }}
          />

          <span className="flex-1 text-left truncate">{task.name}</span>

          {task.item_count != null && task.item_count > 0 && (
            <span className="text-[10px] text-foreground/40 shrink-0">
              {task.item_count}
            </span>
          )}

          <button
            onClick={(e) => {
              e.stopPropagation();
              deleteTask(task.id);
            }}
            title="删除 Task"
            className="opacity-0 group-hover:opacity-100 hover:text-destructive transition-all shrink-0"
          >
            <Trash2 className="w-3 h-3" />
          </button>
        </div>
      ))}

      {/* Add task button */}
      <button
        onClick={() => setDialogOpen(true)}
        className="flex items-center gap-2.5 w-full px-2 py-1.5 rounded-md text-sm text-foreground/40 hover:bg-accent/40 hover:text-foreground/70 transition-all cursor-pointer"
      >
        <Plus className="w-3.5 h-3.5" />
        <span>新建 Task</span>
      </button>

      {/* Create task dialog */}
      <Dialog open={dialogOpen} onOpenChange={setDialogOpen}>
        <DialogHeader>
          <DialogTitle>新建 Task</DialogTitle>
        </DialogHeader>
        <DialogClose />

        <div className="space-y-3 mt-2">
          <Input
            placeholder="Task 名称"
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && handleCreate()}
          />

          <div>
            <div className="text-xs text-muted-foreground mb-2">颜色</div>
            <div className="flex flex-wrap gap-2">
              {TASK_COLORS.map((color) => (
                <button
                  key={color}
                  onClick={() => setNewColor(color)}
                  className={`
                    w-7 h-7 rounded-full transition-all cursor-pointer
                    ${newColor === color ? "ring-2 ring-offset-2 ring-ring scale-110" : "hover:scale-105"}
                  `}
                  style={{ backgroundColor: color }}
                />
              ))}
            </div>
          </div>

          <Button className="w-full" onClick={handleCreate}>
            创建
          </Button>
        </div>
      </Dialog>
    </div>
  );
}