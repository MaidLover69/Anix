import React, { useRef, useState } from 'react';
import clsx from 'clsx';
import { Row } from './SettingsLayout';
import { Switch } from '../ui/Switch';
import { SegmentedControl } from '../ui/SegmentedControl';
import { Button } from '../ui/Button';
import { ShimmerImage } from '../ShimmerImage';
import { useDesktopAppearance } from '../../hooks/useDesktopAppearance';
import { parseHexColor } from '../../theme/accent';
import { t } from '../../i18n';
import type { AppSettings, ContrastLevel, TimeFormat, UiSize, WindowButtonsSetting } from '../../types';

/** Accent swatches; each theme derives readable shades from the one picked (theme/accent.ts). */
const ACCENT_SWATCHES = [
  { hex: '#8979f2', nameKey: 'appearanceExtras.violet' },
  { hex: '#3b82f6', nameKey: 'appearanceExtras.blue' },
  { hex: '#06b6d4', nameKey: 'appearanceExtras.cyan' },
  { hex: '#14b8a6', nameKey: 'appearanceExtras.teal' },
  { hex: '#22c55e', nameKey: 'appearanceExtras.green' },
  { hex: '#84cc16', nameKey: 'appearanceExtras.lime' },
  { hex: '#f59e0b', nameKey: 'appearanceExtras.amber' },
  { hex: '#f97316', nameKey: 'appearanceExtras.orange' },
  { hex: '#ef4444', nameKey: 'appearanceExtras.red' },
  { hex: '#ec4899', nameKey: 'appearanceExtras.pink' },
] as const;

const SWATCH_CLASS =
  'block h-7 w-7 rounded-full border border-border-strong ring-offset-2 ring-offset-bg-card transition-shadow peer-checked:ring-2 peer-checked:ring-text-primary peer-focus-visible:ring-2 peer-focus-visible:ring-accent-500 peer-disabled:opacity-40';

/** Theme colour, ten swatches and a custom colour, as one radio group. */
const AccentPicker: React.FC<{
  value: string | null;
  disabled: boolean;
  labelId: string;
  hintId?: string;
  onChange: (hex: string | null) => void;
}> = ({ value, disabled, labelId, hintId, onChange }) => {
  const current = parseHexColor(value) ? value!.toLowerCase() : null;
  const isSwatch = ACCENT_SWATCHES.some((s) => s.hex === current);
  const isCustom = current !== null && !isSwatch;
  const [custom, setCustom] = useState<string | null>(null);
  const shown = custom ?? (isCustom ? current : '#8979f2');
  // The colour input fires while it is dragged; each pick is saved once it settles.
  const timer = useRef<number>();
  const pick = (hex: string) => {
    setCustom(hex);
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => onChange(hex), 300);
  };
  const option = (key: string, checked: boolean, select: () => void, label: string, swatch: React.ReactNode) => (
    <label key={key} title={label} className={clsx('relative', disabled ? 'cursor-not-allowed' : 'cursor-pointer')}>
      <input
        type="radio"
        name="accent-colour"
        className="peer sr-only"
        checked={checked}
        disabled={disabled}
        onChange={select}
        aria-label={label}
      />
      {swatch}
    </label>
  );
  return (
    <div role="radiogroup" aria-labelledby={labelId} aria-describedby={hintId} className="flex flex-wrap items-center gap-2">
      {option(
        'theme',
        current === null,
        () => onChange(null),
        t('appearanceExtras.themeColour'),
        <span aria-hidden="true" className={clsx(SWATCH_CLASS, 'bg-accent-500')} />
      )}
      {ACCENT_SWATCHES.map((s) =>
        option(
          s.hex,
          current === s.hex,
          () => onChange(s.hex),
          t(s.nameKey),
          <span aria-hidden="true" className={SWATCH_CLASS} style={{ backgroundColor: s.hex }} />
        )
      )}
      <label
        title={t('appearanceExtras.customColour')}
        className={clsx('relative flex items-center', disabled ? 'cursor-not-allowed' : 'cursor-pointer')}
      >
        {/* The radio gives the group its selected state; choosing it applies the colour shown. */}
        <input
          type="radio"
          name="accent-colour"
          className="peer sr-only"
          checked={isCustom}
          disabled={disabled}
          onChange={() => onChange(shown)}
          aria-label={t('appearanceExtras.customColour')}
        />
        <input
          type="color"
          aria-label={t('appearanceExtras.editCustomColour')}
          disabled={disabled}
          value={shown}
          // Picking the colour already shown fires no change event, so a click selects it.
          onClick={() => !isCustom && onChange(shown)}
          onChange={(e) => pick(e.target.value.toLowerCase())}
          className={clsx(
            'h-7 w-10 cursor-[inherit] rounded-md border border-border-strong bg-transparent p-0.5 ring-offset-2 ring-offset-bg-card peer-focus-visible:ring-2 peer-focus-visible:ring-accent-500 disabled:opacity-40',
            isCustom && 'ring-2 ring-text-primary'
          )}
        />
      </label>
    </div>
  );
};

/** Desktops named in the accent hint; the portal and an unknown source get their own sentences. */
const SOURCE_LABEL: Record<string, string> = {
  gnome: 'GNOME',
  kde: 'KDE Plasma',
};

/**
 * The Appearance rows after the theme picker: accent (system or picked), contrast, interface
 * size, background art, time format, episode spoiler blur, window-button side and playful touches.
 */
export const AppearanceExtras: React.FC<{
  settings: AppSettings;
  onChange: <K extends keyof AppSettings>(key: K, value: AppSettings[K]) => void;
}> = ({ settings, onChange }) => {
  const { data: desktop } = useDesktopAppearance();
  const accent = desktop?.accent_color ?? null;
  const layoutSource = desktop?.button_layout_source ?? 'default';

  const accentSource = desktop?.accent_source ?? '';
  const accentHint = !accent
    ? t('appearanceExtras.noAccentHint')
    : accentSource === 'portal'
      ? t('appearanceExtras.accentHintPortal', { color: accent })
      : SOURCE_LABEL[accentSource]
        ? t('appearanceExtras.accentHintFrom', { color: accent, desktop: SOURCE_LABEL[accentSource] })
        : t('appearanceExtras.accentHintDesktop', { color: accent });

  const buttonsHint =
    layoutSource === 'default'
      ? t('appearanceExtras.buttonsHintDefault')
      : t('appearanceExtras.buttonsHintFollow', { desktop: layoutSource === 'kde' ? 'KDE Plasma' : 'GNOME' });

  return (
    <>
      <Row label={t('appearanceExtras.useSystemAccent')} hint={accentHint}>
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
      <Row
        label={t('appearanceExtras.accentColour')}
        hint={
          settings.use_system_accent
            ? t('appearanceExtras.accentInUseHint')
            : t('appearanceExtras.accentHint')
        }
        keywords={t('appearanceExtras.accentKeywords')}
        stacked
      >
        {({ labelId, hintId }) => (
          <AccentPicker
            value={settings.accent_color ?? null}
            disabled={settings.use_system_accent}
            labelId={labelId}
            hintId={hintId}
            onChange={(hex) => onChange('accent_color', hex)}
          />
        )}
      </Row>
      <Row label={t('appearanceExtras.contrast')} hint={t('appearanceExtras.contrastHint')} keywords={t('appearanceExtras.contrastKeywords')}>
        {() => (
          <SegmentedControl<ContrastLevel>
            aria-label={t('appearanceExtras.contrast')}
            value={settings.contrast ?? 'standard'}
            onChange={(value) => onChange('contrast', value)}
            options={[
              { value: 'standard', label: t('appearanceExtras.standard') },
              { value: 'medium', label: t('appearanceExtras.medium') },
              { value: 'high', label: t('appearanceExtras.high') },
            ]}
          />
        )}
      </Row>
      <Row label={t('appearanceExtras.interfaceSize')} hint={t('appearanceExtras.interfaceSizeHint')} keywords={t('appearanceExtras.interfaceSizeKeywords')}>
        {() => (
          <SegmentedControl<UiSize>
            aria-label={t('appearanceExtras.interfaceSize')}
            value={settings.ui_size ?? 'medium'}
            onChange={(value) => onChange('ui_size', value)}
            options={[
              { value: 'small', label: t('appearanceExtras.small') },
              { value: 'medium', label: t('appearanceExtras.medium') },
              { value: 'large', label: t('appearanceExtras.large') },
            ]}
          />
        )}
      </Row>
      <Row
        label={t('appearanceExtras.backgroundArt')}
        hint={
          settings.background_art
            ? t('appearanceExtras.backgroundArtHint', { title: settings.background_art.title })
            : t('appearanceExtras.backgroundArtEmptyHint')
        }
        keywords={t('appearanceExtras.backgroundArtKeywords')}
      >
        {() =>
          settings.background_art ? (
            <span className="flex items-center gap-2">
              <ShimmerImage src={settings.background_art.url} alt="" className="h-9 w-16 rounded-md" />
              <SegmentedControl<'blur' | 'plain'>
                aria-label={t('appearanceExtras.backgroundArtStyle')}
                value={settings.background_art.style === 'plain' ? 'plain' : 'blur'}
                onChange={(style) => settings.background_art && onChange('background_art', { ...settings.background_art, style })}
                options={[
                  { value: 'blur', label: t('appearanceExtras.blurred') },
                  { value: 'plain', label: t('appearanceExtras.plain') },
                ]}
              />
              <Button variant="ghost" size="sm" onClick={() => onChange('background_art', null)}>
                {t('common.remove')}
              </Button>
            </span>
          ) : (
            <span className="text-xs text-text-muted">{t('common.none')}</span>
          )
        }
      </Row>
      <Row label={t('appearanceExtras.timeFormat')} hint={t('appearanceExtras.timeFormatHint')} keywords={t('appearanceExtras.timeFormatKeywords')}>
        {() => (
          <SegmentedControl<TimeFormat>
            aria-label={t('appearanceExtras.timeFormat')}
            value={settings.time_format ?? 'system'}
            onChange={(value) => onChange('time_format', value)}
            options={[
              { value: 'system', label: t('appearanceExtras.system') },
              { value: '12h', label: t('appearanceExtras.hour12') },
              { value: '24h', label: t('appearanceExtras.hour24') },
            ]}
          />
        )}
      </Row>
      <Row
        label={t('appearanceExtras.blurUnwatched')}
        hint={t('appearanceExtras.blurUnwatchedHint')}
        keywords={t('appearanceExtras.blurUnwatchedKeywords')}
      >
        {({ labelId, hintId }) => (
          <Switch
            aria-labelledby={labelId}
            aria-describedby={hintId}
            checked={settings.blur_unwatched_art === true}
            onChange={(checked) => onChange('blur_unwatched_art', checked)}
          />
        )}
      </Row>
      <Row label={t('appearanceExtras.windowButtons')} hint={buttonsHint}>
        {() => (
          <SegmentedControl<WindowButtonsSetting>
            aria-label={t('appearanceExtras.windowButtons')}
            value={settings.window_buttons ?? 'desktop'}
            onChange={(value) => onChange('window_buttons', value)}
            options={[
              { value: 'desktop', label: t('appearanceExtras.followDesktop') },
              { value: 'left', label: t('appearanceExtras.left') },
              { value: 'right', label: t('appearanceExtras.right') },
            ]}
          />
        )}
      </Row>
      <Row
        label={t('appearanceExtras.playfulTouches')}
        hint={t('appearanceExtras.playfulTouchesHint')}
      >
        {({ labelId, hintId }) => (
          <Switch
            aria-labelledby={labelId}
            aria-describedby={hintId}
            checked={settings.playful_touches !== false}
            onChange={(checked) => onChange('playful_touches', checked)}
          />
        )}
      </Row>
    </>
  );
};
