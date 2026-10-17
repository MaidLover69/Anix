import React, { useRef } from 'react';
import clsx from 'clsx';

/** `#rrggbb` (AniList `coverImage.color`) as the RGB channels the theme tokens use. */
export function hexToChannels(hex: string | null | undefined): string | null {
  const match = hex?.trim().match(/^#?([0-9a-f]{6})$/i);
  if (!match) return null;
  const value = parseInt(match[1], 16);
  return `${(value >> 16) & 255} ${(value >> 8) & 255} ${value & 255}`;
}

interface AmbientGlowProps {
  /** The cover colour; none fades the glow out. */
  color: string | null | undefined;
  /** Position and size (absolute utilities). */
  className?: string;
  /** Multiplies the theme's --ambient-strength. */
  intensity?: number;
}

/**
 * A soft radial glow in a title's cover colour. A colour change cross-fades (the theme CSS
 * transitions it, except under reduced motion); without a colour the last one fades out.
 */
export const AmbientGlow: React.FC<AmbientGlowProps> = ({ color, className, intensity = 1 }) => {
  const channels = hexToChannels(color);
  const last = useRef(channels);
  if (channels) last.current = channels;
  return (
    <div
      aria-hidden="true"
      className={clsx('ui-ambient absolute', className)}
      style={{ '--ambient-rgb': last.current ?? '0 0 0', opacity: channels ? intensity : 0 } as React.CSSProperties}
    />
  );
};
