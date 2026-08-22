import { create } from "zustand";
import type { TrackedItem, DuplicateGroup, Task, Stats } from "./types";
import { invoke } from "@tauri-apps/api/core";

// ─── Pending Removals ──────────────────────────────
// When an item is being optimistically removed (close), we track its ID here.
// Any refresh() that runs while the close is in-flight will filter out these
// IDs so the item doesn't reappear from a CDP scan that hasn't caught up yet.
const pendingRemovals = new Set<string>();

// ─── Window Store ───────────────────────────────────
interface WindowState {
  items: TrackedItem[];
  duplicates: DuplicateGroup[];
  stats: Stats | null;
  loading: boolean;
  error: string | null;

  refresh: (silent?: boolean) => Promise<void>;
  detectDuplicates: () => Promise<void>;
  closeDuplicates: (groupIds: string[], keepIndices?: number[]) => number;
  focusWindow: (itemId: string) => Promise<void>;
  closeWindow: (itemId: string) => void;
}

export const useWindowStore = create<WindowState>((set, get) => ({
  items: [],
  duplicates: [],
  stats: null,
  loading: false,
  error: null,

  refresh: async (silent = false) => {
    if (!silent) set({ loading: true, error: null });
    try {
      const items = await invoke<TrackedItem[]>("get_tracked_items");
      // Filter out items that are in the process of being closed
      // (CDP scan may still return them before the close takes effect)
      const visible = items.filter((it) => !pendingRemovals.has(it.id));
      set({ items: visible, loading: false });
    } catch (e) {
      const msg = String(e);
      console.error("[TabFlow] refresh failed:", msg);
      set({ error: msg, loading: false });
    }

    try {
      const stats = await invoke<Stats>("get_stats");
      set({ stats });
    } catch (_) {}
  },

  detectDuplicates: async () => {
    try {
      const duplicates = await invoke<DuplicateGroup[]>("detect_duplicates");
      set({ duplicates });
    } catch (e) {
      console.error("[TabFlow] detectDuplicates failed:", e);
    }
  },

  closeDuplicates: (groupIds, keepIndices) => {
    const matchedGroups = get().duplicates.filter((g) =>
      groupIds.includes(g.id) || groupIds.includes(g.match_pattern)
    );
    const keepIdx = keepIndices?.[0] ?? 0;
    const removedIds: string[] = [];
    for (const g of matchedGroups) {
      for (let i = 0; i < g.items.length; i++) {
        if (i !== keepIdx) {
          removedIds.push(g.items[i].id);
          pendingRemovals.add(g.items[i].id);
        }
      }
    }

    // Optimistic update: remove items and groups immediately
    const removedSet = new Set(removedIds);
    set({
      items: get().items.filter((it) => !removedSet.has(it.id)),
      duplicates: get().duplicates.filter(
        (g) => !groupIds.includes(g.id) && !groupIds.includes(g.match_pattern)
      ),
    });

    // Fire-and-forget: close on backend, don't await
    invoke<number>("close_duplicates", { groupIds, keepIndices })
      .then(() => {
        // Delay clearing pendingRemovals so CDP close has time to take effect
        setTimeout(() => {
          for (const id of removedIds) pendingRemovals.delete(id);
          get().refresh(true);
          get().detectDuplicates();
        }, 1200);
      })
      .catch((e) => {
        console.error("[TabFlow] closeDuplicates failed:", e);
        // Rollback: clear pending and do full refresh
        for (const id of removedIds) pendingRemovals.delete(id);
        set({ error: `关闭重复项失败: ${e}` });
        get().refresh();
        get().detectDuplicates();
      });

    return matchedGroups.reduce((sum, g) => sum + g.items.length - 1, 0);
  },

  focusWindow: async (itemId) => {
    try {
      await invoke("focus_window", { itemId });
    } catch (e) {
      console.error("focus failed:", e);
    }
  },

  closeWindow: (itemId) => {
    // Track this item as pending removal
    pendingRemovals.add(itemId);

    // Snapshot for rollback
    const removedItem = get().items.find((it) => it.id === itemId);

    // Optimistic update: remove item from list immediately
    set({
      items: get().items.filter((it) => it.id !== itemId),
      duplicates: get().duplicates
        .map((g) => ({
          ...g,
          items: g.items.filter((it) => it.id !== itemId),
          count: g.items.filter((it) => it.id !== itemId).length,
        }))
        .filter((g) => g.items.length > 1),
    });

    // Fire-and-forget: close on backend, don't await
    invoke("close_window", { itemId })
      .then(() => {
        // Delay clearing pendingRemovals so CDP close has time to take effect
        setTimeout(() => {
          pendingRemovals.delete(itemId);
          get().refresh(true);
        }, 1200);
      })
      .catch((e) => {
        console.error("[TabFlow] closeWindow failed:", e);
        // Rollback: allow the item back
        pendingRemovals.delete(itemId);
        if (removedItem) {
          const currentItems = get().items;
          if (!currentItems.some((it) => it.id === itemId)) {
            set({ items: [...currentItems, removedItem] });
          }
        }
        set({ error: `关闭失败: ${removedItem?.title ?? itemId}` });
        get().refresh();
        get().detectDuplicates();
      });
  },
}));

// ─── Task Store ─────────────────────────────────────
interface TaskState {
  tasks: Task[];
  selectedTaskId: string | null;

  loadTasks: () => Promise<void>;
  selectTask: (taskId: string | null) => void;
  createTask: (name: string, color: string) => Promise<void>;
  updateTask: (id: string, name: string, color: string) => Promise<void>;
  deleteTask: (id: string) => Promise<void>;
  assignToTask: (itemId: string, taskId: string) => Promise<void>;
  unassignFromTask: (itemId: string, taskId: string) => Promise<void>;
}

export const useTaskStore = create<TaskState>((set, get) => ({
  tasks: [],
  selectedTaskId: null,

  loadTasks: async () => {
    try {
      const tasks = await invoke<Task[]>("get_all_tasks");
      set({ tasks });
    } catch (e) {
      console.error("Failed to load tasks:", e);
    }
  },

  selectTask: (taskId) => set({ selectedTaskId: taskId }),

  createTask: async (name, color) => {
    try {
      await invoke("create_task", { name, color });
      await get().loadTasks();
    } catch (e) {
      console.error("Failed to create task:", e);
    }
  },

  updateTask: async (id, name, color) => {
    try {
      await invoke("update_task", { id, name, color });
      await get().loadTasks();
    } catch (e) {
      console.error("Failed to update task:", e);
    }
  },

  deleteTask: async (id) => {
    try {
      await invoke("delete_task", { id });
      if (get().selectedTaskId === id) {
        set({ selectedTaskId: null });
      }
      await get().loadTasks();
    } catch (e) {
      console.error("Failed to delete task:", e);
    }
  },

  assignToTask: async (itemId, taskId) => {
    try {
      await invoke("assign_item_to_task", { itemId, taskId });
      await get().loadTasks();
      await useWindowStore.getState().refresh();
    } catch (e) {
      console.error("Failed to assign item:", e);
    }
  },

  unassignFromTask: async (itemId, taskId) => {
    try {
      await invoke("unassign_item_from_task", { itemId, taskId });
      await get().loadTasks();
      await useWindowStore.getState().refresh();
    } catch (e) {
      console.error("Failed to unassign item:", e);
    }
  },
}));

// ─── UI Store ───────────────────────────────────────
interface UIState {
  theme: "light" | "dark";
  searchOpen: boolean;
  sidebarCollapsed: boolean;

  toggleTheme: () => void;
  setSearchOpen: (open: boolean) => void;
  toggleSidebar: () => void;
}

export const useUIStore = create<UIState>((set) => ({
  theme:
    typeof window !== "undefined" &&
    window.matchMedia("(prefers-color-scheme: dark)").matches
      ? "dark"
      : "light",
  searchOpen: false,
  sidebarCollapsed: false,

  toggleTheme: () =>
    set((s) => {
      const next = s.theme === "dark" ? "light" : "dark";
      document.documentElement.classList.toggle("dark", next === "dark");
      return { theme: next };
    }),

  setSearchOpen: (open) => set({ searchOpen: open }),

  toggleSidebar: () => set((s) => ({ sidebarCollapsed: !s.sidebarCollapsed })),
}));
