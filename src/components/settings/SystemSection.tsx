import React, { useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { Cpu } from 'lucide-react';
import { Button } from '../ui/Button';
import { Switch } from '../ui/Switch';
import { Spinner } from '../ui/Spinner';
import { errorMessage } from '../ui/ErrorState';
import { Row, Section } from './SettingsLayout';
import { useSaveSettings, useSettings } from '../../hooks/useSettings';
import { DESKTOP_STATUS_KEY, useDesktopStatus, useHardwareReport } from '../../hooks/useSystem';
import { useUIStore } from '../../stores/uiStore';
import type { HardwareReport } from '../../types';

const VENDOR_LABELS: Record<string, string> = { intel: 'Intel', amd: 'AMD', nvidia: 'NVIDIA' };

const Fact: React.FC<{ label: string; value: React.ReactNode; tone?: 'ok' | 'bad' | 'plain' }> = ({
  label,
  value,
  tone = 'plain',
}) => (
  <div className="flex items-baseline justify-between gap-4 py-1.5">
    <dt className="text-xs text-text-muted">{label}</dt>
    <dd
      className={`min-w-0 break-words text-right text-xs select-text ${
        tone === 'ok' ? 'text-state-success' : tone === 'bad' ? 'text-state-error' : 'text-text-secondary'
      }`}
    >
      {value}
    </dd>
  </div>
);

const ReportView: React.FC<{ report: HardwareReport }> = ({ report }) => {
  const va = report.vaapi;
  const gst = report.gstreamer;
  const vaapiValue = !va.vainfo_installed
    ? 'Not checked (vainfo is not installed)'
    : va.info
      ? va.info.decode.length > 0
        ? `Working — decodes ${va.info.decode.join(', ')}`
        : 'Loaded, but no video decoding'
      : 'Not working';
  const gstValue = gst.va
    ? `VA plugin found${gst.bundled ? ' (bundled with the AppImage)' : ''}`
    : gst.vaapi
      ? 'Only the older vaapi plugin found'
      : 'No VA plugin found';

  return (
    <div className="flex w-full flex-col gap-3">
      <dl className="divide-y divide-border rounded-md ui-inset bg-bg-surface px-3">
        <Fact label="VA-API" value={vaapiValue} tone={va.ok ? 'ok' : va.vainfo_installed ? 'bad' : 'plain'} />
        {va.info?.driver && <Fact label="VA-API driver" value={va.info.driver} />}
        {va.error && <Fact label="vainfo said" value={<span className="whitespace-pre-wrap">{va.error}</span>} />}
        <Fact label="GStreamer (built-in player)" value={gstValue} tone={gst.va || gst.vaapi ? 'ok' : 'bad'} />
        <Fact label="Software decoders (libav)" value={gst.libav ? 'Found' : 'Not found'} tone={gst.libav ? 'plain' : 'bad'} />
        <Fact label="WebKitGTK" value={report.webkitgtk_version ?? 'Unknown'} />
        <Fact
          label="Display server"
          value={
            report.session === 'wayland' ? 'Wayland' : report.session === 'x11' ? 'X11' : 'Unknown'
          }
        />
        <Fact
          label="Graphics"
          value={
            report.render_node
              ? `${VENDOR_LABELS[report.gpu_vendor ?? ''] ?? report.gpu_vendor ?? 'Unknown'} (${report.render_node})`
              : 'No render device'
          }
          tone={report.render_node ? 'plain' : 'bad'}
        />
        <Fact
          label="DMA-BUF renderer"
          value={
            report.dmabuf_disabled
              ? `Off (WEBKIT_DISABLE_DMABUF_RENDERER=${report.dmabuf_value ?? '1'}${
                  report.dmabuf_set_by_app ? ', set by Anix' : ''
                })`
              : 'On'
          }
        />
      </dl>
      <ul className="flex flex-col gap-1.5">
        {report.tips.map((tip) => (
          <li key={tip} className="rounded-md bg-ink/[0.04] px-3 py-2 text-xs leading-[18px] text-text-secondary select-text">
            {tip}
          </li>
        ))}
      </ul>
      {va.info && va.info.profiles.length > 0 && (
        <details className="text-xs text-text-muted">
          <summary className="cursor-pointer">All VA-API profiles ({va.info.profiles.length})</summary>
          <pre className="mt-2 max-h-48 overflow-auto whitespace-pre-wrap rounded-md bg-bg-surface p-2 text-[11px] select-text">
            {va.info.profiles.join('\n')}
          </pre>
        </details>
      )}
    </div>
  );
};

/** Settings › System: tray and background mode, autostart, and the hardware check. */
export const SystemSection: React.FC = () => {
  const queryClient = useQueryClient();
  const showToast = useUIStore((s) => s.showToast);
  const { data: settings } = useSettings();
  const save = useSaveSettings();
  const desktop = useDesktopStatus();
  const [checkRequested, setCheckRequested] = useState(false);
  const report = useHardwareReport(checkRequested);

  const update = (changes: { minimize_to_tray?: boolean; start_with_desktop?: boolean }) => {
    save.mutate(changes, {
      onSettled: () => queryClient.invalidateQueries({ queryKey: DESKTOP_STATUS_KEY }),
      onError: (err) => showToast({ type: 'error', title: 'Could not save the setting', message: errorMessage(err) }),
    });
  };

  const status = desktop.data;
  const trayOn = settings?.minimize_to_tray ?? false;
  const trayHint = !status
    ? 'Closing the window hides Anix in the tray; release notifications keep coming.'
    : !status.tray_supported
      ? 'Not available: the AppIndicator library (libayatana-appindicator3) is not installed.'
      : !status.tray_host && trayOn
        ? 'Your desktop shows no tray right now (on GNOME, install the AppIndicator extension), so closing the window still quits.'
        : 'Closing the window hides Anix in the tray; release notifications keep coming.';
  const autostartHint =
    status?.autostart_installed && status.autostart_path
      ? `Starts hidden in the tray at login (${status.autostart_path}).`
      : 'Starts Anix hidden in the tray when you log in. Needs the tray setting.';

  return (
    <Section title="System">
      <Row label="Keep running in the tray when the window is closed" hint={trayHint}>
        {({ labelId, hintId }) => (
          <Switch
            aria-labelledby={labelId}
            aria-describedby={hintId}
            checked={trayOn}
            disabled={!settings || status?.tray_supported === false}
            onChange={(checked) => update(checked ? { minimize_to_tray: true } : { minimize_to_tray: false, start_with_desktop: false })}
          />
        )}
      </Row>
      <Row label="Start with the desktop session (minimised to tray)" hint={autostartHint}>
        {({ labelId, hintId }) => (
          <Switch
            aria-labelledby={labelId}
            aria-describedby={hintId}
            checked={(settings?.start_with_desktop ?? false) && trayOn}
            disabled={!settings || !trayOn}
            onChange={(checked) => update({ start_with_desktop: checked })}
          />
        )}
      </Row>
      <Row
        label="Hardware video decoding"
        hint="Checks VA-API, the GStreamer plugins the built-in player uses and the display setup. Nothing is changed."
        stacked={checkRequested}
      >
        {() =>
          !checkRequested ? (
            <Button variant="secondary" size="sm" onClick={() => setCheckRequested(true)}>
              <Cpu aria-hidden="true" className="h-3.5 w-3.5" />
              Check
            </Button>
          ) : report.isPending || report.isFetching ? (
            <div aria-busy="true" className="flex items-center gap-2 text-xs text-text-muted">
              <Spinner size="sm" label="Checking" />
              Checking…
            </div>
          ) : report.isError ? (
            <div className="flex w-full items-center justify-between gap-4">
              <p role="alert" className="text-xs text-state-error select-text">
                The check failed: {errorMessage(report.error)}
              </p>
              <Button variant="ghost" size="sm" onClick={() => report.refetch()}>
                Retry
              </Button>
            </div>
          ) : report.data ? (
            <div className="flex w-full flex-col gap-2">
              <ReportView report={report.data} />
              <div>
                <Button variant="ghost" size="sm" onClick={() => report.refetch()}>
                  Check again
                </Button>
              </div>
            </div>
          ) : null
        }
      </Row>
    </Section>
  );
};
