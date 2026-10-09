import React from 'react';
import { CircleAlert, LoaderCircle, Puzzle } from 'lucide-react';
import { Button } from '../ui/Button';

export interface PlayerStatusAction {
  label: string;
  onClick: () => void;
  primary?: boolean;
}

interface PlayerStatusProps {
  /** `setup` is for something the user has to provide first (a streaming plugin), not a failure. */
  kind: 'loading' | 'error' | 'setup';
  title: string;
  /** For errors: the reason as reported by the backend or the media element. */
  detail?: string | null;
  actions?: PlayerStatusAction[];
  /** Dims a video frame that is still visible underneath. */
  scrim?: boolean;
}

/** Centred loading or error state of the player; it always says what is going on. */
export const PlayerStatus: React.FC<PlayerStatusProps> = ({
  kind,
  title,
  detail,
  actions = [],
  scrim = false,
}) => (
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
    <p className={kind === 'loading' ? 'text-[13px] font-medium text-white/80' : 'text-base font-semibold text-white'}>
      {title}
    </p>
    {detail && (
      <p className="max-h-32 max-w-xl overflow-y-auto break-words text-[13px] leading-5 text-text-secondary select-text">
        {detail}
      </p>
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
