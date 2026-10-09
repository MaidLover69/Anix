import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import type { HistoryEntry, WatchlistEntry } from '../types';

export function useWatchlist() {
  return useQuery<WatchlistEntry[]>({
    queryKey: ['watchlist'],
    queryFn: async () => {
      return await invoke<WatchlistEntry[]>('get_watchlist');
    },
  });
}

export function useIsInWatchlist(anilistId: number | null) {
  return useQuery<boolean>({
    queryKey: ['isInWatchlist', anilistId],
    queryFn: async () => {
      if (!anilistId) return false;
      return await invoke<boolean>('is_in_watchlist', { anilistId });
    },
    enabled: !!anilistId,
  });
}

export function useWatchedEpisodes(anilistId: number | null) {
  return useQuery<number[]>({
    queryKey: ['watchedEpisodes', anilistId],
    queryFn: async () => {
      if (!anilistId) return [];
      return await invoke<number[]>('get_watched_episodes', { anilistId });
    },
    enabled: !!anilistId,
  });
}

/**
 * Saved `[positionMs, durationMs]` of one episode, read fresh for each player load.
 * `loadKey` identifies that load, so nothing saved during an earlier one is served from cache.
 */
export function useResumePosition(
  anilistId: number,
  episodeNumber: number,
  loadKey: string | number
) {
  return useQuery<[number, number] | null>({
    queryKey: ['resumePosition', anilistId, episodeNumber, loadKey],
    queryFn: () =>
      invoke<[number, number] | null>('get_episode_progress', { anilistId, episodeNumber }),
    staleTime: Infinity,
    gcTime: 0,
    retry: false,
    // A local database read; it must not wait for a network connection.
    networkMode: 'always',
    refetchOnReconnect: false,
    refetchOnWindowFocus: false,
  });
}

export function useContinueWatching() {
  return useQuery<HistoryEntry[]>({
    queryKey: ['continueWatching'],
    queryFn: async () => {
      return await invoke<HistoryEntry[]>('get_continue_watching');
    },
  });
}

export function useToggleWatchlist() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async ({
      anilistId,
      title,
      cover,
      format,
      averageScore,
      inWatchlist,
    }: {
      anilistId: number;
      title: string;
      cover?: string | null;
      format?: string | null;
      averageScore?: number | null;
      inWatchlist: boolean;
    }) => {
      if (inWatchlist) {
        await invoke('remove_from_watchlist', { anilistId });
      } else {
        await invoke('add_to_watchlist', {
          anilistId,
          title,
          cover: cover || null,
          format: format || null,
          averageScore: averageScore !== undefined ? averageScore : null,
        });
      }
    },
    onSuccess: (_, variables) => {
      queryClient.invalidateQueries({ queryKey: ['watchlist'] });
      queryClient.invalidateQueries({ queryKey: ['isInWatchlist', variables.anilistId] });
    },
  });
}

export function useUpdateWatchProgress() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async ({
      anilistId,
      title,
      cover,
      episodeNumber,
      episodeTitle,
      provider,
      category,
      positionMs,
      durationMs,
    }: {
      anilistId: number;
      title: string;
      cover?: string | null;
      episodeNumber: number;
      episodeTitle?: string | null;
      provider: string;
      category: string;
      positionMs: number;
      durationMs: number;
    }) => {
      await invoke('update_watch_progress', {
        anilistId,
        title,
        cover: cover || null,
        episodeNumber,
        episodeTitle: episodeTitle || null,
        provider,
        category,
        positionMs,
        durationMs,
      });
    },
    onSuccess: (_, variables) => {
      // Everything that shows progress: the Library history, Home's Continue Watching rail
      // (part of `homeData`) and the watched ticks on the detail page.
      queryClient.invalidateQueries({ queryKey: ['continueWatching'] });
      queryClient.invalidateQueries({ queryKey: ['homeData'] });
      queryClient.invalidateQueries({ queryKey: ['watchedEpisodes', variables.anilistId] });
    },
  });
}
