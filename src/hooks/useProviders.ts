import { useQuery } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import type { EpisodesResult, SourcesResult, SkipTimes, EpisodeItem } from '../types';

interface QueryToggle {
  enabled?: boolean;
}

export function useAnimeCatalogEpisodes(
  anilistId: number | null,
  malId?: number | null,
  totalEpisodes?: number | null,
  titleRomaji?: string | null,
  status?: string | null,
  nextAiringEpisode?: number | null,
  options?: QueryToggle
) {
  return useQuery<EpisodeItem[]>({
    queryKey: ['animeCatalogEpisodes', anilistId, malId, totalEpisodes, titleRomaji, status, nextAiringEpisode],
    queryFn: async () => {
      if (!anilistId) return [];
      return await invoke<EpisodeItem[]>('get_anime_catalog_episodes', {
        anilistId,
        malId: malId || null,
        totalEpisodes: totalEpisodes || null,
        titleRomaji: titleRomaji || null,
        status: status || null,
        nextAiringEpisode: nextAiringEpisode || null,
      });
    },
    enabled: !!anilistId && (options?.enabled ?? true),
    staleTime: 30 * 60 * 1000,
  });
}

/** Episode lists of every provider. This fans out to all of them, so only enable it when the list is shown. */
export function useEpisodes(
  anilistId: number | null,
  malId?: number | null,
  titleRomaji?: string | null,
  options?: QueryToggle
) {
  return useQuery<EpisodesResult>({
    queryKey: ['episodes', anilistId, malId, titleRomaji],
    queryFn: async () => {
      if (!anilistId) return {};
      return await invoke<EpisodesResult>('get_all_episodes', {
        anilistId,
        malId: malId || null,
        titleRomaji: titleRomaji || null,
      });
    },
    enabled: !!anilistId && (options?.enabled ?? true),
    staleTime: 10 * 60 * 1000,
  });
}

export interface EpisodeSourcesRequest {
  anilistId: number;
  malId: number | null;
  episodeNumber: number;
  category: 'sub' | 'dub';
  titleRomaji: string | null;
  /** A provider key, or null to try every provider. */
  provider: string | null;
  /** When `provider` is only a preference: try it first, then every provider. */
  fallbackToAny?: boolean;
  enabled?: boolean;
}

export function useEpisodeSources({
  anilistId,
  malId,
  episodeNumber,
  category,
  titleRomaji,
  provider,
  fallbackToAny = false,
  enabled = true,
}: EpisodeSourcesRequest) {
  return useQuery<SourcesResult>({
    queryKey: ['episodeSources', anilistId, episodeNumber, category, provider ?? 'auto', fallbackToAny],
    queryFn: async () => {
      const request = { anilistId, malId, episodeNumber, category, titleRomaji };
      if (provider && fallbackToAny) {
        try {
          const preferred = await invoke<SourcesResult>('get_episode_sources', { ...request, provider });
          if (preferred.streams.length > 0) return preferred;
        } catch {
          // The preferred provider has nothing for this episode; ask all of them.
        }
        return await invoke<SourcesResult>('get_episode_sources', { ...request, provider: null });
      }
      return await invoke<SourcesResult>('get_episode_sources', { ...request, provider });
    },
    enabled,
    // Stream URLs are fetched once per visit. A background refetch would hand the player
    // new URLs and restart playback, so only an explicit retry refetches.
    staleTime: Infinity,
    gcTime: 10 * 60 * 1000,
    retry: false,
    refetchOnReconnect: false,
    refetchOnWindowFocus: false,
  });
}

/** AniSkip intro/outro times. `durationS` is the episode length AniSkip matches against. */
export function useSkipTimes(
  malId: number | null | undefined,
  episodeNumber: number,
  durationS: number,
  enabled: boolean = true
) {
  const length = Math.round(durationS);
  return useQuery<SkipTimes>({
    queryKey: ['skipTimes', malId, episodeNumber, length],
    queryFn: () => invoke<SkipTimes>('get_skip_times', { malId, episodeNumber, durationS: length }),
    enabled: enabled && !!malId && length > 0,
    staleTime: 60 * 60 * 1000,
    retry: false,
  });
}

export function useFillerEpisodes(malId: number | null | undefined) {
  return useQuery<number[]>({
    queryKey: ['fillerEpisodes', malId],
    queryFn: async () => {
      if (!malId) return [];
      return await invoke<number[]>('get_filler_episodes', { malId });
    },
    enabled: !!malId,
    staleTime: 24 * 60 * 60 * 1000,
  });
}
