import React from 'react';
import { useSearchParams } from 'react-router-dom';
import { useWatchlist, useContinueWatching } from '../hooks/useLibrary';
import { useAllDownloads } from '../hooks/useDownloads';
import { usePlaylists } from '../hooks/usePlaylists';
import { useLocalScan } from '../hooks/useLocalScan';
import { Tabs } from '../components/ui';
import type { TabItem } from '../components/ui';
import { PlaylistsTab } from '../components/PlaylistsTab';
import { WatchlistTab } from '../components/library/WatchlistTab';
import { HistoryTab } from '../components/library/HistoryTab';
import { DownloadsTab } from '../components/library/DownloadsTab';
import { AniListTab } from '../components/library/AniListTab';
import { LocalFilesTab } from '../components/library/LocalFilesTab';

const TAB_VALUES = ['watchlist', 'history', 'playlists', 'downloads', 'anilist', 'local'] as const;
type LibraryTab = (typeof TAB_VALUES)[number];

function parseTab(value: string | null): LibraryTab {
  return TAB_VALUES.find((tab) => tab === value) ?? 'watchlist';
}

export const Library: React.FC = () => {
  // The tab lives in the URL so Back from an anime or the player returns to the same tab.
  const [searchParams, setSearchParams] = useSearchParams();
  const activeTab = parseTab(searchParams.get('tab'));
  const setActiveTab = (tab: LibraryTab) => {
    setSearchParams(tab === 'watchlist' ? {} : { tab }, { replace: true });
  };

  const watchlistQuery = useWatchlist();
  const historyQuery = useContinueWatching();
  const playlistsQuery = usePlaylists();
  const downloadsQuery = useAllDownloads({ watch: activeTab === 'downloads' });
  // Kept here so a scan and its results survive switching tabs.
  const localScan = useLocalScan({ active: activeTab === 'local' });

  const tabs: TabItem<LibraryTab>[] = [
    { value: 'watchlist', label: 'Watchlist', count: watchlistQuery.data?.length },
    { value: 'history', label: 'History', count: historyQuery.data?.length },
    { value: 'playlists', label: 'Playlists', count: playlistsQuery.data?.length },
    { value: 'downloads', label: 'Downloads', count: downloadsQuery.data?.length },
    { value: 'anilist', label: 'AniList' },
    {
      value: 'local',
      label: 'Local files',
      count: localScan.status === 'done' ? localScan.files.length : undefined,
    },
  ];

  return (
    <div className="flex flex-col gap-6 pb-16 animate-fade-in">
      <Tabs aria-label="Library sections" tabs={tabs} value={activeTab} onChange={setActiveTab} />

      {activeTab === 'watchlist' && <WatchlistTab query={watchlistQuery} />}
      {activeTab === 'history' && <HistoryTab query={historyQuery} />}
      {activeTab === 'playlists' && <PlaylistsTab />}
      {activeTab === 'downloads' && <DownloadsTab query={downloadsQuery} />}
      {activeTab === 'anilist' && <AniListTab />}
      {activeTab === 'local' && <LocalFilesTab scan={localScan} />}
    </div>
  );
};
