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
// Errors carry text worth reading, so they stay a little longer.
const ERROR_TOAST_MS = 6000;

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
  setAuthDialogOpen: (open: boolean) => void;
  setUpdateDialogOpen: (open: boolean) => void;
  openBulkDownloadDialog: (media: Media) => void;
  closeBulkDownloadDialog: () => void;
  setCommandPaletteOpen: (open: boolean) => void;
  setHotkeyCheatSheetOpen: (open: boolean) => void;
}

export const useUIStore = create<UIState>((set) => ({
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

    setTimeout(() => {
      set((state) => ({ toasts: state.toasts.filter((t) => t.id !== id) }));
    }, toast.duration ?? (toast.type === 'error' ? ERROR_TOAST_MS : DEFAULT_TOAST_MS));
  },

  removeToast: (id) =>
    set((state) => ({ toasts: state.toasts.filter((t) => t.id !== id) })),

  setAuthDialogOpen: (authDialogOpen) => set({ authDialogOpen }),
  setUpdateDialogOpen: (updateDialogOpen) => set({ updateDialogOpen }),
  openBulkDownloadDialog: (media) =>
    set({ bulkDownloadDialogOpen: true, activeMediaForDownload: media }),
  closeBulkDownloadDialog: () =>
    set({ bulkDownloadDialogOpen: false, activeMediaForDownload: null }),
  setCommandPaletteOpen: (commandPaletteOpen) => set({ commandPaletteOpen }),
  setHotkeyCheatSheetOpen: (hotkeyCheatSheetOpen) => set({ hotkeyCheatSheetOpen }),
}));
