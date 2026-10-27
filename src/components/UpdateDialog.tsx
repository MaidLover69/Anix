import React, { useEffect, useState } from 'react';
import { Download, RotateCw } from 'lucide-react';
import { useQuery } from '@tanstack/react-query';
import { useLocation } from 'react-router-dom';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Dialog } from './Dialog';
import { Button } from './ui/Button';
import { Spinner } from './ui/Spinner';
import { errorMessage } from './ui/ErrorState';
import { useUIStore } from '../stores/uiStore';
import { useCheckUpdate, useSaveSettings, useSettings } from '../hooks/useSettings';
import { shouldPromptUpdate } from '../lib/version';
import { useOpenExternal } from '../hooks/useOpenExternal';
import type { UpdateInstallSupport } from '../types';
import { locale, t } from '../i18n';

/** How often a pending update window looks for a moment with no other dialog open. */
const PROMPT_RETRY_MS = 3000;

type InstallState =
  | { step: 'idle' }
  | { step: 'installing'; fraction: number | null }
  | { step: 'installed' }
  | { step: 'failed'; message: string };

export const UpdateDialog: React.FC = () => {
  const updateDialogOpen = useUIStore((s) => s.updateDialogOpen);
  const setUpdateDialogOpen = useUIStore((s) => s.setUpdateDialogOpen);
  const { data: updateInfo } = useCheckUpdate();
  const openExternal = useOpenExternal();
  const [install, setInstall] = useState<InstallState>({ step: 'idle' });
  // Install shows only for an AppImage Anix may replace; other packages use the download page.
  const support = useQuery<UpdateInstallSupport>({
    queryKey: ['updateInstallSupport'],
    queryFn: () => invoke<UpdateInstallSupport>('get_update_install_support'), // i18n-ignore
    enabled: updateDialogOpen,
    staleTime: Infinity,
    retry: false,
  }).data;

  const settings = useSettings().data;
  const saveSettings = useSaveSettings().mutate;
  const path = useLocation().pathname;
  const latest = updateInfo?.version;
  const prompted = settings?.update_prompted_version;

  // A newer release opens this window once by itself, when no other dialog (setup, player
  // check) is showing and not over the player. Settings › About can still open it any time.
  useEffect(() => {
    if (!settings || updateDialogOpen || !shouldPromptUpdate(latest, prompted, path)) return;
    const timer = window.setInterval(() => {
      if (document.querySelector('[role="dialog"]')) return;
      window.clearInterval(timer);
      saveSettings({ update_prompted_version: latest });
      setUpdateDialogOpen(true);
    }, PROMPT_RETRY_MS);
    return () => window.clearInterval(timer);
  }, [settings, latest, prompted, path, updateDialogOpen, saveSettings, setUpdateDialogOpen]);

  const installing = install.step === 'installing';
  useEffect(() => {
    if (!installing) return;
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    listen<number>('update-progress', (event) => {
      setInstall((current) => (current.step === 'installing' ? { step: 'installing', fraction: event.payload } : current));
    })
      .then((stop) => {
        if (cancelled) stop();
        else unlisten = stop;
      })
      .catch(() => {});
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [installing]);

  if (!updateInfo) return null;

  const published = updateInfo.published_at ? new Date(updateInfo.published_at) : null;
  const publishedLabel =
    published && !Number.isNaN(published.getTime()) ? published.toLocaleDateString(locale()) : null;
  const canInstall = Boolean(updateInfo.appimage && support?.appimage);
  const close = () => {
    if (!installing) setUpdateDialogOpen(false);
  };

  const runInstall = async () => {
    setInstall({ step: 'installing', fraction: null });
    try {
      await invoke<string>('install_update');
      setInstall({ step: 'installed' });
    } catch (err) {
      setInstall({ step: 'failed', message: errorMessage(err, t('updateDialog.installFailed')) });
    }
  };

  const restart = async () => {
    try {
      await invoke('restart_after_update');
    } catch (err) {
      setInstall({ step: 'failed', message: errorMessage(err, t('updateDialog.restartFailed')) });
    }
  };

  const openPage = (
    <Button
      variant={canInstall ? 'ghost' : 'primary'}
      data-autofocus={canInstall ? undefined : true}
      disabled={installing}
      onClick={() => {
        openExternal(updateInfo.release_url);
        setUpdateDialogOpen(false);
      }}
    >
      {!canInstall && <Download aria-hidden="true" className="h-4 w-4" />}
      {t('updateDialog.openPage')}
    </Button>
  );

  const percent = install.step === 'installing' && install.fraction !== null ? Math.round(install.fraction * 100) : null;

  return (
    <Dialog
      isOpen={updateDialogOpen}
      onClose={close}
      title={t('updateDialog.title', { version: updateInfo.version })}
      maxWidth="max-w-md"
      footer={
        install.step === 'installed' ? (
          <>
            <Button variant="ghost" onClick={close}>
              {t('updateDialog.later')}
            </Button>
            <Button variant="primary" data-autofocus onClick={restart}>
              <RotateCw aria-hidden="true" className="h-4 w-4" />
              {t('updateDialog.restartNow')}
            </Button>
          </>
        ) : (
          <>
            <Button variant="ghost" onClick={close} disabled={installing}>
              {t('updateDialog.later')}
            </Button>
            {openPage}
            {canInstall && (
              <Button variant="primary" data-autofocus onClick={runInstall} disabled={installing}>
                {installing ? <Spinner size="sm" label={t('updateDialog.installingLabel')} /> : <Download aria-hidden="true" className="h-4 w-4" />}
                {installing
                  ? t('updateDialog.installing')
                  : install.step === 'failed'
                    ? t('common.tryAgain')
                    : t('updateDialog.install')}
              </Button>
            )}
          </>
        )
      }
    >
      <div className="flex flex-col gap-2">
        {publishedLabel && <p className="text-xs text-text-muted">{t('updateDialog.released', { date: publishedLabel })}</p>}
        <h3 className="text-[13px] font-medium text-text-primary">{t('updateDialog.releaseNotes')}</h3>
        <div className="max-h-56 overflow-y-auto whitespace-pre-wrap rounded-md ui-card bg-bg-card p-3 text-[13px] leading-relaxed text-text-secondary select-text">
          {updateInfo.changelog || t('updateDialog.noNotes')}
        </div>
        {canInstall && install.step === 'idle' && (
          <p className="text-xs text-text-muted">{t('updateDialog.installHint')}</p>
        )}
        {installing && (
          <div aria-live="polite" className="flex flex-col gap-1.5">
            <div
              role="progressbar"
              aria-label={t('updateDialog.downloadingLabel')}
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={percent ?? undefined}
              className="h-1.5 w-full overflow-hidden rounded-full bg-ink/10"
            >
              <div className="h-full rounded-full bg-accent-500" style={{ width: `${percent ?? 5}%` }} />
            </div>
            <p className="text-xs text-text-muted">
              {percent !== null && percent < 100
                ? t('updateDialog.downloadingPercent', { percent })
                : t('updateDialog.checkingInstalling')}
            </p>
          </div>
        )}
        {install.step === 'installed' && (
          <p role="status" className="text-xs text-state-success">
            {t('updateDialog.installed', { version: updateInfo.version })}
          </p>
        )}
        {install.step === 'failed' && (
          <p role="alert" className="break-words text-xs text-state-error select-text">
            {install.message}
          </p>
        )}
      </div>
    </Dialog>
  );
};
