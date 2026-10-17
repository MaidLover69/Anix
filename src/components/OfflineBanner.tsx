import React from 'react';
import { WifiOff } from 'lucide-react';
import { useQueryClient } from '@tanstack/react-query';
import { useOfflineStatus } from '../hooks/useSystem';

function savedLabel(savedAt: number | null): string {
  if (!savedAt) return '';
  return ` from ${new Date(savedAt * 1000).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' })}`;
}

/** A thin strip under the title bar while AniList is out of reach and saved data is shown. */
export const OfflineBanner: React.FC = () => {
  const { offline, saved_at } = useOfflineStatus();
  const queryClient = useQueryClient();
  if (!offline) return null;
  return (
    <div
      role="status"
      className="flex h-7 w-full flex-shrink-0 items-center justify-center gap-2 bg-state-warning/15 px-3 text-xs text-state-warning"
    >
      <WifiOff aria-hidden="true" className="h-3.5 w-3.5" />
      <span className="truncate">
        {saved_at ? `Offline — showing saved data${savedLabel(saved_at)}` : 'Offline — AniList cannot be reached'}
      </span>
      <button
        type="button"
        onClick={() =>
          // Streams are left alone: refetching the one on screen would restart playback.
          queryClient.invalidateQueries({ predicate: (query) => query.queryKey[0] !== 'episodeSources' })
        }
        className="rounded px-1.5 font-medium underline-offset-2 hover:underline"
      >
        Retry
      </button>
    </div>
  );
};
