import { useEffect } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { create } from 'zustand';
import { errorMessage } from '../components/ui/ErrorState';
import { useUIStore } from '../stores/uiStore';
import type {
  AccountChangedEvent,
  AccountLoginEvent,
  AccountService,
  AccountsStatus,
  AccountSyncedEvent,
} from '../types';

export const ACCOUNT_SERVICES: AccountService[] = ['anilist', 'mal', 'comick'];

export const ACCOUNT_LABELS: Record<AccountService, string> = {
  anilist: 'AniList',
  mal: 'MyAnimeList',
  comick: 'Comick',
};

/** Services whose sign-in page opens in the system browser instead of an app window. */
export const BROWSER_LOGIN_SERVICES: AccountService[] = ['comick'];

/** Whether this build can connect `service` at all. */
export function isAccountAvailable(service: AccountService, status: AccountsStatus | undefined): boolean {
  return service !== 'comick' || (status?.comick_available ?? false);
}

const ACCOUNT_STATUS_KEY = ['accountStatus'] as const;

/** Which accounts are connected, and as whom. Kept current by `useAccountEvents`. */
export function useAccountStatus() {
  return useQuery<AccountsStatus>({
    queryKey: ACCOUNT_STATUS_KEY,
    queryFn: () => invoke<AccountsStatus>('get_account_status'),
    staleTime: Infinity,
    // A read of the local settings file.
    networkMode: 'always',
  });
}

/** Services whose sign-in window (or browser sign-in) is waiting. */
const usePendingLogins = create<{
  pending: Record<AccountService, boolean>;
  setPending: (service: AccountService, pending: boolean) => void;
}>((set) => ({
  pending: { anilist: false, mal: false, comick: false },
  setPending: (service, pending) =>
    set((state) => ({ pending: { ...state.pending, [service]: pending } })),
}));

export function useLoginPending(service: AccountService): boolean {
  return usePendingLogins((state) => state.pending[service]);
}

/**
 * Opens the sign-in window of a service, or brings it to the front when it is already open.
 * The outcome arrives through `useAccountEvents`.
 */
export function useStartAccountLogin() {
  const showToast = useUIStore((s) => s.showToast);
  const setPending = usePendingLogins((s) => s.setPending);
  return async (service: AccountService) => {
    setPending(service, true);
    try {
      await invoke('start_account_login', { service });
    } catch (err) {
      setPending(service, false);
      showToast({
        type: 'error',
        title: `Could not start the ${ACCOUNT_LABELS[service]} sign-in`,
        message: errorMessage(err),
      });
    }
  };
}

/** Stops a sign-in that is still waiting; it ends with a `cancelled` login event. */
export function useCancelAccountLogin() {
  const setPending = usePendingLogins((s) => s.setPending);
  return async (service: AccountService) => {
    try {
      await invoke('cancel_account_login', { service });
    } catch (err) {
      console.warn(`[Accounts] could not cancel the ${service} sign-in:`, err);
    }
    setPending(service, false);
  };
}

export function useDisconnectAccount() {
  const queryClient = useQueryClient();
  const showToast = useUIStore((s) => s.showToast);
  return useMutation({
    mutationFn: (service: AccountService) => invoke<AccountsStatus>('disconnect_account', { service }),
    onSuccess: (status, service) => {
      queryClient.setQueryData<AccountsStatus>(ACCOUNT_STATUS_KEY, status);
      showToast({ type: 'info', title: `Disconnected from ${ACCOUNT_LABELS[service]}` });
    },
    onError: (err, service) =>
      showToast({
        type: 'error',
        title: `Could not disconnect from ${ACCOUNT_LABELS[service]}`,
        message: errorMessage(err),
      }),
  });
}

/**
 * Listens to the backend's account events for as long as the caller is mounted: reports how
 * a sign-in ended, and refetches account state and the AniList list when they change.
 * Mount it once, in a component that lives as long as the app.
 */
export function useAccountEvents() {
  const queryClient = useQueryClient();
  const showToast = useUIStore((s) => s.showToast);
  const setPending = usePendingLogins((s) => s.setPending);

  useEffect(() => {
    let cancelled = false;
    const unlisten: Array<() => void> = [];
    const subscribe = <T>(event: string, handler: (payload: T) => void) => {
      listen<T>(event, (e) => handler(e.payload))
        .then((stop) => {
          // The effect may have been cleaned up before the listener was registered.
          if (cancelled) stop();
          else unlisten.push(stop);
        })
        .catch((err) => console.warn(`[Accounts] could not listen to ${event}:`, err));
    };

    subscribe<AccountLoginEvent>('account-login', ({ service, outcome, name, message }) => {
      setPending(service, false);
      const label = ACCOUNT_LABELS[service];
      if (outcome === 'connected') {
        showToast({
          type: 'success',
          title: name ? `Connected to ${label} as ${name}` : `Connected to ${label}`,
        });
      } else if (outcome === 'failed') {
        showToast({
          type: 'error',
          title: `Could not connect to ${label}`,
          message: message ?? undefined,
        });
      }
    });

    subscribe<AccountChangedEvent>('account-changed', ({ service, connected, message }) => {
      queryClient.invalidateQueries({ queryKey: ACCOUNT_STATUS_KEY });
      if (service === 'anilist') {
        if (connected) {
          queryClient.invalidateQueries({ queryKey: ['aniListViewer'] });
          queryClient.invalidateQueries({ queryKey: ['aniListLibrary'] });
        } else {
          queryClient.removeQueries({ queryKey: ['aniListViewer'] });
          queryClient.removeQueries({ queryKey: ['aniListLibrary'] });
        }
      }
      if (service === 'comick') {
        if (connected) queryClient.invalidateQueries({ queryKey: ['comickLibrary'] });
        else queryClient.removeQueries({ queryKey: ['comickLibrary'] });
      }
      if (!connected && message) {
        showToast({ type: 'error', title: `Signed out of ${ACCOUNT_LABELS[service]}`, message });
      }
    });

    subscribe<AccountSyncedEvent>('account-synced', ({ service }) => {
      if (service === 'comick') {
        queryClient.invalidateQueries({ queryKey: ['comickLibrary'] });
      }
      if (service === 'anilist') {
        queryClient.invalidateQueries({ queryKey: ['aniListLibrary'] });
        queryClient.invalidateQueries({ queryKey: ['aniListViewer'] });
      }
    });

    return () => {
      cancelled = true;
      unlisten.forEach((stop) => stop());
    };
  }, [queryClient, showToast, setPending]);
}
