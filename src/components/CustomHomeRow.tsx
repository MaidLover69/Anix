/**
 * The rows a user adds to Home in Settings › Appearance: airing this week, one list status,
 * or one playlist or collection. Each mounts (and so fetches) only when it scrolls near the
 * screen, so any number of rows keeps Home fast.
 */
import React, { useEffect, useRef, useState } from 'react';
import { HorizontalRail } from './HorizontalRail';
import { useAiringSchedule } from '../hooks/useSchedule';
import { useWatchlist } from '../hooks/useLibrary';
import { usePlaylistItems, usePlaylists } from '../hooks/usePlaylists';
import { useSearchAnime } from '../hooks/useAniList';
import { uniqueCards } from '../lib/cardMedia';
import { discoverLink, franchiseFromRule, searchFromRule } from '../lib/smartRule';
import { useFranchiseOrder } from '../hooks/useFranchise';
import type { RowKind, StatusRowValue } from '../lib/homeRows';
import { t, type TKey } from '../i18n';
import type { CustomPlaylist } from '../types';

const HOUR_S = 3600;
const WEEK_S = 7 * 24 * HOUR_S;
/** Start loading a row this far before it scrolls into view. */
const PRELOAD_MARGIN = '600px 0px';

export const STATUS_ROW_TITLES: Record<StatusRowValue, TKey> = {
  CURRENT: 'homeRows.statusCurrent',
  PLANNING: 'homeRows.statusPlanning',
  PAUSED: 'homeRows.statusPaused',
};

/** Renders its children once the placeholder comes near the viewport, then keeps them. */
const WhenNear: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const ref = useRef<HTMLDivElement>(null);
  const [near, setNear] = useState(() => typeof IntersectionObserver === 'undefined');
  useEffect(() => {
    if (near || !ref.current) return;
    // The margin only works against the element that scrolls (Home scrolls inside <main>).
    const root = ref.current.closest('main');
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) setNear(true);
    }, { root, rootMargin: PRELOAD_MARGIN });
    observer.observe(ref.current);
    return () => observer.disconnect();
  }, [near]);
  return near ? <>{children}</> : <div ref={ref} aria-hidden="true" className="h-72" />;
};

const AiringRow: React.FC = () => {
  // Rounded to the hour so the query key, and so the cache, stays the same between renders.
  const from = Math.floor(Date.now() / 1000 / HOUR_S) * HOUR_S;
  const { data, isLoading } = useAiringSchedule(from, from + WEEK_S);
  const items = uniqueCards(data ?? [], (entry) => ({
    id: entry.media_id,
    title: entry.media_title,
    cover: entry.cover_image,
    format: entry.format,
    score: entry.average_score,
  }));
  return <HorizontalRail title={t('homeRows.airing')} items={items} isLoading={isLoading} viewAllLink="/schedule" />;
};

const StatusRow: React.FC<{ status: StatusRowValue }> = ({ status }) => {
  const { data, isLoading } = useWatchlist();
  const items = uniqueCards(
    (data ?? []).filter((entry) => entry.status === status),
    (entry) => ({ id: entry.anilist_id, title: entry.title, cover: entry.cover, format: entry.format, score: entry.average_score })
  );
  return <HorizontalRail title={t(STATUS_ROW_TITLES[status])} items={items} isLoading={isLoading} viewAllLink="/library" />;
};

const SmartRow: React.FC<{ playlist: CustomPlaylist; rule: string }> = ({ playlist, rule }) => {
  const franchise = franchiseFromRule(rule);
  // A franchise rule is not a Discover filter: its row shows the watch order instead.
  const search = useSearchAnime(searchFromRule(rule), { collectionId: playlist.id, enabled: franchise === null });
  const order = useFranchiseOrder(franchise ?? 0);
  const { data, isLoading } = search;
  if (franchise !== null) {
    const items = uniqueCards(order.entries, ({ node }) => ({ id: node.id, title: node.title?.english || node.title?.romaji || '', cover: node.coverImage?.large ?? null, format: node.format }));
    return <HorizontalRail title={playlist.name} items={items} isLoading={order.loading} viewAllLink={`/library?tab=playlists&playlist=${encodeURIComponent(playlist.id)}`} />;
  }
  return <HorizontalRail title={playlist.name} items={data} isLoading={isLoading} viewAllLink={discoverLink(rule)} />;
};

const HandMadeRow: React.FC<{ playlist: CustomPlaylist }> = ({ playlist }) => {
  const { data, isLoading } = usePlaylistItems(playlist.id);
  const items = uniqueCards(data ?? [], (item) => ({ id: item.anilist_id, title: item.series_title, cover: item.series_cover }));
  return (
    <HorizontalRail
      title={playlist.name}
      items={items}
      isLoading={isLoading}
      viewAllLink={`/library?tab=playlists&playlist=${encodeURIComponent(playlist.id)}`}
    />
  );
};

const PlaylistRow: React.FC<{ playlistId: string }> = ({ playlistId }) => {
  const { data } = usePlaylists();
  const playlist = data?.find((p) => p.id === playlistId);
  if (!playlist) return null;
  return playlist.rule ? <SmartRow playlist={playlist} rule={playlist.rule} /> : <HandMadeRow playlist={playlist} />;
};

/** One added Home row. Built-in rows are drawn by Home itself. */
export const CustomHomeRow: React.FC<{ kind: Exclude<RowKind, { kind: 'builtin' }> }> = ({ kind }) => (
  <WhenNear>
    {kind.kind === 'airing' ? (
      <AiringRow />
    ) : kind.kind === 'status' ? (
      <StatusRow status={kind.status} />
    ) : (
      <PlaylistRow playlistId={kind.playlistId} />
    )}
  </WhenNear>
);
