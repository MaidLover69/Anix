import React, { useEffect, useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import { ListVideo, Plus } from 'lucide-react';
import { Button, EmptyState, ErrorState, Spinner } from './ui';
import { usePlaylists } from '../hooks/usePlaylists';
import { usePlayful } from '../hooks/useSettings';
import type { CustomPlaylist } from '../types';
import { CreatePlaylistModal } from './library/CreatePlaylistModal';
import { PlaylistCollage } from './library/PlaylistCollage';
import { PlaylistDetail } from './library/PlaylistDetail';
import { t } from '../i18n';

export const PlaylistsTab: React.FC = () => {
  const playlistsQuery = usePlaylists();
  const playlists = playlistsQuery.data ?? [];
  const playful = usePlayful();

  // The open playlist is in the URL so Back from the player returns to it.
  const [searchParams, setSearchParams] = useSearchParams();
  const selectedId = searchParams.get('playlist');
  const [showCreateModal, setShowCreateModal] = useState(false);
  // Shown until the refreshed list includes the playlist that was just created.
  const [justCreated, setJustCreated] = useState<CustomPlaylist | null>(null);

  const selected = selectedId
    ? playlists.find((playlist) => playlist.id === selectedId) ??
      (justCreated?.id === selectedId ? justCreated : null)
    : null;

  const selectPlaylist = (playlistId: string | null) => {
    const next = new URLSearchParams(searchParams);
    next.set('tab', 'playlists');
    if (playlistId) next.set('playlist', playlistId);
    else next.delete('playlist');
    setSearchParams(next, { replace: true });
  };

  // A playlist id that no longer exists (deleted, or an old link) falls back to the list.
  const selectionIsStale = !!selectedId && !selected && playlistsQuery.isSuccess && !playlistsQuery.isFetching;
  useEffect(() => {
    if (selectionIsStale) selectPlaylist(null);
  }, [selectionIsStale]);

  if (selected) {
    return (
      <PlaylistDetail
        key={selected.id}
        playlist={selected}
        onBack={() => selectPlaylist(null)}
        onDeleted={() => selectPlaylist(null)}
      />
    );
  }

  return (
    <div className="flex flex-col gap-6">
      <div className="flex items-center justify-between gap-4">
        <p className="text-[13px] text-text-muted">
          {t('playlistsTab.intro')}
        </p>
        <Button variant="primary" onClick={() => setShowCreateModal(true)}>
          <Plus className="w-4 h-4" aria-hidden="true" />
          <span>{t('playlistsTab.newPlaylist')}</span>
        </Button>
      </div>

      {playlistsQuery.isLoading || (selectedId && playlistsQuery.isFetching) ? (
        <div className="flex justify-center py-16">
          <Spinner size="lg" label={t('playlistsTab.loading')} className="text-text-muted" />
        </div>
      ) : playlistsQuery.isError ? (
        <ErrorState
          title={t('playlistsTab.loadError')}
          error={playlistsQuery.error}
          onRetry={() => playlistsQuery.refetch()}
        />
      ) : playlists.length === 0 ? (
        <EmptyState
          icon={<ListVideo className="w-8 h-8" aria-hidden="true" />}
          title={t('playlistsTab.empty')}
          description={
            playful
              ? t('playlistsTab.emptyHintPlayful')
              : t('playlistsTab.emptyHint')
          }
          mood="sleepy"
          kaomoji="(^_^)"
          action={
            <Button variant="primary" onClick={() => setShowCreateModal(true)}>
              <Plus className="w-4 h-4" aria-hidden="true" />
              <span>{t('playlistsTab.create')}</span>
            </Button>
          }
        />
      ) : (
        <ul className="grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-4">
          {playlists.map((playlist) => (
            <li key={playlist.id}>
              <button
                type="button"
                onClick={() => selectPlaylist(playlist.id)}
                className="flex flex-col w-full gap-2 p-2 rounded-lg ui-card bg-bg-card hover:bg-bg-elevated text-start cursor-pointer transition-colors"
              >
                <PlaylistCollage covers={playlist.covers} className="w-full aspect-video rounded-md" />
                <span className="flex flex-col gap-0.5 px-1 pb-1 min-w-0">
                  <span className="text-[13px] font-medium text-text-primary truncate">
                    {playlist.name}
                  </span>
                  <span className="text-xs text-text-muted truncate">
                    {t('common.episodeCount', { count: playlist.item_count })}
                    {playlist.description ? ` · ${playlist.description}` : ''}
                  </span>
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}

      {showCreateModal && (
        <CreatePlaylistModal
          onClose={() => setShowCreateModal(false)}
          onCreated={(playlist) => {
            setShowCreateModal(false);
            setJustCreated(playlist);
            selectPlaylist(playlist.id);
          }}
        />
      )}
    </div>
  );
};
