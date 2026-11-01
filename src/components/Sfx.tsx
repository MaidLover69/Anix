/** Big outlined manga sound-effect text (Meme mode › Medium and up), decoration only. */
import React from 'react';
import clsx from 'clsx';

export const Sfx: React.FC<{ text: string; className?: string }> = ({ text, className }) => (
  <span aria-hidden="true" className={clsx('fun-sfx', className)}>
    {text}
  </span>
);
