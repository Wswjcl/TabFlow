import { useState, useEffect, useRef, useMemo } from "react";
import { useWindowStore, useUIStore } from "@/stores";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Search, X } from "lucide-react";
import { itemTypeIcon, browserIcon } from "@/stores/types";
import type { TrackedItem } from "@/stores/types";

export function SearchOverlay() {
  const { setSearchOpen } = useUIStore();
  const { items, focusWindow } = useWindowStore();
  const [query, setQuery] = useState("");
  const [selectedIndex, setSelectedIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  // Focus input on mount
  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  // Search results
  const results = useMemo(() => {
    if (!query.trim()) return items.slice(0, 20);
    const q = query.toLowerCase();
    return items
      .filter(
        (item) =>
          item.title.toLowerCase().includes(q) ||
          item.url?.toLowerCase().includes(q) ||
          item.path?.toLowerCase().includes(q) ||
          item.process_name.toLowerCase().includes(q)
      )
      .slice(0, 20);
  }, [items, query]);

  // Keyboard navigation
  useEffect(() => {
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setSearchOpen(false);
      } else if (e.key === "ArrowDown") {
        e.preventDefault();
        setSelectedIndex((i) => Math.min(i + 1, results.length - 1));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setSelectedIndex((i) => Math.max(i - 1, 0));
      } else if (e.key === "Enter") {
        e.preventDefault();
        if (results[selectedIndex]) {
          handleSelect(results[selectedIndex]);
        }
      }
    };

    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  }, [results, selectedIndex]);

  const handleSelect = (item: TrackedItem) => {
    focusWindow(item.id);
    setSearchOpen(false);
  };

  return (
    <div
      className="fixed inset-0 z-50 search-overlay-backdrop flex items-start justify-center pt-[15vh]"
      onClick={() => setSearchOpen(false)}
      ref={containerRef}
    >
      <div
        className="w-full max-w-lg bg-popover border rounded-xl shadow-2xl overflow-hidden"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Search input */}
        <div className="flex items-center gap-3 px-4 py-3 border-b">
          <Search className="w-5 h-5 text-muted-foreground shrink-0" />
          <Input
            ref={inputRef}
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setSelectedIndex(0);
            }}
            placeholder="搜索窗口、标签页、文件管理器..."
            className="border-0 shadow-none focus-visible:ring-0 text-base h-auto py-0"
          />
          <button
            onClick={() => setSearchOpen(false)}
            className="shrink-0 text-muted-foreground hover:text-foreground"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Results */}
        <ScrollArea className="max-h-[320px]">
          {results.length === 0 && (
            <div className="px-4 py-8 text-center text-sm text-muted-foreground">
              {query ? "未找到匹配结果" : "输入关键词搜索..."}
            </div>
          )}

          {results.map((item, idx) => {
            const icon =
              item.item_type === "browser_tab"
                ? browserIcon(item.browser_name)
                : itemTypeIcon(item.item_type);

            return (
              <button
                key={item.id}
                onClick={() => handleSelect(item)}
                className={`
                  w-full flex items-center gap-3 px-4 py-2.5 text-left transition-colors
                  ${idx === selectedIndex
                    ? "bg-accent text-accent-foreground"
                    : "hover:bg-accent/50"
                  }
                `}
              >
                <span className="text-sm shrink-0">{icon}</span>
                <div className="flex-1 min-w-0">
                  <div className="text-sm font-medium truncate">
                    {item.title}
                  </div>
                  <div className="text-xs text-muted-foreground truncate">
                    {item.url?.replace(/^title:/, "") ||
                      item.path ||
                      item.process_name}
                  </div>
                </div>
                <span className="text-[10px] text-muted-foreground/60 shrink-0">
                  {item.process_name}
                </span>
              </button>
            );
          })}
        </ScrollArea>

        {/* Footer hint */}
        <div className="flex items-center gap-4 px-4 py-2 border-t text-[10px] text-muted-foreground">
          <span>↑↓ 导航</span>
          <span>Enter 跳转</span>
          <span>Esc 关闭</span>
        </div>
      </div>
    </div>
  );
}