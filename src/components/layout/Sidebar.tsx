import React from "react";
import { useTaskStore, useUIStore, useWindowStore } from "@/stores";
import {
  FILTER_APP,
  FILTER_BROWSER,
  FILTER_EXPLORER,
  FILTER_IGNORED,
} from "@/stores/types";
import { TaskPanel } from "@/components/tasks/TaskPanel";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  LayoutDashboard,
  FolderKanban,
  Globe,
  FolderOpen,
  Monitor,
  EyeOff,
  PanelLeftClose,
  PanelLeft,
} from "lucide-react";

export function Sidebar() {
  const { sidebarCollapsed, toggleSidebar } = useUIStore();
  const { selectedTaskId, selectTask } = useTaskStore();
  const ignoredCount = useWindowStore((s) => s.ignored.length);

  if (sidebarCollapsed) {
    return (
      <div className="sidebar w-12 flex flex-col items-center py-3 gap-2 shrink-0 z-10">
        <Button
          variant="ghost"
          size="icon"
          onClick={toggleSidebar}
          className="h-8 w-8"
        >
          <PanelLeft className="w-4 h-4" />
        </Button>
      </div>
    );
  }

  return (
    <aside className="sidebar w-56 flex flex-col shrink-0 select-none z-10">
      {/* Header */}
      <div className="flex items-center justify-between px-3 py-3 border-b border-border/50">
        <span className="text-xs font-semibold text-muted-foreground uppercase tracking-wider">
          导航
        </span>
        <Button
          variant="ghost"
          size="icon"
          onClick={toggleSidebar}
          className="h-6 w-6 text-muted-foreground hover:text-foreground"
        >
          <PanelLeftClose className="w-3.5 h-3.5" />
        </Button>
      </div>

      <ScrollArea className="flex-1">
        {/* Main nav */}
        <div className="px-2 py-2 space-y-0.5">
          <SidebarItem
            icon={<LayoutDashboard className="w-4 h-4" />}
            label="概览"
            active={selectedTaskId === null}
            onClick={() => selectTask(null)}
          />
        </div>

        {/* Type filters */}
        <div className="px-2 py-2 border-t border-border/40">
          <div className="mb-1 px-2">
            <span className="text-[10px] font-semibold text-muted-foreground/60 uppercase tracking-widest">
              按类型筛选
            </span>
          </div>
          <SidebarItem
            icon={<Globe className="w-4 h-4" />}
            label="浏览器标签"
            active={selectedTaskId === FILTER_BROWSER}
            onClick={() => selectTask(FILTER_BROWSER)}
          />
          <SidebarItem
            icon={<FolderOpen className="w-4 h-4" />}
            label="文件管理器"
            active={selectedTaskId === FILTER_EXPLORER}
            onClick={() => selectTask(FILTER_EXPLORER)}
          />
          <SidebarItem
            icon={<Monitor className="w-4 h-4" />}
            label="应用窗口"
            active={selectedTaskId === FILTER_APP}
            onClick={() => selectTask(FILTER_APP)}
          />
          <SidebarItem
            icon={<EyeOff className="w-4 h-4" />}
            label="已忽略"
            badge={ignoredCount}
            active={selectedTaskId === FILTER_IGNORED}
            onClick={() => selectTask(FILTER_IGNORED)}
          />
        </div>

        {/* Tasks */}
        <div className="px-2 py-2 border-t border-border/40 flex-1">
          <div className="flex items-center justify-between px-2 mb-1">
            <span className="text-[10px] font-semibold text-muted-foreground/60 uppercase tracking-widest">
              Tasks
            </span>
            <FolderKanban className="w-3 h-3 text-muted-foreground/30" />
          </div>
          <TaskPanel />
        </div>
      </ScrollArea>
    </aside>
  );
}

function SidebarItem({
  icon,
  label,
  active,
  badge,
  onClick,
}: {
  icon: React.ReactNode;
  label: string;
  active?: boolean;
  badge?: number;
  onClick?: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className={`
        flex items-center gap-2.5 w-full px-2 py-1.5 rounded-md text-sm
        transition-all duration-150 cursor-pointer
        ${
          active
            ? "bg-accent text-accent-foreground font-medium shadow-sm"
            : "text-foreground/60 hover:bg-accent/60 hover:text-foreground"
        }
      `}
    >
      <span className="shrink-0">{icon}</span>
      <span className="flex-1 text-left truncate">{label}</span>
      {badge != null && badge > 0 && (
        <span className="shrink-0 text-[10px] px-1.5 py-0.5 rounded-full bg-foreground/10 text-foreground/60 font-medium min-w-[20px] text-center">
          {badge}
        </span>
      )}
    </button>
  );
}