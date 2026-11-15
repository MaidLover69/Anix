import React, { useEffect, useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import { FileUp, ListVideo, Plus, X } from 'lucide-react';
import { Button, EmptyState, ErrorState, Spinner, errorMessage } from './ui';
import { useImportPlaylist, usePlaylists, type ImportResult } from '../hooks/usePlaylists';
import { usePlayful } from '../hooks/useSettings';
import type { CustomPlaylist } from '../types';
import { CreatePlaylistModal } from './library/CreatePlaylistModal';
import { PlaylistCollage } from './library/PlaylistCollage';
import { PlaylistDetail } from './library/PlaylistDetail';
import { Meme } from './meme/Meme';
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
  const importPlaylist = useImportPlaylist();
  const [imported, setImported] = useState<ImportResult | null>(null);
  const [importError, setImportError] = useState<string | null>(null);

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

  const runImport = async () => {
    if (importPlaylist.isPending) return;
    setImportError(null);
    setImported(null);
    try {
      const result = await importPlaylist.mutateAsync();
      if (result) setImported(result);
    } catch (err) {
      setImportError(errorMessage(err, t('playlistsTab.importError')));
    }
  };

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
        <div className="flex flex-wrap items-center justify-end gap-2">
          <Button variant="secondary" onClick={runImport} disabled={importPlaylist.isPending}>
            {importPlaylist.isPending ? (
              <Spinner size="sm" label={t('playlistsTab.importing')} />
            ) : (
              <FileUp className="w-4 h-4" aria-hidden="true" />
            )}
            <span>{t('playlistsTab.import')}</span>
          </Button>
          <Button variant="primary" onClick={() => setShowCreateModal(true)}>
            <Plus className="w-4 h-4" aria-hidden="true" />
            <span>{t('playlistsTab.newPlaylist')}</span>
          </Button>
        </div>
      </div>

      {(imported || importError) && (
        <div
          role={importError ? 'alert' : 'status'}
          className={`flex items-start gap-3 p-4 rounded-lg ${importError ? 'bg-state-error/10' : 'bg-bg-elevated'}`}
        >
          <div className="min-w-0 flex-1 text-[13px] select-text">
            {importError ? (
              <p className="text-state-error">{importError}</p>
            ) : imported ? (
              <>
                <p className="text-text-primary">
                  {t('playlistsTab.imported', { name: imported.playlist.name, count: imported.added })}
                </p>
                {imported.skipped.length > 0 && (
                  <>
                    <p className="mt-2 text-text-secondary">{t('playlistsTab.importSkipped', { count: imported.skipped.length })}</p>
                    <ul className="mt-1 max-h-32 overflow-y-auto list-disc ps-5 text-xs text-text-muted">
                      {imported.skipped.map((entry, i) => (
                        <li key={i}>{entry}</li>
                      ))}
                    </ul>
                  </>
                )}
              </>
            ) : null}
          </div>
          {imported && (
            <Button variant="ghost" size="sm" onClick={() => selectPlaylist(imported.playlist.id)}>
              {t('playlistsTab.openImported')}
            </Button>
          )}
          <Button
            icon
            variant="ghost"
            size="sm"
            aria-label={t('playlistDetail.dismiss')}
            onClick={() => {
              setImported(null);
              setImportError(null);
            }}
          >
            <X className="w-4 h-4" aria-hidden="true" />
          </Button>
        </div>
      )}

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
              <span>
                <Meme spot="newSaga" plain={t('playlistsTab.create')} />
              </span>
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
                    {playlist.rule ? t('playlistDetail.smartCollection') : t('common.episodeCount', { count: playlist.item_count })}
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
