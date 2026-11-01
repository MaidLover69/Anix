/**
 * Meme mode › High, loaded only through `loadHigh()` (Meme.tsx): the line catalogue, the label
 * and image pieces and all High CSS. Images come from the bundled meme pack (`memes/<id>.webp`)
 * and are decorative; the plain words beside them, in the tooltip or in screen-reader text carry
 * the meaning.
 */
import React from 'react';
import clsx from 'clsx';
import '../../theme/meme-high.css';
import '../../theme/meme-high-shell.css';
import '../../theme/meme-high-pages.css';
import {
  ANIMATED,
  CREDITS,
  LINES,
  UBW_SOURCE,
  WORDS_ONLY_SERIES,
  fillLine,
  type MemeImageName,
  type MemeLine,
  type MemeSpot,
} from '../../lib/memeCatalogue';
import { t } from '../../i18n';

export { LINES, NEW_EP_POOL, CREDITS, WORDS_ONLY_SERIES, UBW_SOURCE, fillLine } from '../../lib/memeCatalogue';

type Vars = Record<string, string | number>;

const entry = (spot: MemeSpot): MemeLine => LINES[spot];

/** The spot's line, filled, in the reading direction of the page. */
export function memeLine(spot: MemeSpot, vars?: Vars): string {
  const { line, lineRtl } = entry(spot);
  return fillLine(lineRtl && document.documentElement.dir === 'rtl' ? lineRtl : line, vars);
}

/** The spot's own image, if it has one. */
export const spotImage = (spot: MemeSpot): MemeImageName | undefined => entry(spot).img;

const creditOf = (spot: MemeSpot) => (entry(spot).credit === 'ubw' ? t('meme.lineCredit', { source: UBW_SOURCE }) : '');

/** The meme as one string: "meme (plain)", or for kept spots the plain words first so a cut-off field still shows them. */
export function memeText(spot: MemeSpot, plain: string, vars?: Vars): string {
  const meme = memeLine(spot, vars);
  if (!entry(spot).keep) return `${meme} (${plain})`;
  return meme.toLowerCase().includes(plain.toLowerCase()) ? meme : `${plain} — ${meme.replace(/[\s:—]+$/, '')}`;
}

/**
 * The meme as markup. A kept spot shows the meme as decoration and the plain words visibly
 * (`.mm-k`); any other shows only the meme, with the plain words in the tooltip and for screen
 * readers (`.mm`). The dash is its own span, so right to left it lands between the two.
 */
export const MemeLabel: React.FC<{ spot: MemeSpot; plain: string; vars?: Vars }> = ({ spot, plain, vars }) => {
  const meme = memeLine(spot, vars);
  const credit = creditOf(spot);
  if (!entry(spot).keep) {
    return (
      <span className="mm" title={credit ? `${plain} · ${credit}` : plain}>
        <span aria-hidden="true">{meme}</span>
        <span className="sr-only">{plain}</span>
      </span>
    );
  }
  const joined = /[:…—!?.~”’"]$/.test(meme);
  return (
    <span className={clsx('mm-k', joined && 'nd')}>
      <span className="mm-d" aria-hidden="true" title={credit || undefined}>
        {meme}
      </span>
      {!joined && (
        <span className="mm-sep" aria-hidden="true">
          {' —'}
        </span>
      )}{' '}
      <span className="mm-p">{plain}</span>
    </span>
  );
};

const imageProps = {
  alt: '',
  'aria-hidden': true,
  draggable: false,
  loading: 'lazy',
  decoding: 'async',
  // A missing file hides the frame instead of showing a broken-image icon.
  onError: (e: React.SyntheticEvent<HTMLImageElement>) => e.currentTarget.classList.add('miss'),
} as const;

/**
 * One meme image. An animated one renders with its still frame; CSS shows the still under reduced
 * motion or Fewer effects, live, and a hidden lazy image is never fetched.
 */
export const MemeImg: React.FC<{ name: MemeImageName; className?: string }> = ({ name, className }) =>
  ANIMATED.has(name) ? (
    <>
      <img {...imageProps} className={clsx('mimg mimg-anim', className)} src={`memes/${name}.webp`} />
      <img {...imageProps} className={clsx('mimg mimg-still', className)} src={`memes/${name}.still.webp`} />
    </>
  ) : (
    <img {...imageProps} className={clsx('mimg', className)} src={`memes/${name}.webp`} />
  );

/** The spot's image; a `plate` spot also draws its line as a caption on it (decoration). */
export const MemePlate: React.FC<{ spot: MemeSpot; className?: string }> = ({ spot, className }) => {
  const { img, plate } = entry(spot);
  if (!img) return null;
  if (!plate) return <MemeImg name={img} className={className} />;
  return (
    <span className={clsx('meme-plate', className)} aria-hidden="true">
      <MemeImg name={img} />
      <span className="cap-plate">{memeLine(spot)}</span>
    </span>
  );
};

/** The isekai truck beside a portal, for the offline banner (no character). */
export const MemeTruck: React.FC = () => (
  <svg
    className="meme-art meme-truck"
    viewBox="0 0 160 64"
    aria-hidden="true"
    focusable="false"
    fill="none"
    stroke="currentColor"
    strokeWidth={2}
    strokeLinecap="round"
    strokeLinejoin="round"
  >
    <path d="M6 18h40v26H6zM46 28h16l10 10v6H46z" />
    <circle cx="18" cy="48" r="5" />
    <circle cx="60" cy="48" r="5" />
    <path d="M50 30h10l6 7H50z" />
    <path d="M124 32m-20 0a20 20 0 1 0 40 0a15 15 0 1 0-30 0a10 10 0 1 0 20 0a5 5 0 1 0-10 0" />
  </svg>
);

/** A manga anger mark in the error colour, beside a failed download. */
export const MemeVein: React.FC = () => (
  <svg className="meme-art meme-vein" viewBox="0 0 24 24" aria-hidden="true" focusable="false" fill="none" strokeWidth={2.4} strokeLinecap="round">
    <path d="M4 9q5 0 5-5M15 4q0 5 5 5M20 15q-5 0-5 5M9 20q0-5-5-5" />
  </svg>
);

/** Settings › About on High: who the meme images belong to, the parody lines' source, and the series quoted in words only. */
export const MemeCredits: React.FC = () => (
  <div className="meme-credits flex flex-col gap-2 py-3">
    <span className="text-[13px] font-medium text-text-primary">{t('settings.memeCredits')}</span>
    <p className="text-xs text-text-muted">{t('settings.memeCreditsHint')}</p>
    <ul className="meme-credits-list flex flex-col gap-1 text-xs text-text-secondary">
      {CREDITS.map((credit) => (
        <li key={credit.series} lang="en">
          <b className="text-text-primary">{credit.series}</b>
          {' · '}
          {credit.characters}
          <small className="block text-text-muted">{credit.holders}</small>
        </li>
      ))}
    </ul>
    <p className="text-xs text-text-secondary">{t('settings.memeCreditsUbw', { source: UBW_SOURCE })}</p>
    <p className="text-xs text-text-muted">{t('settings.memeCreditsSources', { series: WORDS_ONLY_SERIES.join(', ') })}</p>
  </div>
);
