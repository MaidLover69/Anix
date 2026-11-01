/**
 * Meme mode's game pieces (Settings › Appearance › Meme mode, Medium and up): the player level
 * and day combo in the sidebar and Stats, and the power-level meter on the anime page. All of it
 * is drawn from data the app already has: 1 EXP per watched episode, the watch streak, the
 * anime's score. Looks live in theme/fun.css.
 */
import React, { useEffect, useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import clsx from 'clsx';
import { useWatchedHistory } from '../hooks/useLibrary';
import { useUIStore } from '../stores/uiStore';
import { watchStreak } from './library/format';
import { playerLevel, powerLevel, type PlayerLevel } from '../lib/funLevel';
import { formatNumber, t, type TKey } from '../i18n';
import { getSeenLevel, setSeenLevel } from '../lib/funSeen';
import { useMemeLevel } from '../hooks/useSettings';
import { Meme, MemeImage } from './meme/Meme';

export { Sfx } from './Sfx';

const rankName = (level: PlayerLevel) => t(`fun.rank.${level.rank}` as TKey);

/** EXP and day combo from the watch history and the days an episode was watched (same queries as Stats). */
export function useFunStats() {
  const history = useWatchedHistory();
  const watchTimes = useQuery<number[]>({ queryKey: ['watchTimes'], queryFn: () => invoke<number[]>('get_watch_times') });
  const rows = history.data;
  const level = useMemo(() => (rows ? playerLevel(rows.length) : null), [rows]);
  const streak = useMemo(
    () => watchStreak(watchTimes.data ?? rows?.map((r) => r.updated_at) ?? []),
    [watchTimes.data, rows]
  );
  return { exp: rows?.length ?? 0, level, streak };
}

const ExpBar: React.FC<{ level: PlayerLevel; className?: string }> = ({ level, className }) => (
  <span aria-hidden="true" className={clsx('fun-exp block', className)}>
    <span className="fun-exp-fill block h-full" style={{ width: `${(level.intoLevel / level.levelSize) * 100}%` }} />
  </span>
);

/** The sidebar footer's level badge, EXP bar and day combo; one "LEVEL UP!" toast per level gained. */
export const FunLevelBadge: React.FC<{ expanded: boolean }> = ({ expanded }) => {
  const { level, streak } = useFunStats();
  const memeLevel = useMemeLevel();
  const showToast = useUIStore((s) => s.showToast);
  useEffect(() => {
    if (!level) return;
    const seen = getSeenLevel();
    if (seen !== null && level.level > seen) {
      showToast({
        type: 'success',
        title: t('fun.levelUp'),
        message: t('fun.levelUpMessage', { level: level.level, rank: rankName(level) }),
        art: 'levelUp',
        meme: level.rank === 'legend' ? 'levelTop' : undefined,
      });
    }
    setSeenLevel(level.level);
  }, [level, showToast]);
  if (!level) return null;
  const label = t('fun.levelLabel', { level: level.level, rank: rankName(level) });
  return (
    <div className="fun-badge mx-1 mb-1 flex flex-col gap-1 rounded-md px-2 py-1.5" title={expanded ? undefined : label}>
      <span className="sr-only">{label}</span>
      <span aria-hidden="true" className="flex items-baseline gap-1.5">
        {/* Collapsed (64 px), only the number fits; the label is in the title and sr-only text. */}
        <span className="fun-level text-[12px]">
          {expanded ? t('fun.levelShort', { level: level.level }) : formatNumber(level.level)}
        </span>
        {expanded && <span className="truncate text-[11px] font-semibold text-text-secondary">{rankName(level)}</span>}
      </span>
      <ExpBar level={level} className="h-1.5" />
      {expanded && streak.current > 0 && (
        <span className="fun-combo text-[11px]">
          {memeLevel === 'high'
            ? `🔥 ${t('statsTab.streakDays', { count: streak.current })}`
            : t('fun.combo', { count: streak.current })}
        </span>
      )}
    </div>
  );
};

/** Stats tab: the level card. */
export const FunLevelCard: React.FC = () => {
  const { exp, level } = useFunStats();
  if (!level) return null;
  return (
    <section className="ui-card fun-level-card relative flex flex-col gap-2 overflow-hidden rounded-xl bg-bg-card p-4">
      <MemeImage name="jinwoo-arise" className="lvl" />
      <span className="text-xs font-semibold uppercase tracking-wider text-text-muted">
        <Meme spot="levelCard" plain={t('fun.levelCard')} />
      </span>
      <span className="flex flex-wrap items-baseline gap-x-3">
        <span className="fun-level text-3xl">{t('fun.levelShort', { level: level.level })}</span>
        <span className="text-base font-semibold text-text-primary">{rankName(level)}</span>
      </span>
      <ExpBar level={level} className="h-2.5" />
      <span className="text-xs tabular-nums text-text-secondary">
        {t('fun.expProgress', { exp: formatNumber(level.intoLevel), next: formatNumber(level.levelSize) })}
        {' · '}
        {t('fun.expTotal', { exp: formatNumber(exp) })}
      </span>
    </section>
  );
};

/** The anime page's score as a power-level meter; 90 and up is "over 9000". */
export const PowerLevelMeter: React.FC<{ percentScore: number | null; malScore: number | null }> = ({
  percentScore,
  malScore,
}) => {
  const power = powerLevel(percentScore, malScore);
  if (!power) return null;
  return (
    <div className="fun-power flex max-w-xs flex-col gap-1">
      <span className="flex items-baseline justify-between gap-3 text-xs font-bold">
        <span className="text-text-secondary">{t('fun.powerLevel', { value: formatNumber(power.value) })}</span>
        {power.over9000 && <span className="fun-over9000">{t('fun.over9000')}</span>}
      </span>
      <span aria-hidden="true" className="fun-exp block h-2">
        <span className="fun-exp-fill block h-full" style={{ width: `${power.percent}%` }} />
      </span>
    </div>
  );
};
