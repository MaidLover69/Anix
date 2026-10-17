import { useEffect, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { DesktopStatus, HardwareReport, OfflineStatus } from '../types';

const ONLINE: OfflineStatus = { offline: false, saved_at: null };

/** Whether AniList data is currently served from the saved copy; follows `offline-status`. */
export function useOfflineStatus(): OfflineStatus {
  const [status, setStatus] = useState<OfflineStatus>(ONLINE);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    listen<OfflineStatus>('offline-status', (event) => setStatus(event.payload ?? ONLINE))
      .then((stop) => {
        if (cancelled) stop();
        else unlisten = stop;
      })
      .catch((err) => console.debug('[Offline] listen failed (not running in Tauri?):', err));
    invoke<OfflineStatus>('get_offline_status')
      .then((initial) => {
        if (!cancelled) setStatus(initial);
      })
      .catch(() => {});

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  return status;
}

export const DESKTOP_STATUS_KEY = ['desktopStatus'] as const;

/** Tray and autostart state. */
export function useDesktopStatus() {
  return useQuery<DesktopStatus>({
    queryKey: DESKTOP_STATUS_KEY,
    queryFn: () => invoke<DesktopStatus>('get_desktop_status'),
    staleTime: 0,
    retry: false,
  });
}

/** The hardware-acceleration check. It runs `vainfo`, so only when asked for. */
export function useHardwareReport(enabled: boolean) {
  return useQuery<HardwareReport>({
    queryKey: ['hardwareReport'],
    queryFn: () => invoke<HardwareReport>('get_hardware_report'),
    enabled,
    staleTime: Infinity,
    retry: false,
  });
}
