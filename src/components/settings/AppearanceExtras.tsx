import React from 'react';
import { Row } from './SettingsLayout';
import { Switch } from '../ui/Switch';
import { SegmentedControl } from '../ui/SegmentedControl';
import { useDesktopAppearance } from '../../hooks/useDesktopAppearance';
import type { AppSettings, WindowButtonsSetting } from '../../types';

const SOURCE_LABEL: Record<string, string> = {
  portal: 'the desktop portal',
  gnome: 'GNOME',
  kde: 'KDE Plasma',
};

/** The Appearance rows after the theme picker: system accent and window-button side. */
export const AppearanceExtras: React.FC<{
  settings: AppSettings;
  onChange: <K extends keyof AppSettings>(key: K, value: AppSettings[K]) => void;
}> = ({ settings, onChange }) => {
  const { data: desktop } = useDesktopAppearance();
  const accent = desktop?.accent_color ?? null;
  const layoutSource = desktop?.button_layout_source ?? 'default';

  const accentHint = accent
    ? `Uses ${accent} from ${SOURCE_LABEL[desktop?.accent_source ?? ''] ?? 'the desktop'}, adjusted to stay readable. Manga stays black and white.`
    : 'Your desktop does not report an accent colour, so the theme’s own is used.';

  const buttonsHint =
    layoutSource === 'default'
      ? 'The desktop’s layout could not be read; “Follow desktop” puts them on the right.'
      : `Follow desktop uses the ${layoutSource === 'kde' ? 'KDE Plasma' : 'GNOME'} button layout.`;

  return (
    <>
      <Row label="Use system accent colour" hint={accentHint}>
        {({ labelId, hintId }) => (
          <span className="flex items-center gap-3">
            {accent && (
              <span
                aria-hidden="true"
                className="h-4 w-4 rounded-full border border-border-strong"
                style={{ backgroundColor: accent }}
              />
            )}
            <Switch
              aria-labelledby={labelId}
              aria-describedby={hintId}
              checked={settings.use_system_accent}
              onChange={(checked) => onChange('use_system_accent', checked)}
            />
          </span>
        )}
      </Row>
      <Row label="Window buttons" hint={buttonsHint}>
        {() => (
          <SegmentedControl<WindowButtonsSetting>
            aria-label="Window buttons"
            value={settings.window_buttons ?? 'desktop'}
            onChange={(value) => onChange('window_buttons', value)}
            options={[
              { value: 'desktop', label: 'Follow desktop' },
              { value: 'left', label: 'Left' },
              { value: 'right', label: 'Right' },
            ]}
          />
        )}
      </Row>
    </>
  );
};
