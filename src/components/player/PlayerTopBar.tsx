import React, { useEffect, useState } from 'react';
import type { RefObject } from 'react';
import { ArrowLeft } from 'lucide-react';
import { IconButton } from './IconButton';
import { endsAt } from './playerLogic';
import { formatTime } from '../../lib/time';
import { useTimeFormat } from '../../hooks/useSettings';
import { t } from '../../i18n';

interface PlayerTopBarProps {
  title: string;
  /** One muted line: episode, audio, provider. */
  meta?: string;
  onBack: () => void;
  visible?: boolean;
  /** Ghost icon buttons on the right. */
  children?: React.ReactNode;
}

export const PlayerTopBar: React.FC<PlayerTopBarProps> = ({
  title,
  meta,
  onBack,
  visible = true,
  children,
}) => (
  <div
    className={`pointer-events-none absolute inset-x-0 top-0 z-20 bg-gradient-to-b from-black/85 via-black/50 to-transparent pb-12 transition-opacity duration-200 ${
      visible ? 'opacity-100' : 'opacity-0'
    }`}
  >
    <div
      data-player-bar="top"
      className={`flex items-center gap-2 px-4 pt-3 ${visible ? 'pointer-events-auto' : ''}`}
      onMouseDown={(e) => e.preventDefault()}
    >
      <IconButton label={t('common.back')} onClick={onBack}>
        <ArrowLeft aria-hidden="true" className="h-5 w-5" />
      </IconButton>
      <div className="min-w-0 flex-1 [text-shadow:0_1px_2px_rgb(0_0_0/0.6)]">
        <h1 className="truncate text-sm font-semibold text-white">{title}</h1>
        {meta && <p className="truncate text-xs text-white/80">{meta}</p>}
      </div>
      {children && <div className="flex flex-shrink-0 items-center gap-1">{children}</div>}
    </div>
  </div>
);


/** The time now and when the episode ends at the current speed. */
export const PlayerClock: React.FC<{ videoRef: RefObject<HTMLVideoElement> }> = ({ videoRef }) => {
  const [now, setNow] = useState(() => Date.now());
  const format = useTimeFormat();
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, []);
  const video = videoRef.current;
  const end = video ? endsAt(now, video.currentTime, video.duration, video.playbackRate) : null;
  return (
    <p className="mr-2 hidden whitespace-nowrap text-xs tabular-nums text-white/80 [text-shadow:0_1px_2px_rgb(0_0_0/0.6)] min-[720px]:block">
      <span>{formatTime(now, format)}</span>
      {end !== null && <span className="ml-2 text-white/60">{t('playerTopBar.endsAt', { time: formatTime(end, format) })}</span>}
    </p>
  );
};

interface PauseDetailsProps {
  title: string;
  episode: number;
  episodeTitle: string | null;
  meta: string;
}

/** Shown over the picture after a long pause. */
export const PauseDetails: React.FC<PauseDetailsProps> = ({ title, episode, episodeTitle, meta }) => (
  <div
    aria-hidden="true"
    // The panel stays on the left of the LTR player; dir="auto" gives each line its own text direction.
    className="pointer-events-none absolute inset-y-0 left-0 z-10 flex w-full max-w-xl flex-col justify-center gap-2 text-left bg-gradient-to-r from-black/80 via-black/50 to-transparent px-10 animate-fade-in motion-reduce:animate-none [text-shadow:0_1px_3px_rgb(0_0_0/0.7)]"
  >
    <p dir="auto" className="text-xs font-semibold uppercase tracking-wider text-white/70">{t('playerTopBar.paused')}</p>
    <h2 dir="auto" className="text-2xl font-semibold text-white">{title}</h2>
    <p dir="auto" className="text-sm text-white/90">
      {t('common.episodeNumber', { number: episode })}
      {episodeTitle && episodeTitle.trim().toLowerCase() !== `episode ${episode}` ? ` · ${episodeTitle}` : ''}
    </p>
    {meta && <p dir="auto" className="text-xs text-white/70">{meta}</p>}
  </div>
);
