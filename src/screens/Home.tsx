import React from 'react';
import { HeroBanner } from '../components/HeroBanner';
import { HorizontalRail } from '../components/HorizontalRail';
import { ErrorState } from '../components/ui/ErrorState';
import { useHomeData } from '../hooks/useAniList';
import { useContinueWatching } from '../hooks/useLibrary';
import { useSettings } from '../hooks/useSettings';

export const Home: React.FC = () => {
  const { data, isLoading, isError, error, refetch, isFetching } = useHomeData();
  // History is local, so it is read on its own and still shows when AniList is unreachable.
  const { data: localHistory } = useContinueWatching();
  const { data: settings } = useSettings();

  const continueWatching = localHistory ?? data?.continue_watching ?? [];
  const heroItems = data?.trending?.slice(0, 5) ?? [];

  const catalogIsEmpty =
    !!data && !data.trending?.length && !data.popular?.length && !data.top_rated?.length;

  return (
    <div className="flex flex-col gap-8 pb-16 animate-fade-in">
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

      <HorizontalRail title="Continue watching" historyItems={continueWatching} />

      {isError || catalogIsEmpty ? (
        <ErrorState
          title="Could not load anime from AniList"
          message={
            isError
              ? undefined
              : data?.error || 'AniList returned nothing. Check your connection and try again.'
          }
          error={isError ? error : undefined}
          onRetry={() => refetch()}
          retryLabel={isFetching ? 'Retrying…' : 'Retry'}
        />
      ) : (
        <>
          <HorizontalRail
            title="Trending now"
            items={data?.trending}
            isLoading={isLoading}
            viewAllLink="/discover?sort=TRENDING_DESC"
          />
          <HorizontalRail
            title="All-time popular"
            items={data?.popular}
            isLoading={isLoading}
            viewAllLink="/discover?sort=POPULARITY_DESC"
          />
          <HorizontalRail
            title="Highest rated"
            items={data?.top_rated}
            isLoading={isLoading}
            viewAllLink="/discover?sort=SCORE_DESC"
          />
        </>
      )}
    </div>
  );
};
