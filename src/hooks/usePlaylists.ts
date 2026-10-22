import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import type {
  CustomPlaylist,
  CustomPlaylistItem,
  ExternalPlaylistItem,
  SourcesResult,
} from '../types';
import { errorMessage } from '../components/ui/ErrorState';
import { t } from '../i18n';

export function usePlaylists() {
  return useQuery<CustomPlaylist[]>({
    queryKey: ['customPlaylists'],
    queryFn: async () => {
      return await invoke<CustomPlaylist[]>('list_playlists');
    },
  });
}

export function usePlaylistItems(playlistId: string | null) {
  return useQuery<CustomPlaylistItem[]>({
    queryKey: ['customPlaylistItems', playlistId],
    queryFn: async () => {
      if (!playlistId) return [];
      return await invoke<CustomPlaylistItem[]>('get_playlist_items', { playlistId });
    },
    enabled: !!playlistId,
  });
}

export function useCreatePlaylist() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async ({ name, description }: { name: string; description?: string }) => {
      return await invoke<CustomPlaylist>('create_playlist', {
        name,
        description: description ?? null,
      });
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['customPlaylists'] });
    },
  });
}

export function useAddMultipleToPlaylist() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async ({ items }: { playlistId: string; items: CustomPlaylistItem[] }) => {
      await invoke('add_multiple_to_playlist', { items });
    },
    // Also after a failure: the list on screen must match what the backend kept.
    onSettled: (_data, _error, { playlistId }) => {
      queryClient.invalidateQueries({ queryKey: ['customPlaylists'] });
      queryClient.invalidateQueries({ queryKey: ['customPlaylistItems', playlistId] });
    },
  });
}

export function useRemoveFromPlaylist() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async ({ itemId }: { itemId: string; playlistId: string }) => {
      await invoke('remove_from_playlist', { itemId });
    },
    onSuccess: (_data, { playlistId }) => {
      queryClient.invalidateQueries({ queryKey: ['customPlaylists'] });
      queryClient.invalidateQueries({ queryKey: ['customPlaylistItems', playlistId] });
    },
  });
}

export function useDeletePlaylist() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (playlistId: string) => {
      await invoke('delete_playlist', { playlistId });
    },
    onSuccess: (_data, playlistId) => {
      queryClient.setQueryData<CustomPlaylist[]>(['customPlaylists'], (current) =>
        current?.filter((playlist) => playlist.id !== playlistId)
      );
      queryClient.invalidateQueries({ queryKey: ['customPlaylists'] });
      queryClient.removeQueries({ queryKey: ['customPlaylistItems', playlistId] });
    },
  });
}

export interface ResolvedPlaylist {
  resolved: ExternalPlaylistItem[];
  failures: { item: CustomPlaylistItem; reason: string }[];
  cancelled: boolean;
}

/**
 * Resolves a stream for every playlist item, in order. Sources are looked up by anime id and
 * episode number, so no episode list is needed. Items that cannot be resolved are reported
 * rather than dropped silently.
 */
export async function resolvePlaylistStreams(
  items: CustomPlaylistItem[],
  options: {
    /** A `get_all_episodes` provider key, or null / 'auto' to try every provider. */
    provider: string | null;
    onProgress?: (done: number, total: number) => void;
    isCancelled?: () => boolean;
  }
): Promise<ResolvedPlaylist> {
  const result: ResolvedPlaylist = { resolved: [], failures: [], cancelled: false };
  const preferred = options.provider && options.provider !== 'auto' ? options.provider : null;

  for (let index = 0; index < items.length; index++) {
    if (options.isCancelled?.()) {
      result.cancelled = true;
      break;
    }
    const item = items[index];
    options.onProgress?.(index, items.length);

    try {
      const request = {
        anilistId: item.anilist_id,
        malId: item.mal_id ?? null,
        episodeNumber: item.episode_num,
        category: item.category === 'dub' ? 'dub' : 'sub',
        titleRomaji: item.series_title || null,
      };
      const fetchSources = (provider: string | null) =>
        invoke<SourcesResult>('get_episode_sources', { ...request, provider });
      // The preferred provider goes first; an item it cannot serve still gets the others.
      const sources = preferred
        ? await fetchSources(preferred).catch(() => fetchSources(null))
        : await fetchSources(null);

      const stream = sources?.streams?.[0];
      if (!stream) {
        result.failures.push({ item, reason: t('usePlaylists.noStream') });
        continue;
      }
      result.resolved.push({
        url: stream.url,
        title: t('usePlaylists.streamTitle', { title: item.series_title, number: item.episode_num }),
        referer: stream.referer ?? stream.headers?.Referer ?? stream.headers?.referer ?? null,
      });
    } catch (err) {
      result.failures.push({
        item,
        reason: errorMessage(err),
      });
    }
  }

  if (options.isCancelled?.()) result.cancelled = true;
  options.onProgress?.(items.length, items.length);
  return result;
}

export async function launchPlaylistExternalPlayer(
  player: 'mpv' | 'vlc',
  playlistName: string,
  items: ExternalPlaylistItem[]
): Promise<boolean> {
  return await invoke<boolean>('launch_playlist_external_player', {
    player,
    playlistName,
    items,
  });
}
