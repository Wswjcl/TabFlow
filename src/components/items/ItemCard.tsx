import React from "react";
import { useWindowStore, useTaskStore } from "@/stores";
import { Button } from "@/components/ui/button";
import {
  ExternalLink,
  X,
  Copy,
} from "lucide-react";
import type { TrackedItem } from "@/stores/types";
import { itemTypeIcon, browserIcon } from "@/stores/types";

interface ItemCardProps {
  item: TrackedItem;
}

export function ItemCard({ item }: ItemCardProps) {
  const { focusWindow, closeWindow } = useWindowStore();
  const { selectedTaskId } = useTaskStore();

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

  // Try to extract domain from URL-like title
  const displayUrl = item.url
    ?.replace(/^page:/, "📄 ")
    ?.replace(/^title:/, "")
    ?.slice(0, 60);

  const icon =
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
    >
      {/* Icon */}
      <span className="text-sm shrink-0">{icon}</span>

      {/* Content */}
      <div className="flex-1 min-w-0">
        <div className="text-sm font-medium truncate">{item.title}</div>
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
    </div>
  );
}