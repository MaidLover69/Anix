import React, { useEffect, useState } from 'react';
import { Minus, Square, X } from 'lucide-react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type { Window as TauriWindow } from '@tauri-apps/api/window';

type WindowAction = 'minimize' | 'toggleMaximize' | 'close';

async function runWindowAction(action: WindowAction) {
  try {
    const appWindow: TauriWindow = getCurrentWindow();
    await appWindow[action]();
  } catch (err) {
    console.error(`[TitleBar] window.${action}() failed:`, err);
  }
}

const CONTROL_CLASS =
  'flex h-8 w-11 items-center justify-center text-text-muted cursor-pointer transition-colors hover:bg-white/10 hover:text-text-primary';

/** Custom title bar; the window is created without native decorations. */
export const TitleBar: React.FC = () => {
  const [isFullscreen, setIsFullscreen] = useState(() => !!document.fullscreenElement);

  useEffect(() => {
    const handler = () => setIsFullscreen(!!document.fullscreenElement);
    document.addEventListener('fullscreenchange', handler);
    return () => document.removeEventListener('fullscreenchange', handler);
  }, []);

  if (isFullscreen) return null;

  return (
    <header
      data-tauri-drag-region
      className="relative z-[70] flex h-8 w-full flex-shrink-0 items-center justify-between bg-bg-deepest pl-3 select-none"
    >
      <span className="pointer-events-none text-xs font-medium text-text-muted">Anilili</span>

      <div className="flex items-center">
        <button
          type="button"
          onClick={() => runWindowAction('minimize')}
          aria-label="Minimize"
          className={CONTROL_CLASS}
        >
          <Minus aria-hidden="true" className="h-3.5 w-3.5" />
        </button>
        <button
          type="button"
          onClick={() => runWindowAction('toggleMaximize')}
          aria-label="Maximize or restore"
          className={CONTROL_CLASS}
        >
          <Square aria-hidden="true" className="h-3 w-3" />
        </button>
        <button
          type="button"
          onClick={() => runWindowAction('close')}
          aria-label="Close"
          className="flex h-8 w-11 items-center justify-center text-text-muted cursor-pointer transition-colors hover:bg-state-error hover:text-bg-deepest"
        >
          <X aria-hidden="true" className="h-3.5 w-3.5" />
        </button>
      </div>
    </header>
  );
};
