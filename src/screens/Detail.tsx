import { NotifyBell } from '../components/NotifyBell';
import React, { useEffect, useMemo, useRef, useState } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { ArrowLeft, Play, DownloadCloud, Video, SearchX, HardDrive, Image as ImageIcon } from 'lucide-react';
import { ShimmerImage } from '../components/ShimmerImage';
import { EpisodeBrowser } from '../components/EpisodeBrowser';
import { AmbientGlow, hexToChannels } from '../components/AmbientGlow';
import { DetailStickyBar, ListStatusSelect } from '../components/DetailStickyBar';
import { TrailerModal, isPlayableTrailer } from '../components/TrailerModal';
import { CharacterRail } from '../components/CharacterRail';
import { RelationsRail } from '../components/RelationsRail';
import { WatchOrder } from '../components/WatchOrder';
import { Recommendations } from '../components/Recommendations';
import { rememberDub } from '../components/schedule/dubKnown';
import { AnimeThemesPlayer } from '../components/AnimeThemesPlayer';
import { LocalMediaDialog } from '../components/LocalMediaDialog';
import { LOCAL_PROVIDER, useLocalMedia } from '../hooks/useLocalMedia';
import { Button } from '../components/ui/Button';
import { Chip } from '../components/ui/Chip';
import { EmptyState } from '../components/ui/EmptyState';
import { ErrorState } from '../components/ui/ErrorState';
import { useAnimeDetails, useMalAnimeDetails } from '../hooks/useAniList';
import { useAnimeCatalogEpisodes, useEpisodes } from '../hooks/useProviders';
import {
  useWatchedEpisodes,
  useContinueWatching,
} from '../hooks/useLibrary';
import { useMemeLevel, usePlayful, useSettings, useSaveSettings, AUTO_PROVIDER } from '../hooks/useSettings';
import { PowerLevelMeter, Sfx } from '../components/FunBits';
import { useStreamingProviders } from '../hooks/usePlugins';
import { openWith, useRememberedSource } from '../hooks/useRememberedSource';
import {
  getContinueTarget,
  historyProgressPercent,
  isHistoryEntryFinished,
  releasedEpisodeCount,
  watchPath,
} from '../hooks/watchProgress';
import { useUIStore } from '../stores/uiStore';
import { mediaFormatLabel } from '../components/library/format';
import { CompletedStamp } from '../components/CompletedStamp';
import { Meme, MemeImage, useHighModule } from '../components/meme/Meme';
import { isNotFoundError } from '../lib/errorCodes';
import { parseAnimeId } from '../lib/routes';
import { detailScores } from '../lib/scores';
import type { AnimeDetails, EpisodeItem as EpisodeItemType, FuzzyDate, Media } from '../types';
import { formatNumber, locale, t, type TKey } from '../i18n';

function sentenceCase(value: string): string {
  const lower = value.replace(/_/g, ' ').toLowerCase();
  return lower.charAt(0).toUpperCase() + lower.slice(1);
}

const STATUS_KEYS: Record<string, TKey> = {
  FINISHED: 'detail.statusFinished',
  RELEASING: 'detail.statusReleasing',
  NOT_YET_RELEASED: 'detail.statusNotYetReleased',
  CANCELLED: 'detail.statusCancelled',
  HIATUS: 'detail.statusHiatus',
};

const SEASON_KEYS: Record<string, TKey> = {
  WINTER: 'detail.seasonWinter',
  SPRING: 'detail.seasonSpring',
  SUMMER: 'detail.seasonSummer',
  FALL: 'detail.seasonFall',
};

/** The label for an AniList enum value; a value without a key reads as written. */
function enumLabel(keys: Record<string, TKey>, value: string): string {
  const key = keys[value];
  return key ? t(key) : sentenceCase(value);
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

/** "12 Oct 2023", "Oct 2023" or "2023", as much as is known. */
function fuzzyDateLabel(date: FuzzyDate | null | undefined): string | null {
  if (!date?.year) return null;
  if (date.month && date.day) {
    return new Date(date.year, date.month - 1, date.day).toLocaleDateString(locale(), { dateStyle: 'medium' });
  }
  if (date.month) return new Date(date.year, date.month - 1, 1).toLocaleDateString(locale(), { month: 'short', year: 'numeric' });
  return String(date.year);
}

const FactRow: React.FC<{ label: React.ReactNode; children: React.ReactNode }> = ({ label, children }) => (
  <div className="flex flex-col gap-1">
    <dt className="text-[11px] font-semibold uppercase tracking-wide text-text-muted">{label}</dt>
    <dd className="text-[13px] text-text-secondary select-text">{children}</dd>
  </div>
);

/**
 * The facts card: studio, dates, genres, tags and the trailer. A sticky right-hand column on a
 * wide window (1200 px and up); narrower windows show it under the main column.
 */
const DetailFacts: React.FC<{ details: AnimeDetails; seasonLabel: string; onTrailer: () => void }> = ({
  details,
  seasonLabel,
  onTrailer,
}) => {
  const studios = [...(details.studios?.edges ?? [])]
    .sort((a, b) => Number(b.isMain) - Number(a.isMain))
    .map((edge) => edge.node.name)
    .filter((name, i, all) => name && all.indexOf(name) === i);
  const start = fuzzyDateLabel(details.startDate);
  const end = fuzzyDateLabel(details.endDate);
  const aired = start
    ? end && end !== start
      ? t('detail.airedRange', { start, end })
      : details.status === 'RELEASING'
        ? t('detail.airedOngoing', { start })
        : start
    : null;
  const tags = (details.tags ?? [])
    .filter((tag) => !tag.isMediaSpoiler)
    .sort((a, b) => (b.rank ?? 0) - (a.rank ?? 0))
    .slice(0, 10);
  const trailer = details.trailer;
  const playable = isPlayableTrailer(trailer);
  const thumbnail = playable ? trailer?.thumbnail || `https://i.ytimg.com/vi/${trailer?.id}/hqdefault.jpg` : null;

  return (
    // top-20 clears the sticky bar (DetailStickyBar).
    <aside aria-label={t('detail.facts')} className="flex flex-col gap-4 rounded-xl ui-card bg-bg-card p-4 min-[1200px]:sticky min-[1200px]:top-20">
      {playable ? (
        <button
          type="button"
          onClick={onTrailer}
          aria-label={t('detail.playTrailer')}
          className="group relative aspect-video w-full overflow-hidden rounded-md bg-bg-elevated"
        >
          {thumbnail && <ShimmerImage src={thumbnail} alt="" className="h-full w-full" />}
          <span className="dark-scope absolute inset-0 flex items-center justify-center bg-black/30 transition-colors group-hover:bg-black/45">
            <span className="flex items-center gap-1.5 rounded-full bg-black/70 px-3 py-1.5 text-xs font-medium text-white">
              <Video aria-hidden="true" className="h-4 w-4" />
              {t('detail.trailer')}
            </span>
          </span>
        </button>
      ) : (
        details.servedBy && <p className="text-xs text-text-muted">{t('detail.noTrailerFrom', { name: details.servedBy })}</p>
      )}
      <dl className="flex flex-col gap-3">
        {studios.length > 0 && (
          <FactRow label={<Meme spot="studio" plain={studios.length === 1 ? t('detail.studio') : t('detail.studios')} />}>
            {studios.join(', ')}
          </FactRow>
        )}
        {aired && <FactRow label={t('detail.aired')}>{aired}</FactRow>}
        {seasonLabel && <FactRow label={t('detail.season')}>{seasonLabel}</FactRow>}
        {details.genres && details.genres.length > 0 && (
          <FactRow label={t('detail.genres')}>
            <span className="mt-0.5 flex flex-wrap gap-1.5">
              {details.genres.map((genre) => (
                <Chip key={genre} tone="accent">
                  {genre}
                </Chip>
              ))}
            </span>
          </FactRow>
        )}
        {tags.length > 0 && (
          <FactRow label={t('detail.tags')}>
            <span className="mt-0.5 flex flex-wrap gap-1.5">
              {tags.map((tag) => (
                <Chip key={tag.name}>{tag.name}</Chip>
              ))}
            </span>
          </FactRow>
        )}
      </dl>
    </aside>
  );
};

const BackButton: React.FC = () => {
  const navigate = useNavigate();
  return (
    <Button
      variant="ghost"
      size="sm"
      className="-ms-3 self-start"
      onClick={() => {
        // Opened directly (deep link): there is nothing to go back to.
        if ((window.history.state?.idx ?? 0) > 0) navigate(-1);
        else navigate('/');
      }}
    >
      <ArrowLeft aria-hidden="true" className="h-4 w-4 rtl:-scale-x-100" />
      {t('common.back')}
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
  // Negative ids stand for anime AniList does not list (see the backend's `catalogue::ids`).
  const anilistId = parseAnimeId(id);

  const openBulkDownloadDialog = useUIStore((s) => s.openBulkDownloadDialog);

  const detailsQuery = useAnimeDetails(anilistId);
  const details = detailsQuery.data as AnimeDetails | null | undefined;
  const malId = details?.idMal ?? null;
  const { data: malDetails } = useMalAnimeDetails(malId);
  const { data: watchedEpisodes = [] } = useWatchedEpisodes(anilistId);
  const { data: continueWatching = [] } = useContinueWatching();
  const { data: settings } = useSettings();
  const saveSettings = useSaveSettings();
  const playful = usePlayful();
  // Medium's game bits; High has Jotaro's header instead.
  const medium = useMemeLevel() === 'medium';
  const high = useHighModule();
  const providers = useStreamingProviders();
  // The server last picked for this anime is what its episodes open with.
  const { remembered } = useRememberedSource(anilistId ?? 0);

  const [descriptionExpanded, setDescriptionExpanded] = useState(false);
  const [isTrailerOpen, setIsTrailerOpen] = useState(false);
  const [bannerFailed, setBannerFailed] = useState(false);
  const [localOpen, setLocalOpen] = useState(false);
  // The audio picked in the episode list or the sticky bar; null until the user picks one.
  const [chosenCategory, setChosenCategory] = useState<'sub' | 'dub' | null>(null);
  const [dubAvailable, setDubAvailable] = useState(true);
  const heroRef = useRef<HTMLElement>(null);
  const local = useLocalMedia(anilistId);

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
  // A plugin listing dub episodes is what the Schedule's "Dub available" filter knows.
  useEffect(() => {
    if (anilistId && episodesData && Object.values(episodesData).some((p) => (p?.dub?.length ?? 0) > 0)) rememberDub(anilistId);
  }, [anilistId, episodesData]);

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
          title={t('detail.notFound')}
          meme="notFound"
          description={t('detail.invalidId', { id: id ?? '' })}
          action={
            <Button variant="secondary" onClick={() => navigate('/')}>
              {t('detail.goHome')}
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

  // A catalogue answered that it has no such anime: "not found", not a failure to retry.
  const isMissing = detailsQuery.isError && isNotFoundError(detailsQuery.error);
  if (detailsQuery.isError && !isMissing) {
    return (
      <div className="flex flex-col gap-4">
        <BackButton />
        <ErrorState
          title={t('detail.couldNotLoad')}
          error={detailsQuery.error}
          onRetry={() => detailsQuery.refetch()}
          retryLabel={detailsQuery.isFetching ? t('detail.retrying') : t('common.retry')}
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
          title={t('detail.notFound')}
          meme="notFound"
          description={t('detail.noCatalogueEntry')}
          action={
            <Button variant="secondary" onClick={() => navigate('/')}>
              {t('detail.goHome')}
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
    t('detail.untitled');

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
  const watchedSet = new Set(watchedEpisodes);
  const finished =
    playful &&
    details.status === 'FINISHED' &&
    (details.episodes ?? 0) > 0 &&
    Array.from({ length: details.episodes! }, (_, i) => i + 1).every((n) => watchedSet.has(n));
  const continueTarget = getContinueTarget({
    latest: latestHistory,
    watched: watchedEpisodes,
    episodeNumbers: releasedNumbers,
    released,
  });
  const firstEpisode = releasedNumbers.length > 0 ? Math.min(...releasedNumbers) : 1;
  const hasReleasedEpisodes = releasedNumbers.length > 0 || released === null || released > 0;
  const { category: defaultCategory, provider: defaultProvider } = openWith(
    remembered,
    providers.accepts,
    settings?.prefer_dub,
    settings?.preferred_provider
  );

  // Only carry a provider from history if it is a plugin that can still be asked.
  const historyProvider =
    continueTarget?.provider &&
    (providers.accepts(continueTarget.provider) || (episodesData && continueTarget.provider in episodesData))
      ? continueTarget.provider
      : null;

  const handlePlay = () => {
    const episode = continueTarget?.episode ?? firstEpisode;
    const provider = continueTarget ? historyProvider ?? AUTO_PROVIDER : defaultProvider;
    // An episode with a file of the user's plays from it unless a streaming plugin was chosen.
    const fromFile =
      local.hasEpisode(episode) &&
      (provider === AUTO_PROVIDER || continueTarget?.provider === LOCAL_PROVIDER || providers.none);
    navigate(
      watchPath(
        anilistId,
        episode,
        chosenCategory ?? continueTarget?.category ?? defaultCategory,
        fromFile ? LOCAL_PROVIDER : provider
      )
    );
  };

  const handleSelectEpisode = (ep: EpisodeItemType, category: 'sub' | 'dub', provider: string) => {
    navigate(watchPath(anilistId, ep.number, category, provider));
  };

  const listTarget = {
    id: anilistId,
    title,
    cover: coverImage,
    format: details.format ?? null,
    averageScore: details.averageScore ?? null,
  };

  const seasonName = details.season ? enumLabel(SEASON_KEYS, details.season) : null;
  const seasonLabel =
    seasonName && details.seasonYear
      ? t('detail.seasonYear', { season: seasonName, year: details.seasonYear })
      : seasonName ?? (details.seasonYear ? String(details.seasonYear) : '');

  const scores = detailScores(details.averageScore, details.servedBy, malDetails?.score);
  // Above the title: what the anime is; below it: its numbers.
  const headerTags = [
    details.status ? enumLabel(STATUS_KEYS, details.status) : null,
    mediaFormatLabel(details.format) || null,
    seasonLabel || null,
  ].filter((tag): tag is string => !!tag);
  const metadata = [
    // Scores are shown only where the source really has one, credited to it, each source once.
    scores.percent ? t('detail.sourceScore', { name: scores.percent.source, score: scores.percent.score }) : null,
    scores.mal !== null
      ? t('detail.malScore', { score: formatNumber(scores.mal, { minimumFractionDigits: 2, maximumFractionDigits: 2 }) })
      : null,
    details.episodes ? t('common.episodeCount', { count: details.episodes }) : null,
    details.duration ? t('detail.durationMinutes', { minutes: details.duration }) : null,
  ].filter((item): item is string => !!item);
  const watchedCount = details.episodes
    ? Array.from(watchedSet).filter((n) => Number.isInteger(n) && n >= 1 && n <= details.episodes!).length
    : null;

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

  const playLabel = !hasReleasedEpisodes
    ? t('detail.notYetAired')
    : continueTarget
    ? t('detail.continueEpisode', { episode: continueTarget.episode })
    : t('detail.watchEpisode', { episode: firstEpisode });
  // The same audio the episode list shows; playing follows it once the user has picked one.
  const requestedCategory = chosenCategory ?? defaultCategory;
  const barCategory: 'sub' | 'dub' = requestedCategory === 'dub' && dubAvailable ? 'dub' : 'sub';
  const coverColor = details.coverImage?.color ?? null;
  const coverChannels = hexToChannels(coverColor);
  // "Use as background" takes the wide banner, else the cover.
  const backgroundUrl = details.bannerImage || coverImage;
  const isBackground = !!backgroundUrl && settings?.background_art?.url === backgroundUrl;

  return (
    // isolate: the header's wash and glow sit behind the page's content but above the app background.
    <div className="relative isolate flex flex-col gap-8 pb-16 animate-fade-in">
      <DetailStickyBar
        anchorRef={heroRef}
        title={title}
        cover={coverImage}
        listTarget={listTarget}
        playLabel={
          !hasReleasedEpisodes
            ? t('detail.notYetAired')
            : continueTarget
            ? t('detail.continueEp', { episode: continueTarget.episode })
            : t('detail.playEp', { episode: firstEpisode })
        }
        playDisabled={!hasReleasedEpisodes}
        onPlay={handlePlay}
        category={barCategory}
        dubAvailable={dubAvailable}
        onCategoryChange={setChosenCategory}
      />

      {/* Full width: the negative margins undo <main>'s padding (router.tsx), the padding puts it back. */}
      <header
        ref={heroRef}
        className="relative isolate -ms-6 -me-[18px] -mt-6 overflow-hidden pb-2 pe-[18px] ps-6 pt-4 xl:-ms-8 xl:-me-[26px] xl:-mt-8 xl:pe-[26px] xl:ps-8"
      >
        {medium && <Sfx text={t('fun.sfxMenacing')} className="fun-sfx-hero" />}
        {coverChannels && (
          <div
            aria-hidden="true"
            className="ui-detail-wash absolute inset-0 -z-10"
            style={{ '--ambient-rgb': coverChannels } as React.CSSProperties}
          />
        )}
        {bannerImage && (
          <div aria-hidden="true" className="absolute inset-x-0 top-0 -z-10 h-72">
            <img
              src={bannerImage}
              alt=""
              draggable={false}
              onError={() => setBannerFailed(true)}
              className="ui-detail-banner h-full w-full object-cover object-center"
            />
          </div>
        )}
        {/* Behind the poster: the cover colour, softly. */}
        <AmbientGlow color={coverColor} intensity={0.8} className="-z-10 start-[5%] top-[25%] h-[70%] w-[45%]" />

        <BackButton />

        <div className="ui-detail-head mt-6 grid grid-cols-[clamp(9rem,15vw,13rem)_minmax(0,1fr)] items-end gap-[clamp(1rem,2.5vw,2rem)]">
          <ShimmerImage
            src={coverImage}
            alt={t('detail.coverAlt', { title })}
            className="ui-detail-cover aspect-[2/3] w-full rounded-xl bg-bg-elevated shadow-pop"
          />

          <div className="flex min-w-0 flex-col gap-3">
            {headerTags.length > 0 && (
              <div className="ui-detail-tags flex flex-wrap items-center gap-1.5">
                {headerTags.map((tag, i) => (
                  <Chip key={tag} tone={i === 0 && details.status ? 'accent' : 'neutral'}>
                    {i === 0 && details.status === 'RELEASING' ? <Meme spot="airing" plain={tag} /> : tag}
                  </Chip>
                ))}
              </div>
            )}
            <div>
              <h1 className="text-[clamp(1.6rem,3vw,2.6rem)] font-black leading-tight tracking-[-0.02em] text-text-primary select-text">
                <bdi>{title}</bdi>
              </h1>
              {/* Read right after the title; drawn in the header's corner (the header is its positioned box). */}
              {finished && (
                <div className="ui-detail-stamp pointer-events-none absolute end-[clamp(1.5rem,4vw,3rem)] top-6 z-10">
                  <CompletedStamp />
                </div>
              )}
              {alternateTitles && (
                <p title={alternateTitles} className="mt-1 text-sm font-semibold text-text-secondary line-clamp-2 select-text">
                  {alternateTitles}
                </p>
              )}
            </div>
            {metadata.length > 0 && (
              <div className="flex flex-wrap items-center gap-1.5 tabular-nums select-text">
                {metadata.map((item) => (
                  <Chip key={item}>{item}</Chip>
                ))}
              </div>
            )}
            {medium && <PowerLevelMeter percentScore={scores.percent?.score ?? null} malScore={scores.mal} />}

            <div className="mt-1 flex flex-wrap items-center gap-2">
              <Button variant="primary" onClick={handlePlay} disabled={!hasReleasedEpisodes}>
                <Play aria-hidden="true" className="h-4 w-4 fill-current" />
                {playLabel}
              </Button>

              <ListStatusSelect target={listTarget} />

              {/* Kept together, so a narrow window moves the whole group to a second row
                  instead of leaving one button on its own. */}
              <div className="flex flex-wrap items-center gap-2">
                {(details.status === 'RELEASING' || details.status === 'NOT_YET_RELEASED') && (
                  <NotifyBell mediaId={anilistId} title={title} cover={coverImage} variant="button" />
                )}

                {isPlayableTrailer(details.trailer) && (
                  <Button variant="neutral" onClick={() => setIsTrailerOpen(true)}>
                    <Video aria-hidden="true" className="h-4 w-4" />
                    {t('detail.trailer')}
                  </Button>
                )}

                <Button
                  variant="neutral"
                  onClick={() => setLocalOpen(true)}
                  title={t('detail.localFilesHint')}
                >
                  <HardDrive aria-hidden="true" className="h-4 w-4" />
                  {local.data?.linked || local.episodes.length > 0
                    ? t('detail.yourFiles', { count: local.episodes.length })
                    : t('detail.loadOwnEpisodes')}
                </Button>

                <Button
                  variant="neutral"
                  onClick={() => openBulkDownloadDialog(toMedia(details))}
                  disabled={!hasReleasedEpisodes}
                >
                  <DownloadCloud aria-hidden="true" className="h-4 w-4" />
                  {t('detail.bulkDownload')}
                </Button>

                {backgroundUrl && (
                  <Button
                    icon
                    variant="neutral"
                    aria-pressed={isBackground}
                    aria-label={isBackground ? t('detail.stopBackgroundLabel') : t('detail.useBackgroundLabel')}
                    title={isBackground ? t('detail.removeBackground') : t('detail.useAsBackground')}
                    onClick={() =>
                      saveSettings.mutate({
                        background_art: isBackground
                          ? null
                          : { url: backgroundUrl, title, style: settings?.background_art?.style ?? 'blur' },
                      })
                    }
                  >
                    <ImageIcon aria-hidden="true" className="h-4 w-4" />
                  </Button>
                )}
              </div>
            </div>
          </div>
          {high && (
            // High: Jotaro's menacing pose in its own column, with ゴ drawn by CSS.
            <div aria-hidden="true" className="meme-jojo">
              <MemeImage name="jotaro-pose-menacing" />
              <span className="meme-sfx meme-gogo" />
              <span className="meme-sfx meme-gogo" />
              <span className="meme-sfx meme-gogo" />
            </div>
          )}
        </div>
      </header>

      <div className="flex flex-col gap-8 min-[1200px]:grid min-[1200px]:grid-cols-[minmax(0,1fr)_19rem] min-[1200px]:items-start">
        <div className="flex min-w-0 flex-col gap-8">
          <section className="flex flex-col gap-2">
            <h2 className="text-xl font-semibold text-text-primary">
              <Meme spot="about" plain={t('detail.aboutThisAnime')} />
            </h2>
            <div className="max-w-[62ch]">
              <p
                id="detail-synopsis"
                className={`text-sm leading-relaxed text-text-secondary select-text ${
                  descriptionExpanded ? '' : 'line-clamp-4'
                }`}
              >
                {cleanDescription || t('detail.noSynopsis')}
              </p>
              {isLongDescription && (
                <button
                  type="button"
                  aria-expanded={descriptionExpanded}
                  aria-controls="detail-synopsis"
                  onClick={() => setDescriptionExpanded(!descriptionExpanded)}
                  className="mt-1 cursor-pointer rounded-sm text-[13px] font-medium text-accent-300 transition-colors hover:text-accent-200"
                >
                  {descriptionExpanded ? (
                    <Meme spot="showLess" plain={t('detail.showLess')} />
                  ) : (
                    <Meme spot="showMore" plain={t('detail.showMore')} />
                  )}
                </button>
              )}
            </div>
          </section>

          <section className="flex flex-col gap-3">
            <div className="flex items-baseline gap-3">
              <h2 className="text-xl font-semibold text-text-primary">
                <Meme spot="epsH" plain={t('common.episodes')} />
              </h2>
              {watchedCount !== null && watchedCount > 0 && (
                <span className="text-xs tabular-nums text-text-muted">
                  {t('detail.watchedCount', { watched: watchedCount, total: details.episodes! })}
                </span>
              )}
            </div>
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
              localEpisodes={local.episodes}
              onSelectEpisode={handleSelectEpisode}
              chosenCategory={chosenCategory}
              onChosenCategoryChange={setChosenCategory}
              onDubAvailableChange={setDubAvailable}
              currentEpisode={continueTarget?.episode ?? null}
            />
          </section>

          <WatchOrder key={details.id} details={details} />

          {details.relations?.edges && details.relations.edges.length > 0 && (
            <RelationsRail relations={details.relations.edges} />
          )}

          <CharacterRail characters={details.characters?.edges ?? []} unavailableFrom={details.servedBy ?? null} />

          <Recommendations details={details} />

          <AnimeThemesPlayer malId={malId} animeTitle={title} />
        </div>

        <DetailFacts details={details} seasonLabel={seasonLabel} onTrailer={() => setIsTrailerOpen(true)} />
      </div>

      <LocalMediaDialog
        isOpen={localOpen}
        onClose={() => setLocalOpen(false)}
        anilistId={anilistId}
        animeTitle={title}
      />

      <TrailerModal
        isOpen={isTrailerOpen}
        onClose={() => setIsTrailerOpen(false)}
        trailer={details.trailer}
        animeTitle={title}
      />
    </div>
  );
};
