/** Motion checks for JS-driven movement, which CSS's reduced-motion rules cannot reach. */

/** True under the OS "reduce motion" setting or Settings › Appearance › Fewer effects. */
export function prefersLessMotion(): boolean {
  return (
    window.matchMedia('(prefers-reduced-motion: reduce)').matches ||
    document.documentElement.getAttribute('data-effects') === 'fewer'
  );
}

/** The `behavior` for scrollTo/scrollBy calls. */
export function scrollBehavior(): ScrollBehavior {
  return prefersLessMotion() ? 'auto' : 'smooth';
}
