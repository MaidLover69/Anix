import { useEffect, useMemo } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { AUTO_PROVIDER } from './useSettings';
import { t } from '../i18n';
import type { PluginInfo, PluginRepo } from '../types';

const PLUGINS_KEY = ['plugins'] as const;
const PLUGIN_REPOS_KEY = ['pluginRepos'] as const;

/** Route to Settings with the Plugins section scrolled into view and focused. */
export const PLUGIN_SETTINGS_PATH = '/settings?section=plugins';

/** Every installed plugin, whatever its state. Kept current by `usePluginEvents`. */
export function usePlugins() {
  return useQuery<PluginInfo[]>({
    queryKey: PLUGINS_KEY,
    queryFn: () => invoke<PluginInfo[]>('list_plugins'),
    staleTime: Infinity,
    retry: 1,
  });
}

export function usePluginRepos() {
  return useQuery<PluginRepo[]>({
    queryKey: PLUGIN_REPOS_KEY,
    queryFn: () => invoke<PluginRepo[]>('list_plugin_repos'),
    staleTime: Infinity,
    retry: 1,
  });
}

const NO_PLUGINS: PluginInfo[] = [];

export interface StreamingProviders {
  /** False until `list_plugins` has answered or failed. */
  settled: boolean;
  /** Enabled plugins that loaded, by name. These are the providers the backend asks. */
  usable: PluginInfo[];
  /** Plugins installed in any state. */
  installed: number;
  /** True once it is known that no plugin can serve a stream. Stays false if the list could not be read. */
  none: boolean;
  /** Plugin name for a provider key; `auto` reads "Auto" and unknown keys are shown as they are. */
  label: (key: string) => string;
  /**
   * Whether `key` can be asked for as a specific provider. When the plugin list could not be
   * read every key is passed on and the backend decides.
   */
  accepts: (key: string | null | undefined) => boolean;
  /** "Auto" plus every usable plugin, for a provider select. */
  options: { value: string; label: string }[];
}

/**
 * The streaming providers as the plugin list describes them. Every caller shares the one
 * `list_plugins` query.
 */
export function useStreamingProviders(): StreamingProviders {
  const query = usePlugins();
  const plugins = query.data ?? NO_PLUGINS;
  const settled = !query.isPending;
  const listed = query.data !== undefined;

  return useMemo(() => {
    const names = new Map(plugins.map((plugin) => [plugin.id, plugin.name.trim() || plugin.id]));
    const usable = plugins
      .filter((plugin) => plugin.enabled && plugin.status === 'ready')
      .sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: 'base' }));
    const usableIds = new Set(usable.map((plugin) => plugin.id));
    return {
      settled,
      usable,
      installed: plugins.length,
      none: listed && usable.length === 0,
      label: (key) => (key === AUTO_PROVIDER ? t('common.auto') : names.get(key) ?? key),
      accepts: (key) => !!key && key !== AUTO_PROVIDER && (!listed || usableIds.has(key)),
      options: [
        { value: AUTO_PROVIDER, label: t('usePlugins.autoOption') },
        ...usable.map((plugin) => ({ value: plugin.id, label: names.get(plugin.id) ?? plugin.id })),
      ],
    };
  }, [plugins, settled, listed]);
}

/** The stored preferred provider if it is a usable plugin, otherwise `auto`. */
export function usePreferredProvider(stored: string | null | undefined): string {
  const { accepts, settled } = useStreamingProviders();
  return settled && stored && accepts(stored) ? stored : AUTO_PROVIDER;
}

/** Downloads the repository's index and every provider plugin in it. */
export function useAddPluginRepo() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (url: string) => invoke<PluginRepo>('add_plugin_repo', { url }),
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: PLUGIN_REPOS_KEY });
      queryClient.invalidateQueries({ queryKey: PLUGINS_KEY });
    },
  });
}

export function useRemovePluginRepo() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (url: string) => invoke<void>('remove_plugin_repo', { url }),
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: PLUGIN_REPOS_KEY });
      queryClient.invalidateQueries({ queryKey: PLUGINS_KEY });
    },
  });
}

/** Downloads every repository again and picks up new plugin versions. */
export function useRefreshPluginRepos() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => invoke<PluginRepo[]>('refresh_plugin_repos'),
    onSuccess: (repos) => {
      queryClient.setQueryData<PluginRepo[]>(PLUGIN_REPOS_KEY, repos);
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: PLUGINS_KEY });
    },
  });
}

/** Turns a plugin on or off. The list shows the new state at once and goes back if it is refused. */
export function useSetPluginEnabled() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) =>
      invoke<void>('set_plugin_enabled', { id, enabled }),
    onMutate: async ({ id, enabled }) => {
      await queryClient.cancelQueries({ queryKey: PLUGINS_KEY });
      const previous = queryClient.getQueryData<PluginInfo[]>(PLUGINS_KEY)?.find((p) => p.id === id);
      queryClient.setQueryData<PluginInfo[]>(PLUGINS_KEY, (plugins) =>
        plugins?.map((plugin) =>
          plugin.id === id
            ? {
                ...plugin,
                enabled,
                // A plugin that failed to load stays failed whichever way the switch is.
                status: plugin.status === 'error' ? 'error' : enabled ? 'ready' : 'disabled',
              }
            : plugin
        )
      );
      return { previous };
    },
    onError: (_err, { id }, context) => {
      const previous = context?.previous;
      if (!previous) return;
      // Only this plugin goes back, so a switch flipped meanwhile on another one is kept.
      queryClient.setQueryData<PluginInfo[]>(PLUGINS_KEY, (plugins) =>
        plugins?.map((plugin) =>
          plugin.id === id ? { ...plugin, enabled: previous.enabled, status: previous.status } : plugin
        )
      );
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: PLUGINS_KEY });
    },
  });
}

/**
 * Refetches plugin state whenever the backend reports a change, for as long as the caller is
 * mounted. Mount it once, in a component that lives as long as the app.
 */
export function usePluginEvents() {
  const queryClient = useQueryClient();

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    listen('plugins-changed', () => {
      queryClient.invalidateQueries({ queryKey: PLUGINS_KEY });
      queryClient.invalidateQueries({ queryKey: PLUGIN_REPOS_KEY });
      // Provider episode lists depend on which plugins are enabled.
      queryClient.invalidateQueries({ queryKey: ['episodes'] });
      // Streams resolved earlier may come from a plugin that is gone. The one on screen is
      // left alone: refetching it would restart playback.
      queryClient.removeQueries({ queryKey: ['episodeSources'], type: 'inactive' });
    })
      .then((stop) => {
        // The effect may have been cleaned up before the listener was registered.
        if (cancelled) stop();
        else unlisten = stop;
      })
      .catch((err) => console.warn('[Plugins] could not listen to plugins-changed:', err));

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [queryClient]);
}
