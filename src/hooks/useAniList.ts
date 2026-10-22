import { useQuery, useMutation, useQueryClient, keepPreviousData } from '@tanstack/react-query';
import type { QueryClient } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import type {
  Media,
  HomeData,
  Viewer,
  MediaListEntry,
  AnimeThemeEntry,
  MalAnimeDetails,
  SavedListEntry,
} from '../types';

/** Query keys whose results depend on the "hide adult content" setting. */
const ADULT_FILTERED_QUERY_KEYS = ['homeData', 'searchAnime', 'airingSchedule'] as const;

/** Refetches every AniList list that the backend filters by the adult-content setting. */
export function invalidateAdultFilteredQueries(queryClient: QueryClient) {
  for (const key of ADULT_FILTERED_QUERY_KEYS) {
    queryClient.invalidateQueries({ queryKey: [key] });
  }
}

export function useHomeData() {
  return useQuery<HomeData>({
    queryKey: ['homeData'],
    queryFn: async () => {
      return await invoke<HomeData>('fetch_home_data');
    },
    staleTime: 5 * 60 * 1000, // 5 minutes
  });
}

export interface SearchParams {
  query?: string;
  genres?: string[];
  format?: string;
  status?: string;
  sort?: string;
  page?: number;
  perPage?: number;
  excludeGenres?: string[];
  /** Lowest average score, 0–100. */
  minScore?: number;
  year?: number;
  /** WINTER, SPRING, SUMMER or FALL. */
  season?: string;
}

export function useSearchAnime(
  params: SearchParams,
  options?: {
    enabled?: boolean;
    /** Keep showing the previous result while a new search or page loads. */
    keepPrevious?: boolean;
  }
) {
  return useQuery<Media[]>({
    queryKey: ['searchAnime', params],
    queryFn: async () => {
      return await invoke<Media[]>('search_anime', {
        query: params.query || null,
        genres: params.genres && params.genres.length > 0 ? params.genres : null,
        format: params.format || null,
        status: params.status || null,
        sort: params.sort || 'POPULARITY_DESC',
        page: params.page || 1,
        perPage: params.perPage || 24,
        excludeGenres: params.excludeGenres && params.excludeGenres.length > 0 ? params.excludeGenres : null,
        minScore: params.minScore || null,
        year: params.year || null,
        season: params.season || null,
      });
    },
    enabled: options?.enabled,
    placeholderData: options?.keepPrevious ? keepPreviousData : undefined,
    staleTime: 2 * 60 * 1000,
  });
}

export function useAnimeDetails(id: number | null) {
  return useQuery<any>({
    queryKey: ['animeDetails', id],
    queryFn: async () => {
      if (!id) return null;
      return await invoke<any>('fetch_anime_details', { id });
    },
    enabled: !!id,
    staleTime: 15 * 60 * 1000,
  });
}

export function useMalAnimeDetails(malId: number | null | undefined) {
  return useQuery<MalAnimeDetails | null>({
    queryKey: ['malAnimeDetails', malId],
    queryFn: async () => {
      if (!malId) return null;
      return await invoke<MalAnimeDetails>('fetch_mal_anime_synopsis', { malId });
    },
    enabled: !!malId,
    staleTime: 60 * 60 * 1000,
  });
}

export function useAnimeThemes(malId: number | null | undefined) {
  return useQuery<AnimeThemeEntry[]>({
    queryKey: ['animeThemes', malId],
    queryFn: async () => {
      if (!malId) return [];
      return await invoke<AnimeThemeEntry[]>('fetch_anime_themes', { malId });
    },
    enabled: !!malId,
    staleTime: 24 * 60 * 60 * 1000,
  });
}

/** The connected AniList account; the backend reads its own token. */
export function useAniListViewer(connected: boolean) {
  return useQuery<Viewer>({
    queryKey: ['aniListViewer'],
    queryFn: () => invoke<Viewer>('get_anilist_viewer'),
    enabled: connected,
    staleTime: 30 * 60 * 1000,
    retry: 1,
  });
}

/** One status list of the connected AniList account. */
export function useAniListLibrary(connected: boolean, status?: string) {
  return useQuery<MediaListEntry[]>({
    queryKey: ['aniListLibrary', status ?? null],
    queryFn: () => invoke<MediaListEntry[]>('fetch_anilist_library', { status: status || null }),
    enabled: connected,
  });
}

/** Saves the status and progress of one list entry to AniList and, when connected, MyAnimeList. */
export function useSaveAniListEntry() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ mediaId, status, progress }: { mediaId: number; status: string; progress: number }) =>
      invoke<SavedListEntry>('save_anilist_entry', { mediaId, status, progress }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['aniListLibrary'] });
      queryClient.invalidateQueries({ queryKey: ['aniListViewer'] });
    },
  });
}
