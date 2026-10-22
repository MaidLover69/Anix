import React, { useState, useMemo, useEffect, useCallback } from 'react';
import { Link, useNavigate, useSearchParams } from 'react-router-dom';
import { Bookmark, Calendar, List, Grid, Menu, Mic } from 'lucide-react';
import clsx from 'clsx';
import { useAiringSchedule, useNotificationBells } from '../hooks/useSchedule';
import { ShimmerImage } from '../components/ShimmerImage';
import { NotifyBell } from '../components/NotifyBell';
import { SegmentedControl } from '../components/ui/SegmentedControl';
import { EmptyState } from '../components/ui/EmptyState';
import { ErrorState } from '../components/ui/ErrorState';
import { Button } from '../components/ui/Button';
import { locale, t, tn } from '../i18n';
import type { AiringEntry } from '../types';
import { useStoredChoice } from '../hooks/useStoredChoice';
import { useWatchlist } from '../hooks/useLibrary';
import { useMediaQuery } from '../hooks/useMediaQuery';
import { usePlayful, useTimeFormat } from '../hooks/useSettings';
import { formatTime, type TimeFormat } from '../lib/time';
import { Chip } from '../components/ui/Chip';
import { mediaFormatLabel } from '../components/library/format';
import { MonthCalendar, UpNextCard } from '../components/schedule/ScheduleSide';
import { knownDubs } from '../components/schedule/dubKnown';
import {
  countPerDay,
  filterSchedule,
  formatCountdown,
  nowLineIndex,
  startOfLocalDay,
  upNext,
  type ScheduleFilters,
} from '../components/schedule/timeline';

const VIEW_MODES = ['compact', 'list', 'grid'] as const;
type ViewMode = (typeof VIEW_MODES)[number];

const DAY_COUNT = 7;

const FORMATS = ['TV', 'TV_SHORT', 'MOVIE', 'ONA', 'OVA', 'SPECIAL'];

const viewOptions = () => [
  { value: 'compact' as const, icon: <Menu aria-hidden="true" className="h-4 w-4" />, 'aria-label': t('schedule.compactRows'), title: t('schedule.compactRows') },
  { value: 'list' as const, icon: <List aria-hidden="true" className="h-4 w-4" />, 'aria-label': t('schedule.timeline'), title: t('schedule.timeline') },
  { value: 'grid' as const, icon: <Grid aria-hidden="true" className="h-4 w-4" />, 'aria-label': t('schedule.posters'), title: t('schedule.posters') },
];

function timeOf(seconds: number, format: TimeFormat): string {
  return formatTime(seconds * 1000, format, { hour: '2-digit', minute: '2-digit' }, locale());
}

const episodeText = (episode: number) =>
  episode > 0 ? t('common.episodeNumber', { number: episode }) : t('schedule.newEpisode');

/** The NOW mark between the episodes that aired and the ones still to come. */
const NowLine: React.FC<{ now: number; timeline: boolean }> = ({ now, timeline }) => {
  const format = useTimeFormat();
  return (
  <li className={clsx('relative flex items-center gap-2', timeline ? 'py-1' : 'px-3 py-1')}>
    {timeline && (
      <span
        aria-hidden="true"
        className="absolute -start-[24px] top-1/2 h-3 w-3 -translate-y-1/2 rounded-full bg-accent-500 motion-safe:animate-pulse"
      />
    )}
    <span className="text-[11px] font-semibold uppercase tracking-wider tabular-nums text-accent-300">
      {tn('schedule.nowAt', { time: <span className="font-medium normal-case tracking-normal">{timeOf(now, format)}</span> })}
    </span>
    <span aria-hidden="true" className="h-px flex-1 bg-accent-500" />
  </li>
  );
};

/** One schedule entry; the three layouts differ only in arrangement. */
const ScheduleEntry: React.FC<{ entry: AiringEntry; now: number; variant: ViewMode; mine: boolean }> = ({
  entry,
  now,
  variant,
  mine,
}) => {
  const hasAired = entry.airing_at <= now;
  const airsSoon = !hasAired && entry.airing_at - now < 3600;
  const time = timeOf(entry.airing_at, useTimeFormat());
  const countdown = formatCountdown(entry.airing_at, now);

  // The link and the bell are siblings: a button nested inside a link is invalid and
  // cannot be used on its own from the keyboard.
  const rowClass = 'group rounded-md transition-colors hover:bg-ink/5';
  // Episodes that have already aired step back.
  const linkClass = clsx('rounded-md', hasAired && 'opacity-60');
  const to = `/detail/${entry.media_id}`;
  const timeNode = <span className="text-[13px] font-medium tabular-nums text-text-secondary">{time}</span>;
  const countdownNode = (
    <span className={clsx('text-xs tabular-nums', airsSoon ? 'text-accent-300' : 'text-text-muted')}>
      {countdown}
    </span>
  );
  const bell = (kind: 'icon' | 'overlay', className?: string) => (
    <NotifyBell
      mediaId={entry.media_id}
      title={entry.media_title}
      cover={entry.cover_image}
      variant={kind}
      className={className}
    />
  );

  if (variant === 'compact') {
    return (
      <div className={clsx(rowClass, 'flex h-9 items-center gap-1 pe-1')}>
        <Link
          to={to}
          draggable={false}
          className={clsx(linkClass, 'flex h-full min-w-0 flex-1 items-center gap-4 ps-3 pe-2')}
        >
          <span className="w-[4.5rem] flex-shrink-0 whitespace-nowrap">{timeNode}</span>
          <span className="min-w-0 flex-1 truncate text-[13px] text-text-primary">{entry.media_title}</span>
          <span className="flex-shrink-0 text-xs text-text-muted">{episodeText(entry.episode)}</span>
          <span className="w-24 flex-shrink-0 text-end">{countdownNode}</span>
        </Link>
        {bell('icon')}
      </div>
    );
  }

  if (variant === 'grid') {
    return (
      <div className={clsx(rowClass, 'relative')}>
        <Link to={to} draggable={false} className={clsx(linkClass, 'flex flex-col')}>
          <div className="relative aspect-[2/3] w-full overflow-hidden rounded-xl ui-card bg-bg-card">
            <ShimmerImage
              src={entry.cover_image}
              alt=""
              className="h-full w-full transition-transform duration-150 group-hover:scale-[1.04]"
            />
            <span className="absolute start-2 top-2 rounded-sm bg-black/70 px-1.5 py-0.5 text-[11px] font-semibold leading-[14px] tabular-nums text-white">
              {time}
            </span>
          </div>
          <span className="mt-2 text-[13px] font-medium leading-[18px] text-text-primary line-clamp-2">
            {entry.media_title}
          </span>
          <span className="mt-0.5 flex items-center justify-between gap-2 text-xs text-text-muted">
            <span>{episodeText(entry.episode)}</span>
            {countdownNode}
          </span>
        </Link>
        {bell('overlay', 'absolute end-2 top-2')}
      </div>
    );
  }

  // Timeline card: the left edge is the accent for anime on the user's list.
  return (
    <div
      className={clsx(
        rowClass,
        'flex items-center gap-1 border-s-[3px] bg-bg-card pe-2',
        mine ? 'border-s-accent-500' : 'border-s-border-strong'
      )}
    >
      <Link to={to} draggable={false} className={clsx(linkClass, 'flex min-w-0 flex-1 items-center gap-3 p-2')}>
        <span className="flex w-[5.5rem] flex-shrink-0 flex-col whitespace-nowrap">
          {timeNode}
          {countdownNode}
        </span>
        <ShimmerImage src={entry.cover_image} alt="" className="aspect-[2/3] w-12 flex-shrink-0 rounded-md" />
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="text-[13px] font-medium leading-[18px] text-text-primary line-clamp-2">
            {entry.media_title}
          </span>
          <span className="mt-0.5 flex items-center gap-2 text-xs text-text-muted">
            <span>{episodeText(entry.episode)}</span>
            {entry.format && (
              <>
                <span aria-hidden="true">·</span>
                <span>{mediaFormatLabel(entry.format)}</span>
              </>
            )}
          </span>
        </span>
      </Link>
      {bell('icon')}
    </div>
  );
};

export const Schedule: React.FC = () => {
  // Day and filters live in the URL, so Back and a reload keep them.
  const [params, setParams] = useSearchParams();
  const navigate = useNavigate();
  const selectedDayOffset = Math.min(DAY_COUNT - 1, Math.max(0, parseInt(params.get('day') ?? '0', 10) || 0));
  const filters: ScheduleFilters = {
    format: FORMATS.includes(params.get('format') ?? '') ? params.get('format')! : '',
    myList: params.get('mine') === '1',
    dub: params.get('dub') === '1',
  };
  const setParam = useCallback(
    (key: string, value: string | null) =>
      setParams(
        (prev) => {
          const next = new URLSearchParams(prev);
          if (value === null) next.delete(key);
          else next.set(key, value);
          return next;
        },
        { replace: true }
      ),
    [setParams]
  );

  // One-time move of 1.2.0's saved "Only my watchlist" choice into the URL filter.
  useEffect(() => {
    try {
      const key = 'anix-choice:schedule-scope';
      if (window.localStorage.getItem(key) === 'watchlist' && !params.has('mine')) setParam('mine', '1');
      window.localStorage.removeItem(key);
    } catch {
      // Storage can be unavailable; nothing to move then.
    }
    // Runs once, on the first visit.
  }, []);

  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000));
  const [viewMode, setViewMode] = useStoredChoice<ViewMode>('schedule-layout', VIEW_MODES, 'list');
  const playful = usePlayful();
  const wide = useMediaQuery('(min-width: 1200px)');
  const { data: watchlist = [] } = useWatchlist();
  const { data: bells = [] } = useNotificationBells();
  const myListIds = useMemo(
    () => new Set([...watchlist.map((entry) => entry.anilist_id), ...bells.map((bell) => bell.media_id)]),
    [watchlist, bells]
  );
  // Read once per visit; the Detail page adds to it.
  const [dubIds] = useState(knownDubs);

  // Drives the countdowns and the NOW line and, because "today" is derived from it, the day rollover.
  useEffect(() => {
    const interval = window.setInterval(() => setNow(Math.floor(Date.now() / 1000)), 30000);
    return () => window.clearInterval(interval);
  }, []);

  const todayStart = startOfLocalDay(now);

  const days = useMemo(
    () =>
      Array.from({ length: DAY_COUNT }, (_, i) => {
        const start = startOfLocalDay(todayStart, i);
        const date = new Date(start * 1000);
        return {
          offset: i,
          start,
          end: startOfLocalDay(todayStart, i + 1),
          weekday: date.toLocaleDateString(locale(), { weekday: 'short' }),
          longLabel: date.toLocaleDateString(locale(), { weekday: 'long', month: 'long', day: 'numeric' }),
          dayOfMonth: date.getDate(),
        };
      }),
    [todayStart]
  );

  const { data: schedule = [], isLoading, isError, error, refetch, isFetching } = useAiringSchedule(
    days[0].start,
    days[DAY_COUNT - 1].end
  );

  const activeDay = days[selectedDayOffset] ?? days[0];
  // Entries built without AniList carry negative ids.
  const estimated = useMemo(() => schedule.some((entry) => entry.id < 0), [schedule]); // i18n-ignore: a comparison, not text

  const filtered = useMemo(
    () => filterSchedule(schedule, filters, myListIds, dubIds),
    // `filters` is rebuilt each render from the URL; its fields are what matter.
    [schedule, filters.format, filters.myList, filters.dub, myListIds, dubIds]
  );
  const counts = useMemo(() => countPerDay(filtered, days), [filtered, days]);

  const entries = useMemo(
    () =>
      filtered
        .filter((entry) => entry.airing_at >= activeDay.start && entry.airing_at < activeDay.end)
        .sort((a, b) => a.airing_at - b.airing_at),
    [filtered, activeDay.start, activeDay.end]
  );
  const nowIndex = viewMode === 'grid' ? null : nowLineIndex(entries, now, activeDay.offset === 0);
  const next = useMemo(() => upNext(schedule, myListIds, now), [schedule, myListIds, now]);
  const anyFilter = !!filters.format || filters.myList || filters.dub;
  const clearFilters = () =>
    setParams(
      (prev) => {
        const nextParams = new URLSearchParams(prev);
        ['format', 'mine', 'dub'].forEach((key) => nextParams.delete(key));
        return nextParams;
      },
      { replace: true }
    );

  const list = (
    <>
      {isLoading ? (
        <div aria-busy="true" className="flex flex-col gap-2">
          {Array.from({ length: 8 }).map((_, i) => (
            <div key={i} className="h-[88px] rounded-md shimmer-placeholder" />
          ))}
        </div>
      ) : isError ? (
        <ErrorState
          title={t('schedule.couldNotLoad')}
          error={error}
          onRetry={() => refetch()}
          retryLabel={isFetching ? t('schedule.retrying') : t('common.retry')}
        />
      ) : entries.length === 0 ? (
        filters.myList && myListIds.size === 0 ? (
          <EmptyState
            icon={<Bookmark aria-hidden="true" className="h-8 w-8" />}
            title={t('schedule.listEmptyTitle')}
            description={t('schedule.listEmptyDescription')}
            mood="sleepy"
            action={
              <Button variant="secondary" onClick={() => navigate('/discover')}>
                {t('schedule.browseAnime')}
              </Button>
            }
          />
        ) : (
          <EmptyState
            icon={<Calendar aria-hidden="true" className="h-8 w-8" />}
            title={
              anyFilter
                ? t('schedule.noMatchTitle')
                : playful
                  ? t('schedule.quietDayTitle')
                  : t('schedule.nothingScheduledTitle')
            }
            description={
              anyFilter
                ? t('schedule.noMatchDescription', { date: activeDay.longLabel })
                : playful
                  ? t('schedule.quietDayDescription', { date: activeDay.longLabel })
                  : t('schedule.nothingScheduledDescription', { date: activeDay.longLabel })
            }
            mood="sleepy"
            kaomoji={anyFilter ? undefined : '(-_-) zzz' /* i18n-ignore */}
            action={
              anyFilter ? (
                <Button variant="ghost" onClick={clearFilters}>
                  {t('schedule.showEverything')}
                </Button>
              ) : undefined
            }
          />
        )
      ) : viewMode === 'grid' ? (
        <div className="grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-x-4 gap-y-6">
          {entries.map((entry) => (
            <ScheduleEntry key={entry.id} entry={entry} now={now} variant="grid" mine={myListIds.has(entry.media_id)} />
          ))}
        </div>
      ) : (
        <ol
          aria-label={t('schedule.episodesOn', { date: activeDay.longLabel })}
          className={clsx(
            'relative flex flex-col',
            viewMode === 'list' &&
              'gap-2 ps-7 before:absolute before:bottom-3 before:start-[9px] before:top-3 before:w-px before:bg-border-strong'
          )}
        >
          {entries.map((entry, i) => {
            const aired = entry.airing_at <= now;
            const mine = myListIds.has(entry.media_id);
            return (
              <React.Fragment key={entry.id}>
                {nowIndex === i && <NowLine now={now} timeline={viewMode === 'list'} />}
                <li className="relative">
                  {viewMode === 'list' && (
                    <span
                      aria-hidden="true"
                      className={clsx(
                        'absolute -start-[23px] top-1/2 h-2.5 w-2.5 -translate-y-1/2 rounded-full border-2',
                        aired
                          ? 'border-border-strong bg-bg-page'
                          : mine
                            ? 'border-accent-500 bg-accent-500'
                            : 'border-text-muted bg-bg-page'
                      )}
                    />
                  )}
                  <ScheduleEntry entry={entry} now={now} variant={viewMode} mine={mine} />
                </li>
              </React.Fragment>
            );
          })}
          {nowIndex === entries.length && <NowLine now={now} timeline={viewMode === 'list'} />}
        </ol>
      )}
    </>
  );

  return (
    <div className="flex flex-col gap-5 pb-16 animate-fade-in">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex flex-col gap-0.5">
          <h1 className="text-xl font-semibold text-text-primary">{t('schedule.title')}</h1>
          {estimated && (
            <p className="text-xs text-text-muted">
              {t('schedule.estimatedTimes')}
            </p>
          )}
        </div>
        <SegmentedControl aria-label={t('schedule.layout')} value={viewMode} onChange={setViewMode} options={viewOptions()} />
      </div>

      <div className="-mx-1 overflow-x-auto px-1 no-scrollbar">
        <SegmentedControl
          aria-label={t('schedule.day')}
          value={String(activeDay.offset)}
          onChange={(value) => setParam('day', value === '0' ? null : value)}
          options={days.map((day, i) => ({
            value: String(day.offset),
            'aria-label': t(day.offset === 0 ? 'schedule.todayEpisodes' : 'schedule.dayEpisodes', {
              date: day.longLabel,
              count: counts[i],
            }),
            label: (
              <span className="flex items-center gap-1.5">
                {day.offset === 0 && <span aria-hidden="true" className="h-1.5 w-1.5 rounded-full bg-accent-500" />}
                <span>{day.weekday}</span>
                <span className="tabular-nums">{day.dayOfMonth}</span>
                {!isLoading && !isError && (
                  <span className="text-[11px] font-normal tabular-nums text-text-muted">{t('schedule.episodeCountShort', { count: counts[i] })}</span>
                )}
              </span>
            ),
          }))}
        />
      </div>

      <div role="group" aria-label={t('schedule.filters')} className="flex flex-wrap items-center gap-2">
        <Chip selected={!filters.format} onClick={() => setParam('format', null)}>
          {t('schedule.allFormats')}
        </Chip>
        {FORMATS.map((format) => (
          <Chip
            key={format}
            selected={filters.format === format}
            onClick={() => setParam('format', filters.format === format ? null : format)}
          >
            {mediaFormatLabel(format)}
          </Chip>
        ))}
        <span aria-hidden="true" className="mx-1 h-4 w-px bg-border-strong" />
        <Chip selected={filters.myList} onClick={() => setParam('mine', filters.myList ? null : '1')} className="gap-1.5">
          <Bookmark aria-hidden="true" className={clsx('h-3.5 w-3.5', filters.myList && 'fill-current')} />
          {t('schedule.myList')}
        </Chip>
        <span title={t('schedule.dubHint')}>
          <Chip selected={filters.dub} onClick={() => setParam('dub', filters.dub ? null : '1')} className="gap-1.5">
            <Mic aria-hidden="true" className="h-3.5 w-3.5" />
            {t('schedule.dubAvailable')}
          </Chip>
        </span>
      </div>

      {wide ? (
        <div className="grid grid-cols-[minmax(0,1fr)_300px] items-start gap-6">
          <div className="min-w-0">{list}</div>
          <aside aria-label={t('schedule.thisWeek')} className="sticky top-0 flex flex-col gap-4">
            <UpNextCard entry={next} now={now} />
            <MonthCalendar
              days={days.map((day, i) => ({ ...day, count: counts[i] }))}
              selected={activeDay.offset}
              onSelect={(offset) => setParam('day', offset === 0 ? null : String(offset))}
            />
          </aside>
        </div>
      ) : (
        list
      )}
    </div>
  );
};

export default Schedule;
