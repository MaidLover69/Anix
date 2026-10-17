import React, { useState, useEffect } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { Play, Info, ChevronLeft, ChevronRight } from 'lucide-react';
import clsx from 'clsx';
import { Button } from './ui/Button';
import { AmbientGlow } from './AmbientGlow';
import { useMediaQuery } from '../hooks/useMediaQuery';
import { getContinueTarget, releasedEpisodeCount, watchPath } from '../hooks/watchProgress';
import { mediaFormatLabel } from './library/format';
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

const NAV_BUTTON_CLASS =
  'absolute top-1/2 z-30 flex h-9 w-9 -translate-y-1/2 items-center justify-center rounded-full bg-black/60 text-white cursor-pointer opacity-0 transition-opacity hover:bg-black/80 focus-visible:opacity-100 group-hover:opacity-100';

function titleOf(media: Media): string {
  return media.title.english || media.title.user_preferred || media.title.romaji || 'Untitled';
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
  const reduceMotion = useMediaQuery('(prefers-reduced-motion: reduce)');

  const count = items.length;
  const currentIndex = count > 0 ? index % count : 0;

  // Restarts whenever the slide changes, so a manual jump gets a full interval too.
  useEffect(() => {
    if (count <= 1 || paused || reduceMotion) return;
    const timer = window.setTimeout(() => setIndex((i) => (i + 1) % count), SLIDE_INTERVAL_MS);
    return () => window.clearTimeout(timer);
  }, [currentIndex, count, paused, reduceMotion]);

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

  const caption = [
    mediaFormatLabel(current.format),
    [current.season ? current.season.charAt(0) + current.season.slice(1).toLowerCase() : null, current.season_year]
      .filter(Boolean)
      .join(' '),
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
        aria-label="Trending now"
        onMouseEnter={() => setPaused(true)}
        onMouseLeave={() => setPaused(false)}
        onFocus={() => setPaused(true)}
        onBlur={(e) => {
          if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setPaused(false);
        }}
        // clip-path (not overflow + radius) flattens the slides and scrims before rounding, so no
        // sliver of artwork shows along the corners.
        className="group relative h-[clamp(260px,40vh,420px)] w-full flex-shrink-0 ui-hero bg-bg-card [clip-path:inset(0_round_16px)]"
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
                'pointer-events-none absolute inset-0 h-full w-full object-cover object-center transition-opacity duration-300',
                i === currentIndex ? 'opacity-100' : 'opacity-0'
              )}
            />
          );
        })}
        {/* Two scrims: one under the text block, one along the bottom edge. The artwork stays
            visible everywhere, so the banner keeps an edge on all four sides. */}
        <div className="pointer-events-none absolute inset-0 bg-gradient-to-t from-bg-deepest/95 via-bg-deepest/55 to-transparent" />
        <div className="pointer-events-none absolute inset-y-0 left-0 w-4/5 bg-gradient-to-r from-bg-deepest/90 via-bg-deepest/65 to-transparent" />
        <div className="pointer-events-none absolute inset-0 rounded-2xl ring-1 ring-inset ring-ink/[0.06]" />

        <div className="relative z-20 flex h-full max-w-2xl flex-col justify-end p-8 [text-shadow:0_1px_2px_rgb(var(--bg-deepest)/0.45)]">
          {caption && <p className="mb-2 text-xs text-text-secondary">{caption}</p>}

          <h2 className="mb-2 text-[28px] font-bold leading-[34px] tracking-[-0.02em] text-text-primary line-clamp-2 select-text">
            <Link to={`/detail/${current.id}`} draggable={false} className="rounded-sm hover:underline">
              {title}
            </Link>
          </h2>

          {cleanDescription && (
            <p className="mb-4 text-sm text-text-secondary line-clamp-2 select-text">{cleanDescription}</p>
          )}

          <div className="flex items-center gap-2 [text-shadow:none]">
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
                {target ? `Continue Episode ${watchEpisode}` : 'Watch Episode 1'}
              </Button>
            )}
            <Button variant="neutral" onClick={() => navigate(`/detail/${current.id}`)}>
              <Info aria-hidden="true" className="h-4 w-4" />
              Details
            </Button>
          </div>
        </div>

        {count > 1 && (
          <>
            <button
              type="button"
              onClick={() => go(currentIndex - 1)}
              aria-label="Previous title"
              className={clsx(NAV_BUTTON_CLASS, 'left-3')}
            >
              <ChevronLeft aria-hidden="true" className="h-5 w-5" />
            </button>
            <button
              type="button"
              onClick={() => go(currentIndex + 1)}
              aria-label="Next title"
              className={clsx(NAV_BUTTON_CLASS, 'right-3')}
            >
              <ChevronRight aria-hidden="true" className="h-5 w-5" />
            </button>

            <div className="absolute bottom-4 right-6 z-30 flex items-center gap-1">
              {items.map((item, i) => (
                <button
                  key={item.id}
                  type="button"
                  onClick={() => go(i)}
                  aria-label={`Show ${titleOf(item)}`}
                  aria-current={i === currentIndex}
                  className="flex h-6 cursor-pointer items-center px-0.5"
                >
                  <span
                    className={clsx(
                      'block h-1.5 rounded-full transition-[width,background-color] duration-200',
                      i === currentIndex ? 'w-6 bg-accent-500' : 'w-1.5 bg-ink/40'
                    )}
                  />
                </button>
              ))}
            </div>
          </>
        )}
      </section>
    </div>
  );
};

export default HeroBanner;
