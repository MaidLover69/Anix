import React, { useMemo, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Server } from 'lucide-react';
import { Modal } from './ui/Modal';
import { SegmentedControl } from './ui/SegmentedControl';
import { Spinner } from './ui/Spinner';
import { useStreamingProviders } from '../hooks/usePlugins';
import { NoPluginsNotice } from './NoPluginsNotice';
import { watchPath } from '../hooks/watchProgress';
import type { EpisodeItem as EpisodeItemType, EpisodesResult } from '../types';

type Category = 'sub' | 'dub';

interface ServerSelectDialogProps {
  isOpen: boolean;
  onClose: () => void;
  anilistId: number;
  animeTitle: string;
  episode: EpisodeItemType | null;
  /** Result of `useEpisodes`: every provider's episode lists. */
  episodesData?: EpisodesResult;
  isLoading?: boolean;
  /** Shown when the provider lists could not be loaded. */
  errorMessage?: string | null;
  /** Audio version selected when the dialog opens. Defaults to sub. */
  initialCategory?: Category;
  /** The provider currently playing, marked in the list. */
  activeProvider?: string | null;
  /** Called with the choice. Without it the dialog opens the player itself. */
  onSelectServer?: (provider: string, category: Category) => void;
  /** Portal target; the player passes the element it shows fullscreen. */
  container?: HTMLElement | null;
}

interface ServerListProps {
  episode: EpisodeItemType;
  episodesData: EpisodesResult;
  isLoading: boolean;
  errorMessage?: string | null;
  initialCategory: Category;
  activeProvider?: string | null;
  onSelect: (provider: string, category: Category) => void;
  /** Closes the dialog before the app leaves for Settings. */
  onLeave: () => void;
}

const ServerList: React.FC<ServerListProps> = ({
  episode,
  episodesData,
  isLoading,
  errorMessage,
  initialCategory,
  activeProvider,
  onSelect,
  onLeave,
}) => {
  const [category, setCategory] = useState<Category>(initialCategory);
  const { label: providerLabel, none: noPlugins, installed: installedPlugins } = useStreamingProviders();

  const servers = useMemo(
    () =>
      Object.keys(episodesData)
        .filter((name) =>
          (episodesData[name]?.[category] ?? []).some(
            (ep) => Math.abs(ep.number - episode.number) < 0.01
          )
        )
        .sort((a, b) =>
          providerLabel(a).localeCompare(providerLabel(b), undefined, { sensitivity: 'base' })
        ),
    [episodesData, category, episode.number, providerLabel]
  );

  if (noPlugins) {
    return (
      <div className="flex flex-col gap-4">
        <p className="text-[13px] text-text-secondary">Episode {episode.number}</p>
        <NoPluginsNotice
          installed={installedPlugins}
          detail="Servers come from streaming plugins. Add or turn one on in Settings to see them here."
          onOpenSettings={onLeave}
        />
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between gap-3">
        <p className="min-w-0 truncate text-[13px] text-text-secondary">
          Episode {episode.number}
          {episode.title && !/^episode\s+[\d.]+$/i.test(episode.title.trim())
            ? ` · ${episode.title}`
            : ''}
        </p>
        <SegmentedControl
          size="sm"
          aria-label="Audio version"
          value={category}
          onChange={setCategory}
          options={[
            { value: 'sub', label: 'Sub' },
            { value: 'dub', label: 'Dub' },
          ]}
        />
      </div>

      <div className="flex max-h-72 flex-col gap-1 overflow-y-auto pr-1">
        {isLoading ? (
          <div className="flex flex-col items-center gap-3 py-10 text-center">
            <Spinner size="lg" label="Loading servers" className="text-accent-300" />
            <p className="text-[13px] text-text-secondary">
              Asking every provider for this episode. This can take a while.
            </p>
          </div>
        ) : servers.length === 0 ? (
          <div className="flex flex-col items-center gap-1 py-10 text-center">
            <Server aria-hidden="true" className="mb-2 h-6 w-6 text-text-muted" />
            <p className="text-[13px] font-medium text-text-primary">
              {errorMessage
                ? 'The server list could not be loaded'
                : `No server lists this episode in ${category === 'dub' ? 'dub' : 'sub'}`}
            </p>
            <p className="max-w-sm break-words text-xs text-text-muted select-text">
              {errorMessage || 'Try the other audio version.'}
            </p>
          </div>
        ) : (
          servers.map((name, index) => {
            const current = name === activeProvider;
            return (
              <button
                key={name}
                type="button"
                data-autofocus={index === 0 ? '' : undefined}
                onClick={() => onSelect(name, category)}
                className="flex h-11 flex-shrink-0 items-center justify-between gap-3 rounded-md ui-inset bg-bg-surface px-3 text-left cursor-pointer transition-colors hover:bg-bg-input"
              >
                <span className="flex min-w-0 items-center gap-3">
                  <Server aria-hidden="true" className="h-4 w-4 flex-shrink-0 text-text-muted" />
                  <span className="truncate text-[13px] font-medium text-text-primary">
                    {providerLabel(name)}
                  </span>
                </span>
                {current && (
                  <span className="flex-shrink-0 rounded-sm bg-accent-tint px-2 py-0.5 text-[11px] font-semibold text-accent-300">
                    Playing
                  </span>
                )}
              </button>
            );
          })
        )}
      </div>
    </div>
  );
};

export const ServerSelectDialog: React.FC<ServerSelectDialogProps> = ({
  isOpen,
  onClose,
  anilistId,
  animeTitle,
  episode,
  episodesData = {},
  isLoading = false,
  errorMessage,
  initialCategory = 'sub',
  activeProvider,
  onSelectServer,
  container,
}) => {
  const navigate = useNavigate();

  if (!episode) return null;

  const handleSelect = (provider: string, category: Category) => {
    onClose();
    if (onSelectServer) onSelectServer(provider, category);
    else navigate(watchPath(anilistId, episode.number, category, provider));
  };

  return (
    <Modal
      isOpen={isOpen}
      onClose={onClose}
      title="Choose a server"
      header={<p className="mt-0.5 truncate text-xs text-text-muted">{animeTitle}</p>}
      container={container}
    >
      <ServerList
        episode={episode}
        episodesData={episodesData}
        isLoading={isLoading}
        errorMessage={errorMessage}
        initialCategory={initialCategory}
        activeProvider={activeProvider}
        onSelect={handleSelect}
        onLeave={onClose}
      />
    </Modal>
  );
};

export default ServerSelectDialog;
