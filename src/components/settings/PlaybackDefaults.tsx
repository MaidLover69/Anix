import React, { useState } from 'react';
import { CustomSelect } from '../CustomSelect';
import { Row } from './SettingsLayout';
import { t } from '../../i18n';
import { STREAMING_RESOLUTION_VALUES } from '../../lib/resolution';
import { CUSTOM_SPEED_MAX, CUSTOM_SPEED_MIN, CUSTOM_SPEED_STEP, episodeStartRate, parseCustomSpeed } from '../../lib/speed';
import type { AppSettings } from '../../types';

/** Settings › Playback: the stream an episode starts on, and the speed it starts at. */
export const PlaybackDefaults: React.FC<{
  settings: AppSettings;
  onChange: <K extends keyof AppSettings>(key: K, value: AppSettings[K]) => void;
}> = ({ settings, onChange }) => {
  const [typed, setTyped] = useState<string | null>(null);
  const speed = episodeStartRate(settings.default_playback_speed);
  const resolution = STREAMING_RESOLUTION_VALUES.includes(settings.streaming_resolution) ? settings.streaming_resolution : 'auto';
  const invalid = typed !== null && parseCustomSpeed(typed) === null;

  return (
    <>
      <Row label={t('settings.streamingResolution')} hint={t('settings.streamingResolutionHint')}>
        {({ labelId, hintId }) => (
          <CustomSelect
            aria-labelledby={labelId}
            aria-describedby={hintId}
            className="w-36"
            value={resolution}
            onChange={(value) => onChange('streaming_resolution', value)}
            options={STREAMING_RESOLUTION_VALUES.map((value) => ({
              value,
              label: value === 'auto' ? t('settings.resolutionAuto') : `${value}p`,
            }))}
          />
        )}
      </Row>
      <Row label={t('settings.defaultPlaybackSpeed')} hint={t('settings.defaultPlaybackSpeedHint')}>
        {({ labelId, hintId }) => (
          <input
            type="number"
            inputMode="decimal"
            aria-labelledby={labelId}
            aria-describedby={hintId}
            aria-invalid={invalid}
            min={CUSTOM_SPEED_MIN}
            max={CUSTOM_SPEED_MAX}
            step={CUSTOM_SPEED_STEP}
            value={typed ?? speed}
            onChange={(e) => {
              setTyped(e.target.value);
              const next = parseCustomSpeed(e.target.value);
              if (next !== null) onChange('default_playback_speed', next);
            }}
            onBlur={() => setTyped(null)}
            className="ui-select h-9 w-28 rounded-md bg-bg-input px-3 text-[13px] tabular-nums text-text-primary aria-[invalid=true]:ring-1 aria-[invalid=true]:ring-red-400"
          />
        )}
      </Row>
    </>
  );
};
