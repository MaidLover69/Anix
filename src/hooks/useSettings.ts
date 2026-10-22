import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { getVersion } from '@tauri-apps/api/app';
import type { AppSettings, UpdateInfo } from '../types';
import { DEFAULT_THEME } from '../theme/themes';
import { isTimeFormat, type TimeFormat } from '../lib/time';
import { isNewerVersion } from '../lib/version';

export const DEFAULT_SETTINGS: AppSettings = {
  autoplay: true,
  auto_skip_intro_outro: false,
  skip_intro: false,
  skip_outro: false,
  prefer_dub: false,
  preferred_provider: 'auto',
  external_player: 'builtin',
  hide_adult_content: true,
  release_notifications: true,
  enable_doh: true,
  doh_mode: 'auto',
  doh_provider: 'cloudflare',
  catalogue_source: 'auto',
  download_quality: 'best',
  download_dir: '',
  theme: DEFAULT_THEME,
  minimize_to_tray: false,
  start_with_desktop: false,
  preferred_sub_languages: ['en'],
  preferred_dub_languages: ['en'],
  only_preferred_languages: true,
  window_buttons: 'desktop',
  use_system_accent: false,
  playful_touches: true,
  release_check_hours: 3,
  background_alerts: false,
  // Settings from before the key existed (1.2.0) load as embedded mpv.
  player_engine: 'mpv',
  hwdec_mode: 'auto',
  player_buffer_mb: 150,
  player_readahead_secs: 30,
  player_disk_cache: false,
  player_disk_cache_mb: 1024,
};

/** Stands for "no specific provider" wherever a provider key (a plugin id) is expected. */
export const AUTO_PROVIDER = 'auto';

const SETTINGS_KEY = ['settings'] as const;

/** Account keys (`anilist_*`, `mal_*`) belong to the backend; see `useAccounts`. */
const isAccountKey = (key: string) => key.startsWith('anilist_') || key.startsWith('mal_');

type StoredSettings = Record<string, unknown>;

function pick(stored: StoredSettings, keep: (key: string) => boolean): StoredSettings {
  return Object.fromEntries(Object.entries(stored).filter(([key]) => keep(key)));
}

async function loadSettings(): Promise<AppSettings> {
  const stored = await invoke<StoredSettings>('get_settings');
  // 1.2.0 had one switch for both; it becomes the value of each.
  const oldSkip = stored.auto_skip_intro_outro === true;
  return {
    ...DEFAULT_SETTINGS,
    skip_intro: oldSkip,
    skip_outro: oldSkip,
    // Before `doh_mode`, the DNS-over-HTTPS switch was on or off; a saved one keeps its value.
    ...(typeof stored.enable_doh === 'boolean' ? { doh_mode: stored.enable_doh ? 'on' : 'off' } : {}),
    ...pick(stored, (key) => !isAccountKey(key)),
  };
}

/**
 * `settings` plus whatever account keys the backend hands out, as they are stored right now.
 * A save writes the whole object, so it must never pass on an older copy of them.
 */
async function withStoredAccountKeys(settings: AppSettings): Promise<StoredSettings> {
  const stored = await invoke<StoredSettings>('get_settings');
  return { ...settings, ...pick(stored, isAccountKey) };
}

export function useSettings() {
  return useQuery<AppSettings>({
    queryKey: SETTINGS_KEY,
    queryFn: loadSettings,
    staleTime: Infinity,
  });
}

/** Settings › Appearance › Time format, for `formatTime` (lib/time.ts). */
export function useTimeFormat(): TimeFormat {
  const value = useSettings().data?.time_format;
  return isTimeFormat(value) ? value : 'system';
}

/** The "Playful touches" switch; on unless it was turned off (older settings lack the key). */
export function usePlayful(): boolean {
  return useSettings().data?.playful_touches !== false;
}

export function useSaveSettings() {
  const queryClient = useQueryClient();
  return useMutation({
    // Saves write the whole object, so they run one at a time and each one merges
    // into the result of the one before it.
    scope: { id: 'settings' },
    mutationFn: async (changes: Partial<AppSettings>) => {
      // Never merge into defaults: saving before the stored settings have been read
      // would overwrite them.
      const current: AppSettings =
        queryClient.getQueryData<AppSettings>(SETTINGS_KEY) ??
        (await queryClient.fetchQuery<AppSettings>({ queryKey: SETTINGS_KEY, queryFn: loadSettings }));
      // `changes` never carries explicit undefined values, so the merge is a full object.
      const merged = { ...current, ...changes } as AppSettings;
      queryClient.setQueryData<AppSettings>(SETTINGS_KEY, merged);
      await invoke('save_settings', { settings: await withStoredAccountKeys(merged) });
      return merged;
    },
    onError: () => {
      // The optimistic value may not be on disk; read the file again.
      queryClient.invalidateQueries({ queryKey: SETTINGS_KEY });
    },
  });
}

export { isNewerVersion };

export function useAppVersion() {
  return useQuery<string>({
    queryKey: ['appVersion'],
    queryFn: () => getVersion(),
    staleTime: Infinity,
    gcTime: Infinity,
    retry: false,
  });
}

/** The latest release if it is newer than the running app, otherwise null. */
export function useCheckUpdate() {
  return useQuery<UpdateInfo | null>({
    queryKey: ['checkUpdate'],
    queryFn: async () => {
      const [latest, current] = await Promise.all([
        invoke<UpdateInfo | null>('check_for_update'),
        getVersion(),
      ]);
      return latest && isNewerVersion(latest.version, current) ? latest : null;
    },
    staleTime: 60 * 60 * 1000,
    retry: 1,
  });
}
