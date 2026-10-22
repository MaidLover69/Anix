import React from 'react';
import { Info, WifiOff } from 'lucide-react';
import { useQueryClient } from '@tanstack/react-query';
import { useLocation } from 'react-router-dom';
import { useOfflineStatus } from '../hooks/useSystem';
import { locale, t } from '../i18n';

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
 */
export const OfflineBanner: React.FC = () => {
  const { offline, saved_at, fallback } = useOfflineStatus();
  const queryClient = useQueryClient();
  const { pathname } = useLocation();
  if (!offline && fallback) {
    if (!showsCatalogue(pathname)) return null;
    return (
      <div
        role="status"
        className="flex h-6 w-full flex-shrink-0 items-center justify-center gap-2 bg-bg-card px-3 text-xs text-text-secondary"
      >
        <Info aria-hidden="true" className="h-3.5 w-3.5" />
        <span className="truncate">{t('offlineBanner.fallback', { name: fallback })}</span>
      </div>
    );
  }
  if (!offline) return null;
  return (
    <div
      role="status"
      className="flex h-7 w-full flex-shrink-0 items-center justify-center gap-2 bg-state-warning/15 px-3 text-xs text-state-warning"
    >
      <WifiOff aria-hidden="true" className="h-3.5 w-3.5" />
      <span className="truncate">
        {saved_at
          ? t('offlineBanner.savedFrom', { time: savedTime(saved_at) })
          : t('offlineBanner.unreachable')}
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
