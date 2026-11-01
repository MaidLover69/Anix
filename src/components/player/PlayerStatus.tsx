import React from 'react';
import { CircleAlert, LoaderCircle, Puzzle } from 'lucide-react';
import { Button } from '../ui/Button';
import { t } from '../../i18n';
import { useFunMode } from '../../hooks/useSettings';

export interface PlayerStatusAction {
  label: string;
  onClick: () => void;
  primary?: boolean;
}

/** One extension of a source lookup: still being asked, or it had nothing for this episode. */
export interface ProviderChip {
  label: string;
  state: 'asking' | 'failed';
}

/** At most this many chips show; the rest are counted. */
const MAX_CHIPS = 10;

interface PlayerStatusProps {
  /** `setup` is for something the user has to provide first (a streaming plugin), not a failure. */
  kind: 'loading' | 'error' | 'setup';
  title: string;
  /** For errors: the reason as reported by the backend or the media element. */
  detail?: string | null;
  /** Technical text behind a "Details" disclosure, under `detail`. */
  moreDetail?: string | null;
  actions?: PlayerStatusAction[];
  /** Short live lines under a loading state (which extensions are still being asked). */
  progress?: string[];
  /** One chip per extension asked; `progress` says the same in words for screen readers. */
  chips?: ProviderChip[];
  /** Dims a video frame that is still visible underneath. */
  scrim?: boolean;
  /** Fun mode: the last episode ended and auto-play moved on, so this loading screen is the end card. */
  cleared?: boolean;
}

/** Centred loading or error state of the player; it always says what is going on. */
export const PlayerStatus: React.FC<PlayerStatusProps> = ({
  kind,
  title,
  detail,
  moreDetail,
  actions = [],
  progress = [],
  chips = [],
  scrim = false,
  cleared = false,
}) => {
  // Fun mode: a "charging" bar while loading, and the extensions asked shown as the party.
  const fun = useFunMode();
  return (
    <div
      role={kind === 'error' ? 'alert' : 'status'}
      className={`absolute inset-0 z-10 flex flex-col items-center justify-center gap-3 px-6 py-20 text-center ${
        scrim ? 'bg-black/75' : ''
      }`}
    >
      {kind === 'loading' ? (
        <LoaderCircle aria-hidden="true" className="h-8 w-8 animate-spin text-white/80" />
      ) : kind === 'setup' ? (
        <div className="flex h-12 w-12 items-center justify-center rounded-full bg-white/10 text-white/80">
          <Puzzle aria-hidden="true" className="h-6 w-6" />
        </div>
      ) : (
        // Same mark as the app's other error states.
        <div className="flex h-12 w-12 items-center justify-center rounded-full bg-state-error/15 text-state-error">
          <CircleAlert aria-hidden="true" className="h-6 w-6" />
        </div>
      )}
      {fun && cleared && kind === 'loading' && (
        <p aria-hidden="true" dir="auto" className="fun-clear self-center text-lg">
          {t('fun.episodeClear')} <span className="fun-clear-exp">{t('fun.plusExp')}</span>
        </p>
      )}
      <p className={kind === 'loading' ? 'text-[13px] font-medium text-white/80' : 'text-base font-semibold text-white'}>
        {title}
      </p>
      {fun && cleared && kind === 'loading' && (
        <p aria-hidden="true" dir="auto" className="fun-continued">
          {t('fun.toBeContinued')}
        </p>
      )}
      {fun && kind === 'loading' && (
        <span aria-hidden="true" className="fun-charge block h-2 w-56 max-w-full">
          <span className="block h-full" />
        </span>
      )}
      {detail && (
        <p className="max-h-32 max-w-xl overflow-y-auto break-words text-[13px] leading-5 text-text-secondary select-text">
          {detail}
        </p>
      )}
      {fun && chips.length > 0 && (
        <p aria-hidden="true" className="text-[11px] font-extrabold uppercase tracking-widest text-white/60">
          {t('fun.partyMembers')}
        </p>
      )}
      {chips.length > 0 && (
        <ul aria-hidden="true" className="fun-party flex max-w-xl flex-wrap justify-center gap-1.5">
          {chips.slice(0, MAX_CHIPS).map((chip, i) => (
            <li
              // Two extensions can share a name.
              key={`${i}-${chip.label}`}
              dir="auto"
              className={`inline-flex items-center gap-1.5 rounded-full border border-white/15 bg-white/10 px-2.5 py-1 text-[12px] font-semibold text-white ${
                chip.state === 'asking' ? 'player-asking' : 'opacity-60 line-through'
              }`}
            >
              {chip.label}
            </li>
          ))}
          {chips.length > MAX_CHIPS && (
            <li className="rounded-full border border-white/15 bg-white/10 px-2.5 py-1 text-[12px] font-semibold tabular-nums text-white">
              {`+${chips.length - MAX_CHIPS}`}
            </li>
          )}
        </ul>
      )}
      {progress.length > 0 && (
        <div aria-live="polite" className="max-w-xl text-[12px] leading-5 text-white/60">
          {progress.map((line) => (
            <p key={line} className="break-words">
              {line}
            </p>
          ))}
        </div>
      )}
      {moreDetail && (
        <details className="max-w-xl text-[12px] text-text-muted">
          <summary className="cursor-pointer select-none rounded-sm hover:text-text-secondary">
            {t('common.details')}
          </summary>
          <p className="mt-1 max-h-32 overflow-y-auto whitespace-pre-wrap break-words text-left leading-5 select-text">
            {moreDetail}
          </p>
        </details>
      )}
      {actions.length > 0 && (
        <div className="mt-1 flex flex-wrap items-center justify-center gap-2">
          {actions.map((action) => (
            <Button
              key={action.label}
              variant={action.primary ? 'primary' : 'secondary'}
              onClick={action.onClick}
            >
              {action.label}
            </Button>
          ))}
        </div>
      )}
    </div>
  );
};
