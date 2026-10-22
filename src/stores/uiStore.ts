import { create } from 'zustand';
import type { Media } from '../types';

export interface ToastMessage {
  id: string;
  type: 'info' | 'success' | 'warning' | 'error';
  title: string;
  message?: string;
  duration?: number;
}

const DEFAULT_TOAST_MS = 3000;
// Errors and warnings carry text worth reading, so they stay a little longer.
const ERROR_TOAST_MS = 6000;
// After a pause (pointer or focus on the toast) it stays at least this long.
const RESUMED_TOAST_MS = 1500;

/** Per-toast countdown; `handle` is null while paused. */
const toastTimers = new Map<string, { handle: number | null; remaining: number; startedAt: number }>();

interface UIState {
  sidebarExpanded: boolean;
  toasts: ToastMessage[];
  authDialogOpen: boolean;
  updateDialogOpen: boolean;
  bulkDownloadDialogOpen: boolean;
  activeMediaForDownload: Media | null;
  commandPaletteOpen: boolean;
  hotkeyCheatSheetOpen: boolean;

  toggleSidebar: () => void;
  setSidebarExpanded: (expanded: boolean) => void;
  showToast: (toast: Omit<ToastMessage, 'id'>) => void;
  removeToast: (id: string) => void;
  /** Stops a toast's countdown while it is hovered or focused. */
  pauseToast: (id: string) => void;
  resumeToast: (id: string) => void;
  setAuthDialogOpen: (open: boolean) => void;
  setUpdateDialogOpen: (open: boolean) => void;
  openBulkDownloadDialog: (media: Media) => void;
  closeBulkDownloadDialog: () => void;
  setCommandPaletteOpen: (open: boolean) => void;
  setHotkeyCheatSheetOpen: (open: boolean) => void;
}

export const useUIStore = create<UIState>((set) => {
  const expire = (id: string) => {
    toastTimers.delete(id);
    set((state) => ({ toasts: state.toasts.filter((t) => t.id !== id) }));
  };

  return {
    sidebarExpanded: true,
    toasts: [],
    authDialogOpen: false,
    updateDialogOpen: false,
    bulkDownloadDialogOpen: false,
    activeMediaForDownload: null,
    commandPaletteOpen: false,
    hotkeyCheatSheetOpen: false,

    toggleSidebar: () => set((state) => ({ sidebarExpanded: !state.sidebarExpanded })),
    setSidebarExpanded: (sidebarExpanded) => set({ sidebarExpanded }),

    showToast: (toast) => {
      const id = Math.random().toString(36).substring(2, 9);
      const newToast: ToastMessage = { ...toast, id };
      set((state) => ({ toasts: [...state.toasts, newToast] }));

      const ms =
        toast.duration ?? (toast.type === 'error' || toast.type === 'warning' ? ERROR_TOAST_MS : DEFAULT_TOAST_MS);
      toastTimers.set(id, { handle: window.setTimeout(() => expire(id), ms), remaining: ms, startedAt: Date.now() });
    },

    removeToast: (id) => {
      const timer = toastTimers.get(id);
      if (timer?.handle != null) window.clearTimeout(timer.handle);
      expire(id);
    },

    pauseToast: (id) => {
      const timer = toastTimers.get(id);
      if (!timer || timer.handle === null) return;
      window.clearTimeout(timer.handle);
      timer.handle = null;
      timer.remaining -= Date.now() - timer.startedAt;
    },

    resumeToast: (id) => {
      const timer = toastTimers.get(id);
      if (!timer || timer.handle !== null) return;
      timer.startedAt = Date.now();
      timer.remaining = Math.max(timer.remaining, RESUMED_TOAST_MS);
      timer.handle = window.setTimeout(() => expire(id), timer.remaining);
    },

    setAuthDialogOpen: (authDialogOpen) => set({ authDialogOpen }),
    setUpdateDialogOpen: (updateDialogOpen) => set({ updateDialogOpen }),
    openBulkDownloadDialog: (media) =>
      set({ bulkDownloadDialogOpen: true, activeMediaForDownload: media }),
    closeBulkDownloadDialog: () =>
      set({ bulkDownloadDialogOpen: false, activeMediaForDownload: null }),
    setCommandPaletteOpen: (commandPaletteOpen) => set({ commandPaletteOpen }),
    setHotkeyCheatSheetOpen: (hotkeyCheatSheetOpen) => set({ hotkeyCheatSheetOpen }),
  };
});
