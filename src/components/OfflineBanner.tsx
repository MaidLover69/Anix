import React from 'react';
import { HardDrive, Info, WifiOff } from 'lucide-react';
import { useQueryClient } from '@tanstack/react-query';
import { useLocation } from 'react-router-dom';
import { useDataDiskLow, useOfflineStatus } from '../hooks/useSystem';
import { locale, t } from '../i18n';
import { useFunMode } from '../hooks/useSettings';

function savedTime(savedAt: number): string {
  return new Date(savedAt * 1000).toLocaleString(locale(), { dateStyle: 'medium', timeStyle: 'short' });
}

/** Screens whose data comes from the catalogue; the others (Library, Settings, the player) show no "data from …" notice. */
export function showsCatalogue(pathname: string): boolean {
  return pathname === '/' || /^\/(discover|detail|schedule)(\/|$)/.test(pathname);
}

/**
 * A thin strip under the title bar while saved data is shown because no catalogue can be
 * reached, or a quieter one on catalogue screens while another catalogue stands in for AniList.
 * A full disk takes its place.
 */
export const OfflineBanner: React.FC = () => {
  const { offline, saved_at, fallback } = useOfflineStatus();
  const diskLow = useDataDiskLow();
  const fun = useFunMode();
  const queryClient = useQueryClient();
  const { pathname } = useLocation();
  // Comes first: with a full disk, extension and watch-progress saves fail and other errors mislead.
  if (diskLow) {
    return (
      <div
        role="alert"
        className="ui-banner flex h-7 w-full flex-shrink-0 items-center justify-center gap-2 border-b border-state-error/40 bg-state-error/[0.12] px-3 text-xs font-medium text-text-primary"
      >
        <HardDrive aria-hidden="true" className="h-3.5 w-3.5 flex-shrink-0 text-state-error" />
        <span className="truncate">{t('offlineBanner.diskFull')}</span>
      </div>
    );
  }
  if (!offline && fallback) {
    if (!showsCatalogue(pathname)) return null;
    return (
      <div
        role="status"
        data-tone="info"
        className="ui-banner flex h-6 w-full flex-shrink-0 items-center justify-center gap-2 border-b border-accent-500/40 bg-accent-tint px-3 text-xs font-medium text-text-primary"
      >
        <Info aria-hidden="true" className="h-3.5 w-3.5 flex-shrink-0 text-accent-300" />
        <span className="truncate">{t('offlineBanner.fallback', { name: fallback })}</span>
      </div>
    );
  }
  if (!offline) return null;
  return (
    <div
      role="status"
      className="ui-banner flex h-7 w-full flex-shrink-0 items-center justify-center gap-2 border-b border-state-warning/40 bg-state-warning/[0.12] px-3 text-xs font-medium text-text-primary"
    >
      <WifiOff aria-hidden="true" className="h-3.5 w-3.5 flex-shrink-0 text-state-warning" />
      <span className="truncate">
        {saved_at
          ? t(fun ? 'fun.offlineSaved' : 'offlineBanner.savedFrom', { time: savedTime(saved_at) })
          : t(fun ? 'fun.offlineUnreachable' : 'offlineBanner.unreachable')}
      </span>
      <button
        type="button"
        onClick={() =>
          // Streams are left alone: refetching the one on screen would restart playback.
          queryClient.invalidateQueries({ predicate: (query) => query.queryKey[0] !== 'episodeSources' })
        }
        className="rounded px-1.5 font-medium underline-offset-2 hover:underline"
      >
        {t('common.retry')}
      </button>
    </div>
  );
};
