import { useCallback, useEffect, useMemo, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { DirectoryList, PluginHealth, PluginUpdate } from '../types';

export const PLUGIN_UPDATES_KEY = ['pluginUpdates'] as const;

/** Installed plugins whose repository offers another version. Read once per session. */
export function usePluginUpdates(enabled: boolean) {
  return useQuery<PluginUpdate[]>({
    queryKey: PLUGIN_UPDATES_KEY,
    queryFn: () => invoke<PluginUpdate[]>('check_plugin_updates'),
    enabled,
    staleTime: 30 * 60 * 1000,
    retry: false,
  });
}

/** The extension lists the project site recommends; empty when it publishes none. */
export function usePluginDirectory() {
  return useQuery<DirectoryList[]>({
    queryKey: ['pluginDirectory'],
    queryFn: () => invoke<DirectoryList[]>('get_plugin_directory'),
    staleTime: 60 * 60 * 1000,
    retry: false,
  });
}

export type HealthState = PluginHealth | 'testing';

const PLUGIN_HEALTH_KEY = ['pluginHealth'] as const;

/**
 * Test searches through plugins and their latest outcome per plugin id: from Test, Test all and
 * the background check, which runs about once a day and updates these as it goes.
 */
export function usePluginHealth() {
  const queryClient = useQueryClient();
  const saved = useQuery<PluginHealth[]>({
    queryKey: PLUGIN_HEALTH_KEY,
    queryFn: () => invoke<PluginHealth[]>('get_plugin_health'),
    staleTime: Infinity,
    retry: false,
  });
  const [live, setLive] = useState<Record<string, HealthState>>({});
  const [testingAll, setTestingAll] = useState(false);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    listen('plugin-health-changed', () => queryClient.invalidateQueries({ queryKey: PLUGIN_HEALTH_KEY }))
      .then((stop) => {
        if (cancelled) stop();
        else unlisten = stop;
      })
      .catch((err) => console.debug('[pluginHealth] listen failed:', err));
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [queryClient]);

  // A newer saved result (from the background check) replaces one shown from an earlier test.
  const results = useMemo(() => {
    const merged: Record<string, HealthState> = { ...live };
    for (const health of saved.data ?? []) {
      const shown = merged[health.id];
      if (!shown || (shown !== 'testing' && (shown.checked_at_ms ?? 0) < (health.checked_at_ms ?? 0))) {
        merged[health.id] = health;
      }
    }
    return merged;
  }, [live, saved.data]);

  const test = useCallback((id: string) => {
    setLive((current) => ({ ...current, [id]: 'testing' }));
    invoke<PluginHealth>('test_plugin', { id })
      .then((health) => setLive((current) => ({ ...current, [id]: health })))
      .catch((err) =>
        setLive((current) => ({
          ...current,
          [id]: { id, status: 'error', latency_ms: 0, results: 0, error: String(err), checked_at_ms: Date.now() },
        }))
      );
  }, []);

  const testAll = useCallback((ids: string[]) => {
    setTestingAll(true);
    setLive((current) => {
      const next = { ...current };
      for (const id of ids) next[id] = 'testing';
      return next;
    });
    invoke<PluginHealth[]>('test_all_plugins')
      .then((list) =>
        setLive((current) => {
          const next = { ...current };
          for (const health of list) next[health.id] = health;
          return next;
        })
      )
      .catch(() => {})
      .finally(() => {
        setTestingAll(false);
        setLive((current) => {
          const next = { ...current };
          for (const id of ids) if (next[id] === 'testing') delete next[id];
          return next;
        });
      });
  }, []);

  return { results, test, testAll, testingAll };
}
