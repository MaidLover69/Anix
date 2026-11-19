import React, { useState } from 'react';
import { FolderOpen } from 'lucide-react';
import { Button } from '../ui/Button';
import { Spinner } from '../ui/Spinner';
import { errorMessage } from '../ui/ErrorState';
import { Row, Section } from './SettingsLayout';
import { CustomSelect } from '../CustomSelect';
import { useSaveSettings, useSettings } from '../../hooks/useSettings';
import { DOWNLOAD_QUALITY_VALUES } from '../../lib/resolution';
import { useDownloadDirectory, useDownloadFolder } from '../../hooks/useDownloads';
import { useUIStore } from '../../stores/uiStore';
import { t } from '../../i18n';

/** Settings › Downloads: the folder new downloads are saved in. */
export const DownloadSettings: React.FC = () => {
  const showToast = useUIStore((s) => s.showToast);
  const { data: settings } = useSettings();
  const save = useSaveSettings();
  const directory = useDownloadDirectory();
  const { change, reset } = useDownloadFolder();
  const [problem, setProblem] = useState<string | null>(null);
  const custom = Boolean(settings?.download_dir?.trim());
  const busy = change.isPending || reset.isPending;

  const hint = directory.data
    ? custom
      ? directory.data
      : t('downloadSettings.defaultFolder', { folder: directory.data })
    : directory.isError
      ? t('downloadSettings.folderUnusable', { error: errorMessage(directory.error) })
      : t('downloadSettings.reading');

  return (
    <Section title={t('settingsLayout.downloads')} id="downloads">
      <Row label={t('downloadSettings.label')} hint={hint} keywords={t('downloadSettings.keywords')}>
        {({ labelId, hintId }) => (
          <div role="group" aria-labelledby={labelId} aria-describedby={hintId} className="flex flex-col items-end gap-1.5">
            <div className="flex items-center gap-2">
              <Button
                variant="ghost"
                size="sm"
                disabled={busy || !custom}
                title={custom ? t('downloadSettings.resetTitle') : t('downloadSettings.defaultInUse')}
                onClick={() => {
                  setProblem(null);
                  reset.mutate(undefined, {
                    onSuccess: () => showToast({ type: 'info', title: t('downloadSettings.resetDone') }),
                    onError: (err) => setProblem(errorMessage(err)),
                  });
                }}
              >
                {t('downloadSettings.reset')}
              </Button>
              <Button
                variant="secondary"
                size="sm"
                disabled={busy}
                onClick={() => {
                  setProblem(null);
                  change.mutate(undefined, {
                    onSuccess: (picked) =>
                      picked && showToast({ type: 'success', title: t('downloadSettings.changed'), message: picked }),
                    onError: (err) => setProblem(errorMessage(err)),
                  });
                }}
              >
                {change.isPending ? (
                  <Spinner size="sm" label={t('downloadSettings.choosing')} />
                ) : (
                  <FolderOpen aria-hidden="true" className="h-3.5 w-3.5" />
                )}
                {t('downloadSettings.change')}
              </Button>
            </div>
            {problem && (
              <p role="alert" className="max-w-xs break-words text-end text-xs text-state-error select-text">
                {problem}
              </p>
            )}
          </div>
        )}
      </Row>
      <Row label={t('downloadSettings.downloadResolution')} hint={t('downloadSettings.downloadResolutionHint')}>
        {({ labelId, hintId }) => (
          <CustomSelect
            aria-labelledby={labelId}
            aria-describedby={hintId}
            className="w-36"
            value={DOWNLOAD_QUALITY_VALUES.includes(settings?.download_quality ?? '') ? settings?.download_quality ?? 'best' : 'best'}
            onChange={(value) => save.mutate({ download_quality: value })}
            options={DOWNLOAD_QUALITY_VALUES.map((value) => ({
              value,
              label: value === 'best' ? t('bulkDownloadDialog.best') : value,
            }))}
          />
        )}
      </Row>
    </Section>
  );
};
