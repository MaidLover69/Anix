import React, { useState, useEffect } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { Play, Info, ChevronLeft, ChevronRight } from 'lucide-react';
import clsx from 'clsx';
import { Button } from './ui/Button';
import { AmbientGlow } from './AmbientGlow';
import { ShimmerImage } from './ShimmerImage';
import { useReducedMotion } from '../hooks/useMediaQuery';
import { useWindowActive } from '../hooks/useWindowActive';
import { getContinueTarget, releasedEpisodeCount, watchPath } from '../hooks/watchProgress';
import { mediaFormatLabel } from './library/format';
import { t, type TKey } from '../i18n';
import { Meme, MemeFrame, useHighModule } from './meme/Meme';
import type { Media, HistoryEntry } from '../types';

interface HeroBannerProps {
  items: Media[];
  /** Most recent history row per anime; decides where the watch button goes. */
  history?: HistoryEntry[];
  /** Audio and provider used when the anime has no history yet. */
  defaultCategory?: 'sub' | 'dub';
  defaultProvider?: string;
}

const SLIDE_INTERVAL_MS = 7000;

// Fixed dark-on-art buttons: they sit on the artwork, whose colours no theme controls.
const NAV_BUTTON_CLASS =
  'flex h-8 w-8 items-center justify-center rounded-full bg-black/60 text-white cursor-pointer transition-colors hover:bg-black/80';

const SEASON_LABELS: Record<string, TKey> = {
  WINTER: 'heroBanner.seasonWinter',
  SPRING: 'heroBanner.seasonSpring',
  SUMMER: 'heroBanner.seasonSummer',
  FALL: 'heroBanner.seasonFall',
};

function titleOf(media: Media): string {
  return media.title.english || media.title.user_preferred || media.title.romaji || t('heroBanner.untitled');
}

function backdropOf(media: Media): string | null {
  return media.banner_image || media.cover_image.extra_large || media.cover_image.large || null;
}

export const HeroBanner: React.FC<HeroBannerProps> = ({
  items,
  history = [],
  defaultCategory = 'sub',
  defaultProvider = 'auto',
}) => {
  const navigate = useNavigate();
  const [index, setIndex] = useState(0);
  const [paused, setPaused] = useState(false);
  const [failedImages, setFailedImages] = useState<Set<number>>(() => new Set());
  const reduceMotion = useReducedMotion();
  const high = useHighModule();
  // Nobody watches a rotation in a hidden or background window; each slide change costs a repaint.
  const windowActive = useWindowActive();

  const count = items.length;
  const currentIndex = count > 0 ? index % count : 0;

  // Restarts whenever the slide changes, so a manual jump gets a full interval too.
  useEffect(() => {
    if (count <= 1 || paused || reduceMotion || !windowActive) return;
    const timer = window.setTimeout(() => setIndex((i) => (i + 1) % count), SLIDE_INTERVAL_MS);
    return () => window.clearTimeout(timer);
  }, [currentIndex, count, paused, reduceMotion, windowActive]);

  const current = items[currentIndex];
  if (!current) return null;

  const title = titleOf(current);
  const cleanDescription = (current.description || '').replace(/<[^>]*>?/gm, '').trim();

  const latest = history.find((h) => h.anilist_id === current.id) ?? null;
  const released = releasedEpisodeCount(
    current.status,
    current.episodes,
    current.next_airing_episode?.episode
  );
  const target = getContinueTarget({ latest, released });
  const canWatch = released === null || released > 0;
  const watchEpisode = target?.episode ?? 1;

  const seasonKey = current.season ? SEASON_LABELS[current.season] : undefined;
  const season = seasonKey
    ? t(seasonKey)
    : current.season
      ? current.season.charAt(0) + current.season.slice(1).toLowerCase()
      : null;
  const caption = [
    mediaFormatLabel(current.format),
    season && current.season_year ? t('heroBanner.seasonYear', { season, year: current.season_year }) : season || current.season_year,
    current.average_score ? `${current.average_score}%` : null,
    current.genres?.slice(0, 3).join(', '),
  ]
    .filter(Boolean)
    .join(' · ');

  const go = (next: number) => setIndex((next + count) % count);

  return (
    // isolate keeps the glow behind the banner but in front of the page background.
    <div className="relative isolate flex-shrink-0">
      <AmbientGlow color={current.cover_image.color} className="-inset-x-4 -top-4 -bottom-16 -z-10" />
      <section
        aria-roledescription="carousel"
        aria-label={t('heroBanner.trendingNow')}
        onMouseEnter={() => setPaused(true)}
        onMouseLeave={() => setPaused(false)}
        onFocus={() => setPaused(true)}
        onBlur={(e) => {
          if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setPaused(false);
        }}
        // A manga panel: one corner cut at an angle (browse.css › .ui-hero-panel), faint speed lines
        // and a screentone corner. The drift and slide-in pause with the rotation in a background window.
        data-idle={!windowActive || undefined}
        className="ui-hero ui-hero-panel relative flex min-h-[clamp(260px,40vh,420px)] w-full flex-shrink-0 flex-col justify-end overflow-hidden rounded-2xl bg-bg-card"
      >
        {/* Every slide's image stays mounted so the change is a cross-fade, not a cut. */}
        {items.map((item, i) => {
          const url = backdropOf(item);
          if (!url || failedImages.has(item.id)) return null;
          return (
            <img
              key={item.id}
              src={url}
              alt=""
              draggable={false}
              onError={() => setFailedImages((prev) => new Set(prev).add(item.id))}
              className={clsx(
                'ui-hero-art pointer-events-none absolute inset-0 h-full w-full object-cover object-center transition-opacity duration-300',
                // Only the first title drifts, once per visit: a drift per title would keep Home animating.
                i === 0 && 'ui-hero-drift',
                i === currentIndex ? 'opacity-100' : 'opacity-0'
              )}
            />
          );
        })}
        {/* Two scrims: one under the text block, one along the bottom edge. The artwork stays
            visible everywhere, so the banner keeps an edge on all four sides. */}
        <div className="pointer-events-none absolute inset-0 bg-gradient-to-t from-bg-deepest/95 via-bg-deepest/55 to-transparent" />
        <div className="pointer-events-none absolute inset-y-0 start-0 w-4/5 bg-gradient-to-r rtl:bg-gradient-to-l from-bg-deepest/90 via-bg-deepest/65 to-transparent" />
        <div aria-hidden="true" className="ui-hero-speed pointer-events-none absolute inset-0" />
        <div aria-hidden="true" className="ui-hero-tone pointer-events-none absolute bottom-0 end-0 h-3/5 w-[45%]" />
        {high && (
          // High: Goku's beam crosses the banner behind the text to the poster (meme-high-shell.css).
          <div aria-hidden="true" className="meme-kbeam">
            <span>
              <i />
              <b className="meme-ktip" />
            </span>
          </div>
        )}

        {/* The poster as a tilted sticker; hidden where it would crowd the text. The title is the real link. */}
        <Link
          to={`/detail/${current.id}`}
          tabIndex={-1}
          aria-hidden="true"
          draggable={false}
          className="ui-hero-sticker absolute bottom-10 end-[clamp(1.5rem,5vw,4.5rem)] z-10 hidden w-[clamp(7rem,12vw,10.5rem)] xl:block"
        >
          {high && (
            <>
              <MemeFrame spot="heroKicker" className="meme-kimg" />
              <b className="meme-kcap" />
            </>
          )}
          <ShimmerImage
            key={current.id}
            src={current.cover_image.large || current.cover_image.extra_large}
            alt=""
            className="ui-hero-swap aspect-[2/3] w-full rounded-lg border-4 border-white/90 shadow-pop"
          />
        </Link>

        <div
          key={current.id}
          className="ui-hero-swap meme-hero-body relative z-20 flex max-w-2xl flex-col p-8 pb-12 [text-shadow:0_1px_2px_rgb(var(--bg-deepest)/0.45)] xl:max-w-[min(42rem,calc(100%-14rem))]"
        >
          {high && (
            <p className="meme-hero-kicker mb-1 text-xs font-semibold text-text-secondary">
              <Meme spot="heroKicker" plain={t('heroBanner.trendingNow')} />
            </p>
          )}
          {caption && <p className="mb-2 text-xs text-text-secondary">{caption}</p>}

          <h2 className="mb-2 text-[clamp(1.75rem,3vw,2.5rem)] font-black leading-tight tracking-[-0.02em] text-text-primary line-clamp-2 select-text">
            <Link to={`/detail/${current.id}`} draggable={false} className="rounded-sm hover:underline">
              {/* bdi: a Latin title in Arabic keeps its "!" and "…" at its own end. */}
              <bdi>{title}</bdi>
            </Link>
          </h2>

          {cleanDescription && (
            // dir auto: an English synopsis in Arabic keeps its "…" at its own end; still aligned to the page side.
            <p dir="auto" className="mb-4 text-sm text-text-secondary line-clamp-2 select-text rtl:text-right">
              {cleanDescription}
            </p>
          )}

          <div className="flex flex-wrap items-center gap-2 [text-shadow:none]">
            {canWatch && (
              <Button
                variant="primary"
                onClick={() =>
                  navigate(
                    watchPath(
                      current.id,
                      watchEpisode,
                      target?.category ?? defaultCategory,
                      target?.provider ?? defaultProvider
                    )
                  )
                }
              >
                <Play aria-hidden="true" className="h-4 w-4 fill-current" />
                <Meme
                  spot="heroPlay"
                  plain={
                    target ? t('heroBanner.continueEpisode', { episode: watchEpisode }) : t('heroBanner.watchFirstEpisode')
                  }
                />
              </Button>
            )}
            <Button variant="neutral" onClick={() => navigate(`/detail/${current.id}`)}>
              <Info aria-hidden="true" className="h-4 w-4" />
              {t('common.details')}
            </Button>
          </div>
        </div>

        {count > 1 && (
          <div className="absolute end-4 top-4 z-30 flex items-center gap-1">
            <div className="me-1 flex items-center">
              {items.map((item, i) => (
                <button
                  key={item.id}
                  type="button"
                  onClick={() => go(i)}
                  aria-label={t('heroBanner.showTitle', { title: titleOf(item) })}
                  aria-current={i === currentIndex}
                  className="flex h-6 cursor-pointer items-center px-0.5"
                >
                  <span
                    className={clsx(
                      'ui-hero-dot block h-1.5 rounded-full',
                      i === currentIndex ? 'w-6 bg-accent-500' : 'w-1.5 bg-ink/40'
                    )}
                  />
                </button>
              ))}
            </div>
            <button
              type="button"
              onClick={() => go(currentIndex - 1)}
              aria-label={t('heroBanner.previousTitle')}
              className={NAV_BUTTON_CLASS}
            >
              <ChevronLeft aria-hidden="true" className="h-4 w-4 rtl:-scale-x-100" />
            </button>
            <button
              type="button"
              onClick={() => go(currentIndex + 1)}
              aria-label={t('heroBanner.nextTitle')}
              className={NAV_BUTTON_CLASS}
            >
              <ChevronRight aria-hidden="true" className="h-4 w-4 rtl:-scale-x-100" />
            </button>
          </div>
        )}
      </section>
    </div>
  );
};

export default HeroBanner;
