import React, { useId, useMemo } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { BarChart3, Compass } from 'lucide-react';
import { useQuery } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { Button, EmptyState, ErrorState, Spinner } from '../ui';
import { useWatchedHistory } from '../../hooks/useLibrary';
import { useAccountStatus } from '../../hooks/useAccounts';
import { usePlayful } from '../../hooks/useSettings';
import { useUIStore } from '../../stores/uiStore';
import { ShimmerImage } from '../ShimmerImage';
import { watchStreak } from './format';
import type { AniListStats, AniListStatsEntry, WatchedEpisodeRow } from '../../types';
import { formatNumber, locale, t, tn, type TKey } from '../../i18n';

/** Minutes assumed for an episode whose length nobody knows. */
const FALLBACK_EPISODE_MINUTES = 24;

/** The AniList half; one list request plus two voice-actor batches, cached for an hour. */
function useAniListStats(connected: boolean) {
  return useQuery<AniListStats>({
    queryKey: ['aniListStats'],
    queryFn: () => invoke<AniListStats>('fetch_anilist_stats'),
    enabled: connected,
    staleTime: 60 * 60 * 1000,
    gcTime: 6 * 60 * 60 * 1000,
    retry: 1,
  });
}

function formatHours(minutes: number): string {
  const hours = minutes / 60;
  if (hours < 1) return t('statsTab.minutes', { count: Math.round(minutes) });
  return t('statsTab.hours', {
    hours: hours < 10 ? formatNumber(hours, { minimumFractionDigits: 1, maximumFractionDigits: 1 }) : formatNumber(Math.round(hours)),
  });
}

interface Bucket {
  label: string;
  value: number;
  /** Full label for the tooltip and the table. */
  title?: string;
}

/** Ranks names by how often they occur, most first. */
function topCounts(names: string[], limit: number): Bucket[] {
  const counts = new Map<string, number>();
  for (const name of names) counts.set(name, (counts.get(name) ?? 0) + 1);
  return [...counts.entries()]
    .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
    .slice(0, limit)
    .map(([label, value]) => ({ label, value }));
}

const Card: React.FC<{ title: string; children: React.ReactNode; className?: string; note?: string }> = ({
  title,
  children,
  className,
  note,
}) => (
  <section className={`flex flex-col gap-3 rounded-xl ui-card bg-bg-card p-4 ${className ?? ''}`}>
    <div className="flex items-baseline justify-between gap-3">
      <h3 className="text-sm font-semibold text-text-primary">{title}</h3>
      {note && <span className="text-xs text-text-muted">{note}</span>}
    </div>
    {children}
  </section>
);

const Stat: React.FC<{ label: string; value: string; detail?: string }> = ({ label, value, detail }) => (
  <div className="flex min-w-0 flex-col rounded-xl ui-card bg-bg-card p-4">
    <span className="text-xs text-text-muted">{label}</span>
    <span className="mt-1 text-2xl font-semibold tabular-nums text-text-primary">{value}</span>
    {detail && <span className="mt-0.5 text-xs text-text-muted">{detail}</span>}
  </div>
);

/**
 * Vertical bars, one series in the accent. Each bar has a tooltip (its <title>); the same
 * numbers are in a table for screen readers.
 */
/** `unit` is a plural key such as "{count} titles". */
const ColumnChart: React.FC<{ data: Bucket[]; label: string; unit: TKey; highlight?: number }> = ({
  data,
  label,
  unit,
  highlight,
}) => {
  const tableId = useId();
  const width = 600;
  const height = 150;
  const axis = 18;
  const max = Math.max(1, ...data.map((d) => d.value));
  const slot = width / data.length;
  const barWidth = Math.min(36, slot - 6);
  const plot = height - axis;

  return (
    <figure className="flex flex-col">
      <svg
        viewBox={`0 0 ${width} ${height}`}
        role="img"
        aria-label={label}
        aria-describedby={tableId}
        className="h-auto w-full overflow-visible"
      >
        {[0.5, 1].map((f) => (
          <line key={f} x1={0} x2={width} y1={plot - plot * f} y2={plot - plot * f} className="ui-chart-grid" strokeWidth={1} />
        ))}
        <line x1={0} x2={width} y1={plot} y2={plot} className="ui-chart-grid" strokeWidth={1} />
        {data.map((d, i) => {
          const h = d.value > 0 ? Math.max(3, (d.value / max) * (plot - 14)) : 0;
          const x = i * slot + (slot - barWidth) / 2;
          return (
            <g key={d.label}>
              {/* Hit area larger than the bar, for the tooltip. */}
              <rect x={i * slot} y={0} width={slot} height={plot} fill="transparent">
                <title>{t('statsTab.barTooltip', { label: d.title ?? d.label, value: t(unit, { count: d.value }) })}</title>
              </rect>
              {h > 0 && (
                <path
                  className={highlight === undefined || highlight === i ? 'ui-chart-bar' : 'ui-chart-bar-muted'}
                  d={`M${x},${plot} v${-(h - 4)} q0,-4 4,-4 h${barWidth - 8} q4,0 4,4 v${h - 4} z`}
                  pointerEvents="none"
                />
              )}
              {d.value > 0 && d.value === max && (
                <text
                  x={x + barWidth / 2}
                  y={plot - h - 4}
                  textAnchor="middle"
                  className="fill-[var(--text-secondary)] text-[11px] tabular-nums"
                >
                  {formatNumber(d.value)}
                </text>
              )}
              <text
                x={i * slot + slot / 2}
                y={height - 4}
                textAnchor="middle"
                className="fill-[var(--text-muted)] text-[11px]"
              >
                {d.label}
              </text>
            </g>
          );
        })}
      </svg>
      <table id={tableId} className="sr-only">
        <caption>{label}</caption>
        <tbody>
          {data.map((d) => (
            <tr key={d.label}>
              <th scope="row">{d.title ?? d.label}</th>
              <td>{t(unit, { count: d.value })}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </figure>
  );
};

/** A ranked list with a bar per row: labels stay readable at any length. */
const RankedBars: React.FC<{ data: Bucket[]; label: string; unit: TKey }> = ({ data, label, unit }) => {
  const max = Math.max(1, ...data.map((d) => d.value));
  return (
    <ol aria-label={label} className="flex flex-col gap-2">
      {data.map((d) => (
        <li key={d.label} className="flex flex-col gap-1">
          <span className="flex items-baseline justify-between gap-3 text-xs">
            <span className="truncate text-text-secondary">{d.label}</span>
            <span className="flex-shrink-0 tabular-nums text-text-muted">
              <span aria-hidden="true">{formatNumber(d.value)}</span>
              <span className="sr-only">{t(unit, { count: d.value })}</span>
            </span>
          </span>
          <span aria-hidden="true" className="h-1.5 w-full rounded-full bg-ink/[0.08]">
            <span
              className="block h-full rounded-full bg-accent-500"
              style={{ width: `${Math.max(2, (d.value / max) * 100)}%` }}
            />
          </span>
        </li>
      ))}
    </ol>
  );
};

function localTotals(rows: WatchedEpisodeRow[], minutesById: Map<number, number>) {
  let knownMinutes = 0;
  let estimatedMinutes = 0;
  for (const row of rows) {
    if (row.duration_ms > 1000) knownMinutes += row.duration_ms / 60000;
    else estimatedMinutes += minutesById.get(row.anilist_id) ?? FALLBACK_EPISODE_MINUTES;
  }
  return { knownMinutes, estimatedMinutes, titles: new Set(rows.map((r) => r.anilist_id)).size };
}

export const StatsTab: React.FC = () => {
  const accounts = useAccountStatus().data;
  const connected = accounts?.anilist.connected ?? false;
  const openAuthDialog = useUIStore((s) => s.setAuthDialogOpen);
  const navigate = useNavigate();
  const historyQuery = useWatchedHistory();
  const statsQuery = useAniListStats(connected);
  const playful = usePlayful();

  const rows = historyQuery.data ?? [];
  // Every day an episode was watched; a rewatch adds a day instead of moving the old one.
  const watchTimes = useQuery<number[]>({ queryKey: ['watchTimes'], queryFn: () => invoke<number[]>('get_watch_times') });
  const streak = useMemo(
    () => watchStreak(watchTimes.data ?? rows.map((r) => r.updated_at)),
    [watchTimes.data, rows]
  );
  const entries: AniListStatsEntry[] = statsQuery.data?.entries ?? [];
  const year = new Date().getFullYear();

  const minutesById = useMemo(
    () => new Map(entries.filter((e) => e.duration).map((e) => [e.media_id, e.duration as number])),
    [entries]
  );

  const local = useMemo(() => localTotals(rows, minutesById), [rows, minutesById]);

  const aniList = useMemo(() => {
    const started = entries.filter((e) => e.status !== 'PLANNING');
    const scored = entries.filter((e) => e.score > 0);
    const scoreBuckets: Bucket[] = Array.from({ length: 10 }, (_, i) => ({
      label: String((i + 1) * 10),
      title: t('statsTab.scoreRange', { from: i * 10 + 1, to: (i + 1) * 10 }),
      value: scored.filter((e) => Math.min(9, Math.floor((e.score - 1) / 10)) === i).length,
    }));
    const episodes = started.reduce((sum, e) => sum + e.progress * (1 + e.repeat), 0);
    const minutes = started.reduce(
      (sum, e) => sum + e.progress * (1 + e.repeat) * (e.duration ?? FALLBACK_EPISODE_MINUTES),
      0
    );
    return {
      completed: entries.filter((e) => e.status === 'COMPLETED').length,
      episodes,
      minutes,
      meanScore: scored.length ? scored.reduce((s, e) => s + e.score, 0) / scored.length : null,
      scoreBuckets,
      scoredCount: scored.length,
      genres: topCounts(started.flatMap((e) => e.genres), 8),
      studios: topCounts(started.flatMap((e) => e.studios), 6),
    };
  }, [entries]);

  const review = useMemo(() => {
    const thisYear = rows.filter((r) => new Date(r.updated_at).getFullYear() === year);
    const monthName = new Intl.DateTimeFormat(locale(), { month: 'short' });
    const months: Bucket[] = Array.from({ length: 12 }, (_, i) => ({
      label: monthName.format(new Date(year, i, 1)),
      value: thisYear.filter((r) => new Date(r.updated_at).getMonth() === i).length,
    }));
    const byTitle = new Map<number, { title: string; cover: string | null; count: number }>();
    for (const r of thisYear) {
      const known = byTitle.get(r.anilist_id);
      if (known) known.count += 1;
      else byTitle.set(r.anilist_id, { title: r.title, cover: r.cover, count: 1 });
    }
    const topTitles = [...byTitle.entries()].sort((a, b) => b[1].count - a[1].count).slice(0, 5);
    const completed = entries
      .filter((e) => e.status === 'COMPLETED' && e.completed_year === year)
      .sort((a, b) => b.score - a.score);
    return { thisYear, months, topTitles, completed, totals: localTotals(thisYear, minutesById) };
  }, [rows, entries, year, minutesById]);

  if (historyQuery.isLoading) {
    return (
      <div className="flex justify-center py-20">
        <Spinner size="lg" label={t('statsTab.loading')} className="text-text-muted" />
      </div>
    );
  }
  if (historyQuery.isError) {
    return (
      <ErrorState title={t('statsTab.historyError')} error={historyQuery.error} onRetry={() => historyQuery.refetch()} />
    );
  }
  if (rows.length === 0 && !connected) {
    return (
      <EmptyState
        icon={<BarChart3 className="h-8 w-8" aria-hidden="true" />}
        title={t('statsTab.empty')}
        description={
          playful
            ? t('statsTab.emptyHintPlayful')
            : t('statsTab.emptyHint')
        }
        action={
          <div className="flex flex-wrap justify-center gap-2">
            <Button variant="primary" onClick={() => navigate('/discover')}>
              <Compass className="h-4 w-4" aria-hidden="true" />
              {t('statsTab.browseAnime')}
            </Button>
            <Button variant="secondary" onClick={() => openAuthDialog(true)}>
              {t('statsTab.connectAniList')}
            </Button>
          </div>
        }
      />
    );
  }

  const totalMinutes = local.knownMinutes + local.estimatedMinutes;
  const currentMonth = new Date().getMonth();

  return (
    <div className="flex flex-col gap-6">
      <div className={`grid grid-cols-2 gap-3 ${connected && statsQuery.data ? 'lg:grid-cols-5' : 'lg:grid-cols-3'}`}>
        <Stat
          label={t('statsTab.episodesHere')}
          value={formatNumber(rows.length)}
          detail={t('statsTab.titleCount', { count: local.titles })}
        />
        <Stat
          label={t('statsTab.timeHere')}
          value={formatHours(totalMinutes)}
          detail={
            local.estimatedMinutes > 0
              ? t('statsTab.estimated', { time: formatHours(local.estimatedMinutes) })
              : t('statsTab.fromPlayback')
          }
        />
        <Stat
          label={t('statsTab.streak')}
          value={
            streak.current > 0
              ? t('statsTab.streakDays', { count: streak.current })
              : t('statsTab.longestValue', { count: streak.longest })
          }
          detail={
            streak.current > 0
              ? `${t('statsTab.longestDetail', { count: streak.longest })}${playful && streak.current >= 7 ? ' (^o^)/' : ''}`
              : t('statsTab.streakHint')
          }
        />
        {connected && statsQuery.data && (
          <>
            <Stat
              label={t('statsTab.aniListEpisodes')}
              value={formatNumber(aniList.episodes)}
              detail={`${formatHours(aniList.minutes)} · ${t('statsTab.completedCount', { count: aniList.completed })}`}
            />
            <Stat
              label={t('statsTab.meanScore')}
              value={
                aniList.meanScore !== null
                  ? formatNumber(aniList.meanScore, { minimumFractionDigits: 1, maximumFractionDigits: 1 })
                  : '—'
              }
              detail={t('statsTab.scoredTitles', { count: aniList.scoredCount })}
            />
          </>
        )}
      </div>

      <Card
        title={t('statsTab.inReview', { year })}
        note={t('statsTab.reviewNote', {
          count: review.thisYear.length,
          time: formatHours(review.totals.knownMinutes + review.totals.estimatedMinutes),
        })}
      >
        {review.thisYear.length === 0 && review.completed.length === 0 ? (
          <p className="text-xs text-text-muted">{t('statsTab.nothingThisYear', { year })}</p>
        ) : (
          <div className="grid gap-6 lg:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]">
            <ColumnChart
              data={review.months}
              label={t('statsTab.perMonth', { year })}
              unit="common.episodeCount"
              highlight={currentMonth}
            />
            <div className="flex flex-col gap-4">
              {review.topTitles.length > 0 && (
                <div className="flex flex-col gap-2">
                  <h4 className="text-xs font-medium text-text-muted">{t('statsTab.mostWatched')}</h4>
                  <ol className="flex flex-col gap-1.5">
                    {review.topTitles.map(([id, top]) => (
                      <li key={id}>
                        <Link
                          to={`/detail/${id}`}
                          className="flex items-center gap-2 rounded-md text-xs transition-colors hover:text-text-primary"
                        >
                          <ShimmerImage src={top.cover} alt="" className="aspect-[2/3] w-6 flex-shrink-0 rounded-sm" />
                          <span className="min-w-0 flex-1 truncate text-text-secondary">{top.title}</span>
                          <span className="tabular-nums text-text-muted">{t('statsTab.episodesShort', { count: top.count })}</span>
                        </Link>
                      </li>
                    ))}
                  </ol>
                </div>
              )}
              {review.completed.length > 0 && (
                <p className="text-xs text-text-secondary">
                  {tn(
                    review.completed[0].score > 0 ? 'statsTab.completedThisYearTop' : 'statsTab.completedThisYear',
                    {
                      number: <span className="font-semibold tabular-nums text-text-primary">{review.completed.length}</span>,
                      title: <span className="text-text-primary">{review.completed[0].title}</span>,
                    },
                    { score: review.completed[0].score }
                  )}
                </p>
              )}
            </div>
          </div>
        )}
      </Card>

      {!connected ? (
        <Card title={t('statsTab.yourList')}>
          <p className="text-xs text-text-muted">
            {t('statsTab.connectHint')}
          </p>
          <button
            type="button"
            onClick={() => openAuthDialog(true)}
            className="self-start cursor-pointer rounded-sm text-[13px] font-medium text-accent-300 hover:text-accent-200"
          >
            {t('statsTab.connectAniList')}
          </button>
        </Card>
      ) : statsQuery.isLoading ? (
        <div className="flex items-center gap-2 py-6 text-xs text-text-muted">
          <Spinner size="sm" /> {t('statsTab.loadingList')}
        </div>
      ) : statsQuery.isError ? (
        <ErrorState title={t('statsTab.listError')} error={statsQuery.error} onRetry={() => statsQuery.refetch()} />
      ) : (
        <div className="grid gap-4 lg:grid-cols-2">
          <Card title={t('statsTab.scoreDistribution')} note={aniList.scoredCount ? undefined : t('statsTab.noScores')} className="lg:col-span-2">
            <ColumnChart data={aniList.scoreBuckets} label={t('statsTab.scoreChartLabel')} unit="statsTab.titleCount" />
          </Card>
          <Card title={t('statsTab.topGenres')} note={t('statsTab.startedOrFinished')}>
            {aniList.genres.length ? <RankedBars data={aniList.genres} label={t('statsTab.topGenres')} unit="statsTab.titleCount" /> : <p className="text-xs text-text-muted">{t('statsTab.noGenres')}</p>}
          </Card>
          <Card title={t('statsTab.topStudios')}>
            {aniList.studios.length ? <RankedBars data={aniList.studios} label={t('statsTab.topStudios')} unit="statsTab.titleCount" /> : <p className="text-xs text-text-muted">{t('statsTab.noStudios')}</p>}
          </Card>
          <Card
            title={t('statsTab.topVoiceActors')}
            note={statsQuery.data?.voice_actor_titles ? t('statsTab.voiceActorsNote', { count: statsQuery.data.voice_actor_titles }) : undefined}
            className="lg:col-span-2"
          >
            {statsQuery.data && statsQuery.data.voice_actors.length > 0 ? (
              <ol className="grid grid-cols-[repeat(auto-fill,minmax(180px,1fr))] gap-3">
                {statsQuery.data.voice_actors.map((va) => (
                  <li key={va.id} className="flex items-center gap-3">
                    <ShimmerImage src={va.image} alt="" className="h-10 w-10 flex-shrink-0 rounded-full" />
                    <span className="flex min-w-0 flex-col">
                      <span className="truncate text-xs font-medium text-text-primary">{va.name}</span>
                      <span className="text-xs tabular-nums text-text-muted">
                        {t('statsTab.titleCount', { count: va.count })}
                      </span>
                    </span>
                  </li>
                ))}
              </ol>
            ) : (
              <p className="text-xs text-text-muted">
                {statsQuery.data?.voice_actor_titles
                  ? t('statsTab.noVoiceActors')
                  : t('statsTab.voiceActorsHint')}
              </p>
            )}
          </Card>
        </div>
      )}
    </div>
  );
};
