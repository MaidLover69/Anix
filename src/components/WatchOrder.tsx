import React, { useEffect, useMemo, useRef, useState } from 'react';
import { Link } from 'react-router-dom';
import { useQueries } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import clsx from 'clsx';
import { ChevronLeft, ChevronRight } from 'lucide-react';
import { ShimmerImage } from './ShimmerImage';
import { Spinner } from './ui/Spinner';
import { Button } from './ui/Button';
import { mediaFormatLabel } from './library/format';
import { MAX_ENTRIES, watchOrder, type OrderDetails } from '../lib/watchOrder';
import type { AnimeDetails } from '../types';
import { t } from '../i18n';

function titleOf(node: OrderDetails): string {
  return node.title?.english || node.title?.userPreferred || node.title?.romaji || t('watchOrder.untitled');
}

/**
 * The franchise in watch order, numbered, with "You're here" on this anime. Prequels and
 * sequels are fetched one after another (from the same cache as their own pages) until the
 * chain ends. Hidden when this anime has no prequel or sequel.
 */
export const WatchOrder: React.FC<{ details: AnimeDetails }> = ({ details }) => {
  const [ids, setIds] = useState<number[]>([]);
  const scrollRef = useRef<HTMLOListElement>(null);
  const results = useQueries({
    queries: ids.map((id) => ({
      queryKey: ['animeDetails', id],
      queryFn: () => invoke<OrderDetails>('fetch_anime_details', { id }), // i18n-ignore
      staleTime: 15 * 60 * 1000,
      retry: 1,
    })),
  });

  const known = useMemo(() => {
    const map = new Map<number, OrderDetails>([[details.id, details as OrderDetails]]);
    results.forEach((r, i) => r.data && map.set(ids[i], r.data));
    return map;
    // `results` is a new array each render; its data is what matters.
  }, [details, ids, results.map((r) => r.dataUpdatedAt).join()]);

  const { entries, missing } = useMemo(() => watchOrder(details.id, known), [details.id, known]);
  const loading = results.some((r) => r.isFetching);

  useEffect(() => {
    const add = missing.filter((id) => !ids.includes(id));
    if (add.length > 0 && ids.length < MAX_ENTRIES) setIds((prev) => [...prev, ...add].slice(0, MAX_ENTRIES));
  }, [missing, ids]);

  if (entries.length < 2) return null;

  const heading = t('watchOrder.heading');
  const scroll = (direction: 'left' | 'right') => {
    const el = scrollRef.current;
    if (!el) return;
    el.scrollBy({
      left: (direction === 'left' ? -1 : 1) * Math.max(el.clientWidth - 240, 240),
      // An explicit behaviour overrides the CSS reduced-motion rule, so check it here.
      behavior: window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth',
    });
  };

  return (
    <section aria-labelledby="watch-order-heading" className="group/rail flex flex-col gap-3">
      <div className="flex items-center justify-between gap-4">
        <div className="flex items-center gap-3">
          <h2 id="watch-order-heading" className="text-xl font-semibold text-text-primary">
            {heading}
          </h2>
          {loading && <Spinner size="sm" label={t('watchOrder.findingMore')} className="text-text-muted" />}
        </div>
        <div dir="ltr" className="flex items-center gap-1 opacity-0 transition-opacity focus-within:opacity-100 group-hover/rail:opacity-100">
          <Button icon variant="ghost" size="sm" aria-label={t('horizontalRail.scrollLeft', { title: heading })} onClick={() => scroll('left')}>
            <ChevronLeft aria-hidden="true" className="h-4 w-4" />
          </Button>
          <Button icon variant="ghost" size="sm" aria-label={t('horizontalRail.scrollRight', { title: heading })} onClick={() => scroll('right')}>
            <ChevronRight aria-hidden="true" className="h-4 w-4" />
          </Button>
        </div>
      </div>
      <ol ref={scrollRef} className="-mx-1 flex snap-x gap-3 overflow-x-auto px-1 py-1 scroll-px-1 no-scrollbar">
        {entries.map(({ node, number, side, current }) => {
          const caption = [side ? t('watchOrder.sideStory') : null, mediaFormatLabel(node.format ?? null), node.seasonYear]
            .filter(Boolean)
            .join(' · ');
          const body = (
            <>
              <span
                aria-hidden="true"
                className={clsx(
                  'flex h-6 w-6 flex-shrink-0 items-center justify-center rounded-full text-xs font-semibold tabular-nums',
                  current ? 'bg-accent-600 text-on-accent' : 'bg-ink/10 text-text-secondary'
                )}
              >
                {number}
              </span>
              <ShimmerImage
                src={node.coverImage?.large || node.coverImage?.extraLarge || null}
                alt=""
                className="aspect-[2/3] w-10 flex-shrink-0 rounded-sm"
              />
              <span className="flex min-w-0 flex-1 flex-col">
                <span className="text-[13px] font-medium leading-[18px] text-text-primary line-clamp-2">
                  <span className="sr-only">{number}. </span>
                  {titleOf(node)}
                </span>
                <span className="truncate text-xs text-text-muted">
                  {current ? <span className="font-medium text-accent-300">{t('watchOrder.youreHere')}</span> : caption}
                </span>
              </span>
            </>
          );
          const boxClass = clsx(
            'flex w-60 items-center gap-2.5 rounded-xl border p-2 text-start',
            current ? 'border-accent-500 bg-accent-tint' : 'border-border bg-bg-card',
            side && 'border-dashed'
          );
          return (
            <li key={node.id} className="flex-shrink-0 snap-start">
              {current ? (
                <div aria-current="page" className={boxClass}>
                  {body}
                </div>
              ) : (
                <Link
                  to={`/detail/${node.id}`}
                  draggable={false}
                  className={clsx(boxClass, 'transition-colors hover:bg-ink/5')}
                >
                  {body}
                </Link>
              )}
            </li>
          );
        })}
      </ol>
    </section>
  );
};
