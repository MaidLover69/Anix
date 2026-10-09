import { useCallback, useEffect, useRef, useState } from 'react';

/**
 * Renders a long list in steps: the first `step` items at once, then another `step` each time
 * the sentinel element scrolls near the viewport. `resetKey` restarts from the first step.
 */
export function useProgressiveList<T>(items: T[], step: number = 60, resetKey?: unknown) {
  const [count, setCount] = useState(step);
  const observerRef = useRef<IntersectionObserver | null>(null);

  useEffect(() => {
    setCount(step);
  }, [resetKey, step]);

  const total = items.length;
  const hasMore = count < total;

  const sentinelRef = useCallback(
    (node: HTMLElement | null) => {
      observerRef.current?.disconnect();
      observerRef.current = null;
      if (!node) return;
      if (!('IntersectionObserver' in window)) {
        setCount(total);
        return;
      }
      const observer = new IntersectionObserver(
        (entries) => {
          if (entries.some((entry) => entry.isIntersecting)) {
            setCount((current) => Math.min(total, current + step));
          }
        },
        { rootMargin: '600px' }
      );
      observer.observe(node);
      observerRef.current = observer;
    },
    // `count` is a dependency so the sentinel is re-observed after each step; an observer only
    // reports changes, and a sentinel that is still in range would otherwise never fire again.
    [step, total, count]
  );

  useEffect(() => () => observerRef.current?.disconnect(), []);

  return {
    visible: hasMore ? items.slice(0, count) : items,
    hasMore,
    showMore: () => setCount((current) => Math.min(total, current + step)),
    sentinelRef,
  };
}
