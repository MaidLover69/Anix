import { useCallback, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
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

/** Test searches through plugins and their latest outcome per plugin id. */
export function usePluginHealth() {
  const [results, setResults] = useState<Record<string, HealthState>>({});
  const [testingAll, setTestingAll] = useState(false);

  const test = useCallback((id: string) => {
    setResults((current) => ({ ...current, [id]: 'testing' }));
    invoke<PluginHealth>('test_plugin', { id })
      .then((health) => setResults((current) => ({ ...current, [id]: health })))
      .catch((err) =>
        setResults((current) => ({
          ...current,
          [id]: { id, status: 'error', latency_ms: 0, results: 0, error: String(err) },
        }))
      );
  }, []);

  const testAll = useCallback((ids: string[]) => {
    setTestingAll(true);
    setResults((current) => {
      const next = { ...current };
      for (const id of ids) next[id] = 'testing';
      return next;
    });
    invoke<PluginHealth[]>('test_all_plugins')
      .then((list) =>
        setResults((current) => {
          const next = { ...current };
          for (const health of list) next[health.id] = health;
          return next;
        })
      )
      .catch(() => {})
      .finally(() => {
        setTestingAll(false);
        setResults((current) => {
          const next = { ...current };
          for (const id of ids) if (next[id] === 'testing') delete next[id];
          return next;
        });
      });
  }, []);

  return { results, test, testAll, testingAll };
}
