import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Search, Moon, Sun, Minus, Square, Copy, X } from "lucide-react";
import { useUIStore } from "@/stores";

export function TitleBar() {
  const { theme, toggleTheme, setSearchOpen } = useUIStore();
  const [maximized, setMaximized] = useState(false);
  const appWindow = getCurrentWindow();

  // Track maximize state to swap the maximize/restore icon.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    appWindow.isMaximized().then(setMaximized);
    appWindow
      .onResized(() => appWindow.isMaximized().then(setMaximized))
      .then((fn) => {
        unlisten = fn;
      });
    return () => unlisten?.();
  }, []);

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

      {/* Right: theme toggle + window controls (decorations are off, this
          bar is the window's only title bar) */}
      <div className="flex items-center">
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

        <div className="flex items-center ml-1 -mr-3 -my-0 border-l border-border">
          <button
            onClick={() => appWindow.minimize()}
            className="h-9 w-11 inline-flex items-center justify-center text-muted-foreground hover:bg-accent hover:text-foreground transition-colors"
            title="最小化"
          >
            <Minus className="w-3.5 h-3.5" />
          </button>
          <button
            onClick={() => appWindow.toggleMaximize()}
            className="h-9 w-11 inline-flex items-center justify-center text-muted-foreground hover:bg-accent hover:text-foreground transition-colors"
            title={maximized ? "还原" : "最大化"}
          >
            {maximized ? (
              <Copy className="w-3 h-3 -scale-x-100" />
            ) : (
              <Square className="w-3 h-3" />
            )}
          </button>
          <button
            onClick={() => appWindow.close()}
            className="h-9 w-11 inline-flex items-center justify-center text-muted-foreground hover:bg-destructive hover:text-white transition-colors"
            title="关闭"
          >
            <X className="w-4 h-4" />
          </button>
        </div>
      </div>
    </div>
  );
}
