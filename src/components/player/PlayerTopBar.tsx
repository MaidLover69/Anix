import React from 'react';
import { ArrowLeft } from 'lucide-react';
import { IconButton } from './IconButton';

interface PlayerTopBarProps {
  title: string;
  /** One muted line: episode, audio, provider. */
  meta?: string;
  onBack: () => void;
  visible?: boolean;
  /** Ghost icon buttons on the right. */
  children?: React.ReactNode;
}

export const PlayerTopBar: React.FC<PlayerTopBarProps> = ({
  title,
  meta,
  onBack,
  visible = true,
  children,
}) => (
  <div
    className={`pointer-events-none absolute inset-x-0 top-0 z-20 bg-gradient-to-b from-black/85 via-black/50 to-transparent pb-12 transition-opacity duration-200 ${
      visible ? 'opacity-100' : 'opacity-0'
    }`}
  >
    <div
      className={`flex items-center gap-2 px-4 pt-3 ${visible ? 'pointer-events-auto' : ''}`}
      onMouseDown={(e) => e.preventDefault()}
    >
      <IconButton label="Back" onClick={onBack}>
        <ArrowLeft aria-hidden="true" className="h-5 w-5" />
      </IconButton>
      <div className="min-w-0 flex-1 [text-shadow:0_1px_2px_rgb(0_0_0/0.6)]">
        <h1 className="truncate text-sm font-semibold text-white">{title}</h1>
        {meta && <p className="truncate text-xs text-white/80">{meta}</p>}
      </div>
      {children && <div className="flex flex-shrink-0 items-center gap-1">{children}</div>}
    </div>
  </div>
);
