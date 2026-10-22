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
import { ComickTab } from '../components/library/ComickTab';
import { useAccountStatus } from '../hooks/useAccounts';
import { LocalFilesTab } from '../components/library/LocalFilesTab';
import { StatsTab } from '../components/library/StatsTab';
import { t } from '../i18n';

const TAB_VALUES = ['watchlist', 'history', 'playlists', 'downloads', 'anilist', 'comick', 'local', 'stats'] as const;
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
  const accounts = useAccountStatus().data;
  const showComick = Boolean(accounts?.comick_available || accounts?.comick.connected);

  const tabs: TabItem<LibraryTab>[] = [
    { value: 'watchlist', label: t('library.watchlist'), count: watchlistQuery.data?.length },
    { value: 'history', label: t('library.history'), count: historyQuery.data?.length },
    { value: 'playlists', label: t('library.playlists'), count: playlistsQuery.data?.length },
    { value: 'downloads', label: t('library.downloads'), count: downloadsQuery.data?.length },
    { value: 'anilist', label: 'AniList' },
    ...(showComick ? [{ value: 'comick' as const, label: 'Comick' }] : []),
    {
      value: 'local',
      label: t('library.localFiles'),
      count: localScan.status === 'done' ? localScan.files.length : undefined,
    },
    { value: 'stats', label: t('library.stats') },
  ];

  return (
    <div className="flex flex-col gap-6 pb-16 animate-fade-in">
      <h1 className="sr-only">{t('library.title')}</h1>
      <Tabs
        aria-label={t('library.sections')}
        tabs={tabs}
        value={activeTab}
        onChange={setActiveTab}
        panelId="library-panel"
      />

      <div
        id="library-panel"
        role="tabpanel"
        aria-labelledby={`library-panel-${activeTab}`}
        className="flex flex-col gap-6"
      >
        {activeTab === 'watchlist' && <WatchlistTab query={watchlistQuery} />}
        {activeTab === 'history' && <HistoryTab query={historyQuery} />}
        {activeTab === 'playlists' && <PlaylistsTab />}
        {activeTab === 'downloads' && <DownloadsTab query={downloadsQuery} />}
        {activeTab === 'anilist' && <AniListTab />}
        {activeTab === 'comick' && <ComickTab />}
        {activeTab === 'local' && <LocalFilesTab scan={localScan} />}
        {activeTab === 'stats' && <StatsTab />}
      </div>
    </div>
  );
};
