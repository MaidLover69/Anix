import { useEffect } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Notice } from '../types';

/** Sent by the backend after a notice was added or marked read. */
const CHANGED_EVENT = 'notices-changed';

/** Refetches the notices whenever the backend says they changed. */
function useNoticeEvents() {
  const queryClient = useQueryClient();
  useEffect(() => {
    let cancelled = false;
    let stop: (() => void) | undefined;
    listen(CHANGED_EVENT, () => {
      queryClient.invalidateQueries({ queryKey: ['notices'] });
      queryClient.invalidateQueries({ queryKey: ['unreadNotices'] });
    })
      .then((fn) => {
        // The effect may have been cleaned up before the listener was registered.
        if (cancelled) fn();
        else stop = fn;
      })
      .catch((err) => console.debug('[Notices] could not listen:', err));
    return () => {
      cancelled = true;
      stop?.();
    };
  }, [queryClient]);
}

/** The notification centre, newest first; `kind` limits it to one kind. */
export function useNotices(kind: string | null) {
  useNoticeEvents();
  return useQuery<Notice[]>({
    queryKey: ['notices', kind],
    queryFn: () => invoke<Notice[]>('list_notices', { kind }),
    networkMode: 'always',
  });
}

/** How many notices are unread; kept current by the backend's events. */
export function useUnreadNoticeCount() {
  useNoticeEvents();
  return useQuery<number>({
    queryKey: ['unreadNotices'],
    queryFn: () => invoke<number>('get_unread_notice_count'),
    networkMode: 'always',
  });
}

function useNoticeMutation<T>(command: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (args: T) => invoke(command, (args ?? {}) as Record<string, unknown>),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['notices'] });
      queryClient.invalidateQueries({ queryKey: ['unreadNotices'] });
    },
  });
}

/** Marks `ids` read, or every notice without `ids`. */
export const useMarkNoticesRead = () => useNoticeMutation<{ ids?: number[] }>('mark_notices_read');
export const useClearNotices = () => useNoticeMutation<void>('clear_notices');
export const usePushTestNotice = () => useNoticeMutation<void>('push_test_notice');
