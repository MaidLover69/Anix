import { useEffect } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import type { DownloadRecord, StorageCheck } from '../types';
import { errorMessage } from '../components/ui/ErrorState';
import { useSaveSettings } from './useSettings';

const POLL_INTERVAL_MS = 3000;

export function isDownloadActive(record: Pick<DownloadRecord, 'status'>): boolean {
  return record.status === 'QUEUED' || record.status === 'DOWNLOADING';
}

/**
 * `watch` is true while the list is on screen. The query then refreshes on entry and polls,
 * but only for as long as something is queued or downloading.
 */
export function useAllDownloads({ watch = false }: { watch?: boolean } = {}) {
  const query = useQuery<DownloadRecord[]>({
    queryKey: ['allDownloads'],
    queryFn: async () => {
      return await invoke<DownloadRecord[]>('get_all_downloads');
    },
    refetchInterval: (q) =>
      watch && (q.state.data ?? []).some(isDownloadActive) ? POLL_INTERVAL_MS : false,
  });

  const { refetch } = query;
  useEffect(() => {
    if (watch) refetch({ cancelRefetch: false });
  }, [watch, refetch]);

  return query;
}

export function useCheckStorage(quality: string = '720p') {
  return useQuery<StorageCheck>({
    queryKey: ['checkStorage', quality],
    queryFn: async () => {
      return await invoke<StorageCheck>('check_storage_for_download', { quality });
    },
  });
}

export function useDownloadDirectory(enabled: boolean = true) {
  return useQuery<string>({
    queryKey: ['downloadDirectory'],
    queryFn: async () => {
      return await invoke<string>('get_download_directory');
    },
    enabled,
    staleTime: 5 * 60 * 1000,
  });
}

/** A new row for the backend queue worker, which resolves the sources and downloads it. */
export function newQueuedDownload(fields: {
  anilistId: number;
  episodeNum: number;
  episodeTitle: string | null;
  seriesTitle: string;
  seriesCover: string | null;
  provider: string;
  category: string;
  quality: string;
}): DownloadRecord {
  const now = Date.now();
  return {
    id: crypto.randomUUID(),
    anilist_id: fields.anilistId,
    episode_num: fields.episodeNum,
    episode_title: fields.episodeTitle,
    series_title: fields.seriesTitle,
    series_cover: fields.seriesCover,
    provider: fields.provider,
    category: fields.category,
    quality: fields.quality,
    status: 'QUEUED',
    progress: 0,
    file_path: null,
    file_size: null,
    duration_s: null,
    error_msg: null,
    created_at: now,
    updated_at: now,
  };
}

export interface QueueDownloadsResult {
  queued: number;
  failures: { record: DownloadRecord; reason: string }[];
}

/** Creates the rows one by one so a failure on one episode does not stop the rest. */
export function useQueueDownloads() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (records: DownloadRecord[]): Promise<QueueDownloadsResult> => {
      const result: QueueDownloadsResult = { queued: 0, failures: [] };
      for (const record of records) {
        try {
          await invoke<string>('create_download', { record });
          result.queued += 1;
        } catch (err) {
          result.failures.push({
            record,
            reason: errorMessage(err),
          });
        }
      }
      return result;
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ['allDownloads'] });
    },
  });
}

export function useDeleteDownload() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (id: string) => {
      await invoke('delete_download', { id });
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['allDownloads'] });
    },
  });
}

/** Puts a failed download back in the queue; the backend worker picks it up again. */
export function useRetryDownload() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (id: string) => {
      await invoke('update_download_status', { id, status: 'QUEUED', progress: 0, errorMsg: null });
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['allDownloads'] });
    },
  });
}

/** Downloads waiting or running: the count on the sidebar's Downloads entry. */
export function useActiveDownloadCount(): number {
  return (useAllDownloads({ watch: true }).data ?? []).filter(isDownloadActive).length;
}

/**
 * Change and Reset for the download folder. Change opens the folder picker; the backend
 * refuses a folder it cannot write to. Both save `download_dir`; downloads read it when they start.
 */
export function useDownloadFolder() {
  const queryClient = useQueryClient();
  const save = useSaveSettings();
  const refresh = () => {
    queryClient.invalidateQueries({ queryKey: ['downloadDirectory'] });
    queryClient.invalidateQueries({ queryKey: ['checkStorage'] });
  };
  const change = useMutation({
    mutationFn: async (): Promise<string | null> => {
      const picked = await invoke<string | null>('pick_download_directory');
      if (picked) await save.mutateAsync({ download_dir: picked });
      return picked;
    },
    onSettled: refresh,
  });
  const reset = useMutation({
    mutationFn: async () => {
      await save.mutateAsync({ download_dir: '' });
    },
    onSettled: refresh,
  });
  return { change, reset };
}
