import { useEffect, useState } from 'react';

const isActive = () => document.visibilityState === 'visible' && document.hasFocus();

/** True while the window is shown and focused; motion nobody watches can stop meanwhile. */
export function useWindowActive(): boolean {
  const [active, setActive] = useState(isActive);
  useEffect(() => {
    const update = () => setActive(isActive());
    update();
    document.addEventListener('visibilitychange', update);
    window.addEventListener('focus', update);
    window.addEventListener('blur', update);
    return () => {
      document.removeEventListener('visibilitychange', update);
      window.removeEventListener('focus', update);
      window.removeEventListener('blur', update);
    };
  }, []);
  return active;
}
