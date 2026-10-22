import React from 'react';
import { Check, RotateCcw } from 'lucide-react';
import { Button } from './ui/Button';
import { SegmentedControl } from './ui/SegmentedControl';
import { PanelSection, PlayerSidePanel } from './player/PlayerSidePanel';
import { Slider } from './player/Slider';
import { SUBTITLES_OFF } from '../hooks/useSubtitleTracks';
import type { SubtitleOption } from '../hooks/useSubtitleTracks';
import type { MediaTrackOption } from '../hooks/usePlayerEngine';
import { usePlayerStore } from '../stores/playerStore';
import { t } from '../i18n';

interface SubtitleSettingsModalProps {
  isOpen: boolean;
  onClose: () => void;
  subtitles: SubtitleOption[];
  /** `SUBTITLES_OFF` or the id of the option that is really showing. */
  selectedSubtitleId: string;
  onSelectSubtitle: (id: string) => void;
  /** Tracks that can be shown as the second subtitle, at the top of the picture. */
  secondarySubtitles?: SubtitleOption[];
  /** `SUBTITLES_OFF` or the id of the second subtitle. */
  selectedSecondaryId?: string;
  onSelectSecondary?: (id: string) => void;
  audioTracks: MediaTrackOption[];
  selectedAudioTrackId: number;
  onSelectAudioTrack: (id: number) => void;
  category: 'sub' | 'dub';
  onToggleCategory: (category: 'sub' | 'dub') => void;
  /** Portal target; the player passes the element it shows fullscreen. */
  container?: HTMLElement | null;
  /** Embedded mpv draws the subtitles itself: colours and dropped files are web-player only. */
  engine?: 'web' | 'mpv';
}

const COLOR_OPTIONS = [
  { nameKey: 'subtitleSettingsModal.colorWhite', value: '#ffffff' },
  { nameKey: 'subtitleSettingsModal.colorAmber', value: '#facc15' },
  { nameKey: 'subtitleSettingsModal.colorCyan', value: '#22d3ee' },
  { nameKey: 'subtitleSettingsModal.colorLime', value: '#4ade80' },
  { nameKey: 'subtitleSettingsModal.colorRose', value: '#f43f5e' },
] as const;

const BACKGROUND_OPTIONS = [
  { nameKey: 'common.none', value: 'transparent' },
  { nameKey: 'subtitleSettingsModal.backgroundDimmed', value: 'rgba(0, 0, 0, 0.5)' },
  { nameKey: 'subtitleSettingsModal.backgroundSolid', value: '#000000' },
] as const;

const rowClass = (selected: boolean) =>
  `flex h-10 w-full items-center justify-between gap-3 rounded-md px-3 text-start text-[13px] cursor-pointer transition-colors ${
    selected
      ? 'bg-accent-tint text-accent-300'
      : 'ui-inset bg-bg-surface text-text-secondary hover:bg-bg-input hover:text-text-primary'
  }`;

const choiceClass = (selected: boolean) =>
  `flex h-8 items-center justify-center gap-2 rounded-md px-2 text-xs cursor-pointer transition-colors ${
    selected
      ? 'bg-accent-tint text-accent-300'
      : 'ui-inset bg-bg-surface text-text-secondary hover:bg-bg-input hover:text-text-primary'
  }`;

const SectionLabel: React.FC<{ children: React.ReactNode }> = ({ children }) => (
  <h4 className="text-[11px] font-semibold uppercase tracking-wider text-text-muted">{children}</h4>
);

function formatDelay(delayMs: number): string {
  const seconds = (delayMs / 1000).toFixed(2);
  return t('subtitleSettingsModal.seconds', { value: delayMs > 0 ? `+${seconds}` : seconds });
}

export const SubtitleSettingsModal: React.FC<SubtitleSettingsModalProps> = ({
  isOpen,
  onClose,
  subtitles,
  selectedSubtitleId,
  onSelectSubtitle,
  secondarySubtitles = [],
  selectedSecondaryId = SUBTITLES_OFF,
  onSelectSecondary,
  audioTracks,
  selectedAudioTrackId,
  onSelectAudioTrack,
  category,
  onToggleCategory,
  container,
  engine = 'web',
}) => {
  const subtitleFontSize = usePlayerStore((s) => s.subtitleFontSize);
  const setSubtitleFontSize = usePlayerStore((s) => s.setSubtitleFontSize);
  const subtitleRaise = usePlayerStore((s) => s.subtitleRaise);
  const setSubtitleRaise = usePlayerStore((s) => s.setSubtitleRaise);
  const subtitleColor = usePlayerStore((s) => s.subtitleColor);
  const setSubtitleColor = usePlayerStore((s) => s.setSubtitleColor);
  const subtitleBackground = usePlayerStore((s) => s.subtitleBackground);
  const setSubtitleBackground = usePlayerStore((s) => s.setSubtitleBackground);
  const secondaryFontSize = usePlayerStore((s) => s.secondarySubtitleFontSize);
  const setSecondaryFontSize = usePlayerStore((s) => s.setSecondarySubtitleFontSize);
  const subtitleDelayMs = usePlayerStore((s) => s.subtitleDelayMs);
  const setSubtitleDelayMs = usePlayerStore((s) => s.setSubtitleDelayMs);

  const setDelay = (delayMs: number) =>
    setSubtitleDelayMs(Math.max(-5000, Math.min(5000, Math.round(delayMs))));

  return (
    <PlayerSidePanel isOpen={isOpen} onClose={onClose} title={t('subtitleSettingsModal.title')} container={container}>
      <PanelSection title={t('subtitleSettingsModal.tracks')}>
            <section className="flex flex-col gap-2">
              <SectionLabel>{t('subtitleSettingsModal.subtitles')}</SectionLabel>
              <div role="radiogroup" aria-label={t('subtitleSettingsModal.subtitleTrack')} className="flex flex-col gap-1">
                <button
                  type="button"
                  role="radio"
                  aria-checked={selectedSubtitleId === SUBTITLES_OFF}
                  onClick={() => onSelectSubtitle(SUBTITLES_OFF)}
                  className={rowClass(selectedSubtitleId === SUBTITLES_OFF)}
                >
                  <span>{t('common.off')}</span>
                  {selectedSubtitleId === SUBTITLES_OFF && (
                    <Check aria-hidden="true" className="h-4 w-4 flex-shrink-0" />
                  )}
                </button>
                {subtitles.map((option) => {
                  const selected = option.id === selectedSubtitleId;
                  return (
                    <button
                      key={option.id}
                      type="button"
                      role="radio"
                      aria-checked={selected}
                      onClick={() => onSelectSubtitle(option.id)}
                      className={rowClass(selected)}
                    >
                      <span className="flex min-w-0 items-center gap-2">
                        <span className="truncate">{option.label}</span>
                        <span className="flex-shrink-0 text-[11px] uppercase text-text-muted">
                          {option.format}
                          {option.isDefault ? ` · ${t('subtitleSettingsModal.default')}` : ''}
                        </span>
                      </span>
                      {selected && <Check aria-hidden="true" className="h-4 w-4 flex-shrink-0" />}
                    </button>
                  );
                })}
              </div>
              {subtitles.length === 0 && (
                <p className="text-xs text-text-muted">
                  {t('subtitleSettingsModal.noTracks')}
                </p>
              )}
              {engine === 'web' && (
                <p className="text-xs text-text-muted">
                  {t('subtitleSettingsModal.dropHint')}
                </p>
              )}
            </section>

            {onSelectSecondary && secondarySubtitles.length > 0 && (
              <section className="flex flex-col gap-2">
                <SectionLabel>{t('subtitleSettingsModal.secondSubtitle')}</SectionLabel>
                <div role="radiogroup" aria-label={t('subtitleSettingsModal.secondSubtitleTrack')} className="flex flex-col gap-1">
                  {[{ id: SUBTITLES_OFF, label: t('common.off'), format: '', isDefault: false }, ...secondarySubtitles].map(
                    (option) => {
                      const selected = option.id === selectedSecondaryId;
                      return (
                        <button
                          key={option.id}
                          type="button"
                          role="radio"
                          aria-checked={selected}
                          onClick={() => onSelectSecondary(option.id)}
                          className={rowClass(selected)}
                        >
                          <span className="flex min-w-0 items-center gap-2">
                            <span className="truncate">{option.label}</span>
                            {option.format && (
                              <span className="flex-shrink-0 text-[11px] uppercase text-text-muted">
                                {option.format}
                              </span>
                            )}
                          </span>
                          {selected && <Check aria-hidden="true" className="h-4 w-4 flex-shrink-0" />}
                        </button>
                      );
                    }
                  )}
                </div>
              </section>
            )}

            <section className="flex flex-col gap-2">
              <SectionLabel>{t('subtitleSettingsModal.audio')}</SectionLabel>
              <div className="flex items-center justify-between gap-3 rounded-md ui-inset bg-bg-surface px-3 py-2">
                <span className="text-[13px] text-text-secondary">{t('subtitleSettingsModal.version')}</span>
                <SegmentedControl
                  size="sm"
                  aria-label={t('subtitleSettingsModal.audioVersion')}
                  value={category}
                  onChange={onToggleCategory}
                  options={[
                    { value: 'sub', label: t('subtitleSettingsModal.originalSub') },
                    { value: 'dub', label: t('subtitleSettingsModal.englishDub') },
                  ]}
                />
              </div>
              {audioTracks.length > 1 && (
                <div role="radiogroup" aria-label={t('subtitleSettingsModal.audioTrack')} className="flex flex-col gap-1">
                  {audioTracks.map((track) => {
                    const selected = track.id === selectedAudioTrackId;
                    return (
                      <button
                        key={track.id}
                        type="button"
                        role="radio"
                        aria-checked={selected}
                        onClick={() => onSelectAudioTrack(track.id)}
                        className={rowClass(selected)}
                      >
                        <span className="truncate">{track.label}</span>
                        {selected && <Check aria-hidden="true" className="h-4 w-4 flex-shrink-0" />}
                      </button>
                    );
                  })}
                </div>
              )}
            </section>
      </PanelSection>

      <PanelSection title={t('subtitleSettingsModal.appearance')}>
            <section className="flex flex-col gap-2">
              <div className="flex items-center justify-between">
                <SectionLabel>{t('subtitleSettingsModal.textSize')}</SectionLabel>
                <span className="text-xs tabular-nums text-text-secondary">{t('subtitleSettingsModal.px', { value: subtitleFontSize })}</span>
              </div>
              <Slider
                label={t('subtitleSettingsModal.subtitleTextSize')}
                value={subtitleFontSize}
                min={12}
                max={40}
                step={1}
                valueText={t('subtitleSettingsModal.pixels', { value: subtitleFontSize })}
                onChange={setSubtitleFontSize}
              />
            </section>

            <section className="flex flex-col gap-2">
              <div className="flex items-center justify-between">
                <SectionLabel>{t('subtitleSettingsModal.subtitleHeight')}</SectionLabel>
                <span className="text-xs tabular-nums text-text-secondary">
                  {subtitleRaise === 0 ? t('subtitleSettingsModal.bottom') : t('subtitleSettingsModal.percentUp', { value: subtitleRaise })}
                </span>
              </div>
              <Slider
                label={t('subtitleSettingsModal.subtitleHeight')}
                value={subtitleRaise}
                min={0}
                max={40}
                step={1}
                valueText={
                  subtitleRaise === 0
                    ? t('subtitleSettingsModal.atTheBottom')
                    : t('subtitleSettingsModal.percentAboveBottom', { value: subtitleRaise })
                }
                onChange={setSubtitleRaise}
              />
            </section>

            <section className="flex flex-col gap-2">
              <div className="flex items-center justify-between">
                <SectionLabel>{t('subtitleSettingsModal.secondSubtitleSize')}</SectionLabel>
                <span className="text-xs tabular-nums text-text-secondary">{t('subtitleSettingsModal.px', { value: secondaryFontSize })}</span>
              </div>
              <Slider
                label={t('subtitleSettingsModal.secondSubtitleTextSize')}
                value={secondaryFontSize}
                min={12}
                max={40}
                step={1}
                valueText={t('subtitleSettingsModal.pixels', { value: secondaryFontSize })}
                onChange={setSecondaryFontSize}
              />
            </section>

            {engine === 'web' ? (
              <>
            <section className="flex flex-col gap-2">
              <SectionLabel>{t('subtitleSettingsModal.textColour')}</SectionLabel>
              <div className="grid grid-cols-5 gap-2">
                {COLOR_OPTIONS.map((color) => (
                  <button
                    key={color.value}
                    type="button"
                    aria-pressed={subtitleColor === color.value}
                    onClick={() => setSubtitleColor(color.value)}
                    className={choiceClass(subtitleColor === color.value)}
                  >
                    <span
                      aria-hidden="true"
                      className="h-2.5 w-2.5 flex-shrink-0 rounded-full"
                      style={{ backgroundColor: color.value }}
                    />
                    <span>{t(color.nameKey)}</span>
                  </button>
                ))}
              </div>
            </section>

            <section className="flex flex-col gap-2">
              <SectionLabel>{t('subtitleSettingsModal.background')}</SectionLabel>
              <div className="grid grid-cols-3 gap-2">
                {BACKGROUND_OPTIONS.map((background) => (
                  <button
                    key={background.value}
                    type="button"
                    aria-pressed={subtitleBackground === background.value}
                    onClick={() => setSubtitleBackground(background.value)}
                    className={choiceClass(subtitleBackground === background.value)}
                  >
                    {t(background.nameKey)}
                  </button>
                ))}
              </div>
            </section>

              </>
            ) : (
              <p className="text-xs text-text-muted">
                {t('subtitleSettingsModal.mpvStyleNote')}
              </p>
            )}

            <section className="flex flex-col gap-2">
              <div className="flex items-center justify-between">
                <SectionLabel>{t('subtitleSettingsModal.timing')}</SectionLabel>
                <span className="text-xs tabular-nums text-text-secondary">
                  {subtitleDelayMs === 0
                    ? t('subtitleSettingsModal.noDelay')
                    : subtitleDelayMs > 0
                    ? t('subtitleSettingsModal.delayLater', { delay: formatDelay(subtitleDelayMs) })
                    : t('subtitleSettingsModal.delayEarlier', { delay: formatDelay(subtitleDelayMs) })}
                </span>
              </div>
              <div className="flex items-center gap-2">
                <Button size="sm" variant="ghost" onClick={() => setDelay(subtitleDelayMs - 250)}>
                  {t('subtitleSettingsModal.seconds', { value: '−0.25' })}
                </Button>
                <Slider
                  className="flex-1"
                  label={t('subtitleSettingsModal.subtitleDelay')}
                  value={subtitleDelayMs}
                  min={-5000}
                  max={5000}
                  step={100}
                  valueText={formatDelay(subtitleDelayMs)}
                  onChange={setDelay}
                />
                <Button size="sm" variant="ghost" onClick={() => setDelay(subtitleDelayMs + 250)}>
                  {t('subtitleSettingsModal.seconds', { value: '+0.25' })}
                </Button>
                <Button
                  icon
                  size="sm"
                  variant="ghost"
                  aria-label={t('subtitleSettingsModal.resetDelay')}
                  title={t('subtitleSettingsModal.resetDelay')}
                  disabled={subtitleDelayMs === 0}
                  onClick={() => setDelay(0)}
                >
                  <RotateCcw aria-hidden="true" className="h-3.5 w-3.5" />
                </Button>
              </div>
            </section>

            <div className="flex min-h-[64px] items-center justify-center rounded-md bg-black p-4">
              <span
                style={{
                  fontSize: `${subtitleFontSize}px`,
                  color: subtitleColor,
                  backgroundColor: subtitleBackground,
                  padding: '2px 8px',
                  textShadow: '0 0 4px #000, 0 0 8px #000',
                }}
                className="text-center"
              >
                {t('subtitleSettingsModal.preview')}
              </span>
            </div>
      </PanelSection>
    </PlayerSidePanel>
  );
};
