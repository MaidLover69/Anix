import React from 'react';
import { Plus } from 'lucide-react';
import { Button } from '../ui/Button';
import { Spinner } from '../ui/Spinner';
import { errorMessage } from '../ui/ErrorState';
import { useAddPluginRepo } from '../../hooks/usePlugins';
import { usePluginDirectory, type HealthState } from '../../hooks/usePluginHealth';
import { useUIStore } from '../../stores/uiStore';
import type { PluginRepo, PluginUpdate } from '../../types';

const BADGE = 'inline-flex items-center rounded-sm px-1.5 py-0.5 text-[11px] font-semibold leading-[14px]';

/** The outcome of a plugin's test search. */
export const HealthBadge: React.FC<{ state: HealthState | undefined }> = ({ state }) => {
  if (!state) return null;
  if (state === 'testing') {
    return (
      <span className={`${BADGE} gap-1 bg-ink/[0.06] text-text-muted`}>
        <Spinner size="sm" label="Testing" />
        Testing
      </span>
    );
  }
  const seconds = `${(state.latency_ms / 1000).toFixed(state.latency_ms < 10_000 ? 1 : 0)} s`;
  const [tone, text] =
    state.status === 'ok'
      ? ['bg-state-success/15 text-state-success', `OK · ${seconds}`]
      : state.status === 'empty'
        ? ['bg-state-warning/15 text-state-warning', `No results · ${seconds}`]
        : state.status === 'timeout'
          ? ['bg-state-error/15 text-state-error', 'Timed out']
          : state.status === 'verification'
            ? ['bg-state-warning/15 text-state-warning', 'Needs verification']
            : ['bg-state-error/15 text-state-error', 'Error'];
  return (
    <span className={`${BADGE} ${tone}`} title={state.error ?? undefined}>
      {text}
    </span>
  );
};

export const UpdateBadge: React.FC<{ update: PluginUpdate | undefined }> = ({ update }) =>
  update ? (
    <span
      className={`${BADGE} bg-accent-500/15 text-accent-400`}
      title={`Installed ${update.installed || '?'}, available ${update.available || '?'}. Refresh all installs it.`}
    >
      Update available
    </span>
  ) : null;

/** "Discover": extension lists the project site recommends, with one-click Add. Shows nothing without any. */
export const PluginDirectory: React.FC<{ repos: PluginRepo[] }> = ({ repos }) => {
  const directory = usePluginDirectory();
  const addRepo = useAddPluginRepo();
  const showToast = useUIStore((s) => s.showToast);
  const lists = directory.data ?? [];
  if (lists.length === 0) return null;

  const added = (url: string) => repos.some((repo) => repo.url.replace(/\/+$/, '') === url.replace(/\/+$/, ''));

  const add = (url: string, name: string) => {
    addRepo.mutate(url, {
      onSuccess: (repo) =>
        showToast(
          repo.error
            ? { type: 'warning', title: `Added ${name}, with a problem`, message: repo.error }
            : { type: 'success', title: `Added ${name}` }
        ),
      onError: (err) => showToast({ type: 'error', title: `Could not add ${name}`, message: errorMessage(err) }),
    });
  };

  return (
    <div className="py-3">
      <p className="text-[13px] font-medium text-text-primary">Discover</p>
      <p className="mt-0.5 text-xs text-text-muted">Extension lists recommended on the Anix website.</p>
      <ul className="mt-3 flex flex-col gap-2">
        {lists.map((list) => {
          const isAdded = added(list.url);
          const pending = addRepo.isPending && addRepo.variables === list.url;
          return (
            <li key={list.url} className="flex items-start justify-between gap-3 rounded-md ui-inset bg-bg-surface px-3 py-2.5">
              <div className="min-w-0">
                <p className="flex flex-wrap items-baseline gap-x-2">
                  <span className="text-[13px] font-medium text-text-primary select-text">{list.name}</span>
                  {list.languages.length > 0 && (
                    <span className="text-xs text-text-muted">{list.languages.map((l) => l.toUpperCase()).join(' · ')}</span>
                  )}
                </p>
                {list.description && <p className="mt-0.5 text-xs text-text-muted select-text">{list.description}</p>}
                <p className="mt-0.5 truncate text-xs text-text-muted select-text">{list.url}</p>
              </div>
              <Button
                variant={isAdded ? 'ghost' : 'secondary'}
                size="sm"
                disabled={isAdded || addRepo.isPending}
                onClick={() => add(list.url, list.name)}
              >
                {pending ? <Spinner size="sm" label="Adding" /> : !isAdded && <Plus aria-hidden="true" className="h-3.5 w-3.5" />}
                {isAdded ? 'Added' : 'Add'}
              </Button>
            </li>
          );
        })}
      </ul>
    </div>
  );
};
