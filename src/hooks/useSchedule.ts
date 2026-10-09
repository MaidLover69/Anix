import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { errorMessage } from '../components/ui/ErrorState';
import { useUIStore } from '../stores/uiStore';
import { useSettings } from './useSettings';
import type { AiringEntry, NotificationPreference } from '../types';

export function useAiringSchedule(fromTs: number, toTs: number) {
  return useQuery<AiringEntry[]>({
    queryKey: ['airingSchedule', fromTs, toTs],
    queryFn: async () => {
      return await invoke<AiringEntry[]>('fetch_airing_schedule', { fromTs, toTs });
    },
    staleTime: 30 * 60 * 1000,
  });
}

const NOTIFICATION_PREFERENCES_KEY = ['notificationPreferences'] as const;
const NOTIFICATION_MUTATION_KEY = ['notificationPreference'] as const;

/**
 * Whether the release-notification bell of one anime is on. Every bell on screen reads the
 * same list of bells, which is fetched once.
 */
export function useNotificationBell(mediaId: number) {
  return useQuery({
    queryKey: NOTIFICATION_PREFERENCES_KEY,
    queryFn: () => invoke<NotificationPreference[]>('list_notification_preferences'),
    // Only this app changes the list, and every change goes through the mutation below.
    staleTime: Infinity,
    select: (bells) => bells.some((bell) => bell.media_id === mediaId && bell.enabled),
  });
}

/**
 * Switches a bell on or off. The list is updated at once and put back if saving fails;
 * the outcome is reported with a toast.
 */
export function useSetNotificationBell() {
  const queryClient = useQueryClient();
  const showToast = useUIStore((s) => s.showToast);
  const { data: settings } = useSettings();
  // The key is absent from settings written before the setting existed, which means on.
  const silenced = (settings as { release_notifications?: boolean } | undefined)?.release_notifications === false;

  return useMutation({
    mutationKey: NOTIFICATION_MUTATION_KEY,
    mutationFn: (pref: NotificationPreference) => invoke<void>('save_notification_preference', { pref }),
    onMutate: async (pref) => {
      await queryClient.cancelQueries({ queryKey: NOTIFICATION_PREFERENCES_KEY });
      const previous = queryClient
        .getQueryData<NotificationPreference[]>(NOTIFICATION_PREFERENCES_KEY)
        ?.find((bell) => bell.media_id === pref.media_id);
      queryClient.setQueryData<NotificationPreference[]>(NOTIFICATION_PREFERENCES_KEY, (bells = []) => {
        const others = bells.filter((bell) => bell.media_id !== pref.media_id);
        return pref.enabled ? [...others, pref] : others;
      });
      return { previous };
    },
    onSuccess: (_data, pref) => {
      if (!pref.enabled) return;
      if (silenced) {
        showToast({
          type: 'info',
          title: 'Bell saved',
          message: 'Release notifications are switched off in Settings, so nothing will be shown until they are back on.',
        });
        return;
      }
      showToast({
        type: 'success',
        title: 'Notifications on',
        message: `You'll be notified when new episodes of ${pref.media_title} air.`,
      });
    },
    onError: (error, pref, context) => {
      // Only this anime's entry is put back, so a change to another bell made meanwhile stays.
      queryClient.setQueryData<NotificationPreference[]>(NOTIFICATION_PREFERENCES_KEY, (bells = []) => {
        const others = bells.filter((bell) => bell.media_id !== pref.media_id);
        return context?.previous ? [...others, context.previous] : others;
      });
      showToast({
        type: 'error',
        title: pref.enabled ? 'Could not turn notifications on' : 'Could not turn notifications off',
        message: errorMessage(error),
      });
    },
    onSettled: () => {
      // With several bells changing at once, only the last one to finish re-reads the list.
      if (queryClient.isMutating({ mutationKey: NOTIFICATION_MUTATION_KEY }) === 1) {
        queryClient.invalidateQueries({ queryKey: NOTIFICATION_PREFERENCES_KEY });
      }
    },
  });
}
