/**
 * Player › Watch together: host a party on the local network, join one, and while in one see
 * the code, the guests, the host's source and the network help. Opened from the player's top
 * bar; the party itself lives in `useWatchParty` and the backend.
 */
import React, { useEffect, useId, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useNavigate } from 'react-router-dom';
import { Minus, Plus } from 'lucide-react';
import { Modal } from '../ui/Modal';
import { Button } from '../ui/Button';
import { Spinner } from '../ui/Spinner';
import { watchPath } from '../../hooks/watchProgress';
import { NO_PARTY } from '../../hooks/useWatchParty';
import type { PartyStatus, WatchParty } from '../../hooks/useWatchParty';
import { durationDifferenceMs, normalizeCode, partyErrorKey, stepOffset } from '../../lib/watchParty';
import { isKey, t } from '../../i18n';

const MAX_GUESTS = 8;
const DEFAULT_PORT = 47830;
const INPUT_CLASS =
  'h-9 w-full rounded-md border border-border bg-bg-input px-3 text-[13px] text-text-primary placeholder:text-text-muted';

interface FoundParty {
  name: string;
  address: string;
}

interface WatchPartyDialogProps {
  isOpen: boolean;
  onClose: () => void;
  container?: HTMLElement | null;
  party: WatchParty;
  /** What this player plays, to compare with the host. */
  anilistId: number;
  episodeNumber: number;
  category: 'sub' | 'dub';
  provider: string;
  /** Seconds; 0 while unknown. */
  duration: number;
}

function errorText(error: unknown): string {
  const key = partyErrorKey(error);
  return key && isKey(key) ? t(key) : t('party.errors.unknown');
}

/** Commands the host can run when the firewall blocks guests (the app never runs them). */
function firewallLines(port: number): { distros: string; command: string }[] {
  return [
    { distros: 'Fedora, openSUSE', command: `sudo firewall-cmd --add-port=${port}/tcp` },
    { distros: 'Fedora, openSUSE', command: 'sudo firewall-cmd --add-service=mdns' },
    { distros: 'Ubuntu, Debian', command: `sudo ufw allow ${port}/tcp` },
    { distros: 'Ubuntu, Debian', command: 'sudo ufw allow 5353/udp' },
  ];
}

const NetworkHelp: React.FC<{ port: number; open?: boolean }> = ({ port, open = false }) => (
  <details open={open} className="rounded-md ui-inset bg-bg-surface px-3 py-2 text-xs text-text-secondary">
    <summary className="cursor-pointer text-[13px] font-medium text-text-primary">{t('party.helpTitle')}</summary>
    <div className="mt-2 space-y-2">
      <p>{t('party.helpAddress')}</p>
      <p>{t('party.helpFirewall')}</p>
      <ul className="space-y-1" dir="ltr">
        {firewallLines(port).map((line) => (
          <li key={line.command} className="flex flex-wrap items-baseline gap-x-2">
            <code className="select-text rounded bg-bg-input px-1.5 py-0.5 font-mono text-[12px] text-text-primary">
              {line.command}
            </code>
            <span className="text-text-muted">{line.distros}</span>
          </li>
        ))}
      </ul>
      <p>{t('party.helpPermanent')}</p>
      <p>{t('party.helpIsolation')}</p>
      <p>{t('party.helpVpn')}</p>
    </div>
  </details>
);

const HostView: React.FC<{ status: PartyStatus }> = ({ status }) => (
  <div className="space-y-4">
    <div className="rounded-md ui-inset bg-bg-surface px-4 py-3 text-center">
      <p className="text-xs text-text-muted">{t('party.codeLabel')}</p>
      <p dir="ltr" className="select-text font-mono text-3xl font-semibold tracking-[0.3em] text-text-primary">
        {status.code}
      </p>
    </div>
    <p className="text-[13px] text-text-primary" aria-live="polite">
      {t('party.guestsCount', { count: status.guests, max: MAX_GUESTS })}
      {status.waiting && <span className="ms-2 text-text-muted">{t('party.waitingForGuests')}</span>}
    </p>
    {status.locked && (
      <p role="alert" className="text-[13px] text-state-warning">
        {t('party.lockedNote')}
      </p>
    )}
    <div className="text-xs text-text-secondary">
      <p>{t('party.addressesLabel')}</p>
      <p dir="ltr" className="select-text font-mono text-[13px] text-text-primary">
        {status.addresses.map((ip) => `${ip}:${status.port}`).join('  ')}
      </p>
    </div>
    <NetworkHelp port={status.port} />
  </div>
);

const GuestView: React.FC<Omit<WatchPartyDialogProps, 'isOpen' | 'onClose' | 'container'>> = ({
  party,
  anilistId,
  episodeNumber,
  category,
  provider,
  duration,
}) => {
  const navigate = useNavigate();
  const load = party.hostLoad;
  const difference = load ? durationDifferenceMs(load.duration_ms, duration) : null;
  const canUseHostSource = !!load?.extension && load.extension !== provider;
  const offsetLabelId = useId();
  return (
    <div className="space-y-4">
      <p className="text-[13px] text-text-primary">
        {t('party.connectedTo', { address: party.status.addresses[0] ?? '' })}
        {party.status.rtt_ms !== null && (
          <span className="ms-2 text-text-muted">{t('party.roundTrip', { ms: party.status.rtt_ms })}</span>
        )}
      </p>
      {load && (load.server || load.extension) && (
        <p className="text-xs text-text-secondary">{t('party.hostSource', { name: load.server ?? load.extension ?? '' })}</p>
      )}
      {difference !== null && (
        <p role="status" className="text-[13px] text-state-warning">
          {t(difference > 0 ? 'party.longerVersion' : 'party.shorterVersion', {
            seconds: Math.round(Math.abs(difference) / 1000),
          })}
        </p>
      )}
      {canUseHostSource && load && difference !== null && (
        <Button
          variant="neutral"
          size="sm"
          onClick={() =>
            navigate(watchPath(anilistId, episodeNumber, category, load.extension), { replace: true })
          }
        >
          {t('party.useHostSource')}
        </Button>
      )}
      <div className="flex items-center justify-between gap-3 rounded-md ui-inset bg-bg-surface px-3 py-2">
        <div className="min-w-0">
          <p id={offsetLabelId} className="text-[13px] font-medium text-text-primary">
            {t('party.offset')}
          </p>
          <p className="text-xs text-text-muted">{t('party.offsetHint')}</p>
        </div>
        <div role="group" aria-labelledby={offsetLabelId} className="flex items-center gap-1" dir="ltr">
          <Button icon variant="ghost" size="sm" aria-label={t('party.offsetEarlier')} onClick={() => party.setOffsetMs(stepOffset(party.offsetMs, -1))}>
            <Minus aria-hidden="true" className="h-4 w-4" />
          </Button>
          <span className="w-16 text-center text-[13px] tabular-nums text-text-primary" aria-live="polite">
            {t('party.offsetValue', { seconds: (party.offsetMs / 1000).toFixed(1) })}
          </span>
          <Button icon variant="ghost" size="sm" aria-label={t('party.offsetLater')} onClick={() => party.setOffsetMs(stepOffset(party.offsetMs, 1))}>
            <Plus aria-hidden="true" className="h-4 w-4" />
          </Button>
        </div>
      </div>
    </div>
  );
};

export const WatchPartyDialog: React.FC<WatchPartyDialogProps> = (props) => {
  const { isOpen, onClose, container, party } = props;
  const role = party.status.role;
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [found, setFound] = useState<FoundParty[] | null>(null);
  const [address, setAddress] = useState('');
  const [code, setCode] = useState('');
  const addressId = useId();
  const codeId = useId();

  // Look for parties whenever the dialog opens outside a party.
  const [searchRound, setSearchRound] = useState(0);
  useEffect(() => {
    if (!isOpen || role !== 'none') return;
    let current = true;
    setFound(null);
    invoke<FoundParty[]>('party_discover')
      .then((list) => current && setFound(list))
      .catch(() => current && setFound([]));
    return () => {
      current = false;
    };
  }, [isOpen, role, searchRound]);
  useEffect(() => {
    if (isOpen) setError(null);
  }, [isOpen]);

  const act = async (command: string, args?: Record<string, unknown>) => {
    setBusy(true);
    setError(null);
    try {
      const status = await invoke<PartyStatus | null>(command, args);
      party.setStatus(status ?? NO_PARTY);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  const validCode = normalizeCode(code);
  const join = () => {
    if (validCode && address.trim()) void act('party_join', { address: address.trim(), code: validCode });
  };

  const footer =
    role === 'host' ? (
      <Button variant="danger" onClick={() => void act('party_leave')} disabled={busy}>
        {t('party.endParty')}
      </Button>
    ) : role === 'guest' ? (
      <>
        <Button variant="neutral" onClick={party.catchUp}>
          {t('party.catchUp')}
        </Button>
        <Button variant="danger" onClick={() => void act('party_leave')} disabled={busy}>
          {t('party.leave')}
        </Button>
      </>
    ) : undefined;

  return (
    <Modal
      isOpen={isOpen}
      onClose={onClose}
      title={
        <span className="inline-flex items-center gap-2">
          {t('party.title')}
          <span className="rounded-sm bg-accent-tint px-1.5 py-0.5 text-[11px] font-bold uppercase leading-[14px] tracking-wide text-accent-300 rtl:tracking-normal">
            {t('party.beta')}
          </span>
        </span>
      }
      // Not yet tried on two real devices (1.4.0): ask for feedback.
      header={<p className="text-xs text-text-muted">{t('party.betaNote')}</p>}
      size="md"
      container={container}
      footer={footer}
    >
      {role === 'host' ? (
        <HostView status={party.status} />
      ) : role === 'guest' ? (
        <GuestView {...props} />
      ) : (
        <div className="space-y-5">
          <p className="text-[13px] text-text-secondary">{t('party.intro')}</p>
          <section className="space-y-2">
            <h3 className="text-[13px] font-semibold text-text-primary">{t('party.hostTitle')}</h3>
            <p className="text-xs text-text-muted">{t('party.hostHint')}</p>
            <Button variant="primary" onClick={() => void act('party_host')} disabled={busy} data-autofocus>
              {t('party.hostButton')}
            </Button>
          </section>
          <section className="space-y-2">
            <div className="flex items-center justify-between gap-2">
              <h3 className="text-[13px] font-semibold text-text-primary">{t('party.joinTitle')}</h3>
              <Button variant="ghost" size="sm" onClick={() => setSearchRound((n) => n + 1)} disabled={found === null}>
                {t('party.searchAgain')}
              </Button>
            </div>
            {found === null ? (
              <p role="status" className="flex items-center gap-2 text-xs text-text-muted">
                <Spinner size="sm" />
                {t('party.searching')}
              </p>
            ) : found.length > 0 ? (
              <ul className="space-y-1">
                {found.map((item) => (
                  <li key={item.address}>
                    <button
                      type="button"
                      onClick={() => setAddress(item.address)}
                      aria-pressed={address === item.address}
                      className={`flex w-full items-center justify-between gap-2 rounded-md px-3 py-2 text-start text-[13px] cursor-pointer transition-colors ${
                        address === item.address ? 'bg-accent-tint text-accent-300' : 'bg-bg-surface text-text-primary hover:bg-ink/10'
                      }`}
                    >
                      <span className="truncate" dir="auto">
                        {item.name}
                      </span>
                      <span dir="ltr" className="font-mono text-xs text-text-muted">
                        {item.address}
                      </span>
                    </button>
                  </li>
                ))}
              </ul>
            ) : (
              <>
                <p className="text-xs text-text-muted">{t('party.nothingFound')}</p>
                <NetworkHelp port={DEFAULT_PORT} />
              </>
            )}
            <div className="grid grid-cols-[1fr_8rem] gap-2">
              <div>
                <label htmlFor={addressId} className="mb-1 block text-xs text-text-secondary">
                  {t('party.addressLabel')}
                </label>
                <input
                  id={addressId}
                  dir="ltr"
                  value={address}
                  onChange={(e) => setAddress(e.target.value)}
                  placeholder={t('party.addressPlaceholder')}
                  spellCheck={false}
                  autoComplete="off"
                  className={INPUT_CLASS}
                />
              </div>
              <div>
                <label htmlFor={codeId} className="mb-1 block text-xs text-text-secondary">
                  {t('party.codeLabel')}
                </label>
                <input
                  id={codeId}
                  dir="ltr"
                  value={code}
                  onChange={(e) => setCode(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') join();
                  }}
                  inputMode="numeric"
                  maxLength={7}
                  autoComplete="off"
                  placeholder={t('party.codePlaceholder')}
                  className={`${INPUT_CLASS} font-mono tracking-widest`}
                />
              </div>
            </div>
            <Button variant="secondary" onClick={join} disabled={busy || !validCode || !address.trim()}>
              {t('party.joinButton')}
            </Button>
          </section>
        </div>
      )}
      {error && (
        <p role="alert" className="mt-3 text-[13px] text-state-error">
          {error}
        </p>
      )}
    </Modal>
  );
};
