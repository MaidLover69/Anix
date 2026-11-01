import React, { useEffect, useState } from 'react';
import { Check } from 'lucide-react';
import { prefersLessMotion } from '../lib/motion';
import { t } from '../i18n';
import { useFunMode } from '../hooks/useSettings';
import { Sfx } from './Sfx';
import { Meme, useHighModule } from './meme/Meme';

const CONFETTI_COLOURS = ['bg-accent-500', 'bg-state-success', 'bg-state-warning', 'bg-accent-200'];
const CONFETTI_PIECES = 14;
// Burst delay (300 ms) + flight (900 ms) in detail.css, with a little to spare.
const CONFETTI_MS = 1400;

/** A small burst of confetti from the stamp's middle, once; nothing under reduced motion or Fewer effects. */
const Confetti: React.FC = () => {
  const [pieces] = useState(() =>
    prefersLessMotion()
      ? []
      : Array.from({ length: CONFETTI_PIECES }, (_, i) => {
          const angle = Math.random() * Math.PI * 2;
          const distance = 50 + Math.random() * 70;
          return {
            colour: CONFETTI_COLOURS[i % CONFETTI_COLOURS.length],
            style: {
              '--dx': `${Math.cos(angle) * distance}px`,
              '--dy': `${Math.sin(angle) * distance - 20}px`,
              '--r': `${Math.random() * 540 - 270}deg`,
            } as React.CSSProperties,
          };
        })
  );
  const [done, setDone] = useState(pieces.length === 0);
  useEffect(() => {
    if (done) return;
    const timer = window.setTimeout(() => setDone(true), CONFETTI_MS);
    return () => window.clearTimeout(timer);
  }, [done]);
  if (done) return null;
  return (
    <span aria-hidden="true" className="ui-confetti pointer-events-none absolute inset-0 overflow-visible">
      {pieces.map((piece, i) => (
        <i key={i} className={piece.colour} style={piece.style} />
      ))}
    </span>
  );
};

/**
 * The stamp in the anime page's header corner for an anime watched to the end (shown only with
 * Playful touches). All three art variants render; themes.css shows one: the stamp by default,
 * 完 in Manga, a "THE END!" tag in Comic. It slams down (detail.css) with a burst of confetti.
 */
export const CompletedStamp: React.FC = () => {
  // Medium: "Quest complete" with a ドン; High: "QUEST COMPLETE", the plain word kept in the small line.
  const fun = useFunMode();
  const high = useHighModule();
  const plain = t('completedStamp.completed');
  const label = fun && !high ? t('fun.questComplete') : plain;
  return (
    <span className="ui-stamp relative inline-flex select-none">
      {fun && <Sfx text={t('fun.sfxDon')} className="fun-sfx-stamp" />}
      <span aria-hidden="true" className="contents">
        <span
          data-art="ring"
          className="flex-col items-center rounded-md border-4 border-double border-state-success bg-bg-page/60 px-3.5 py-1 text-state-success"
        >
          <span className="flex items-center gap-1.5 text-2xl font-black uppercase tracking-wider">
            <Check className="h-5 w-5" strokeWidth={3} />
            <Meme spot="stamp" plain={plain} medium={t('fun.questComplete')} />
          </span>
          <span className="text-[10px] font-bold uppercase tracking-widest">
            {high ? `${plain} · ${t('completedStamp.theEnd')}` : t('completedStamp.theEnd')}
          </span>
        </span>
        <span data-art="kan">
          {/* 完, drawn as strokes: the roof, then 二 and the two legs of 儿. */}
          <svg
            viewBox="0 0 24 24"
            width={22}
            height={22}
            focusable="false"
            fill="none"
            stroke="currentColor"
            strokeWidth={2}
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <path d="M12 2v2.5M4 8V5.5h16V8M8 10.5h8M4 14h16M10 14c0 4-2 7-6 8.5M14 14v6.5c0 1.5 1 2 2.5 2h2c1 0 1.5-.5 1.5-2.5" />
          </svg>
        </span>
        <span data-art="end">{t('completedStamp.theEnd')}</span>
      </span>
      <span className="sr-only">{label}</span>
      <Confetti />
    </span>
  );
};
