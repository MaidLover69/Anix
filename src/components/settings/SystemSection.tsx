import React, { useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { Cpu, FileText, FolderOpen } from 'lucide-react';
import { Button } from '../ui/Button';
import { Switch } from '../ui/Switch';
import { Spinner } from '../ui/Spinner';
import { errorMessage } from '../ui/ErrorState';
import { Row, Section } from './SettingsLayout';
import { useSaveSettings, useSettings } from '../../hooks/useSettings';
import {
  DESKTOP_STATUS_KEY,
  useDesktopStatus,
  useHardwareReport,
  useOpenLogFolder,
  useSaveDiagnosticsReport,
} from '../../hooks/useSystem';
import { useUIStore } from '../../stores/uiStore';
import type { HardwareReport } from '../../types';
import { useEmbeddedMpvStatus } from '../../hooks/useEmbeddedMpv';
import { selfTestSummary } from './PlayerEngineSettings';
import { t } from '../../i18n';

const VENDOR_LABELS: Record<string, string> = { intel: 'Intel', amd: 'AMD', nvidia: 'NVIDIA' };

const Fact: React.FC<{ label: string; value: React.ReactNode; tone?: 'ok' | 'bad' | 'plain' }> = ({
  label,
  value,
  tone = 'plain',
}) => (
  <div className="flex items-baseline justify-between gap-4 py-1.5">
    <dt className="text-xs text-text-muted">{label}</dt>
    <dd
      className={`min-w-0 break-words text-end text-xs select-text ${
        tone === 'ok' ? 'text-state-success' : tone === 'bad' ? 'text-state-error' : 'text-text-secondary'
      }`}
    >
      {value}
    </dd>
  </div>
);

const ReportView: React.FC<{ report: HardwareReport }> = ({ report }) => {
  const mpvStatus = useEmbeddedMpvStatus().data;
  const va = report.vaapi;
  const gst = report.gstreamer;
  const vaapiValue = !va.vainfo_installed
    ? t('systemSection.vaapiNotChecked')
    : va.info
      ? va.info.decode.length > 0
        ? t('systemSection.vaapiWorking', { formats: va.info.decode.join(', ') })
        : t('systemSection.vaapiNoDecoding')
      : t('systemSection.vaapiNotWorking');
  const gstValue = gst.va
    ? t(gst.bundled ? 'systemSection.gstVaFoundBundled' : 'systemSection.gstVaFound')
    : gst.vaapi
      ? t('systemSection.gstOldVaapi')
      : t('systemSection.gstNoVa');
  const unknown = t('systemSection.unknown');

  return (
    <div className="flex w-full flex-col gap-3">
      <dl className="divide-y divide-border rounded-md ui-inset bg-bg-surface px-3">
        <Fact label="VA-API" /* i18n-ignore */ value={vaapiValue} tone={va.ok ? 'ok' : va.vainfo_installed ? 'bad' : 'plain'} />
        {va.info?.driver && <Fact label={t('systemSection.vaapiDriver')} value={va.info.driver} />}
        {va.error && <Fact label={t('systemSection.vainfoSaid')} value={<span className="whitespace-pre-wrap">{va.error}</span>} />}
        <Fact label={t('systemSection.gstreamer')} value={gstValue} tone={gst.va || gst.vaapi ? 'ok' : 'bad'} />
        <Fact
          label={t('systemSection.softwareDecoders')}
          value={gst.libav ? t('systemSection.found') : t('systemSection.notFound')} tone={gst.libav ? 'plain' : 'bad'} />
        <Fact label="WebKitGTK" /* i18n-ignore */ value={report.webkitgtk_version ?? unknown} />
        <Fact
          label={t('systemSection.displayServer')}
          value={
            report.session === 'wayland' ? 'Wayland' : report.session === 'x11' ? 'X11' : unknown
          }
        />
        <Fact
          label={t('systemSection.graphics')}
          value={
            report.render_node
              ? `${VENDOR_LABELS[report.gpu_vendor ?? ''] ?? report.gpu_vendor ?? unknown} (${report.render_node})`
              : t('systemSection.noRenderDevice')
          }
          tone={report.render_node ? 'plain' : 'bad'}
        />
        <Fact
          label={t('systemSection.dmabuf')}
          value={
            report.dmabuf_disabled
              ? t(report.dmabuf_set_by_app ? 'systemSection.dmabufOffByApp' : 'systemSection.dmabufOff', {
                  value: report.dmabuf_value ?? '1',
                })
              : t('common.on')
          }
        />
        <Fact
          label={t('systemSection.mpvSelfTest')}
          value={selfTestSummary(mpvStatus?.selftest)}
          tone={mpvStatus?.selftest ? (mpvStatus.selftest.ok ? 'ok' : 'bad') : 'plain'}
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
          <summary className="cursor-pointer">{t('systemSection.allProfiles', { count: va.info.profiles.length })}</summary>
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
  const saveReport = useSaveDiagnosticsReport();
  const openLogs = useOpenLogFolder();

  const update = (changes: { minimize_to_tray?: boolean; start_with_desktop?: boolean }) => {
    save.mutate(changes, {
      onSettled: () => queryClient.invalidateQueries({ queryKey: DESKTOP_STATUS_KEY }),
      onError: (err) => showToast({ type: 'error', title: t('systemSection.couldNotSaveSetting'), message: errorMessage(err) }),
    });
  };

  const status = desktop.data;
  const trayOn = settings?.minimize_to_tray ?? false;
  const trayHint = !status
    ? t('systemSection.trayHint')
    : !status.tray_supported
      ? t('systemSection.trayUnavailable')
      : !status.tray_host && trayOn
        ? t('systemSection.trayNoHost')
        : t('systemSection.trayHint');
  const autostartHint =
    status?.autostart_installed && status.autostart_path
      ? t('systemSection.autostartHintPath', { path: status.autostart_path })
      : t('systemSection.autostartHint');

  return (
    <Section title={t('systemSection.title')} id="system">
      <Row label={t('systemSection.keepInTray')} hint={trayHint}>
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
      <Row label={t('systemSection.startWithDesktop')} hint={autostartHint}>
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
        label={t('systemSection.hardwareDecoding')}
        hint={t('systemSection.hardwareDecodingHint')}
        stacked={checkRequested}
      >
        {() => {
          // One button stays mounted through every state, so it keeps keyboard focus.
          const busy = checkRequested && (report.isPending || report.isFetching);
          return (
            <div aria-busy={busy || undefined} className={checkRequested ? 'flex w-full flex-col gap-2' : 'flex'}>
              {checkRequested && !busy && report.isError && (
                <p role="alert" className="text-xs text-state-error select-text">
                  {t('systemSection.checkFailed', { error: errorMessage(report.error) })}
                </p>
              )}
              {checkRequested && !busy && report.data && <ReportView report={report.data} />}
              <div>
                <Button
                  variant={checkRequested ? 'ghost' : 'secondary'}
                  size="sm"
                  aria-disabled={busy || undefined}
                  onClick={() => {
                    if (busy) return;
                    if (checkRequested) report.refetch();
                    else setCheckRequested(true);
                  }}
                >
                  {busy ? (
                    <Spinner size="sm" label={t('systemSection.checking')} />
                  ) : (
                    !checkRequested && <Cpu aria-hidden="true" className="h-3.5 w-3.5" />
                  )}
                  {busy
                    ? t('systemSection.checkingEllipsis')
                    : !checkRequested
                      ? t('systemSection.check')
                      : report.isError
                        ? t('common.retry')
                        : t('systemSection.checkAgain')}
                </Button>
              </div>
            </div>
          );
        }}
      </Row>
      <Row
        label={t('systemSection.diagnostics')}
        hint={t('systemSection.diagnosticsHint')}
        keywords={t('systemSection.diagnosticsKeywords')}
      >
        {() => (
          <Button
            variant="secondary"
            size="sm"
            disabled={saveReport.isPending}
            onClick={() =>
              saveReport.mutate(undefined, {
                onSuccess: (path) => path && showToast({ type: 'success', title: t('systemSection.reportSaved'), message: path }),
                onError: (err) => showToast({ type: 'error', title: t('systemSection.couldNotSaveReport'), message: errorMessage(err) }),
              })
            }
          >
            {saveReport.isPending ? <Spinner size="sm" label={t('systemSection.savingReport')} /> : <FileText aria-hidden="true" className="h-3.5 w-3.5" />}
            {t('systemSection.saveReport')}
          </Button>
        )}
      </Row>
      <Row
        label={t('systemSection.logFolder')}
        hint={t('systemSection.logFolderHint')}
        keywords={t('systemSection.logFolderKeywords')}
      >
        {() => (
          <Button
            variant="ghost"
            size="sm"
            disabled={openLogs.isPending}
            onClick={() =>
              openLogs.mutate(undefined, {
                onError: (err) => showToast({ type: 'error', title: t('systemSection.couldNotOpenLogFolder'), message: errorMessage(err) }),
              })
            }
          >
            <FolderOpen aria-hidden="true" className="h-3.5 w-3.5" />
            {t('systemSection.openLogFolder')}
          </Button>
        )}
      </Row>
    </Section>
  );
};
