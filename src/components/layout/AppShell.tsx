import { useEffect, useState } from "react";
import { Sidebar } from "./Sidebar";
import { TitleBar } from "./TitleBar";
import { SearchOverlay } from "@/components/search/SearchOverlay";
import { ItemList } from "@/components/items/ItemList";
import { Dashboard } from "@/components/stats/Dashboard";
import { useWindowStore, useTaskStore, useUIStore } from "@/stores";
import { listen } from "@tauri-apps/api/event";

export function AppShell() {
  const { refresh, detectDuplicates } = useWindowStore();
  const loadTasks = useTaskStore((s) => s.loadTasks);
  const { searchOpen, theme, setSearchOpen } = useUIStore();
  const [lastRefresh, setLastRefresh] = useState<string>("");

  // Apply theme
  useEffect(() => {
    document.documentElement.classList.toggle("dark", theme === "dark");
  }, [theme]);

  // Initial load
  useEffect(() => {
    doRefresh();
    loadTasks();

    // Listen for push events from backend
    const unlisteners: (() => void)[] = [];

    listen("windows-updated", () => {
      refresh();
      detectDuplicates();
      setLastRefresh(new Date().toLocaleTimeString());
    }).then((fn) => unlisteners.push(fn));

    listen("duplicates-detected", () => {
      detectDuplicates();
    }).then((fn) => unlisteners.push(fn));

    // Global keyboard shortcut: Ctrl+Shift+F
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.shiftKey && e.key === "F") {
        e.preventDefault();
        setSearchOpen(!useUIStore.getState().searchOpen);
      }
    };
    window.addEventListener("keydown", handleKeyDown);

    return () => {
      unlisteners.forEach((fn) => fn());
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, []);

  const doRefresh = async () => {
    await refresh();
    await detectDuplicates();
    const items = useWindowStore.getState().items;
    setLastRefresh(`${new Date().toLocaleTimeString()} (${items.length}项)`);
  };

  return (
    <div className="flex flex-col h-screen w-screen overflow-hidden bg-background">
      <TitleBar />
      <div className="flex flex-1 overflow-hidden">
        <Sidebar />
        <main className="flex-1 overflow-hidden flex flex-col">
          <Dashboard onRefresh={doRefresh} lastRefresh={lastRefresh} />
          <ItemList />
        </main>
      </div>
      {searchOpen && <SearchOverlay />}
    </div>
  );
}