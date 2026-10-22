import React from 'react';

export type MascotMood = 'sleepy' | 'confused' | 'dizzy';

/** Only the face changes per mood; the head, ears and body are shared. */
const FACES: Record<MascotMood, React.ReactNode> = {
  sleepy: (
    <>
      <path d="M21 34q4-4 8 0M35 34q4-4 8 0" />
      <path d="M30 42q2 2 4 0" />
      <polyline points="52,4 58,4 52,10 58,10" />
      <polyline points="58,14 62,14 58,18 62,18" />
    </>
  ),
  confused: (
    <>
      <circle cx="25" cy="33" r="1.5" />
      <path d="M35 34q4-4 8 0" />
      <path d="M27 43q2.5-2 5 0t5 0" />
      <path d="M51 8a4 4 0 1 1 5 4c-1.5 1-2 2-2 4M54 21v.01" />
    </>
  ),
  dizzy: (
    <>
      <path d="M24 33a1.5 1.5 0 0 1 3 0a3 3 0 0 1-6 0a4.5 4.5 0 0 1 9 0" />
      <path d="M38 33a1.5 1.5 0 0 1 3 0a3 3 0 0 1-6 0a4.5 4.5 0 0 1 9 0" />
      <path d="M26 44q1.5-2 3 0t3 0t3 0t3 0" />
    </>
  ),
};

/**
 * The line-art mascot of empty and error screens. One stroke in currentColor and no fill, so
 * it is ink in Manga and Comic and takes the text colour everywhere else.
 */
export const Mascot: React.FC<{ mood: MascotMood; className?: string }> = ({ mood, className }) => (
  <svg
    viewBox="0 0 64 64"
    width={64}
    height={64}
    aria-hidden="true"
    focusable="false"
    fill="none"
    stroke="currentColor"
    strokeWidth={2}
    strokeLinecap="round"
    strokeLinejoin="round"
    className={className}
  >
    <circle cx="32" cy="34" r="20" />
    <path d="M16.5 21.5L15 8L26 15M47.5 21.5L49 8L38 15" />
    <path d="M24 59q8 4 16 0" />
    {FACES[mood]}
  </svg>
);
