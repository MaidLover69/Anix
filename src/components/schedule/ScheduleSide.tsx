import React from 'react';
import { Link } from 'react-router-dom';
import clsx from 'clsx';
import { ShimmerImage } from '../ShimmerImage';
import { formatCountdown, monthWeeks } from './timeline';
import { formatTime } from '../../lib/time';
import { useTimeFormat } from '../../hooks/useSettings';
import type { AiringEntry } from '../../types';
import { locale, t } from '../../i18n';

export interface CalendarDay {
  offset: number;
  start: number;
  longLabel: string;
  count: number;
}

// 2024-01-01 was a Monday.
const weekdays = () =>
  Array.from({ length: 7 }, (_, i) => {
    const date = new Date(2024, 0, 1 + i);
    return {
      narrow: date.toLocaleDateString(locale(), { weekday: 'narrow' }),
      long: date.toLocaleDateString(locale(), { weekday: 'long' }),
    };
  });

/** The next episode from the user's list (watchlist and bells) still to air this week. */
export const UpNextCard: React.FC<{ entry: AiringEntry | null; now: number }> = ({ entry, now }) => {
  const format = useTimeFormat();
  return (
  <section aria-labelledby="schedule-up-next" className="ui-card rounded-xl border border-border bg-bg-card p-3">
    <h2 id="schedule-up-next" className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-text-muted">
      {t('scheduleSide.upNext')}
    </h2>
    {entry ? (
      <Link to={`/detail/${entry.media_id}`} draggable={false} className="flex items-center gap-3 rounded-md transition-colors hover:bg-ink/5">
        <ShimmerImage src={entry.cover_image} alt="" className="aspect-[2/3] w-14 flex-shrink-0 rounded-md" />
        <span className="flex min-w-0 flex-col">
          <span className="text-[13px] font-medium leading-[18px] text-text-primary line-clamp-2">{entry.media_title}</span>
          <span className="mt-0.5 text-xs text-text-muted">{entry.episode > 0 ? t('common.episodeNumber', { number: entry.episode }) : t('scheduleSide.newEpisode')}</span>
          <span className="mt-0.5 text-xs font-medium tabular-nums text-accent-300">
            {formatTime(entry.airing_at * 1000, format, { weekday: 'short', hour: '2-digit', minute: '2-digit' }, locale())}{' '}
            · {formatCountdown(entry.airing_at, now)}
          </span>
        </span>
      </Link>
    ) : (
      <p className="text-xs text-text-muted">
        {t('scheduleSide.nothingThisWeek')}
      </p>
    )}
  </section>
  );
};

/**
 * The month of the selected day, Monday first, with the number of episodes on each day of the
 * loaded week. Those days are buttons that select the day; the others are plain.
 */
export const MonthCalendar: React.FC<{
  days: CalendarDay[];
  selected: number;
  onSelect: (offset: number) => void;
}> = ({ days, selected, onSelect }) => {
  const anchor = new Date((days[selected] ?? days[0]).start * 1000);
  const year = anchor.getFullYear();
  const month = anchor.getMonth();
  const byDate = new Map(
    days
      .map((d) => ({ d, date: new Date(d.start * 1000) }))
      .filter(({ date }) => date.getFullYear() === year && date.getMonth() === month)
      .map(({ d, date }) => [date.getDate(), d])
  );
  const todayStart = days[0]?.start;

  return (
    <section aria-labelledby="schedule-month" className="ui-card rounded-xl border border-border bg-bg-card p-3">
      <h2 id="schedule-month" className="mb-2 text-[13px] font-semibold text-text-primary">
        {anchor.toLocaleDateString(locale(), { month: 'long', year: 'numeric' })}
      </h2>
      <table className="w-full table-fixed border-collapse text-center">
        <thead>
          <tr>
            {weekdays().map((w, i) => (
              <th key={i} scope="col" abbr={w.long} className="pb-1 text-[11px] font-medium text-text-muted">
                <span aria-hidden="true">{w.narrow}</span>
                <span className="sr-only">{w.long}</span>
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {monthWeeks(year, month).map((week, wi) => (
            <tr key={wi}>
              {week.map((date, di) => {
                const day = date !== null ? byDate.get(date) : undefined;
                return (
                  <td key={di} className="p-0.5">
                    {date === null ? null : day ? (
                      <button
                        type="button"
                        onClick={() => onSelect(day.offset)}
                        aria-pressed={day.offset === selected}
                        aria-label={t('scheduleSide.dayEpisodes', { date: day.longLabel, count: day.count })}
                        className={clsx(
                          'flex h-10 w-full cursor-pointer flex-col items-center justify-center rounded-md text-xs tabular-nums transition-colors',
                          day.offset === selected
                            ? 'bg-accent-tint text-accent-300'
                            : 'text-text-primary hover:bg-ink/10',
                          day.start === todayStart && day.offset !== selected && 'ring-1 ring-inset ring-accent-500/60'
                        )}
                      >
                        <span className="font-medium">{date}</span>
                        <span className="text-[10px] leading-3 text-text-muted">{day.count}</span>
                      </button>
                    ) : (
                      <span className="flex h-10 items-center justify-center text-xs tabular-nums text-text-disabled">{date}</span>
                    )}
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
    </section>
  );
};
