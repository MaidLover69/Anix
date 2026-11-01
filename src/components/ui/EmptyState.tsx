import React from 'react';
import clsx from 'clsx';
import { useMemeLevel, usePlayful } from '../../hooks/useSettings';
import { Mascot, type MascotMood } from './Mascot';
import { Meme, MemeFrame, MemeImage, useHighModule, type MemeSpot } from '../meme/Meme';

interface EmptyStateProps {
  title: string;
  description?: React.ReactNode;
  icon?: React.ReactNode;
  /** Usually a `Button`. */
  action?: React.ReactNode;
  /** With "Playful touches" on, the mascot in this mood replaces the icon. */
  mood?: MascotMood;
  /** ASCII only (the Hacker font has no other glyphs); shown with "Playful touches" on. */
  kaomoji?: string;
  /** Meme mode › High: the spot's image replaces the picture and its meme dresses the title. Without it, Zenitsu replaces the sleepy mascot. */
  meme?: MemeSpot;
  /** The spot's line is a caption plate on its image, so the title stays plain. */
  memePlate?: boolean;
  className?: string;
}

export const EmptyState: React.FC<EmptyStateProps> = ({
  title,
  description,
  icon,
  action,
  mood,
  kaomoji,
  meme,
  memePlate = false,
  className,
}) => {
  const playful = usePlayful();
  const level = useMemeLevel();
  const high = useHighModule();
  const iconTile = icon && (
    // The tile sizes the icon, so every empty state has the same mark whatever the caller passes.
    <div className="mb-4 flex h-12 w-12 items-center justify-center rounded-full bg-ink/[0.06] text-text-secondary [&>svg]:h-6 [&>svg]:w-6">
      {icon}
    </div>
  );
  // High shows no mascot and one character per card: the spot's own, else Zenitsu for the sleepy cat.
  const picture = high ? (
    meme && high.spotImage(meme) ? (
      <MemeFrame spot={meme} className="meme-card-img" />
    ) : !meme && mood === 'sleepy' && playful ? (
      <MemeImage name="zenitsu-sleep" className="meme-card-img" />
    ) : (
      iconTile
    )
  ) : mood && playful && level !== 'high' ? (
    <Mascot mood={mood} className="mb-3 text-text-secondary" />
  ) : (
    iconTile
  );
  return (
    // One dashed card for every empty screen: mascot or icon, title, one line, one button.
    <div
      className={clsx(
        'ui-empty flex flex-col items-center justify-center rounded-2xl border-2 border-dashed border-border-strong bg-bg-card/50 px-6 py-12 text-center',
        className
      )}
    >
      {picture}
      <h3 className="text-base font-semibold text-text-primary">
        {meme && !memePlate ? <Meme spot={meme} plain={title} /> : title}
      </h3>
      {description && (
        <p className="mt-1 max-w-sm text-[13px] leading-5 text-text-muted select-text">{description}</p>
      )}
      {kaomoji && playful && level !== 'high' && (
        <span aria-hidden="true" className="mt-2 font-mono text-[13px] text-text-muted">
          {kaomoji}
        </span>
      )}
      {action && <div className="mt-4">{action}</div>}
    </div>
  );
};
