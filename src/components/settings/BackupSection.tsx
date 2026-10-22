import React, { useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { Download, Upload } from 'lucide-react';
import { Button } from '../ui/Button';
import { Spinner } from '../ui/Spinner';
import { errorMessage } from '../ui/ErrorState';
import { Row, Section } from './SettingsLayout';
import { INPUT_CLASS } from '../library/format';
import { useUIStore } from '../../stores/uiStore';
import { t } from '../../i18n';
import type { BackupResult } from '../../types';

/** Where Discover keeps its search history (see useSearchHistory). */
const SEARCH_HISTORY_KEY = 'anix-search-history';

function readSearchHistory(): unknown {
  try {
    return JSON.parse(window.localStorage.getItem(SEARCH_HISTORY_KEY) ?? '[]');
  } catch {
    return [];
  }
}

function writeSearchHistory(history: unknown) {
  if (!Array.isArray(history)) return;
  try {
    window.localStorage.setItem(SEARCH_HISTORY_KEY, JSON.stringify(history.filter((q) => typeof q === 'string')));
  } catch {
    // Storage can be unavailable; the rest of the restore still counts.
  }
}

function describe(result: BackupResult): string {
  const c = result.summary.counts;
  return [
    t('backupSection.watchlistCount', { count: c.watchlist ?? 0 }),
    t('backupSection.historyCount', { count: c.watch_history ?? 0 }),
    t('backupSection.playlistCount', { count: c.custom_playlists ?? 0 }),
  ].join(', ');
}

/** Settings › Data: one file with the watchlist, history, playlists, settings and searches. */
export const BackupSection: React.FC = () => {
  const queryClient = useQueryClient();
  const showToast = useUIStore((s) => s.showToast);
  const [passphrase, setPassphrase] = useState('');
  const [busy, setBusy] = useState<'export' | 'import' | null>(null);

  const run = async (kind: 'export' | 'import') => {
    setBusy(kind);
    try {
      const secret = passphrase.trim() || null;
      if (kind === 'export') {
        const result = await invoke<BackupResult | null>('export_backup', {
          searchHistory: readSearchHistory(),
          passphrase: secret,
        });
        if (result) {
          showToast({
            type: 'success',
            title: t(result.summary.has_logins ? 'backupSection.savedWithLogins' : 'backupSection.saved'),
            message: t('backupSection.savedMessage', { summary: describe(result), path: result.path }),
          });
        }
        return;
      }

      const result = await invoke<BackupResult | null>('import_backup', { passphrase: secret });
      if (!result) return;
      writeSearchHistory(result.summary.search_history);
      // Saving the restored settings once applies them (tray, autostart, release notifications).
      const settings = await invoke<Record<string, unknown>>('get_settings');
      await invoke('save_settings', { settings });
      await queryClient.invalidateQueries();
      showToast({
        type: result.summary.has_logins && !result.summary.logins_restored ? 'warning' : 'success',
        title: t('backupSection.restored'),
        message:
          result.summary.has_logins && !result.summary.logins_restored
            ? t('backupSection.restoredLoginsSkipped', { summary: describe(result) })
            : t('backupSection.restoredMessage', { summary: describe(result) }),
      });
    } catch (err) {
      showToast({
        type: 'error',
        title: t(kind === 'export' ? 'backupSection.couldNotSave' : 'backupSection.couldNotRestore'),
        message: errorMessage(err),
      });
    } finally {
      setBusy(null);
    }
  };

  return (
    <Section title={t('backupSection.title')} id="data">
      <Row
        label={t('backupSection.passphrase')}
        hint={t('backupSection.passphraseHint')}
        stacked
      >
        {({ labelId, hintId }) => (
          <input
            type="password"
            autoComplete="new-password"
            aria-labelledby={labelId}
            aria-describedby={hintId}
            value={passphrase}
            onChange={(event) => setPassphrase(event.target.value)}
            placeholder={t('backupSection.passphrasePlaceholder')}
            className={`${INPUT_CLASS} max-w-sm`}
          />
        )}
      </Row>
      <Row
        label={t('backupSection.backupRestore')}
        hint={t('backupSection.backupRestoreHint')}
      >
        {() => (
          <div className="flex flex-wrap items-center gap-2">
            <Button variant="secondary" size="sm" onClick={() => run('export')} disabled={busy !== null}>
              {busy === 'export' ? <Spinner size="sm" label={t('backupSection.savingBackup')} /> : <Download aria-hidden="true" className="h-4 w-4" />}
              {t('backupSection.export')}
            </Button>
            <Button variant="neutral" size="sm" onClick={() => run('import')} disabled={busy !== null}>
              {busy === 'import' ? <Spinner size="sm" label={t('backupSection.restoringBackup')} /> : <Upload aria-hidden="true" className="h-4 w-4" />}
              {t('backupSection.import')}
            </Button>
          </div>
        )}
      </Row>
    </Section>
  );
};
