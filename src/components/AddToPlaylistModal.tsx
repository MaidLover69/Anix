import React, { useRef, useState } from 'react';
import { Plus } from 'lucide-react';
import { Button, ErrorState, Modal, Spinner, errorMessage } from './ui';
import { usePlaylists, useCreatePlaylist, useAddMultipleToPlaylist } from '../hooks/usePlaylists';
import { useUIStore } from '../stores/uiStore';
import type { EpisodeItem, CustomPlaylist, CustomPlaylistItem } from '../types';
import { INPUT_CLASS } from './library/format';
import { PlaylistCollage } from './library/PlaylistCollage';

interface AddToPlaylistModalProps {
  isOpen: boolean;
  onClose: () => void;
  anilistId: number;
  malId?: number | null;
  seriesTitle: string;
  seriesCover: string | null;
  category?: 'sub' | 'dub';
  /** One episode or several. */
  episodes: EpisodeItem[];
}

export const AddToPlaylistModal: React.FC<AddToPlaylistModalProps> = ({
  isOpen,
  onClose,
  anilistId,
  malId = null,
  seriesTitle,
  seriesCover,
  category = 'sub',
  episodes,
}) => {
  const playlistsQuery = usePlaylists();
  const playlists = playlistsQuery.data ?? [];
  const createPlaylist = useCreatePlaylist();
  const addItems = useAddMultipleToPlaylist();
  const showToast = useUIStore((state) => state.showToast);

  const [formOpen, setFormOpen] = useState(false);
  const [newName, setNewName] = useState('');
  const [newDescription, setNewDescription] = useState('');
  const [addingTo, setAddingTo] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const busy = createPlaylist.isPending || addItems.isPending;
  // `isPending` only changes on the next render, so a ref is what stops a fast double click.
  const submittingRef = useRef(false);
  const trimmedName = newName.trim();
  // With nothing to pick from, the form is the only thing to show.
  const showForm = formOpen || (playlistsQuery.isSuccess && playlists.length === 0);

  const episodeSummary =
    episodes.length === 1
      ? `${seriesTitle} · Episode ${episodes[0].number}`
      : `${episodes.length} episodes of ${seriesTitle}`;

  const handleClose = () => {
    if (!busy) onClose();
  };

  const addTo = async (playlistId: string, playlistName: string): Promise<boolean> => {
    setAddingTo(playlistId);
    // Milliseconds. The sort key is time-based so a later batch always lands after earlier ones.
    const now = Date.now();
    const items: CustomPlaylistItem[] = episodes.map((episode, index) => ({
      id: crypto.randomUUID(),
      playlist_id: playlistId,
      anilist_id: anilistId,
      mal_id: malId ?? null,
      episode_num: episode.number,
      episode_title: episode.title || null,
      series_title: seriesTitle,
      series_cover: seriesCover,
      category,
      sort_order: now + index,
      added_at: now,
    }));

    try {
      await addItems.mutateAsync({ playlistId, items });
      showToast({
        type: 'success',
        title: `Added ${episodes.length} ${episodes.length === 1 ? 'episode' : 'episodes'} to "${playlistName}"`,
      });
      onClose();
      return true;
    } catch (err) {
      setError(`Could not add to "${playlistName}": ${errorMessage(err)}`);
      return false;
    } finally {
      setAddingTo(null);
    }
  };

  const handlePick = async (playlistId: string, playlistName: string) => {
    if (busy || submittingRef.current) return;
    submittingRef.current = true;
    setError(null);
    try {
      await addTo(playlistId, playlistName);
    } finally {
      submittingRef.current = false;
    }
  };

  const handleCreateAndAdd = async (event: React.FormEvent) => {
    event.preventDefault();
    if (!trimmedName || busy || submittingRef.current) return;
    submittingRef.current = true;
    setError(null);

    try {
      let created: CustomPlaylist;
      try {
        created = await createPlaylist.mutateAsync({
          name: trimmedName,
          description: newDescription.trim() || undefined,
        });
      } catch (err) {
        setError(`Could not create the playlist: ${errorMessage(err)}`);
        return;
      }

      const added = await addTo(created.id, created.name);
      if (!added) {
        // The playlist exists now; it is in the list for another try.
        setFormOpen(false);
        setNewName('');
        setNewDescription('');
      }
    } finally {
      submittingRef.current = false;
    }
  };

  return (
    <Modal
      isOpen={isOpen}
      onClose={handleClose}
      title="Add to playlist"
      size="md"
      header={<p className="mt-1 text-xs text-text-muted truncate">{episodeSummary}</p>}
    >
      <div className="flex flex-col gap-4">
        {error && (
          <p role="alert" className="text-xs text-state-error select-text break-words">
            {error}
          </p>
        )}

        {playlistsQuery.isLoading ? (
          <div className="flex justify-center py-8">
            <Spinner size="lg" label="Loading playlists" className="text-text-muted" />
          </div>
        ) : playlistsQuery.isError ? (
          <ErrorState
            compact
            title="Could not load your playlists"
            error={playlistsQuery.error}
            onRetry={() => playlistsQuery.refetch()}
          />
        ) : playlists.length === 0 ? (
          <p className="text-[13px] text-text-muted">
            You have no playlists yet. Create one to add {episodes.length === 1 ? 'this episode' : 'these episodes'}.
          </p>
        ) : (
          <ul className="flex flex-col gap-1 max-h-64 overflow-y-auto -mx-2">
            {playlists.map((playlist) => (
              <li key={playlist.id}>
                <button
                  type="button"
                  onClick={() => handlePick(playlist.id, playlist.name)}
                  disabled={busy}
                  aria-label={`Add to ${playlist.name}`}
                  className="flex items-center gap-3 w-full p-2 rounded-md text-left cursor-pointer hover:bg-white/5 transition-colors disabled:opacity-40 disabled:pointer-events-none focus-visible:-outline-offset-2"
                >
                  <PlaylistCollage
                    covers={playlist.covers.slice(0, 1)}
                    className="w-10 h-10 rounded-md flex-shrink-0"
                  />
                  <span className="flex flex-col flex-1 min-w-0">
                    <span className="text-[13px] font-medium text-text-primary truncate">
                      {playlist.name}
                    </span>
                    <span className="text-xs text-text-muted truncate">
                      {playlist.item_count} {playlist.item_count === 1 ? 'episode' : 'episodes'}
                    </span>
                  </span>
                  {addingTo === playlist.id ? (
                    <Spinner size="sm" label="Adding" className="text-text-muted" />
                  ) : (
                    <Plus className="w-4 h-4 flex-shrink-0 text-text-muted" aria-hidden="true" />
                  )}
                </button>
              </li>
            ))}
          </ul>
        )}

        {showForm ? (
          <form onSubmit={handleCreateAndAdd} className="flex flex-col gap-2">
            <label htmlFor="add-to-playlist-name" className="text-[13px] font-medium text-text-primary">
              New playlist
            </label>
            <input
              id="add-to-playlist-name"
              type="text"
              value={newName}
              onChange={(event) => setNewName(event.target.value)}
              placeholder="Name"
              maxLength={120}
              autoFocus
              data-autofocus
              disabled={busy}
              className={INPUT_CLASS}
            />
            <input
              type="text"
              value={newDescription}
              onChange={(event) => setNewDescription(event.target.value)}
              placeholder="Description (optional)"
              aria-label="Description (optional)"
              maxLength={500}
              disabled={busy}
              className={INPUT_CLASS}
            />
            <div className="flex items-center justify-end gap-2 mt-2">
              {playlists.length > 0 && (
                <Button variant="ghost" onClick={() => setFormOpen(false)} disabled={busy}>
                  Cancel
                </Button>
              )}
              <Button type="submit" variant="primary" disabled={!trimmedName || busy}>
                {createPlaylist.isPending
                  ? 'Creating…'
                  : addItems.isPending
                  ? 'Adding…'
                  : `Create and add ${episodes.length === 1 ? 'episode' : 'episodes'}`}
              </Button>
            </div>
          </form>
        ) : (
          !playlistsQuery.isLoading &&
          !playlistsQuery.isError && (
            <div>
              <Button variant="ghost" size="sm" onClick={() => setFormOpen(true)} disabled={busy}>
                <Plus className="w-4 h-4" aria-hidden="true" />
                <span>New playlist</span>
              </Button>
            </div>
          )
        )}
      </div>
    </Modal>
  );
};
