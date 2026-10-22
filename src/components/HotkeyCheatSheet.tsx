import React from 'react';
import { Modal } from './ui/Modal';
import { t, type TKey } from '../i18n';

interface HotkeyCheatSheetProps {
  isOpen: boolean;
  onClose: () => void;
}

interface Shortcut {
  /** Alternatives; each one is a key or a combination of keys pressed together. */
  keys: string[][];
  description: TKey;
}

interface ShortcutGroup {
  title: TKey;
  shortcuts: Shortcut[];
}

const HOLD_SPACE = 'Hold Space';

// Keep in step with `usePlayerShortcuts` (player) and the global handler in `router.tsx` (app).
const SHORTCUT_GROUPS: ShortcutGroup[] = [
  {
    title: 'hotkeyCheatSheet.player',
    shortcuts: [
      { keys: [['Space'], ['K']], description: 'hotkeyCheatSheet.playPause' },
      // Key names stay as they are; only the word "Hold" is interface text.
      { keys: [[HOLD_SPACE]], description: 'hotkeyCheatSheet.holdFast' },
      { keys: [['['], [']']], description: 'hotkeyCheatSheet.speedStep' },
      { keys: [['←'], ['→']], description: 'hotkeyCheatSheet.seek5' },
      { keys: [['Shift', '←'], ['Shift', '→']], description: 'hotkeyCheatSheet.seek30' },
      { keys: [['J'], ['L']], description: 'hotkeyCheatSheet.seek10' },
      { keys: [['↑'], ['↓']], description: 'hotkeyCheatSheet.volume' },
      { keys: [['M']], description: 'hotkeyCheatSheet.mute' },
      { keys: [['F']], description: 'hotkeyCheatSheet.fullscreen' },
      { keys: [['I']], description: 'hotkeyCheatSheet.miniPlayer' },
      { keys: [['S']], description: 'hotkeyCheatSheet.skip' },
      { keys: [['Enter']], description: 'hotkeyCheatSheet.skipNow' },
      { keys: [['Esc']], description: 'hotkeyCheatSheet.cancelSkip' },
      { keys: [['Shift', 'N']], description: 'hotkeyCheatSheet.nextEpisode' },
      { keys: [['Shift', 'P']], description: 'hotkeyCheatSheet.previousEpisode' },
    ],
  },
  {
    title: 'hotkeyCheatSheet.app',
    shortcuts: [
      { keys: [['Ctrl', 'K']], description: 'hotkeyCheatSheet.search' },
      { keys: [['?']], description: 'hotkeyCheatSheet.toggleList' },
      { keys: [['Esc']], description: 'hotkeyCheatSheet.closeDialog' },
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
    <Modal isOpen={isOpen} onClose={onClose} title={t('hotkeyCheatSheet.title')}>
      <div className="flex max-h-[65vh] flex-col gap-5 overflow-y-auto pe-1">
        {SHORTCUT_GROUPS.map((group) => (
          <section key={group.title} className="flex flex-col gap-1">
            <h3 className="text-[11px] font-semibold uppercase tracking-wider text-text-muted">
              {t(group.title)}
            </h3>
            <dl className="flex flex-col">
              {group.shortcuts.map((shortcut) => (
                <div
                  key={shortcut.description}
                  className="flex items-center justify-between gap-4 border-b border-border py-2 last:border-b-0"
                >
                  <dt className="text-[13px] text-text-secondary">{t(shortcut.description)}</dt>
                  <dd className="flex flex-shrink-0 items-center gap-1.5">
                    {shortcut.keys.map((combo, index) => (
                      <React.Fragment key={combo.join('+')}>
                        {index > 0 && <span className="text-xs text-text-muted">{t('hotkeyCheatSheet.or')}</span>}
                        <span className="flex items-center gap-0.5">
                          {combo.map((key, keyIndex) => (
                            <React.Fragment key={key}>
                              {keyIndex > 0 && <span className="text-xs text-text-muted">+</span>}
                              <Key>{key === HOLD_SPACE ? t('hotkeyCheatSheet.holdSpace') : key}</Key>
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
