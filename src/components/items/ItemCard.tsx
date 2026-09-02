import React, { useState } from "react";
import { useWindowStore, useTaskStore } from "@/stores";
import { Button } from "@/components/ui/button";
import {
  ExternalLink,
  X,
  Copy,
  Pencil,
} from "lucide-react";
import type { TrackedItem } from "@/stores/types";
import { itemTypeIcon, browserIcon } from "@/stores/types";
import { TaskContextMenu } from "@/components/tasks/TaskContextMenu";

interface ItemCardProps {
  item: TrackedItem;
}

export function ItemCard({ item }: ItemCardProps) {
  const { focusWindow, closeWindow } = useWindowStore();
  const { selectedTaskId, tasks } = useTaskStore();
  const [menuPos, setMenuPos] = useState<{ x: number; y: number } | null>(null);

  const handleClick = () => {
    focusWindow(item.id);
  };

  const handleClose = (e: React.MouseEvent) => {
    e.stopPropagation();
    closeWindow(item.id);
  };

  const handleCopyTitle = (e: React.MouseEvent) => {
    e.stopPropagation();
    navigator.clipboard.writeText(item.title);
  };

  const handleContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setMenuPos({ x: e.clientX, y: e.clientY });
  };

  // Try to extract domain from URL-like title
  const displayUrl = item.url
    ?.replace(/^page:/, "📄 ")
    ?.replace(/^title:/, "")
    ?.slice(0, 60);

  // The user's note replaces the live title as the display name
  const displayTitle = item.note?.trim() || item.title;

  const fallbackIcon =
    item.item_type === "browser_tab"
      ? browserIcon(item.browser_name)
      : itemTypeIcon(item.item_type);

  return (
    <div
      className={`
        group flex items-center gap-2 px-3 py-2 rounded-md
        transition-all duration-150 cursor-pointer
        hover:bg-accent/70
        ${selectedTaskId ? "border border-transparent" : ""}
      `}
      onClick={handleClick}
      onContextMenu={handleContextMenu}
      title="单击跳转 · 右键追踪到 Task"
    >
      {/* Icon: the app's real icon extracted from its exe, emoji as fallback */}
      {item.icon ? (
        <img
          src={item.icon}
          alt=""
          draggable={false}
          className="w-4 h-4 shrink-0 object-contain"
        />
      ) : (
        <span className="text-sm shrink-0">{fallbackIcon}</span>
      )}

      {/* Content */}
      <div className="flex-1 min-w-0">
        {/* Note replaces the title as display name; the original moves to
            the hover tooltip so the card keeps its normal 2-line height */}
        <div className="text-sm font-medium truncate flex items-center gap-1">
          {item.note && (
            <Pencil className="w-3 h-3 shrink-0 text-blue-500/80" />
          )}
          <span
            className={`truncate ${item.note ? "text-blue-600 dark:text-blue-400" : ""}`}
            title={item.note ? `原名称：${item.title}` : undefined}
          >
            {displayTitle}
          </span>
        </div>
        {displayUrl && (
          <div className="text-xs text-muted-foreground truncate">
            {displayUrl}
          </div>
        )}
        {item.path && (
          <div className="text-xs text-muted-foreground truncate">
            {item.path}
          </div>
        )}
      </div>

      {/* Item type badge */}
      <span className="text-[10px] text-muted-foreground/50 shrink-0 hidden sm:inline">
        {item.process_name}
      </span>

      {/* Assigned task dots */}
      {item.task_ids.length > 0 && (
        <span className="flex items-center gap-0.5 shrink-0">
          {item.task_ids.map((id) => {
            const t = tasks.find((tt) => tt.id === id);
            return t ? (
              <span
                key={id}
                className="w-2 h-2 rounded-full"
                style={{ backgroundColor: t.color }}
                title={t.name}
              />
            ) : null;
          })}
        </span>
      )}

      {/* Actions (visible on hover) */}
      <div className="flex items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity shrink-0">
        <Button
          variant="ghost"
          size="icon"
          className="h-6 w-6"
          onClick={handleCopyTitle}
          title="复制标题"
        >
          <Copy className="w-3 h-3" />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          className="h-6 w-6"
          onClick={handleClick}
          title="跳转到窗口"
        >
          <ExternalLink className="w-3 h-3" />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          className="h-6 w-6 hover:bg-destructive/20 hover:text-destructive"
          onClick={handleClose}
          title="关闭窗口"
        >
          <X className="w-3 h-3" />
        </Button>
      </div>

      {menuPos && (
        <TaskContextMenu
          x={menuPos.x}
          y={menuPos.y}
          item={item}
          onClose={() => setMenuPos(null)}
        />
      )}
    </div>
  );
}