import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import type { HistoryEntry, ListStatus, ListStatusValue, WatchedEpisodeRow, WatchlistEntry } from '../types';
import { t } from '../i18n';

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
      queryClient.invalidateQueries({ queryKey: ['watchedHistory'] });
    },
  });
}

/**
 * Marks episodes as watched (or unmarks them) in the local history. Marking queues the list
 * progress of the connected trackers once, for the highest episode.
 */
export function useSetEpisodesWatched() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      anilistId,
      title,
      cover,
      episodes,
      watched,
    }: {
      anilistId: number;
      title: string;
      cover?: string | null;
      episodes: number[];
      watched: boolean;
    }) =>
      invoke<number>('set_episodes_watched', {
        anilistId,
        title,
        cover: cover || null,
        episodes,
        watched,
      }),
    onSuccess: (_, variables) => {
      queryClient.invalidateQueries({ queryKey: ['watchedEpisodes', variables.anilistId] });
      queryClient.invalidateQueries({ queryKey: ['continueWatching'] });
      queryClient.invalidateQueries({ queryKey: ['homeData'] });
      queryClient.invalidateQueries({ queryKey: ['watchedHistory'] });
    },
  });
}

/** Every watched episode (Library › Stats). */
export function useWatchedHistory(enabled = true) {
  return useQuery<WatchedEpisodeRow[]>({
    queryKey: ['watchedHistory'],
    queryFn: () => invoke<WatchedEpisodeRow[]>('get_watched_history'),
    enabled,
  });
}

/** The watchlist statuses in display order. Labels are getters so they follow the interface language. */
export const LIST_STATUS_OPTIONS: { value: ListStatusValue; label: string }[] = [
  { value: 'CURRENT', get label() { return t('useLibrary.watching'); } },
  { value: 'PLANNING', get label() { return t('useLibrary.planToWatch'); } },
  { value: 'COMPLETED', get label() { return t('useLibrary.completed'); } },
  { value: 'PAUSED', get label() { return t('useLibrary.onHold'); } },
  { value: 'DROPPED', get label() { return t('useLibrary.dropped'); } },
];

export function listStatusLabel(status: string | null | undefined): string {
  return LIST_STATUS_OPTIONS.find((option) => option.value === status)?.label ?? t('useLibrary.notOnList');
}

/**
 * The status of one anime: the connected AniList/MyAnimeList entry when there is one,
 * otherwise the watchlist's (also while the tracker cannot be reached).
 */
export function useListStatus(anilistId: number | null) {
  const queryClient = useQueryClient();
  return useQuery<ListStatus>({
    queryKey: ['listStatus', anilistId],
    queryFn: async () => {
      const found = await invoke<ListStatus>('get_list_status', { anilistId });
      // The backend copied the tracker's status into the watchlist.
      if (found.source !== 'local') queryClient.invalidateQueries({ queryKey: ['watchlist'] });
      return found;
    },
    enabled: anilistId !== null && anilistId !== 0,
    networkMode: 'always',
  });
}

/** Sets the status of an anime (adding it to the watchlist when needed), or `null` to remove it. */
export function useSetListStatus() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async ({
      anilistId,
      title,
      cover,
      format,
      averageScore,
      status,
    }: {
      anilistId: number;
      title: string;
      cover?: string | null;
      format?: string | null;
      averageScore?: number | null;
      status: ListStatusValue | null;
    }) => {
      if (status === null) {
        await invoke('remove_from_watchlist', { anilistId });
        return;
      }
      await invoke('set_list_status', {
        anilistId,
        title,
        cover: cover || null,
        format: format || null,
        averageScore: averageScore ?? null,
        status,
      });
    },
    onSuccess: (_, variables) => {
      queryClient.invalidateQueries({ queryKey: ['watchlist'] });
      queryClient.invalidateQueries({ queryKey: ['isInWatchlist', variables.anilistId] });
      queryClient.invalidateQueries({ queryKey: ['listStatus', variables.anilistId] });
    },
  });
}

/**
 * Reads the connected AniList list's statuses into the watchlist and sends the statuses
 * waiting to be sent; the watchlist is refetched when something changed.
 */
export function useSyncListStatuses(enabled: boolean) {
  const queryClient = useQueryClient();
  return useQuery<number>({
    queryKey: ['syncListStatuses'],
    queryFn: async () => {
      const changed = await invoke<number>('sync_list_statuses');
      queryClient.invalidateQueries({ queryKey: ['watchlist'] });
      return changed;
    },
    enabled,
    staleTime: 5 * 60_000,
    retry: false,
  });
}
