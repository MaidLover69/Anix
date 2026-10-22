import React, { useState } from 'react';
import { Plus } from 'lucide-react';
import { Button } from '../ui/Button';
import { Modal } from '../ui/Modal';
import { Spinner } from '../ui/Spinner';
import { languageList } from '../../lib/languages';
import { errorMessage } from '../ui/ErrorState';
import { useAddPluginRepo } from '../../hooks/usePlugins';
import { usePluginDirectory, type HealthState } from '../../hooks/usePluginHealth';
import { useUIStore } from '../../stores/uiStore';
import { formatNumber, locale, t } from '../../i18n';
import type { PluginRepo, PluginUpdate } from '../../types';

const BADGE = 'inline-flex items-center rounded-sm px-1.5 py-0.5 text-[11px] font-semibold leading-[14px]';

/** "checked 3 h ago" for a test result's time, or null when it is not known. */
export function checkedLabel(checkedAtMs: number | undefined, now = Date.now()): string | null {
  if (!checkedAtMs || checkedAtMs <= 0) return null;
  const minutes = Math.max(0, Math.round((now - checkedAtMs) / 60_000));
  if (minutes < 1) return t('pluginTools.checkedJustNow');
  if (minutes < 60) return t('pluginTools.checkedMinutesAgo', { count: minutes });
  const hours = Math.round(minutes / 60);
  if (hours < 24) return t('pluginTools.checkedHoursAgo', { count: hours });
  const days = Math.round(hours / 24);
  return days === 1 ? t('pluginTools.checkedYesterday') : t('pluginTools.checkedDaysAgo', { count: days });
}

/** The outcome of a plugin's test search. */
export const HealthBadge: React.FC<{ state: HealthState | undefined }> = ({ state }) => {
  if (!state) return null;
  if (state === 'testing') {
    return (
      <span className={`${BADGE} gap-1 bg-ink/[0.06] text-text-muted`}>
        <Spinner size="sm" label={t('pluginTools.testing')} />
        {t('pluginTools.testing')}
      </span>
    );
  }
  const digits = state.latency_ms < 10_000 ? 1 : 0;
  const seconds = t('pluginTools.seconds', {
    value: formatNumber(state.latency_ms / 1000, { minimumFractionDigits: digits, maximumFractionDigits: digits }),
  });
  const [tone, text] =
    state.status === 'ok'
      ? ['bg-state-success/15 text-state-success', `${t('pluginTools.ok')} · ${seconds}`]
      : state.status === 'empty'
        ? ['bg-state-warning/15 text-state-warning', `${t('pluginTools.noResults')} · ${seconds}`]
        : state.status === 'timeout'
          ? ['bg-state-error/15 text-state-error', t('pluginTools.timedOut')]
          : state.status === 'verification'
            ? ['bg-state-warning/15 text-state-warning', t('pluginTools.needsVerification')]
            : ['bg-state-error/15 text-state-error', t('pluginTools.error')];
  const checked = checkedLabel(state.checked_at_ms);
  return (
    <>
      <span className={`${BADGE} ${tone}`} title={state.error ?? undefined}>
        {text}
      </span>
      {checked && (
        <span
          className="text-[11px] text-text-muted"
          title={new Date(state.checked_at_ms ?? 0).toLocaleString(locale(), { dateStyle: 'medium', timeStyle: 'short' })}
        >
          {checked}
        </span>
      )}
    </>
  );
};

/** "Update available"; with release notes it opens them. */
export const UpdateBadge: React.FC<{ update: PluginUpdate | undefined; name?: string }> = ({ update, name }) => {
  const [open, setOpen] = useState(false);
  if (!update) return null;
  const versions = t('pluginTools.versions', { installed: update.installed || '?', available: update.available || '?' });
  const notes = update.changelog?.trim();
  if (!notes) {
    return (
      <span className={`${BADGE} bg-accent-500/15 text-accent-400`} title={versions}>
        {t('pluginTools.updateAvailable')}
      </span>
    );
  }
  return (
    <>
      <button
        type="button"
        className={`${BADGE} cursor-pointer bg-accent-500/15 text-accent-400 hover:bg-accent-500/25`}
        title={versions}
        aria-haspopup="dialog"
        onClick={() => setOpen(true)}
      >
        {t('pluginTools.updateWhatsNew')}
      </button>
      <Modal
        isOpen={open}
        onClose={() => setOpen(false)}
        title={
          name
            ? t('pluginTools.whatsNewNamed', { name, version: update.available })
            : t('pluginTools.whatsNew', { version: update.available })
        }
        size="md"
        footer={
          <Button variant="primary" onClick={() => setOpen(false)}>
            {t('common.close')}
          </Button>
        }
      >
        <p className="whitespace-pre-wrap break-words text-[13px] leading-5 text-text-secondary select-text">{notes}</p>
        <p className="mt-3 text-xs text-text-muted">{versions}</p>
      </Modal>
    </>
  );
};

/** Plugin repositories the project site recommends, with one-click Add. Shows nothing without any. */
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
            ? { type: 'warning', title: t('pluginTools.addedWithProblem', { name }), message: repo.error }
            : { type: 'success', title: t('pluginTools.addedName', { name }) }
        ),
      onError: (err) => showToast({ type: 'error', title: t('pluginTools.couldNotAdd', { name }), message: errorMessage(err) }),
    });
  };

  return (
    <div className="py-3">
      <p className="text-[13px] font-medium text-text-primary">{t('pluginTools.recommended')}</p>
      <p className="mt-0.5 text-xs text-text-muted">{t('pluginTools.recommendedHint')}</p>
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
                    <span className="text-xs text-text-muted">{languageList(list.languages)}</span>
                  )}
                </p>
                {list.description && <p className="mt-0.5 text-xs text-text-muted select-text">{list.description}</p>}
                <p className="mt-0.5 truncate text-xs text-text-muted select-text">{list.url}</p>
              </div>
              {/* aria-disabled, not disabled: the clicked button keeps focus when it turns into "Added".
                  It stays :enabled, so pointer-events-none keeps the themes' :enabled:hover looks off it. */}
              <Button
                variant={isAdded ? 'ghost' : 'secondary'}
                size="sm"
                aria-disabled={isAdded || addRepo.isPending || undefined}
                className="aria-disabled:pointer-events-none aria-disabled:opacity-40"
                onClick={() => {
                  if (!isAdded && !addRepo.isPending) add(list.url, list.name);
                }}
              >
                {pending ? <Spinner size="sm" label={t('pluginTools.adding')} /> : !isAdded && <Plus aria-hidden="true" className="h-3.5 w-3.5" />}
                {isAdded ? t('pluginTools.added') : t('common.add')}
              </Button>
            </li>
          );
        })}
      </ul>
    </div>
  );
};
