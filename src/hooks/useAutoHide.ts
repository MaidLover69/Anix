import { useCallback, useEffect, useRef, useState } from 'react';

/**
 * Visibility of chrome that hides after `delayMs` without activity.
 * It only hides while `canHide` is true (playing, nothing open); `poke` counts as activity.
 */
export function useAutoHide(canHide: boolean, delayMs: number = 3000) {
  const [visible, setVisible] = useState(true);
  const timerRef = useRef<number | null>(null);
  const canHideRef = useRef(canHide);
  canHideRef.current = canHide;

  const clear = useCallback(() => {
    if (timerRef.current !== null) {
      window.clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  }, []);

  const poke = useCallback(() => {
    clear();
    setVisible(true);
    if (canHideRef.current) {
      timerRef.current = window.setTimeout(() => {
        timerRef.current = null;
        if (canHideRef.current) setVisible(false);
      }, delayMs);
    }
  }, [clear, delayMs]);

  // Start the countdown whenever hiding becomes possible (for example after a keyboard play),
  // and show the chrome again as soon as it is not.
  useEffect(() => {
    poke();
    return clear;
  }, [canHide, poke, clear]);

  return { visible: visible || !canHide, poke };
}
