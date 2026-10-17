import React, { useState } from 'react';
import { Check, RotateCcw } from 'lucide-react';
import { Button } from './ui/Button';
import { Modal } from './ui/Modal';
import { SegmentedControl } from './ui/SegmentedControl';
import { Switch } from './ui/Switch';
import { Tabs } from './ui/Tabs';
import { Slider } from './player/Slider';
import { useSettings, useSaveSettings } from '../hooks/useSettings';
import { SUBTITLES_OFF } from '../hooks/useSubtitleTracks';
import type { SubtitleOption } from '../hooks/useSubtitleTracks';
import type { MediaTrackOption } from '../hooks/usePlayerEngine';
import { usePlayerStore } from '../stores/playerStore';

type SettingsTab = 'tracks' | 'style' | 'playback';

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
}

const TABS: { value: SettingsTab; label: string }[] = [
  { value: 'tracks', label: 'Tracks' },
  { value: 'style', label: 'Appearance' },
  { value: 'playback', label: 'Playback' },
];

const COLOR_OPTIONS = [
  { name: 'White', value: '#ffffff' },
  { name: 'Amber', value: '#facc15' },
  { name: 'Cyan', value: '#22d3ee' },
  { name: 'Lime', value: '#4ade80' },
  { name: 'Rose', value: '#f43f5e' },
];

const BACKGROUND_OPTIONS = [
  { name: 'None', value: 'transparent' },
  { name: 'Dimmed', value: 'rgba(0, 0, 0, 0.5)' },
  { name: 'Solid', value: '#000000' },
];

const rowClass = (selected: boolean) =>
  `flex h-10 w-full items-center justify-between gap-3 rounded-md px-3 text-left text-[13px] cursor-pointer transition-colors ${
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
  <h3 className="text-[11px] font-semibold uppercase tracking-wider text-text-muted">{children}</h3>
);

function formatDelay(delayMs: number): string {
  const seconds = (delayMs / 1000).toFixed(2);
  return delayMs > 0 ? `+${seconds}s` : `${seconds}s`;
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
}) => {
  const [activeTab, setActiveTab] = useState<SettingsTab>('tracks');

  const autoPlayNext = usePlayerStore((s) => s.autoPlayNext);
  const setAutoPlayNext = usePlayerStore((s) => s.setAutoPlayNext);
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

  // The same setting the Settings screen edits; there is no second copy in the player.
  const { data: settings } = useSettings();
  const saveSettings = useSaveSettings();
  const autoSkip = settings?.auto_skip_intro_outro ?? false;

  const setDelay = (delayMs: number) =>
    setSubtitleDelayMs(Math.max(-5000, Math.min(5000, Math.round(delayMs))));

  return (
    <Modal
      isOpen={isOpen}
      onClose={onClose}
      title="Subtitles and audio"
      container={container}
      footer={
        <Button variant="primary" onClick={onClose}>
          Done
        </Button>
      }
    >
      <Tabs tabs={TABS} value={activeTab} onChange={setActiveTab} aria-label="Player settings" />

      <div className="mt-4 flex max-h-[55vh] flex-col gap-5 overflow-y-auto pr-1">
        {activeTab === 'tracks' && (
          <>
            <section className="flex flex-col gap-2">
              <SectionLabel>Subtitles</SectionLabel>
              <div role="radiogroup" aria-label="Subtitle track" className="flex flex-col gap-1">
                <button
                  type="button"
                  role="radio"
                  aria-checked={selectedSubtitleId === SUBTITLES_OFF}
                  onClick={() => onSelectSubtitle(SUBTITLES_OFF)}
                  className={rowClass(selectedSubtitleId === SUBTITLES_OFF)}
                >
                  <span>Off</span>
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
                          {option.isDefault ? ' · default' : ''}
                        </span>
                      </span>
                      {selected && <Check aria-hidden="true" className="h-4 w-4 flex-shrink-0" />}
                    </button>
                  );
                })}
              </div>
              {subtitles.length === 0 && (
                <p className="text-xs text-text-muted">
                  This stream has no separate subtitle tracks. Subtitles may be part of the picture.
                </p>
              )}
              <p className="text-xs text-text-muted">
                Drop a .srt, .ass or .vtt file on the player to add it as a track.
              </p>
            </section>

            {onSelectSecondary && secondarySubtitles.length > 0 && (
              <section className="flex flex-col gap-2">
                <SectionLabel>Second subtitle (top of the picture)</SectionLabel>
                <div role="radiogroup" aria-label="Second subtitle track" className="flex flex-col gap-1">
                  {[{ id: SUBTITLES_OFF, label: 'Off', format: '', isDefault: false }, ...secondarySubtitles].map(
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
              <SectionLabel>Audio</SectionLabel>
              <div className="flex items-center justify-between gap-3 rounded-md ui-inset bg-bg-surface px-3 py-2">
                <span className="text-[13px] text-text-secondary">Version</span>
                <SegmentedControl
                  size="sm"
                  aria-label="Audio version"
                  value={category}
                  onChange={onToggleCategory}
                  options={[
                    { value: 'sub', label: 'Original (sub)' },
                    { value: 'dub', label: 'English dub' },
                  ]}
                />
              </div>
              {audioTracks.length > 1 && (
                <div role="radiogroup" aria-label="Audio track" className="flex flex-col gap-1">
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
          </>
        )}

        {activeTab === 'style' && (
          <>
            <section className="flex flex-col gap-2">
              <div className="flex items-center justify-between">
                <SectionLabel>Text size</SectionLabel>
                <span className="text-xs tabular-nums text-text-secondary">{subtitleFontSize}px</span>
              </div>
              <Slider
                label="Subtitle text size"
                value={subtitleFontSize}
                min={12}
                max={40}
                step={1}
                valueText={`${subtitleFontSize} pixels`}
                onChange={setSubtitleFontSize}
              />
            </section>

            <section className="flex flex-col gap-2">
              <div className="flex items-center justify-between">
                <SectionLabel>Subtitle height</SectionLabel>
                <span className="text-xs tabular-nums text-text-secondary">
                  {subtitleRaise === 0 ? 'Bottom' : `${subtitleRaise}% up`}
                </span>
              </div>
              <Slider
                label="Subtitle height"
                value={subtitleRaise}
                min={0}
                max={40}
                step={1}
                valueText={subtitleRaise === 0 ? 'At the bottom' : `${subtitleRaise} percent above the bottom`}
                onChange={setSubtitleRaise}
              />
            </section>

            <section className="flex flex-col gap-2">
              <div className="flex items-center justify-between">
                <SectionLabel>Second subtitle size</SectionLabel>
                <span className="text-xs tabular-nums text-text-secondary">{secondaryFontSize}px</span>
              </div>
              <Slider
                label="Second subtitle text size"
                value={secondaryFontSize}
                min={12}
                max={40}
                step={1}
                valueText={`${secondaryFontSize} pixels`}
                onChange={setSecondaryFontSize}
              />
            </section>

            <section className="flex flex-col gap-2">
              <SectionLabel>Text colour</SectionLabel>
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
                    <span>{color.name}</span>
                  </button>
                ))}
              </div>
            </section>

            <section className="flex flex-col gap-2">
              <SectionLabel>Background</SectionLabel>
              <div className="grid grid-cols-3 gap-2">
                {BACKGROUND_OPTIONS.map((background) => (
                  <button
                    key={background.value}
                    type="button"
                    aria-pressed={subtitleBackground === background.value}
                    onClick={() => setSubtitleBackground(background.value)}
                    className={choiceClass(subtitleBackground === background.value)}
                  >
                    {background.name}
                  </button>
                ))}
              </div>
            </section>

            <section className="flex flex-col gap-2">
              <div className="flex items-center justify-between">
                <SectionLabel>Timing</SectionLabel>
                <span className="text-xs tabular-nums text-text-secondary">
                  {subtitleDelayMs === 0
                    ? 'No delay'
                    : `${formatDelay(subtitleDelayMs)} (${subtitleDelayMs > 0 ? 'later' : 'earlier'})`}
                </span>
              </div>
              <div className="flex items-center gap-2">
                <Button size="sm" variant="ghost" onClick={() => setDelay(subtitleDelayMs - 250)}>
                  −0.25s
                </Button>
                <Slider
                  className="flex-1"
                  label="Subtitle delay"
                  value={subtitleDelayMs}
                  min={-5000}
                  max={5000}
                  step={100}
                  valueText={formatDelay(subtitleDelayMs)}
                  onChange={setDelay}
                />
                <Button size="sm" variant="ghost" onClick={() => setDelay(subtitleDelayMs + 250)}>
                  +0.25s
                </Button>
                <Button
                  icon
                  size="sm"
                  variant="ghost"
                  aria-label="Reset subtitle delay"
                  title="Reset subtitle delay"
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
                Subtitle preview
              </span>
            </div>
          </>
        )}

        {activeTab === 'playback' && (
          <section className="flex flex-col gap-1">
            <div className="flex items-center justify-between gap-4 rounded-md ui-inset bg-bg-surface px-3 py-3">
              <div className="min-w-0">
                <p id="auto-skip-label" className="text-[13px] font-medium text-text-primary">
                  Skip intros and endings automatically
                </p>
                <p className="text-xs text-text-muted">
                  Each one is skipped once. This is the same switch as in Settings.
                </p>
              </div>
              <Switch
                aria-labelledby="auto-skip-label"
                checked={autoSkip}
                disabled={!settings}
                onChange={(checked) => saveSettings.mutate({ auto_skip_intro_outro: checked })}
              />
            </div>
            <div className="flex items-center justify-between gap-4 rounded-md ui-inset bg-bg-surface px-3 py-3">
              <div className="min-w-0">
                <p id="auto-next-label" className="text-[13px] font-medium text-text-primary">
                  Play the next episode automatically
                </p>
                <p className="text-xs text-text-muted">Starts it when the current episode ends.</p>
              </div>
              <Switch
                aria-labelledby="auto-next-label"
                checked={autoPlayNext}
                onChange={setAutoPlayNext}
              />
            </div>
          </section>
        )}
      </div>
    </Modal>
  );
};
