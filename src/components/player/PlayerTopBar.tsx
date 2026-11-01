import React, { useEffect, useState } from 'react';
import type { RefObject } from 'react';
import { ArrowLeft } from 'lucide-react';
import { IconButton } from './IconButton';
import { endsAt } from './playerLogic';
import { formatTime } from '../../lib/time';
import { useMemeLevel, useTimeFormat } from '../../hooks/useSettings';
import { Meme, MemeFrame, MemeImage, useHighModule } from '../meme/Meme';
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
}) => {
  const high = useHighModule();
  return (
    <div
      className={`pointer-events-none absolute inset-x-0 top-0 z-20 bg-gradient-to-b from-black/85 via-black/50 to-transparent pb-12 transition-[opacity,transform] duration-200 ease-out ${
        visible ? 'opacity-100' : '-translate-y-2.5 opacity-0'
      }`}
    >
      <div
        data-player-bar="top"
        className={`flex items-center gap-2 px-4 pt-3 ${visible ? 'pointer-events-auto' : ''}`}
        onMouseDown={(e) => e.preventDefault()}
      >
        <IconButton label={t('common.back')} title={high?.memeText('back', t('common.back'))} onClick={onBack}>
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
};


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
      {end !== null && (
        <span className="meme-clock ml-2 text-white/60">
          <Meme spot="clock" plain={t('playerTopBar.endsAt', { time: formatTime(end, format) })} />
        </span>
      )}
    </p>
  );
};

interface PauseDetailsProps {
  title: string;
  episode: number;
  episodeTitle: string | null;
  meta: string;
  /** Meme mode Medium and up, once the episode played to its end: "EPISODE CLEAR!" (Medium adds "+1 EXP"). */
  cleared?: boolean;
  /** Meme mode Medium and up, with an episode after this one: the "To be continued" arrow (High: JoJo's plate). */
  continued?: boolean;
}

/** The pause screen: after a long pause, the title large on a scrim down the left of the picture. */
export const PauseDetails: React.FC<PauseDetailsProps> = ({ title, episode, episodeTitle, meta, cleared, continued }) => {
  const medium = useMemeLevel() === 'medium';
  const high = useHighModule();
  return (
    <>
      <div
        aria-hidden="true"
        // The panel stays on the left of the LTR player; dir="auto" gives each line its own text direction.
        className="pointer-events-none absolute inset-y-0 left-0 z-10 flex w-[min(36rem,70%)] flex-col justify-center gap-2 text-left bg-gradient-to-r from-black/85 via-black/45 to-transparent px-10 animate-fade-in motion-reduce:animate-none [text-shadow:0_1px_3px_rgb(0_0_0/0.7)]"
      >
        {cleared && high ? (
          // High: the big words are drawn by CSS (meme-high-shell.css), so no meme line is written here.
          <p className={continued ? 'meme-clear-big' : 'meme-clear-big meme-final'} />
        ) : cleared && medium ? (
          <p dir="auto" className="fun-clear text-xl">
            {t('fun.episodeClear')} <span className="fun-clear-exp">{t('fun.plusExp')}</span>
          </p>
        ) : high ? (
          <div className="meme-pause flex flex-col items-start gap-1">
            <MemeFrame spot="paused" className="meme-za-img" />
            <p className="text-xs font-extrabold uppercase tracking-widest text-white/70">
              <Meme spot="paused" plain={t('playerTopBar.paused')} />
            </p>
          </div>
        ) : (
          <p dir="auto" className="text-xs font-extrabold uppercase tracking-widest text-white/70">{t('playerTopBar.paused')}</p>
        )}
        <h2 dir="auto" className="line-clamp-3 text-[clamp(1.5rem,3vw,2.4rem)] font-black leading-tight text-white">{title}</h2>
        <p dir="auto" className="text-base font-medium text-white/90">
          {t('common.episodeNumber', { number: episode })}
          {episodeTitle && episodeTitle.trim().toLowerCase() !== `episode ${episode}` ? ` · ${episodeTitle}` : ''}
        </p>
        {meta && <p dir="auto" className="text-xs text-white/70">{meta}</p>}
        {continued && medium && (
          <p dir="auto" className="fun-continued mt-4 self-start">
            {t('fun.toBeContinued')}
          </p>
        )}
      </div>
      {continued && <MemeImage name="jojo-to-be-continued" className="meme-tbc" />}
    </>
  );
};
