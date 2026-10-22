import React from 'react';
import { HeroBanner } from '../components/HeroBanner';
import { HorizontalRail } from '../components/HorizontalRail';
import { Recommendations } from '../components/Recommendations';
import { ErrorState } from '../components/ui/ErrorState';
import { useAnimeDetails, useHomeData } from '../hooks/useAniList';
import { useContinueWatching } from '../hooks/useLibrary';
import { usePlayful, useSettings } from '../hooks/useSettings';
import { orderHomeRows, type HomeRowId } from '../lib/homeRows';
import { t, type TKey } from '../i18n';
import type { AnimeDetails } from '../types';

const CATALOG_ROWS: Record<'trending' | 'popular' | 'top_rated', { title: TKey; link: string }> = {
  trending: { title: 'home.trendingNow', link: '/discover?sort=TRENDING_DESC' },
  popular: { title: 'home.allTimePopular', link: '/discover?sort=POPULARITY_DESC' },
  top_rated: { title: 'home.highestRated', link: '/discover?sort=SCORE_DESC' },
};

export const Home: React.FC = () => {
  const { data, isLoading, isError, error, refetch, isFetching } = useHomeData();
  // History is local, so it is read on its own and still shows when AniList is unreachable.
  const { data: localHistory } = useContinueWatching();
  const { data: settings } = useSettings();
  const playful = usePlayful();

  const continueWatching = localHistory ?? data?.continue_watching ?? [];
  const heroItems = data?.trending?.slice(0, 5) ?? [];

  const catalogIsEmpty =
    !!data && !data.trending?.length && !data.popular?.length && !data.top_rated?.length;

  // "Because you watched…" follows the anime watched last (the history is newest first).
  const lastWatched = continueWatching[0] ?? null;
  const { data: lastDetails } = useAnimeDetails(lastWatched?.anilist_id ?? null);

  const order = orderHomeRows(settings?.home_row_order);
  // When the catalogue fails, one error takes the place of the first of its rows.
  const firstCatalogRow = order.find((id) => id in CATALOG_ROWS);

  const row = (id: HomeRowId): React.ReactNode => {
    if (id === 'continue') {
      return <HorizontalRail key={id} title={t('home.continueWatching')} historyItems={continueWatching} />;
    }
    if (id === 'because') {
      return lastWatched ? (
        <Recommendations
          key={id}
          details={lastDetails as AnimeDetails | null | undefined}
          title={t('home.becauseYouWatched', { title: lastWatched.title })}
        />
      ) : null;
    }
    if (isError || catalogIsEmpty) {
      return id !== firstCatalogRow ? null : (
        <ErrorState
          key="catalog-error"
          title={playful ? t('home.anilistDownPlayful') : t('home.anilistDown')}
          mood="sleepy"
          message={
            isError
              ? undefined
              : data?.error || t('home.anilistEmpty')
          }
          error={isError ? error : undefined}
          onRetry={() => refetch()}
          retryLabel={isFetching ? t('home.retrying') : t('common.retry')}
        />
      );
    }
    const { title, link } = CATALOG_ROWS[id];
    return <HorizontalRail key={id} title={t(title)} items={data?.[id]} isLoading={isLoading} viewAllLink={link} />;
  };

  return (
    <div className="flex flex-col gap-8 pb-16 animate-fade-in">
      <h1 className="sr-only">{t('home.title')}</h1>
      {isLoading ? (
        <div
          aria-hidden="true"
          className="h-[clamp(260px,40vh,420px)] w-full flex-shrink-0 rounded-2xl shimmer-placeholder"
        />
      ) : (
        heroItems.length > 0 && (
          <HeroBanner
            items={heroItems}
            history={continueWatching}
            defaultCategory={settings?.prefer_dub ? 'dub' : 'sub'}
            defaultProvider={settings?.preferred_provider}
          />
        )
      )}

      {order.map(row)}
    </div>
  );
};
