import { useMemo } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import type { SourcesResult } from '../types';

/** Provider key of "Your files" in `/watch?prov=` and in watch history. */
export const LOCAL_PROVIDER = 'local';
export const LOCAL_PROVIDER_LABEL = 'Your files';

export interface LocalMediaFile {
  id: number;
  file_path: string;
  file_name: string;
  file_size: number;
  /** Null: linked, but not used for any episode. */
  episode_number: number | null;
  /** The number was set by hand and survives a re-scan. */
  manual: boolean;
  from_folder: boolean;
  exists: boolean;
}

export interface LocalDownloadFile {
  episode_number: number;
  file_path: string;
  file_name: string;
  exists: boolean;
}

export interface LocalMediaLink {
  anilist_id: number;
  linked: boolean;
  folder_path: string | null;
  files: LocalMediaFile[];
  /** Finished downloads of the app for this title; they play like linked files. */
  downloads: LocalDownloadFile[];
  /** Episode numbers with a file on disk, ascending. */
  episodes: number[];
}

export interface LocalEpisodeSource {
  sources: SourcesResult;
  file_path: string;
}

const NO_EPISODES: number[] = [];

export const localMediaKey = (anilistId: number | null) => ['localMedia', anilistId] as const;

export function useLocalMedia(anilistId: number | null) {
  const query = useQuery<LocalMediaLink>({
    queryKey: localMediaKey(anilistId),
    queryFn: () => invoke<LocalMediaLink>('get_local_media', { anilistId }),
    enabled: anilistId !== null,
    // A local database read: always fresh (a download may have finished), never waiting
    // for a network connection.
    staleTime: 0,
    networkMode: 'always',
  });
  const episodes = query.data?.episodes ?? NO_EPISODES;
  const hasEpisode = useMemo(() => {
    return (episode: number) => episodes.some((n) => Math.abs(n - episode) < 0.01);
  }, [episodes]);
  return { ...query, episodes, hasEpisode };
}

type LocalMediaAction =
  | { kind: 'linkFolder'; folderPath: string }
  | { kind: 'addFiles'; filePaths: string[] }
  | { kind: 'rescan' }
  | { kind: 'setEpisode'; fileId: number; episodeNumber: number | null }
  | { kind: 'removeFile'; fileId: number }
  | { kind: 'unlink' };

function runAction(anilistId: number, action: LocalMediaAction): Promise<LocalMediaLink> {
  switch (action.kind) {
    case 'linkFolder':
      return invoke('link_local_folder', { anilistId, folderPath: action.folderPath });
    case 'addFiles':
      return invoke('add_local_files', { anilistId, filePaths: action.filePaths });
    case 'rescan':
      return invoke('rescan_local_media', { anilistId });
    case 'setEpisode':
      return invoke('set_local_file_episode', {
        anilistId,
        fileId: action.fileId,
        episodeNumber: action.episodeNumber,
      });
    case 'removeFile':
      return invoke('remove_local_file', { anilistId, fileId: action.fileId });
    case 'unlink':
      return invoke('unlink_local_media', { anilistId });
  }
}

/** Every change to a title's linked files; the answer is the new state of the link. */
export function useLocalMediaAction(anilistId: number) {
  const queryClient = useQueryClient();
  return useMutation<LocalMediaLink, unknown, LocalMediaAction>({
    mutationFn: (action) => runAction(anilistId, action),
    networkMode: 'always',
    onSuccess: (link) => {
      queryClient.setQueryData(localMediaKey(anilistId), link);
      queryClient.invalidateQueries({ queryKey: ['localEpisodeSource', anilistId] });
    },
  });
}

/** The system folder picker (`files: false`) or file picker; empty when cancelled. */
export function pickLocalMedia(files: boolean): Promise<string[]> {
  return invoke<string[]>('pick_local_media', { files });
}

/** The file of one episode as player sources; null when there is none. */
export function useLocalEpisodeSource(anilistId: number, episodeNumber: number, enabled: boolean) {
  return useQuery<LocalEpisodeSource | null>({
    queryKey: ['localEpisodeSource', anilistId, episodeNumber],
    queryFn: () =>
      invoke<LocalEpisodeSource | null>('get_local_episode_source', { anilistId, episodeNumber }),
    enabled,
    networkMode: 'always',
    staleTime: Infinity,
    gcTime: 60 * 1000,
    retry: false,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  });
}
