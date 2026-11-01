/**
 * Meme mode words and pictures for any screen (Settings › Appearance › Meme mode). Low shows the
 * plain words, Medium its own text where a spot has one, High the meme from the lazy High module
 * (`./high`, with the line catalogue and its CSS). Until that chunk is in, High shows the plain
 * words, so Low and Medium never load it. Kept tiny: everything here is in the main chunk.
 */
import React, { useSyncExternalStore } from 'react';
import { useMemeLevel } from '../../hooks/useSettings';
import type { MemeImageName, MemeSpot } from '../../lib/memeCatalogue';
import type * as HighModule from './high';

export type { MemeImageName, MemeSpot };
type High = typeof HighModule;
type Vars = Record<string, string | number>;

let high: High | null = null;
let loading: Promise<High | null> | null = null;
const listeners = new Set<() => void>();

/** Starts loading the High chunk (once); if it fails, the error is logged once and the plain words stay. */
export function loadHigh(): Promise<High | null> {
  loading ??= import('./high').then(
    (module) => {
      high = module;
      listeners.forEach((listener) => listener());
      return module;
    },
    (error: unknown) => {
      console.error('Meme mode: the High module did not load', error);
      return null;
    }
  );
  return loading;
}

const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
};
const current = () => high;

/** The High module while the level is High and its chunk is in, else null. */
export function useHighModule(): High | null {
  const level = useMemeLevel();
  const module = useSyncExternalStore(subscribe, current);
  return level === 'high' ? module : null;
}

/** Words for a spot: Low → plain; Medium → `medium` (else plain); High → the meme with the plain words kept as the spot says. */
export const Meme: React.FC<{ spot: MemeSpot; plain: string; medium?: string; vars?: Vars }> = ({ spot, plain, medium, vars }) => {
  const level = useMemeLevel();
  const module = useHighModule();
  if (module) return <module.MemeLabel spot={spot} plain={plain} vars={vars} />;
  return <>{level === 'medium' && medium !== undefined ? medium : plain}</>;
};

/** The same as text, for `title`, placeholders, option text and toasts. `aria-label`s stay plain. */
export function useMemeText(): (spot: MemeSpot, plain: string, medium?: string, vars?: Vars) => string {
  const level = useMemeLevel();
  const module = useHighModule();
  return (spot, plain, medium, vars) =>
    module ? module.memeText(spot, plain, vars) : level === 'medium' && medium !== undefined ? medium : plain;
}

/** A meme image (decorative: alt="" and aria-hidden); nothing below High or before the chunk is in. */
export const MemeImage: React.FC<{ name: MemeImageName; className?: string }> = ({ name, className }) => {
  const module = useHighModule();
  return module ? <module.MemeImg name={name} className={className} /> : null;
};

/** The spot's own image, with its line as a caption plate when the spot has one; High only. */
export const MemeFrame: React.FC<{ spot: MemeSpot; className?: string }> = ({ spot, className }) => {
  const module = useHighModule();
  return module ? <module.MemePlate spot={spot} className={className} /> : null;
};
