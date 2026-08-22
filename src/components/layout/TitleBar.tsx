import { useUIStore } from "@/stores";
import { Search, Moon, Sun } from "lucide-react";

export function TitleBar() {
  const { theme, toggleTheme, setSearchOpen } = useUIStore();

  return (
    <div
      data-tauri-drag-region
      className="flex items-center justify-between h-9 px-3 bg-card border-b select-none shrink-0"
    >
      {/* Left: App name */}
      <div className="flex items-center gap-2 text-sm font-medium text-muted-foreground">
        <span className="text-foreground font-semibold">TabFlow</span>
      </div>

      {/* Center: Search trigger */}
      <button
        onClick={() => setSearchOpen(true)}
        className="flex items-center gap-2 px-4 py-1 text-xs text-muted-foreground bg-muted rounded-md hover:bg-accent hover:text-foreground transition-colors mx-4 flex-1 max-w-md"
      >
        <Search className="w-3.5 h-3.5" />
        <span>搜索窗口和标签页...</span>
        <kbd className="ml-auto text-[10px] px-1.5 py-0.5 rounded bg-background border text-muted-foreground">
          Ctrl+Shift+F
        </kbd>
      </button>

      {/* Right: Theme toggle */}
      <button
        onClick={toggleTheme}
        className="h-7 w-7 inline-flex items-center justify-center rounded hover:bg-accent text-muted-foreground hover:text-foreground transition-colors"
        title={theme === "dark" ? "亮色模式" : "暗色模式"}
      >
        {theme === "dark" ? (
          <Sun className="w-3.5 h-3.5" />
        ) : (
          <Moon className="w-3.5 h-3.5" />
        )}
      </button>
    </div>
  );
}