import React, { useRef, useState } from 'react';
import clsx from 'clsx';
import { useLocation } from 'react-router-dom';
import { invoke } from '@tauri-apps/api/core';
import { RefreshCw, Trash2 } from 'lucide-react';
import { CustomSelect } from '../CustomSelect';
import { Button } from '../ui/Button';
import { Switch } from '../ui/Switch';
import { errorMessage } from '../ui/ErrorState';
import { Row } from './SettingsLayout';
import { formatBytes } from '../library/format';
import { useEmbeddedMpvStatus } from '../../hooks/useEmbeddedMpv';
import type { HwdecSelfTest } from '../../hooks/useEmbeddedMpv';
import { useUIStore } from '../../stores/uiStore';
import { locale, t, type TKey } from '../../i18n';
import type { AppSettings, HwdecModeSetting, PlayerEngineSetting } from '../../types';

const ENGINE_OPTIONS: { value: PlayerEngineSetting; labelKey: TKey; lineKey: TKey }[] = [
  { value: 'mpv', labelKey: 'playerEngineSettings.engineMpv', lineKey: 'playerEngineSettings.engineMpvLine' },
  { value: 'web', labelKey: 'playerEngineSettings.engineWeb', lineKey: 'playerEngineSettings.engineWebLine' },
  { value: 'external', labelKey: 'playerEngineSettings.engineExternal', lineKey: 'playerEngineSettings.engineExternalLine' },
];

/** The three engines as choice cards with one plain line each. Behaves as a radio group. */
const EngineCards: React.FC<{
  value: PlayerEngineSetting;
  onChange: (engine: PlayerEngineSetting) => void;
  labelId: string;
  hintId?: string;
}> = ({ value, onChange, labelId, hintId }) => {
  const refs = useRef<Array<HTMLButtonElement | null>>([]);
  const handleKeyDown = (e: React.KeyboardEvent, index: number) => {
    // Right to left, the next card is drawn on the left: swap the two horizontal arrows.
    const rtl = getComputedStyle(e.currentTarget).direction === 'rtl';
    const key = rtl && e.key === 'ArrowLeft' ? 'ArrowRight' : rtl && e.key === 'ArrowRight' ? 'ArrowLeft' : e.key;
    const step = key === 'ArrowRight' || key === 'ArrowDown' ? 1 : key === 'ArrowLeft' || key === 'ArrowUp' ? -1 : 0;
    if (!step) return;
    e.preventDefault();
    const next = (index + step + ENGINE_OPTIONS.length) % ENGINE_OPTIONS.length;
    onChange(ENGINE_OPTIONS[next].value);
    refs.current[next]?.focus();
  };
  return (
    <div
      role="radiogroup"
      aria-labelledby={labelId}
      aria-describedby={hintId}
      className="grid w-full grid-cols-[repeat(auto-fit,minmax(12rem,1fr))] gap-2"
    >
      {ENGINE_OPTIONS.map((option, index) => {
        const selected = option.value === value;
        return (
          <button
            key={option.value}
            ref={(el) => {
              refs.current[index] = el;
            }}
            type="button"
            role="radio"
            aria-checked={selected}
            tabIndex={selected ? 0 : -1}
            onClick={() => onChange(option.value)}
            onKeyDown={(e) => handleKeyDown(e, index)}
            className={clsx(
              'ui-theme-card flex flex-col items-start gap-0.5 rounded-lg border p-3 text-start cursor-pointer transition-colors',
              selected ? 'border-accent-500 bg-accent-tint' : 'border-border-strong bg-bg-elevated hover:bg-bg-input'
            )}
          >
            <span className="text-[13px] font-semibold text-text-primary">{t(option.labelKey)}</span>
            <span className="text-xs text-text-muted">{t(option.lineKey)}</span>
          </button>
        );
      })}
    </div>
  );
};

const HWDEC_OPTIONS: { value: HwdecModeSetting; labelKey: TKey }[] = [
  { value: 'auto', labelKey: 'playerEngineSettings.hwdecAuto' },
  { value: 'copy', labelKey: 'playerEngineSettings.hwdecCopy' },
  { value: 'off', labelKey: 'playerEngineSettings.hwdecOff' },
];

// Labels are built per render: the interface language can change while the module stays loaded.
const labelled = (options: { value: string; labelKey: TKey }[]) =>
  options.map((o) => ({ value: o.value, label: t(o.labelKey) }));
const megabytes = (mb: number) => t('playerEngineSettings.megabytes', { size: mb });
const bufferOptions = () => [64, 150, 300, 600].map((mb) => ({ value: String(mb), label: megabytes(mb) }));
const readaheadOptions = () =>
  [15, 30, 60, 120].map((s) => ({ value: String(s), label: t('playerEngineSettings.seconds', { count: s }) }));
const diskOptions = () =>
  [512, 1024, 2048, 4096].map((mb) => ({
    value: String(mb),
    label: mb >= 1024 ? t('playerEngineSettings.gigabytes', { size: mb / 1024 }) : megabytes(mb),
  }));

function isOneOf<T extends string>(value: unknown, options: { value: T }[]): value is T {
  return options.some((o) => o.value === value);
}

/** The self-test's verdict in one sentence. */
export function selfTestSummary(test: HwdecSelfTest | null | undefined): string {
  if (!test) return t('playerEngineSettings.selfTestNotRun');
  const date = new Date(test.at * 1000).toLocaleDateString(locale());
  return t(test.ok ? 'playerEngineSettings.selfTestPassed' : 'playerEngineSettings.selfTestFailed', { date });
}

interface PlayerEngineSettingsProps {
  settings: AppSettings;
  onChange: <K extends keyof AppSettings>(key: K, value: AppSettings[K]) => void;
}

/** Settings › Playback: the player engine and embedded mpv's decoding, buffer and cache. */
export const PlayerEngineSettings: React.FC<PlayerEngineSettingsProps> = ({ settings, onChange }) => {
  const status = useEmbeddedMpvStatus().data;
  const showToast = useUIStore((s) => s.showToast);
  const location = useLocation();
  const [restartLater, setRestartLater] = useState(false);
  const [clearing, setClearing] = useState(false);

  const engine: PlayerEngineSetting = isOneOf(settings.player_engine, ENGINE_OPTIONS) ? settings.player_engine : 'mpv';
  const wanted = engine === 'mpv' ? 'mpv' : 'web';
  const restartNeeded = !!status && status.running_engine !== wanted && !(engine === 'mpv' && status.requested && !status.available);

  let engineHint = t('playerEngineSettings.engineHint');
  if (engine === 'mpv' && status?.requested && !status.available) {
    engineHint = t('playerEngineSettings.engineFailedHint', { error: status.error ?? t('playerEngineSettings.unknownError') });
  } else if (engine === 'web' && status?.reason) {
    engineHint = status.reason;
  } else if (engine === 'external') {
    engineHint = t('playerEngineSettings.externalHint');
  }

  const restart = () => {
    invoke('embedded_mpv_restart', { route: `${location.pathname}${location.search}` }).catch((err) =>
      showToast({ type: 'error', title: t('playerEngineSettings.couldNotRestart'), message: errorMessage(err) })
    );
  };

  const clearCache = () => {
    setClearing(true);
    invoke<number>('embedded_mpv_clear_cache')
      .then((freed) =>
        showToast({
          type: 'success',
          title: t('playerEngineSettings.cacheCleared'),
          message: t('playerEngineSettings.freed', { size: formatBytes(freed) }),
        })
      )
      .catch((err) => showToast({ type: 'error', title: t('playerEngineSettings.couldNotClearCache'), message: errorMessage(err) }))
      .finally(() => setClearing(false));
  };

  const mpvOnly = engine !== 'mpv';
  const mpvNote = mpvOnly ? ` ${t('playerEngineSettings.mpvOnly')}` : '';

  return (
    <>
      <Row label={t('playerEngineSettings.playerEngine')} hint={engineHint} stacked>
        {({ labelId, hintId }) => (
          <EngineCards
            labelId={labelId}
            hintId={hintId}
            value={engine}
            onChange={(next) => {
              if (next === engine) return;
              onChange('player_engine', next);
              setRestartLater(false);
              invoke('embedded_mpv_engine_changed', { engine: next }).catch(() => {});
            }}
          />
        )}
      </Row>
      {restartNeeded && !restartLater && (
        <div role="status" className="flex flex-wrap items-center justify-between gap-3 rounded-md bg-accent-tint px-3 py-2">
          <p className="text-xs text-text-primary">{t('playerEngineSettings.restartBanner')}</p>
          <div className="flex items-center gap-2">
            <Button size="sm" variant="ghost" onClick={() => setRestartLater(true)}>
              {t('playerEngineSettings.later')}
            </Button>
            <Button size="sm" variant="primary" onClick={restart}>
              <RefreshCw aria-hidden="true" className="h-3.5 w-3.5" />
              {t('playerEngineSettings.restartNow')}
            </Button>
          </div>
        </div>
      )}
      <Row
        label={t('playerEngineSettings.hardwareDecoding')}
        hint={`${t('playerEngineSettings.hardwareDecodingHint', { selfTest: selfTestSummary(status?.selftest) })}${mpvNote}`}
      >
        {({ labelId, hintId }) => (
          <CustomSelect
            aria-labelledby={labelId}
            aria-describedby={hintId}
            className="w-56"
            value={isOneOf(settings.hwdec_mode, HWDEC_OPTIONS) ? settings.hwdec_mode : 'auto'}
            onChange={(next) => {
              if (isOneOf(next, HWDEC_OPTIONS)) onChange('hwdec_mode', next);
            }}
            options={labelled(HWDEC_OPTIONS)}
          />
        )}
      </Row>
      <Row label={t('playerEngineSettings.buffer')} hint={`${t('playerEngineSettings.bufferHint')}${mpvNote}`}>
        {({ labelId, hintId }) => (
          <div className="flex items-center gap-2">
            <CustomSelect
              aria-labelledby={labelId}
              aria-describedby={hintId}
              aria-label={t('playerEngineSettings.bufferSize')}
              className="w-32"
              value={String(settings.player_buffer_mb)}
              onChange={(next) => onChange('player_buffer_mb', Number(next))}
              options={bufferOptions()}
            />
            <CustomSelect
              aria-label={t('playerEngineSettings.readAhead')}
              className="w-36"
              value={String(settings.player_readahead_secs)}
              onChange={(next) => onChange('player_readahead_secs', Number(next))}
              options={readaheadOptions()}
            />
          </div>
        )}
      </Row>
      <Row
        label={t('playerEngineSettings.diskCache')}
        hint={`${t('playerEngineSettings.diskCacheHint')}${mpvNote}`}
      >
        {({ labelId, hintId }) => (
          <div className="flex items-center gap-2">
            {settings.player_disk_cache && (
              <CustomSelect
                aria-label={t('playerEngineSettings.diskCacheLimit')}
                className="w-28"
                value={String(settings.player_disk_cache_mb)}
                onChange={(next) => onChange('player_disk_cache_mb', Number(next))}
                options={diskOptions()}
              />
            )}
            <Button size="sm" variant="ghost" aria-disabled={clearing || undefined} onClick={() => !clearing && clearCache()}>
              <Trash2 aria-hidden="true" className="h-3.5 w-3.5" />
              {t('playerEngineSettings.clearCache')}
            </Button>
            <Switch
              aria-labelledby={labelId}
              aria-describedby={hintId}
              checked={settings.player_disk_cache}
              onChange={(checked) => onChange('player_disk_cache', checked)}
            />
          </div>
        )}
      </Row>
    </>
  );
};
