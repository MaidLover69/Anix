import React, { useId } from 'react';
import { SegmentedControl } from '../ui/SegmentedControl';
import { Switch } from '../ui/Switch';
import { PanelSection, PlayerSidePanel } from './PlayerSidePanel';
import type { AudioMode } from './PlayerControls';
import type { ScreenFit } from './playerLogic';
import { MAX_SPEED } from './playerLogic';
import { useSaveSettings, useSettings } from '../../hooks/useSettings';
import { ANIME4K_MODES, anime4kParts, anime4kPreset } from '../../lib/anime4k';
import type { Anime4kPreset, Anime4kQuality } from '../../lib/anime4k';
import { usePlayerStore } from '../../stores/playerStore';
import type { AppSettings } from '../../types';
import { t, type TKey } from '../../i18n';

/** One choice of a list such as the stream's qualities. */
export interface PlayerMenuOption {
  value: string;
  label: string;
}

const SPEEDS = ['0.5', '0.75', '1', '1.25', '1.5', '2', String(MAX_SPEED)];

const FITS: { value: ScreenFit; labelKey: TKey }[] = [
  { value: 'fit', labelKey: 'playerSettingsPanel.fit' },
  { value: 'crop', labelKey: 'playerSettingsPanel.crop' },
  { value: 'fill', labelKey: 'playerSettingsPanel.fill' },
];

const FIT_HINTS: Record<ScreenFit, TKey> = {
  fit: 'playerSettingsPanel.fitHint',
  crop: 'playerSettingsPanel.cropHint',
  fill: 'playerSettingsPanel.fillHint',
};

const Row: React.FC<{ label: string; hint?: string; children: (labelId: string) => React.ReactNode }> = ({
  label,
  hint,
  children,
}) => {
  const id = useId();
  return (
    <div className="flex items-center justify-between gap-4 rounded-md ui-inset bg-bg-surface px-3 py-3">
      <div className="min-w-0">
        <p id={id} className="text-[13px] font-medium text-text-primary">
          {label}
        </p>
        {hint && <p className="text-xs text-text-muted">{hint}</p>}
      </div>
      {children(id)}
    </div>
  );
};

interface PlayerSettingsPanelProps {
  isOpen: boolean;
  onClose: () => void;
  container?: HTMLElement | null;
  engine: 'web' | 'mpv';
  rate: number;
  onRateChange: (rate: number) => void;
  /** Empty unless the stream offers a choice. */
  qualityOptions: PlayerMenuOption[];
  quality: string;
  onQualityChange: (value: string) => void;
  audioMode: AudioMode;
  onAudioModeChange: (mode: AudioMode) => void;
}

/** "Settings" of the Now playing panel: speed, quality, screen, sound, upscaling, skipping. */
export const PlayerSettingsPanel: React.FC<PlayerSettingsPanelProps> = ({
  isOpen,
  onClose,
  container,
  engine,
  rate,
  onRateChange,
  qualityOptions,
  quality,
  onQualityChange,
  audioMode,
  onAudioModeChange,
}) => {
  const screenFit = usePlayerStore((s) => s.screenFit);
  const setScreenFit = usePlayerStore((s) => s.setScreenFit);
  const anime4k = usePlayerStore((s) => s.anime4k);
  const setAnime4k = usePlayerStore((s) => s.setAnime4k);
  const autoPlayNext = usePlayerStore((s) => s.autoPlayNext);
  const setAutoPlayNext = usePlayerStore((s) => s.setAutoPlayNext);
  // The same settings the Settings screen edits; there is no second copy in the player.
  const { data: settings } = useSettings();
  const save = useSaveSettings();
  const saveSetting = (changes: Partial<AppSettings>) => save.mutate(changes);
  const upscale = anime4kParts(anime4k);
  const speed = SPEEDS.includes(String(rate)) ? String(rate) : '';

  return (
    <PlayerSidePanel isOpen={isOpen} onClose={onClose} title={t('common.settings')} subtitle={t('playerSettingsPanel.subtitle')} container={container}>
      <PanelSection title={t('playerSettingsPanel.playback')}>
        <div className="flex flex-col gap-2">
          <p className="text-[13px] text-text-secondary">
            {t('playerSettingsPanel.speed')}{' '}
            <span className="tabular-nums text-text-muted">
              {t('playerSettingsPanel.speedHint', { rate: Number(rate.toFixed(2)) })}
            </span>
          </p>
          <SegmentedControl
            size="sm"
            aria-label={t('playerSettingsPanel.playbackSpeed')}
            value={speed}
            onChange={(value) => onRateChange(Number(value))}
            options={SPEEDS.map((value) => ({ value, label: value === '1' ? '1×' : `${value}` }))}
          />
        </div>
        {qualityOptions.length > 0 && (
          <div className="flex flex-col gap-1" role="radiogroup" aria-label={t('playerSettingsPanel.quality')}>
            <p className="text-[13px] text-text-secondary">{t('playerSettingsPanel.quality')}</p>
            {qualityOptions.map((option) => (
              <button
                key={option.value}
                type="button"
                role="radio"
                aria-checked={option.value === quality}
                onClick={() => onQualityChange(option.value)}
                className={`flex h-9 w-full items-center rounded-md px-3 text-left text-[13px] cursor-pointer transition-colors ${
                  option.value === quality
                    ? 'bg-accent-tint text-accent-300'
                    : 'ui-inset bg-bg-surface text-text-secondary hover:bg-bg-input hover:text-text-primary'
                }`}
              >
                {option.label}
              </button>
            ))}
          </div>
        )}
        <Row label={t('playerSettingsPanel.autoPlayNext')} hint={t('playerSettingsPanel.autoPlayNextHint')}>
          {(id) => <Switch aria-labelledby={id} checked={autoPlayNext} onChange={setAutoPlayNext} />}
        </Row>
      </PanelSection>

      <PanelSection title={t('playerSettingsPanel.screen')}>
        <SegmentedControl
          size="sm"
          aria-label={t('playerSettingsPanel.screenSize')}
          value={screenFit}
          onChange={setScreenFit}
          options={FITS.map(({ value, labelKey }) => ({ value, label: t(labelKey) }))}
        />
        <p className="text-xs text-text-muted">{t(FIT_HINTS[screenFit])}</p>
      </PanelSection>

      <PanelSection title={t('playerSettingsPanel.sound')}>
        <Row label={t('playerSettingsPanel.dialogueBoost')} hint={t('playerSettingsPanel.dialogueBoostHint')}>
          {(id) => (
            <Switch
              aria-labelledby={id}
              checked={audioMode === 'boost'}
              onChange={(on) => onAudioModeChange(on ? 'boost' : 'normal')}
            />
          )}
        </Row>
      </PanelSection>

      <PanelSection title={t('playerSettingsPanel.upscaling')}>
        {engine === 'mpv' ? (
          <>
            <SegmentedControl
              size="sm"
              aria-label={t('playerSettingsPanel.anime4kMode')}
              value={upscale.mode}
              onChange={(mode) => setAnime4k(anime4kPreset(mode, upscale.quality))}
              options={ANIME4K_MODES.map(({ value, label }) => ({ value, label: value === 'off' ? t('common.off') : label }))}
            />
            <SegmentedControl<Anime4kQuality>
              size="sm"
              aria-label={t('playerSettingsPanel.anime4kQuality')}
              value={upscale.quality}
              onChange={(quality) => setAnime4k(anime4kPreset(upscale.mode === 'off' ? 'a' : upscale.mode, quality) as Anime4kPreset)}
              options={[
                { value: 'fast', label: t('playerSettingsPanel.fast') },
                { value: 'hq', label: t('playerSettingsPanel.hq') },
              ]}
            />
            <p className="text-xs text-text-muted">{t(ANIME4K_MODES.find((m) => m.value === upscale.mode)?.hintKey ?? 'anime4k.offHint')}</p>
          </>
        ) : (
          <p className="text-xs text-text-muted">{t('playerSettingsPanel.mpvOnly')}</p>
        )}
      </PanelSection>

      <PanelSection title={t('playerSettingsPanel.skipping')}>
        <Row label={t('playerSettingsPanel.skipIntros')} hint={t('playerSettingsPanel.skipIntrosHint')}>
          {(id) => (
            <Switch
              aria-labelledby={id}
              checked={settings?.skip_intro ?? false}
              disabled={!settings}
              onChange={(checked) => saveSetting({ skip_intro: checked })}
            />
          )}
        </Row>
        <Row label={t('playerSettingsPanel.skipEndings')} hint={t('playerSettingsPanel.skipEndingsHint')}>
          {(id) => (
            <Switch
              aria-labelledby={id}
              checked={settings?.skip_outro ?? false}
              disabled={!settings}
              onChange={(checked) => saveSetting({ skip_outro: checked })}
            />
          )}
        </Row>
      </PanelSection>
    </PlayerSidePanel>
  );
};
