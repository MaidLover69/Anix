import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import type { CustomPlaylistItem } from '../types';

export const DEFAULT_SUBTITLE_BACKGROUND = 'rgba(0, 0, 0, 0.5)';

interface PlayerState {
  // Preferences, kept across launches.
  autoPlayNext: boolean;
  volume: number;
  muted: boolean;
  subtitleFontSize: number;
  /** How far subtitles are raised from the bottom of the picture, in percent of its height. */
  subtitleRaise: number;
  subtitleColor: string;
  subtitleBackground: string;
  /** Text size of the second subtitle, shown at the top of the picture. */
  secondarySubtitleFontSize: number;

  // Kept for the running session only.
  playbackRate: number;
  subtitleDelayMs: number;
  /** The window is the compact always-on-top mini-player. */
  miniPlayer: boolean;

  // Playlist queue started from the Library; it may span several anime.
  customQueue: CustomPlaylistItem[];
  customQueueIndex: number;
  activePlaylistName: string | null;

  setAutoPlayNext: (auto: boolean) => void;
  setVolume: (volume: number, muted: boolean) => void;
  setPlaybackRate: (rate: number) => void;
  setSubtitleFontSize: (size: number) => void;
  setSubtitleRaise: (percent: number) => void;
  setSubtitleColor: (color: string) => void;
  setSubtitleBackground: (background: string) => void;
  setSubtitleDelayMs: (delay: number) => void;
  setSecondarySubtitleFontSize: (size: number) => void;
  setMiniPlayer: (mini: boolean) => void;
  setCustomQueue: (items: CustomPlaylistItem[], startIndex?: number, playlistName?: string) => void;
  clearCustomQueue: () => void;
  jumpToCustomQueueIndex: (index: number) => CustomPlaylistItem | null;
}

export const usePlayerStore = create<PlayerState>()(
  persist(
    (set, get) => ({
      autoPlayNext: true,
      volume: 1,
      muted: false,
      subtitleFontSize: 20,
      subtitleRaise: 0,
      subtitleColor: '#ffffff',
      subtitleBackground: DEFAULT_SUBTITLE_BACKGROUND,
      secondarySubtitleFontSize: 18,

      playbackRate: 1,
      subtitleDelayMs: 0,
      miniPlayer: false,

      customQueue: [],
      customQueueIndex: 0,
      activePlaylistName: null,

      setAutoPlayNext: (autoPlayNext) => set({ autoPlayNext }),
      setVolume: (volume, muted) => {
        const current = get();
        if (current.volume !== volume || current.muted !== muted) set({ volume, muted });
      },
      setPlaybackRate: (playbackRate) => set({ playbackRate }),
      setSubtitleFontSize: (subtitleFontSize) => set({ subtitleFontSize }),
      setSubtitleRaise: (subtitleRaise) => set({ subtitleRaise: Math.min(40, Math.max(0, subtitleRaise)) }),
      setSubtitleColor: (subtitleColor) => set({ subtitleColor }),
      setSubtitleBackground: (subtitleBackground) => set({ subtitleBackground }),
      setSubtitleDelayMs: (subtitleDelayMs) => set({ subtitleDelayMs }),
      setSecondarySubtitleFontSize: (secondarySubtitleFontSize) => set({ secondarySubtitleFontSize }),
      setMiniPlayer: (miniPlayer) => set({ miniPlayer }),

      setCustomQueue: (items, startIndex = 0, playlistName) => {
        set({
          customQueue: items,
          customQueueIndex: Math.max(0, Math.min(items.length - 1, startIndex)),
          activePlaylistName: playlistName || null,
        });
      },

      clearCustomQueue: () => {
        if (get().customQueue.length === 0 && get().activePlaylistName === null) return;
        set({ customQueue: [], customQueueIndex: 0, activePlaylistName: null });
      },

      jumpToCustomQueueIndex: (index) => {
        const { customQueue } = get();
        if (index < 0 || index >= customQueue.length) return null;
        set({ customQueueIndex: index });
        return customQueue[index];
      },
    }),
    {
      name: 'anilili-player',
      version: 1,
      partialize: (state) => ({
        autoPlayNext: state.autoPlayNext,
        volume: state.volume,
        muted: state.muted,
        subtitleFontSize: state.subtitleFontSize,
        subtitleRaise: state.subtitleRaise,
        subtitleColor: state.subtitleColor,
        subtitleBackground: state.subtitleBackground,
        secondarySubtitleFontSize: state.secondarySubtitleFontSize,
      }),
    }
  )
);
