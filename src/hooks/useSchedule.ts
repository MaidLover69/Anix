import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { errorMessage } from '../components/ui/ErrorState';
import { useUIStore } from '../stores/uiStore';
import { useSettings } from './useSettings';
import type { AiringEntry, NotificationPreference } from '../types';
import { t } from '../i18n';

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

/** Every bell that is on (the same fetch as `useNotificationBell`). */
export function useNotificationBells() {
  return useQuery({
    queryKey: NOTIFICATION_PREFERENCES_KEY,
    queryFn: () => invoke<NotificationPreference[]>('list_notification_preferences'),
    staleTime: Infinity,
  });
}

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
          title: t('useSchedule.bellSaved'),
          message: t('useSchedule.bellSavedSilenced'),
        });
        return;
      }
      showToast({
        type: 'success',
        title: t('useSchedule.notificationsOn'),
        message: t('useSchedule.notifiedWhenAiring', { title: pref.media_title }),
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
        title: pref.enabled ? t('useSchedule.couldNotTurnOn') : t('useSchedule.couldNotTurnOff'),
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

const LEAD_TIMES_KEY = ['bellLeadTimes'] as const;

/** Minutes before air time a bell can remind; 0 is no reminder. */
export const LEAD_CHOICES = [0, 10, 30, 60] as const;

/** How many minutes before an episode airs the bell of one anime reminds (0 = not before). */
export function useBellLeadTime(mediaId: number) {
  return useQuery({
    queryKey: LEAD_TIMES_KEY,
    queryFn: () => invoke<[number, number][]>('list_bell_lead_times'),
    staleTime: Infinity,
    select: (pairs) => pairs.find(([id]) => id === mediaId)?.[1] ?? 0,
  });
}

/** Sets a bell's reminder lead time; the bell must already be on. */
export function useSetBellLeadTime() {
  const queryClient = useQueryClient();
  const showToast = useUIStore((s) => s.showToast);
  return useMutation({
    mutationFn: ({ mediaId, minutes }: { mediaId: number; minutes: number }) =>
      invoke<void>('set_bell_lead_time', { mediaId, minutes }),
    onSuccess: (_data, { minutes }) => {
      if (minutes > 0) {
        showToast({
          type: 'success',
          title: t('useSchedule.reminderSet'),
          message:
            minutes >= 60
              ? t('useSchedule.remindedHoursBefore', { count: minutes / 60 })
              : t('useSchedule.remindedMinutesBefore', { count: minutes }),
        });
      }
    },
    onError: (error) =>
      showToast({ type: 'error', title: t('useSchedule.couldNotSetReminder'), message: errorMessage(error) }),
    onSettled: () => queryClient.invalidateQueries({ queryKey: LEAD_TIMES_KEY }),
  });
}

export function leadTimeLabel(minutes: number): string {
  if (minutes <= 0) return t('useSchedule.onlyWhenAirs');
  return minutes >= 60
    ? t('useSchedule.hoursBefore', { count: minutes / 60 })
    : t('useSchedule.minutesBefore', { count: minutes });
}
