import { NotifyBell } from '../components/NotifyBell';
import React, { useMemo, useState } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { ArrowLeft, Play, Bookmark, DownloadCloud, Video, SearchX } from 'lucide-react';
import { ShimmerImage } from '../components/ShimmerImage';
import { EpisodeBrowser } from '../components/EpisodeBrowser';
import { TrailerModal, isPlayableTrailer } from '../components/TrailerModal';
import { CharacterRail } from '../components/CharacterRail';
import { RelationsRail } from '../components/RelationsRail';
import { AnimeThemesPlayer } from '../components/AnimeThemesPlayer';
import { Button } from '../components/ui/Button';
import { Chip } from '../components/ui/Chip';
import { EmptyState } from '../components/ui/EmptyState';
import { ErrorState, errorMessage } from '../components/ui/ErrorState';
import { useAnimeDetails, useMalAnimeDetails } from '../hooks/useAniList';
import { useAnimeCatalogEpisodes, useEpisodes } from '../hooks/useProviders';
import {
  useIsInWatchlist,
  useToggleWatchlist,
  useWatchedEpisodes,
  useContinueWatching,
} from '../hooks/useLibrary';
import { useSettings, AUTO_PROVIDER } from '../hooks/useSettings';
import { useStreamingProviders } from '../hooks/usePlugins';
import {
  getContinueTarget,
  historyProgressPercent,
  isHistoryEntryFinished,
  releasedEpisodeCount,
  watchPath,
} from '../hooks/watchProgress';
import { useUIStore } from '../stores/uiStore';
import { mediaFormatLabel } from '../components/library/format';
import type { AnimeDetails, EpisodeItem as EpisodeItemType, Media } from '../types';

function sentenceCase(value: string): string {
  const lower = value.replace(/_/g, ' ').toLowerCase();
  return lower.charAt(0).toUpperCase() + lower.slice(1);
}

function toMedia(details: AnimeDetails): Media {
  return {
    id: details.id,
    id_mal: details.idMal ?? null,
    title: {
      romaji: details.title?.romaji ?? null,
      english: details.title?.english ?? null,
      native: details.title?.native ?? null,
      user_preferred: details.title?.userPreferred ?? null,
    },
    cover_image: {
      extra_large: details.coverImage?.extraLarge ?? null,
      large: details.coverImage?.large ?? null,
      color: details.coverImage?.color ?? null,
    },
    banner_image: details.bannerImage ?? null,
    description: details.description ?? null,
    format: details.format ?? null,
    status: details.status ?? null,
    episodes: details.episodes ?? null,
    duration: details.duration ?? null,
    season: details.season ?? null,
    season_year: details.seasonYear ?? null,
    average_score: details.averageScore ?? null,
    mean_score: details.meanScore ?? null,
    popularity: details.popularity ?? null,
    favourites: details.favourites ?? null,
    genres: details.genres ?? [],
    is_adult: details.isAdult ?? false,
  };
}

const BackButton: React.FC = () => {
  const navigate = useNavigate();
  return (
    <Button
      variant="ghost"
      size="sm"
      className="-ml-3 self-start"
      onClick={() => {
        // Opened directly (deep link): there is nothing to go back to.
        if ((window.history.state?.idx ?? 0) > 0) navigate(-1);
        else navigate('/');
      }}
    >
      <ArrowLeft aria-hidden="true" className="h-4 w-4" />
      Back
    </Button>
  );
};

export const Detail: React.FC = () => {
  const { id } = useParams<{ id: string }>();
  // Keyed by id so nothing (expanded synopsis, episode filters) leaks from one anime to the next.
  return <DetailContent key={id} id={id} />;
};

const NO_EPISODES: EpisodeItemType[] = [];

const DetailContent: React.FC<{ id: string | undefined }> = ({ id }) => {
  const navigate = useNavigate();
  const anilistId = id && /^\d+$/.test(id) ? Number(id) : null;

  const openBulkDownloadDialog = useUIStore((s) => s.openBulkDownloadDialog);
  const showToast = useUIStore((s) => s.showToast);

  const detailsQuery = useAnimeDetails(anilistId);
  const details = detailsQuery.data as AnimeDetails | null | undefined;
  const malId = details?.idMal ?? null;
  const { data: malDetails } = useMalAnimeDetails(malId);
  const { data: inWatchlist = false } = useIsInWatchlist(anilistId);
  const toggleWatchlistMutation = useToggleWatchlist();
  const { data: watchedEpisodes = [] } = useWatchedEpisodes(anilistId);
  const { data: continueWatching = [] } = useContinueWatching();
  const { data: settings } = useSettings();
  const providers = useStreamingProviders();

  const [descriptionExpanded, setDescriptionExpanded] = useState(false);
  const [isTrailerOpen, setIsTrailerOpen] = useState(false);
  const [bannerFailed, setBannerFailed] = useState(false);

  // Episode lookups wait for the details: they need the MAL id and title, and running
  // them early would fan out to every provider twice. The arguments match the player's,
  // so both screens share one cached result.
  const lookupId = details ? anilistId : null;
  const titleRomaji = details?.title?.romaji ?? null;
  const { data: catalogEpisodes = NO_EPISODES, isLoading: catalogLoading } = useAnimeCatalogEpisodes(
    lookupId,
    malId,
    details?.episodes,
    titleRomaji,
    details?.status,
    details?.nextAiringEpisode?.episode
  );
  const {
    data: episodesData,
    isLoading: providersLoading,
    isError: providersFailed,
  } = useEpisodes(lookupId, malId, titleRomaji);

  const latestHistory = useMemo(
    () => continueWatching.find((h) => h.anilist_id === anilistId) ?? null,
    [continueWatching, anilistId]
  );

  const progressByEpisode = useMemo(
    () =>
      latestHistory && !isHistoryEntryFinished(latestHistory)
        ? { [latestHistory.episode_number]: historyProgressPercent(latestHistory) }
        : {},
    [latestHistory]
  );

  if (anilistId === null) {
    return (
      <div className="flex flex-col gap-4">
        <BackButton />
        <EmptyState
          icon={<SearchX aria-hidden="true" className="h-8 w-8" />}
          title="Anime not found"
          description={`"${id ?? ''}" is not a valid anime id.`}
          action={
            <Button variant="secondary" onClick={() => navigate('/')}>
              Go to Home
            </Button>
          }
        />
      </div>
    );
  }

  if (detailsQuery.isLoading) {
    return (
      <div aria-busy="true" className="flex flex-col gap-8 pb-16">
        <div className="flex flex-col gap-3">
          <BackButton />
          <div className="flex gap-6 rounded-2xl ui-card bg-bg-card p-6 pt-28">
            <div className="aspect-[2/3] w-48 flex-shrink-0 rounded-xl shimmer-placeholder" />
            <div className="flex flex-1 flex-col">
              <div className="h-[34px] w-2/3 rounded-md shimmer-placeholder" />
              <div className="mt-3 h-4 w-1/3 rounded-sm shimmer-placeholder" />
              <div className="mt-4 h-7 w-56 rounded-sm shimmer-placeholder" />
              <div className="mt-4 h-[88px] w-full max-w-3xl rounded-md shimmer-placeholder" />
              <div className="mt-6 h-9 w-72 rounded-md shimmer-placeholder" />
            </div>
          </div>
        </div>
        <div className="flex flex-col gap-3">
          <div className="h-7 w-28 rounded-md shimmer-placeholder" />
          <div className="h-9 w-full rounded-md shimmer-placeholder" />
        </div>
      </div>
    );
  }

  // The backend reports an unknown id as an error; that is "not found", not a failure to retry.
  const isMissing = detailsQuery.isError && /not found/i.test(String(detailsQuery.error));
  if (detailsQuery.isError && !isMissing) {
    return (
      <div className="flex flex-col gap-4">
        <BackButton />
        <ErrorState
          title="Could not load this anime"
          error={detailsQuery.error}
          onRetry={() => detailsQuery.refetch()}
          retryLabel={detailsQuery.isFetching ? 'Retrying…' : 'Retry'}
        />
      </div>
    );
  }

  if (!details) {
    return (
      <div className="flex flex-col gap-4">
        <BackButton />
        <EmptyState
          icon={<SearchX aria-hidden="true" className="h-8 w-8" />}
          title="Anime not found"
          description="AniList has no entry with this id."
          action={
            <Button variant="secondary" onClick={() => navigate('/')}>
              Go to Home
            </Button>
          }
        />
      </div>
    );
  }

  // MyAnimeList wording is preferred for the title and synopsis when it is available.
  const title =
    malDetails?.title_english ||
    malDetails?.title ||
    details.title?.english ||
    details.title?.userPreferred ||
    details.title?.romaji ||
    'Untitled';

  const bannerImage = bannerFailed ? null : details.bannerImage;
  const coverImage = details.coverImage?.extraLarge || details.coverImage?.large || null;

  const cleanDescription = (malDetails?.synopsis || details.description || '')
    .replace(/<[^>]*>?/gm, '')
    .trim();

  // Where the main button goes.
  const releasedNumbers = catalogEpisodes.map((ep) => ep.number);
  const released = releasedEpisodeCount(
    details.status,
    details.episodes,
    details.nextAiringEpisode?.episode
  );
  const continueTarget = getContinueTarget({
    latest: latestHistory,
    watched: watchedEpisodes,
    episodeNumbers: releasedNumbers,
    released,
  });
  const firstEpisode = releasedNumbers.length > 0 ? Math.min(...releasedNumbers) : 1;
  const hasReleasedEpisodes = releasedNumbers.length > 0 || released === null || released > 0;
  const defaultCategory = settings?.prefer_dub ? 'dub' : 'sub';
  const defaultProvider = settings?.preferred_provider || AUTO_PROVIDER;

  // Only carry a provider from history if it is a plugin that can still be asked.
  const historyProvider =
    continueTarget?.provider &&
    (providers.accepts(continueTarget.provider) || (episodesData && continueTarget.provider in episodesData))
      ? continueTarget.provider
      : null;

  const handlePlay = () => {
    navigate(
      watchPath(
        anilistId,
        continueTarget?.episode ?? firstEpisode,
        continueTarget?.category ?? defaultCategory,
        continueTarget ? historyProvider ?? AUTO_PROVIDER : defaultProvider
      )
    );
  };

  const handleSelectEpisode = (ep: EpisodeItemType, category: 'sub' | 'dub', provider: string) => {
    navigate(watchPath(anilistId, ep.number, category, provider));
  };

  const handleWatchlistToggle = () => {
    toggleWatchlistMutation.mutate(
      {
        anilistId,
        title,
        cover: coverImage,
        format: details.format,
        averageScore: details.averageScore,
        inWatchlist,
      },
      {
        onError: (err) =>
          showToast({
            type: 'error',
            title: 'Could not update the watchlist',
            message: errorMessage(err),
          }),
      }
    );
  };

  const seasonLabel = [details.season ? sentenceCase(details.season) : null, details.seasonYear]
    .filter(Boolean)
    .join(' ');

  const metadata = [
    mediaFormatLabel(details.format) || null,
    details.status ? sentenceCase(details.status) : null,
    seasonLabel || null,
    details.episodes ? `${details.episodes} ${details.episodes === 1 ? 'episode' : 'episodes'}` : null,
    details.duration ? `${details.duration} min` : null,
    // Scores are shown only where the source really has one.
    details.averageScore ? `AniList ${details.averageScore}%` : null,
    typeof malDetails?.score === 'number' && malDetails.score > 0
      ? `MAL ${malDetails.score.toFixed(2)}`
      : null,
  ]
    .filter(Boolean)
    .join(' · ');

  // Other spellings of the title, without repeats ("DEATH NOTE · DEATH NOTE") or a copy of
  // the heading that differs only in letter case.
  const alternateTitles = Array.from(
    new Set(
      [details.title?.romaji, details.title?.native].filter(
        (t): t is string => !!t && t.toLowerCase() !== title.toLowerCase()
      )
    )
  ).join(' · ');

  const isLongDescription = cleanDescription.length > 320;

  return (
    <div className="flex flex-col gap-8 pb-16 animate-fade-in">
      <div className="flex flex-col gap-3">
        <BackButton />

        <header className="relative overflow-hidden rounded-2xl ui-card bg-bg-card">
          {bannerImage && (
            <div aria-hidden="true" className="absolute inset-x-0 top-0 h-56">
              <img
                src={bannerImage}
                alt=""
                draggable={false}
                onError={() => setBannerFailed(true)}
                className="h-full w-full object-cover object-center opacity-70"
              />
              {/* Dense from the middle down, where the title and metadata sit on top of it. */}
              <div className="absolute inset-0 bg-gradient-to-b from-bg-card/10 via-bg-card/75 to-bg-card" />
            </div>
          )}

          <div className={`relative flex gap-6 p-6 ${bannerImage ? 'pt-28' : ''}`}>
            <ShimmerImage
              src={coverImage}
              alt={`${title} cover`}
              className="aspect-[2/3] w-48 flex-shrink-0 self-start rounded-xl bg-bg-elevated shadow-pop"
            />

            <div className="flex min-w-0 flex-1 flex-col">
              <h1 className="text-[28px] font-bold leading-[34px] tracking-[-0.02em] text-text-primary select-text">
                {title}
              </h1>
              {alternateTitles && (
                <p title={alternateTitles} className="mt-1 text-xs leading-[18px] text-text-muted line-clamp-2 select-text">
                  {alternateTitles}
                </p>
              )}
              {metadata && (
                <p className="mt-2 text-xs tabular-nums text-text-secondary select-text">{metadata}</p>
              )}

              {details.genres && details.genres.length > 0 && (
                <div className="mt-3 flex flex-wrap items-center gap-2">
                  {details.genres.map((genre) => (
                    <Chip key={genre} tone="accent">
                      {genre}
                    </Chip>
                  ))}
                </div>
              )}

              <div className="mt-4 max-w-3xl">
                <p
                  id="detail-synopsis"
                  className={`text-sm leading-[22px] text-text-secondary select-text ${
                    descriptionExpanded ? '' : 'line-clamp-4'
                  }`}
                >
                  {cleanDescription || 'No synopsis is available.'}
                </p>
                {isLongDescription && (
                  <button
                    type="button"
                    aria-expanded={descriptionExpanded}
                    aria-controls="detail-synopsis"
                    onClick={() => setDescriptionExpanded(!descriptionExpanded)}
                    className="mt-1 cursor-pointer rounded-sm text-[13px] font-medium text-accent-300 transition-colors hover:text-accent-200"
                  >
                    {descriptionExpanded ? 'Show less' : 'Show more'}
                  </button>
                )}
              </div>

              <div className="mt-6 flex flex-wrap items-center gap-2">
                <Button variant="primary" onClick={handlePlay} disabled={!hasReleasedEpisodes}>
                  <Play aria-hidden="true" className="h-4 w-4 fill-current" />
                  {!hasReleasedEpisodes
                    ? 'Not yet aired'
                    : continueTarget
                    ? `Continue Episode ${continueTarget.episode}`
                    : `Watch Episode ${firstEpisode}`}
                </Button>

                {/* A toggle: neutral while off, accent once the anime is saved. */}
                <Button
                  variant={inWatchlist ? 'secondary' : 'neutral'}
                  onClick={handleWatchlistToggle}
                  aria-pressed={inWatchlist}
                  disabled={toggleWatchlistMutation.isPending}
                >
                  <Bookmark aria-hidden="true" className={`h-4 w-4 ${inWatchlist ? 'fill-current' : ''}`} />
                  {inWatchlist ? 'In watchlist' : 'Add to watchlist'}
                </Button>

                {/* Kept together, so a narrow window moves the whole group to a second row
                    instead of leaving one button on its own. */}
                <div className="flex flex-wrap items-center gap-2">
                  {(details.status === 'RELEASING' || details.status === 'NOT_YET_RELEASED') && (
                    <NotifyBell mediaId={anilistId} title={title} cover={coverImage} variant="button" />
                  )}

                  {isPlayableTrailer(details.trailer) && (
                    <Button variant="neutral" onClick={() => setIsTrailerOpen(true)}>
                      <Video aria-hidden="true" className="h-4 w-4" />
                      Trailer
                    </Button>
                  )}

                  <Button
                    variant="neutral"
                    onClick={() => openBulkDownloadDialog(toMedia(details))}
                    disabled={!hasReleasedEpisodes}
                  >
                    <DownloadCloud aria-hidden="true" className="h-4 w-4" />
                    Bulk download
                  </Button>
                </div>
              </div>
            </div>
          </div>
        </header>
      </div>

      <section className="flex flex-col gap-3">
        <h2 className="text-xl font-semibold text-text-primary">Episodes</h2>
        <EpisodeBrowser
          anilistId={anilistId}
          malId={malId}
          animeTitle={title}
          animeCover={coverImage}
          catalogEpisodes={catalogEpisodes}
          isCatalogLoading={catalogLoading}
          episodesData={episodesData}
          isProvidersLoading={providersLoading}
          isProvidersError={providersFailed}
          defaultCategory={defaultCategory}
          defaultProvider={defaultProvider}
          progressByEpisode={progressByEpisode}
          onSelectEpisode={handleSelectEpisode}
        />
      </section>

      {details.relations?.edges && details.relations.edges.length > 0 && (
        <RelationsRail relations={details.relations.edges} />
      )}

      {details.characters?.edges && details.characters.edges.length > 0 && (
        <CharacterRail characters={details.characters.edges} />
      )}

      <AnimeThemesPlayer malId={malId} animeTitle={title} />

      <TrailerModal
        isOpen={isTrailerOpen}
        onClose={() => setIsTrailerOpen(false)}
        trailer={details.trailer}
        animeTitle={title}
      />
    </div>
  );
};
