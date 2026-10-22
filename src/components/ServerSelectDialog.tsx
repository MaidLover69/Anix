import React, { useMemo, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Server } from 'lucide-react';
import { Modal } from './ui/Modal';
import { SegmentedControl } from './ui/SegmentedControl';
import { Spinner } from './ui/Spinner';
import { usePlugins, useStreamingProviders } from '../hooks/usePlugins';
import { useLanguagePrefs } from '../hooks/useLanguagePrefs';
import { audioOnlyLabel, groupByLanguage, pluginLanguages } from '../lib/languages';
import { NoPluginsNotice } from './NoPluginsNotice';
import { watchPath } from '../hooks/watchProgress';
import { useSettings } from '../hooks/useSettings';
import { useRememberedSource } from '../hooks/useRememberedSource';
import type { EpisodeItem as EpisodeItemType, EpisodesResult } from '../types';
import { t } from '../i18n';

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
  /** "Switchable subtitles · 1080p" per `provider:category`, for servers that played in this visit. */
  streamFacts?: Record<string, string>;
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
  streamFacts?: Record<string, string>;
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
  streamFacts,
  onSelect,
  onLeave,
}) => {
  const [category, setCategory] = useState<Category>(initialCategory);
  const { label: providerLabel, none: noPlugins, installed: installedPlugins, usable } = useStreamingProviders();
  const plugins = usePlugins().data;
  const prefs = useLanguagePrefs();
  const order = useSettings().data?.provider_order;

  // In the order of Settings › Sources; servers it does not name follow by name.
  const servers = useMemo(() => {
    const rank = (name: string) => {
      const at = order?.indexOf(name) ?? -1;
      return at < 0 ? Number.MAX_SAFE_INTEGER : at;
    };
    return Object.keys(episodesData)
      .filter((name) =>
        (episodesData[name]?.[category] ?? []).some((ep) => Math.abs(ep.number - episode.number) < 0.01)
      )
      .sort(
        (a, b) =>
          rank(a) - rank(b) || providerLabel(a).localeCompare(providerLabel(b), undefined, { sensitivity: 'base' })
      );
  }, [episodesData, category, episode.number, providerLabel, order]);
  // An empty dub list is not a fault when no installed extension has dubs at all.
  const noDubAnywhere = category === 'dub' && usable.length > 0 && usable.every((p) => !p.dub_languages?.length);

  // Grouped by the language each server would play in, wanted languages first.
  const groups = useMemo(() => {
    const byId = new Map((plugins ?? []).map((plugin) => [plugin.id, plugin]));
    const wanted = category === 'dub' ? prefs.dub : prefs.sub;
    return groupByLanguage(servers, (name) => pluginLanguages(byId.get(name), category), wanted, category);
  }, [servers, plugins, prefs, category]);

  if (noPlugins) {
    return (
      <div className="flex flex-col gap-4">
        <p className="text-[13px] text-text-secondary">{t('common.episodeNumber', { number: episode.number })}</p>
        <NoPluginsNotice
          installed={installedPlugins}
          detail={t('serverSelectDialog.noPluginsDetail')}
          onOpenSettings={onLeave}
        />
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between gap-3">
        <p className="min-w-0 truncate text-[13px] text-text-secondary">
          {t('common.episodeNumber', { number: episode.number })}
          {episode.title && !/^episode\s+[\d.]+$/i.test(episode.title.trim())
            ? ` · ${episode.title}`
            : ''}
        </p>
        <SegmentedControl
          size="sm"
          aria-label={t('serverSelectDialog.audioVersion')}
          value={category}
          onChange={setCategory}
          options={[
            { value: 'sub', label: t('common.sub') },
            { value: 'dub', label: t('common.dub') },
          ]}
        />
      </div>

      <div className="flex max-h-72 flex-col gap-1 overflow-y-auto pe-1">
        {isLoading ? (
          <div className="flex flex-col items-center gap-3 py-10 text-center">
            <Spinner size="lg" label={t('serverSelectDialog.loadingServers')} className="text-accent-300" />
            <p className="text-[13px] text-text-secondary">
              {t('serverSelectDialog.askingEveryProvider')}
            </p>
          </div>
        ) : servers.length === 0 ? (
          <div className="flex flex-col items-center gap-1 py-10 text-center">
            <Server aria-hidden="true" className="mb-2 h-6 w-6 text-text-muted" />
            <p className="text-[13px] font-medium text-text-primary">
              {errorMessage
                ? t('serverSelectDialog.listFailed')
                : category === 'dub'
                ? t('serverSelectDialog.noServerDub')
                : t('serverSelectDialog.noServerSub')}
            </p>
            <p className="max-w-sm break-words text-xs text-text-muted select-text">
              {errorMessage ||
                (noDubAnywhere
                  ? t('serverSelectDialog.noDubAnywhere')
                  : prefs.strict
                  ? t('serverSelectDialog.tryOtherVersionStrict')
                  : t('serverSelectDialog.tryOtherVersion'))}
            </p>
          </div>
        ) : (
          groups.map((group, groupIndex) => (
            <div key={group.language ?? 'unstated'} role="group" aria-label={group.label} className="flex flex-col gap-1">
              <p
                className={`px-1 pb-0.5 text-[11px] font-semibold uppercase tracking-wider text-text-muted ${
                  groupIndex === 0 ? '' : 'pt-2'
                }`}
              >
                {group.label}
              </p>
              {group.items.map((name, index) => {
                const current = name === activeProvider;
                const only = audioOnlyLabel(plugins?.find((plugin) => plugin.id === name));
                const facts = streamFacts?.[`${name}:${category}`];
                return (
                  <button
                    key={name}
                    type="button"
                    data-autofocus={groupIndex === 0 && index === 0 ? '' : undefined}
                    onClick={() => onSelect(name, category)}
                    className="flex h-11 flex-shrink-0 items-center justify-between gap-3 rounded-md ui-inset bg-bg-surface px-3 text-start cursor-pointer transition-colors hover:bg-bg-input"
                  >
                    <span className="flex min-w-0 items-center gap-3">
                      <Server aria-hidden="true" className="h-4 w-4 flex-shrink-0 text-text-muted" />
                      <span className="flex min-w-0 flex-col">
                        <span className="truncate text-[13px] font-medium text-text-primary">
                          {providerLabel(name)}
                        </span>
                        {(facts || only) && (
                          <span className="truncate text-[11px] text-text-muted">
                            {[facts, only].filter(Boolean).join(' · ')}
                          </span>
                        )}
                      </span>
                    </span>
                    {current && (
                      <span className="flex-shrink-0 rounded-sm bg-accent-tint px-2 py-0.5 text-[11px] font-semibold text-accent-300">
                        {t('serverSelectDialog.playing')}
                      </span>
                    )}
                  </button>
                );
              })}
            </div>
          ))
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
  streamFacts,
  onSelectServer,
  container,
}) => {
  const navigate = useNavigate();
  const { remember } = useRememberedSource(anilistId);

  if (!episode) return null;

  const handleSelect = (provider: string, category: Category) => {
    // Tried first for this anime's next episodes.
    remember(provider, category);
    onClose();
    if (onSelectServer) onSelectServer(provider, category);
    else navigate(watchPath(anilistId, episode.number, category, provider));
  };

  return (
    <Modal
      isOpen={isOpen}
      onClose={onClose}
      title={t('serverSelectDialog.title')}
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
        streamFacts={streamFacts}
        onSelect={handleSelect}
        onLeave={onClose}
      />
    </Modal>
  );
};

export default ServerSelectDialog;
