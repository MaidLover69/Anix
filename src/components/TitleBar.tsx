import React, { useEffect, useState } from 'react';
import { Minus, Square, X } from 'lucide-react';
import clsx from 'clsx';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type { Window as TauriWindow } from '@tauri-apps/api/window';
import { useSettings } from '../hooks/useSettings';
import { DEFAULT_DESKTOP_APPEARANCE, useDesktopAppearance } from '../hooks/useDesktopAppearance';
import type { WindowButtonKind, WindowButtonsSetting } from '../types';
import { t, type TKey } from '../i18n';

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
  'flex h-8 w-11 items-center justify-center text-text-muted cursor-pointer transition-colors hover:bg-ink/10 hover:text-text-primary';

const BUTTONS: Record<WindowButtonKind, { action: WindowAction; labelKey: TKey; icon: React.ReactNode; className: string }> = {
  minimize: {
    action: 'minimize',
    labelKey: 'titleBar.minimize',
    icon: <Minus aria-hidden="true" className="h-3.5 w-3.5" />,
    className: CONTROL_CLASS,
  },
  maximize: {
    action: 'toggleMaximize',
    labelKey: 'titleBar.maximize',
    icon: <Square aria-hidden="true" className="h-3 w-3" />,
    className: CONTROL_CLASS,
  },
  close: {
    action: 'close',
    labelKey: 'common.close',
    icon: <X aria-hidden="true" className="h-3.5 w-3.5" />,
    className:
      'flex h-8 w-11 items-center justify-center text-text-muted cursor-pointer transition-colors hover:bg-state-error hover:text-bg-deepest',
  },
};

// Forced sides use the conventional order for that side: close outermost.
const LEFT_ORDER: WindowButtonKind[] = ['close', 'minimize', 'maximize'];
const RIGHT_ORDER: WindowButtonKind[] = ['minimize', 'maximize', 'close'];

/** The buttons for each side: the desktop's own layout, or its buttons moved to one side. */
export function windowButtonSides(
  setting: WindowButtonsSetting,
  layout: { left: WindowButtonKind[]; right: WindowButtonKind[] }
): { left: WindowButtonKind[]; right: WindowButtonKind[] } {
  if (setting === 'desktop') return layout;
  const present = new Set([...layout.left, ...layout.right]);
  // A desktop that shows no buttons at all would leave no way to close a forced side.
  const shown = present.size > 0 ? present : new Set<WindowButtonKind>(RIGHT_ORDER); // i18n-ignore
  const order = setting === 'left' ? LEFT_ORDER : RIGHT_ORDER;
  const buttons = order.filter((b) => shown.has(b));
  return setting === 'left' ? { left: buttons, right: [] } : { left: [], right: buttons };
}

const ButtonGroup: React.FC<{ buttons: WindowButtonKind[] }> = ({ buttons }) =>
  buttons.length === 0 ? null : (
    <div className="flex items-center">
      {buttons.map((kind) => {
        const button = BUTTONS[kind];
        return (
          <button
            key={kind}
            type="button"
            onClick={() => runWindowAction(button.action)}
            aria-label={t(button.labelKey)}
            className={button.className}
          >
            {button.icon}
          </button>
        );
      })}
    </div>
  );

/** Custom title bar; the window is created without native decorations. */
export const TitleBar: React.FC = () => {
  const [isFullscreen, setIsFullscreen] = useState(() => !!document.fullscreenElement);
  const { data: settings } = useSettings();
  const { data: desktop = DEFAULT_DESKTOP_APPEARANCE } = useDesktopAppearance();

  useEffect(() => {
    const handler = () => setIsFullscreen(!!document.fullscreenElement);
    document.addEventListener('fullscreenchange', handler);
    return () => document.removeEventListener('fullscreenchange', handler);
  }, []);

  if (isFullscreen) return null;

  const setting: WindowButtonsSetting =
    settings?.window_buttons === 'left' || settings?.window_buttons === 'right' ? settings.window_buttons : 'desktop';
  const { left, right } = windowButtonSides(setting, desktop.button_layout);
  // The name sits away from the buttons: on the left unless only the left has buttons.
  const nameOnRight = left.length > 0 && right.length === 0;

  return (
    <header
      // Window buttons sit where the desktop puts them, also in right-to-left languages.
      dir="ltr"
      data-tauri-drag-region
      className={clsx(
        'ui-titlebar relative z-[70] flex h-8 w-full flex-shrink-0 items-center justify-between bg-bg-deepest select-none',
        left.length === 0 && 'pl-3',
        right.length === 0 && 'pr-3'
      )}
    >
      <ButtonGroup buttons={left} />
      <span
        className={clsx(
          'pointer-events-none text-xs font-medium text-text-muted',
          nameOnRight ? 'ml-auto' : left.length > 0 ? 'ml-3 mr-auto' : 'mr-auto'
        )}
      >
        Anix
      </span>
      <ButtonGroup buttons={right} />
    </header>
  );
};
