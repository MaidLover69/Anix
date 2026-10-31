import { useEffect, useState } from 'react';
import { useSettings } from './useSettings';

/** Tracks a CSS media query, e.g. `(max-width: 1099px)`. */
export function useMediaQuery(query: string): boolean {
  const [matches, setMatches] = useState(() => window.matchMedia(query).matches);

  useEffect(() => {
    const list = window.matchMedia(query);
    const update = () => setMatches(list.matches);
    update();
    list.addEventListener('change', update);
    return () => list.removeEventListener('change', update);
  }, [query]);

  return matches;
}

/** The OS "reduce motion" setting, or Settings › Appearance › Fewer effects. */
export function useReducedMotion(): boolean {
  const system = useMediaQuery('(prefers-reduced-motion: reduce)');
  return useSettings().data?.fewer_effects === true || system;
}
