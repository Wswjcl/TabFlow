import { useMemo } from "react";
import { useWindowStore, useTaskStore } from "@/stores";
import { FILTER_APP, FILTER_BROWSER, FILTER_EXPLORER, FILTER_IGNORED } from "@/stores/types";
import { ItemCard } from "./ItemCard";
import { DuplicateGroup } from "./DuplicateGroup";
import { IgnoredList } from "./IgnoredList";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Button } from "@/components/ui/button";
import { Sparkles, AlertCircle, Loader2 } from "lucide-react";

export function ItemList() {
  const { items, duplicates, loading, error, closeDuplicates } = useWindowStore();
  const { selectedTaskId, taskItems } = useTaskStore();

  // Filter by selected task/type
  const filtered = useMemo(() => {
    if (selectedTaskId === FILTER_BROWSER) {
      return items.filter((i) => i.item_type === "browser_tab");
    }
    if (selectedTaskId === FILTER_EXPLORER) {
      return items.filter((i) => i.item_type === "explorer_window");
    }
    if (selectedTaskId === FILTER_APP) {
      return items.filter((i) => i.item_type === "app_window");
    }
    if (selectedTaskId) {
      // Real task: show its assigned live items
      const ids = new Set(taskItems.map((i) => i.id));
      return items.filter((i) => ids.has(i.id));
    }
    return items;
  }, [items, selectedTaskId, taskItems]);

  // Duplicate warnings follow the active view's scope: the overview shows
  // every group, type filters show only their own item type's groups, and
  // task views stay clean lists (their resources are already deduped to one
  // row per key by the backend).
  const scopedDuplicates = useMemo(() => {
    if (selectedTaskId === null) return duplicates;
    const typeFilter =
      selectedTaskId === FILTER_BROWSER
        ? "browser_tab"
        : selectedTaskId === FILTER_EXPLORER
          ? "explorer_window"
          : selectedTaskId === FILTER_APP
            ? "app_window"
            : null;
    if (typeFilter === null) return [];
    return duplicates.filter((g) =>
      g.items.every((i) => i.item_type === typeFilter)
    );
  }, [duplicates, selectedTaskId]);

  // Items that are part of duplicate groups
  const dupItemIds = useMemo(() => {
    const ids = new Set<string>();
    for (const g of scopedDuplicates) {
      for (const item of g.items) {
        ids.add(item.id);
      }
    }
    return ids;
  }, [scopedDuplicates]);

  const standaloneItems = useMemo(
    () => filtered.filter((i) => !dupItemIds.has(i.id)),
    [filtered, dupItemIds]
  );

  const handleCloseAll = async () => {
    const allGroupIds = scopedDuplicates.flatMap((g) => [g.id, g.match_pattern]);
    await closeDuplicates(allGroupIds);
  };

  // The ignore-list management view replaces the item list entirely.
  // (Kept below every hook: an early return above them would break the
  // Rules of Hooks and white-screen the app on view switches.)
  if (selectedTaskId === FILTER_IGNORED) {
    return <IgnoredList />;
  }

  // ── Loading ──
  if (loading && items.length === 0) {
    return (
      <div className="flex-1 flex items-center justify-center text-muted-foreground gap-2">
        <Loader2 className="w-5 h-5 animate-spin" />
        <span className="text-sm">正在扫描窗口...</span>
      </div>
    );
  }

  // ── Error ──
  if (error && items.length === 0) {
    return (
      <div className="flex-1 flex flex-col items-center justify-center text-muted-foreground gap-2">
        <AlertCircle className="w-8 h-8 text-destructive/60" />
        <span className="text-sm">数据加载失败</span>
        <span className="text-xs text-destructive/60 max-w-xs text-center">
          {error}
        </span>
        <Button
          variant="outline"
          size="sm"
          onClick={() => useWindowStore.getState().refresh()}
        >
          重试
        </Button>
      </div>
    );
  }

  // ── Empty ──
  if (!loading && filtered.length === 0 && scopedDuplicates.length === 0) {
    return (
      <div className="flex-1 flex flex-col items-center justify-center text-muted-foreground gap-2 p-8">
        <Sparkles className="w-8 h-8 opacity-30" />
        <span className="text-sm">暂无追踪的窗口</span>
        <span className="text-xs opacity-50 text-center max-w-xs">
          打开一些浏览器标签或文件夹窗口，稍等几秒后会自动出现
        </span>
        <span className="text-[10px] opacity-30 mt-2">
          点击「刷新」手动检查 · 当前 {items.length} 项 · {duplicates.length} 组重复
        </span>
      </div>
    );
  }

  return (
    <ScrollArea className="flex-1 p-4">
      {/* Duplicates section, scoped to the active view */}
      {scopedDuplicates.length > 0 && (
        <div className="mb-6">
          <div className="flex items-center justify-between mb-3">
            <div className="flex items-center gap-2">
              <span className="text-sm font-semibold text-destructive">
                {scopedDuplicates.length} 组重复
              </span>
              <span className="text-xs text-muted-foreground">
                ({scopedDuplicates.reduce((s, g) => s + g.items.length, 0)} 个窗口)
              </span>
            </div>
            <Button variant="destructive" size="sm" onClick={handleCloseAll}>
              一键关闭所有重复
            </Button>
          </div>

          <div className="space-y-3">
            {scopedDuplicates.map((group) => (
              <DuplicateGroup key={group.id} group={group} />
            ))}
          </div>
        </div>
      )}

      {/* Standalone items */}
      {standaloneItems.length > 0 && (
        <div>
          <div className="flex items-center gap-2 mb-3">
            <span className="text-sm font-semibold text-muted-foreground">
              所有窗口 ({standaloneItems.length})
            </span>
          </div>

          <div className="space-y-1">
            {standaloneItems.map((item) => (
              <ItemCard key={item.id} item={item} />
            ))}
          </div>
        </div>
      )}

      {/* If every item of the view sits inside a duplicate group */}
      {standaloneItems.length === 0 && scopedDuplicates.length > 0 && (
        <div className="text-center text-xs text-muted-foreground py-8">
          当前筛选下无独立窗口（所有项都在重复组中）
        </div>
      )}

      {/* Debug info */}
      <div className="mt-4 text-[10px] text-muted-foreground/40 text-right">
        共 {items.length} 项 · {duplicates.length} 组重复 ·{" "}
        {selectedTaskId ? "筛选模式" : "全部"}
      </div>
    </ScrollArea>
  );
}