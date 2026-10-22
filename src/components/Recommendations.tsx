import React from 'react';
import { HorizontalRail } from './HorizontalRail';
import { useSettings } from '../hooks/useSettings';
import { t } from '../i18n';
import type { AnimeDetails, Media, RelationEdge } from '../types';

/**
 * The recommendations of a details answer as cards: AniList's, or MyAnimeList's while AniList
 * is unavailable (the backend fills the same field). Adult titles follow the setting.
 */
export function recommendationMedia(details: AnimeDetails | null | undefined, hideAdult: boolean): Media[] {
  const seen = new Set<number>([0, details?.id ?? 0]);
  const picks: RelationEdge['node'][] = [];
  for (const { mediaRecommendation: m } of details?.recommendations?.nodes ?? []) {
    if (!m || seen.has(m.id) || (hideAdult && m.isAdult)) continue;
    seen.add(m.id);
    picks.push(m);
  }
  return picks.map((m) => ({
    id: m.id,
    id_mal: m.idMal ?? null,
    title: {
      romaji: m.title.romaji,
      english: m.title.english,
      native: m.title.native,
      user_preferred: m.title.userPreferred,
    },
    cover_image: { large: m.coverImage.large, extra_large: m.coverImage.extraLarge, color: m.coverImage.color },
    banner_image: null,
    description: null,
    format: m.format,
    status: m.status,
    episodes: null,
    duration: null,
    season: null,
    season_year: m.seasonYear ?? null,
    average_score: m.averageScore,
    mean_score: null,
    popularity: null,
    favourites: null,
    genres: [],
    is_adult: !!m.isAdult,
  }));
}

/** A rail of recommended titles; nothing when there are none. */
export const Recommendations: React.FC<{ details: AnimeDetails | null | undefined; title?: string }> = ({
  details,
  title = t('recommendations.title'),
}) => {
  const { data: settings } = useSettings();
  const items = recommendationMedia(details, settings?.hide_adult_content !== false);
  if (items.length === 0) return null;
  return <HorizontalRail title={title} items={items} />;
};
