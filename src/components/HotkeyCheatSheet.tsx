import React from 'react';
import { Modal } from './ui/Modal';

interface HotkeyCheatSheetProps {
  isOpen: boolean;
  onClose: () => void;
}

interface Shortcut {
  /** Alternatives; each one is a key or a combination of keys pressed together. */
  keys: string[][];
  description: string;
}

interface ShortcutGroup {
  title: string;
  shortcuts: Shortcut[];
}

// Keep in step with `usePlayerShortcuts` (player) and the global handler in `router.tsx` (app).
const SHORTCUT_GROUPS: ShortcutGroup[] = [
  {
    title: 'Player',
    shortcuts: [
      { keys: [['Space'], ['K']], description: 'Play or pause' },
      { keys: [['←'], ['→']], description: 'Seek 5 seconds back or forward' },
      { keys: [['Shift', '←'], ['Shift', '→']], description: 'Seek 30 seconds back or forward' },
      { keys: [['J'], ['L']], description: 'Seek 10 seconds back or forward' },
      { keys: [['↑'], ['↓']], description: 'Volume up or down' },
      { keys: [['M']], description: 'Mute or unmute' },
      { keys: [['F']], description: 'Enter or leave fullscreen' },
      { keys: [['I']], description: 'Mini-player: a small window that stays on top' },
      { keys: [['S']], description: 'Skip the intro or ending while its button is shown' },
      { keys: [['Shift', 'N']], description: 'Next episode' },
      { keys: [['Shift', 'P']], description: 'Previous episode' },
    ],
  },
  {
    title: 'App',
    shortcuts: [
      { keys: [['Ctrl', 'K']], description: 'Open search and commands' },
      { keys: [['?']], description: 'Show or hide this list' },
      { keys: [['Esc']], description: 'Close a dialog or leave fullscreen' },
    ],
  },
];

const Key: React.FC<{ children: React.ReactNode }> = ({ children }) => (
  <kbd className="inline-flex h-6 min-w-6 items-center justify-center rounded-sm border border-border-strong bg-bg-input px-1.5 font-sans text-[11px] font-medium text-text-primary">
    {children}
  </kbd>
);

export const HotkeyCheatSheet: React.FC<HotkeyCheatSheetProps> = ({ isOpen, onClose }) => {
  return (
    <Modal isOpen={isOpen} onClose={onClose} title="Keyboard shortcuts">
      <div className="flex max-h-[65vh] flex-col gap-5 overflow-y-auto pr-1">
        {SHORTCUT_GROUPS.map((group) => (
          <section key={group.title} className="flex flex-col gap-1">
            <h3 className="text-[11px] font-semibold uppercase tracking-wider text-text-muted">
              {group.title}
            </h3>
            <dl className="flex flex-col">
              {group.shortcuts.map((shortcut) => (
                <div
                  key={shortcut.description}
                  className="flex items-center justify-between gap-4 border-b border-border py-2 last:border-b-0"
                >
                  <dt className="text-[13px] text-text-secondary">{shortcut.description}</dt>
                  <dd className="flex flex-shrink-0 items-center gap-1.5">
                    {shortcut.keys.map((combo, index) => (
                      <React.Fragment key={combo.join('+')}>
                        {index > 0 && <span className="text-xs text-text-muted">or</span>}
                        <span className="flex items-center gap-0.5">
                          {combo.map((key, keyIndex) => (
                            <React.Fragment key={key}>
                              {keyIndex > 0 && <span className="text-xs text-text-muted">+</span>}
                              <Key>{key}</Key>
                            </React.Fragment>
                          ))}
                        </span>
                      </React.Fragment>
                    ))}
                  </dd>
                </div>
              ))}
            </dl>
          </section>
        ))}
      </div>
    </Modal>
  );
};

export default HotkeyCheatSheet;
