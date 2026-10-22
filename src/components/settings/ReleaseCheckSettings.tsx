import React from 'react';
import { CustomSelect } from '../CustomSelect';
import { Switch } from '../ui/Switch';
import { errorMessage } from '../ui/ErrorState';
import { Row } from './SettingsLayout';
import { DEFAULT_SETTINGS, useSaveSettings, useSettings } from '../../hooks/useSettings';
import { useUIStore } from '../../stores/uiStore';
import { t } from '../../i18n';
import type { AppSettings } from '../../types';

const hourOptions = () =>
  [1, 2, 3, 6, 12].map((hours) => ({
    value: String(hours),
    label: t('releaseCheckSettings.hours', { count: hours }),
  }));

/** Settings › Notifications: how long releases are looked for, and alerts while the app is closed. */
export const ReleaseCheckSettings: React.FC = () => {
  const { data: settings } = useSettings();
  const save = useSaveSettings();
  const showToast = useUIStore((s) => s.showToast);

  const update = (changes: Partial<AppSettings>) => {
    save.mutate(changes, {
      onError: (err) => showToast({ type: 'error', title: t('releaseCheckSettings.couldNotSaveSetting'), message: errorMessage(err) }),
    });
  };

  const hours = settings?.release_check_hours ?? DEFAULT_SETTINGS.release_check_hours;
  const off = settings?.release_notifications === false;

  return (
    <>
      <Row
        label={t('releaseCheckSettings.keepLooking')}
        hint={t('releaseCheckSettings.keepLookingHint')}
      >
        {({ labelId, hintId }) => (
          <CustomSelect
            aria-labelledby={labelId}
            aria-describedby={hintId}
            className="w-36"
            value={String(hours)}
            disabled={off}
            onChange={(value) => update({ release_check_hours: Number(value) })}
            options={hourOptions()}
          />
        )}
      </Row>
      <Row
        label={t('releaseCheckSettings.alertsClosed')}
        hint={t('releaseCheckSettings.alertsClosedHint')}
      >
        {({ labelId, hintId }) => (
          <Switch
            aria-labelledby={labelId}
            aria-describedby={hintId}
            checked={settings?.background_alerts ?? false}
            disabled={off && !settings?.background_alerts}
            onChange={(checked) => update({ background_alerts: checked })}
          />
        )}
      </Row>
    </>
  );
};
