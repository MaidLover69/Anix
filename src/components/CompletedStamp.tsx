import React from 'react';
import { Check } from 'lucide-react';
import { t } from '../i18n';

/**
 * The mark after the title of an anime watched to the end. All three art variants render;
 * themes.css shows one: a ring by default, 完 in Manga, a "THE END!" tag in Comic.
 */
export const CompletedStamp: React.FC = () => (
  <span className="ui-stamp ms-3 inline-flex select-none align-middle">
    <span aria-hidden="true" className="contents">
      <span data-art="ring" className="items-center gap-1.5">
        <span className="flex h-7 w-7 items-center justify-center rounded-full border-2 border-accent-500">
          <Check className="h-4 w-4 text-accent-500" />
        </span>
        <span className="text-xs font-semibold tracking-normal text-accent-300">{t('completedStamp.completed')}</span>
      </span>
      <span data-art="kan">
        {/* 完, drawn as strokes: the roof, then 二 and the two legs of 儿. */}
        <svg
          viewBox="0 0 24 24"
          width={22}
          height={22}
          focusable="false"
          fill="none"
          stroke="currentColor"
          strokeWidth={2}
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <path d="M12 2v2.5M4 8V5.5h16V8M8 10.5h8M4 14h16M10 14c0 4-2 7-6 8.5M14 14v6.5c0 1.5 1 2 2.5 2h2c1 0 1.5-.5 1.5-2.5" />
        </svg>
      </span>
      <span data-art="end">{t('completedStamp.theEnd')}</span>
    </span>
    <span className="sr-only">{t('completedStamp.completed')}</span>
  </span>
);
