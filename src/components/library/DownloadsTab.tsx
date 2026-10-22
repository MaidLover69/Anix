import React from 'react';
import { useNavigate } from 'react-router-dom';
import { Download } from 'lucide-react';
import type { UseQueryResult } from '@tanstack/react-query';
import { Button, EmptyState } from '../ui';
import { isDownloadActive } from '../../hooks/useDownloads';
import type { DownloadRecord } from '../../types';
import { t } from '../../i18n';

interface DownloadsTabProps {
  query: UseQueryResult<DownloadRecord[]>;
}

/** Downloads have their own screen; the Library tab points there. */
export const DownloadsTab: React.FC<DownloadsTabProps> = ({ query }) => {
  const navigate = useNavigate();
  const downloads = query.data ?? [];
  const active = downloads.filter(isDownloadActive).length;
  const failed = downloads.filter((d) => d.status.toUpperCase() === 'FAILED').length;
  const summary = [
    active > 0 ? t('downloadsTab.inQueue', { count: active }) : null,
    t('downloadsTab.saved', { count: downloads.length - active - failed }),
    failed > 0 ? t('downloadsTab.failed', { count: failed }) : null,
  ]
    .filter(Boolean)
    .join(' · ');

  return (
    <EmptyState
      icon={<Download aria-hidden="true" />}
      title={t('downloadsTab.title')}
      description={downloads.length > 0 ? summary : t('downloadsTab.description')}
      action={
        <Button variant="primary" onClick={() => navigate('/downloads')}>
          {t('downloadsTab.openDownloads')}
        </Button>
      }
    />
  );
};
