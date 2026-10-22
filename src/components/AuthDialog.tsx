import React, { useState } from 'react';
import { Check, LogOut } from 'lucide-react';
import { Button, Modal, Spinner } from './ui';
import { useUIStore } from '../stores/uiStore';
import {
  ACCOUNT_LABELS,
  ACCOUNT_SERVICES,
  BROWSER_LOGIN_SERVICES,
  isAccountAvailable,
  useAccountEvents,
  useAccountStatus,
  useCancelAccountLogin,
  useDisconnectAccount,
  useLoginPending,
  useStartAccountLogin,
} from '../hooks/useAccounts';
import type { AccountService, AccountStatus } from '../types';
import { t } from '../i18n';

/** The line under an account's name: who is connected, or that nobody is. */
export function accountSummary(
  status: AccountStatus | undefined,
  failed = false,
  available = true,
): string {
  if (!status) return failed ? t('authDialog.couldNotRead') : t('authDialog.checking');
  if (!status.connected && !available) return t('authDialog.notAvailableSentence');
  if (!status.connected) return t('authDialog.notConnected');
  return status.name ? t('authDialog.connectedAs', { name: status.name }) : t('authDialog.connected');
}

/**
 * Connect or Disconnect for one service. Connect opens the service's own sign-in page in a
 * separate window (Comick: in the system browser); while it is waiting the button brings it
 * back to the front.
 */
export const AccountAction: React.FC<{ service: AccountService }> = ({ service }) => {
  const { data: accounts, isError, refetch } = useAccountStatus();
  const startLogin = useStartAccountLogin();
  const disconnect = useDisconnectAccount();
  const cancelLogin = useCancelAccountLogin();
  const loginPending = useLoginPending(service);
  const label = ACCOUNT_LABELS[service];
  const inBrowser = BROWSER_LOGIN_SERVICES.includes(service);

  if (!accounts) {
    return isError ? (
      <Button variant="ghost" size="sm" onClick={() => refetch()}>
        {t('common.tryAgain')}
      </Button>
    ) : (
      <Spinner size="sm" label={t('authDialog.checkingAccount', { service: label })} />
    );
  }

  if (accounts[service].connected) {
    return (
      <Button
        variant="danger"
        size="sm"
        aria-label={t('authDialog.disconnectService', { service: label })}
        onClick={() => disconnect.mutate(service)}
        disabled={disconnect.isPending}
      >
        <LogOut aria-hidden="true" className="h-4 w-4" />
        {t('authDialog.disconnect')}
      </Button>
    );
  }

  if (!isAccountAvailable(service, accounts)) {
    return <span className="text-xs text-text-muted">{t('authDialog.notAvailable')}</span>;
  }

  const connectButton = (
    <Button
      variant={loginPending ? 'secondary' : 'primary'}
      size="sm"
      aria-label={loginPending ? undefined : t('authDialog.connectService', { service: label })}
      title={
        loginPending
          ? inBrowser
            ? t('authDialog.reopenBrowser', { service: label })
            : t('authDialog.showWindow', { service: label })
          : inBrowser
            ? t('authDialog.opensInBrowser', { service: label })
            : undefined
      }
      onClick={() => startLogin(service)}
    >
      {loginPending && <Spinner size="sm" label={t('authDialog.waitingForService', { service: label })} />}
      {loginPending
        ? inBrowser
          ? t('authDialog.waitingBrowser')
          : t('authDialog.waitingSignIn')
        : t('authDialog.connect')}
    </Button>
  );

  if (!(loginPending && inBrowser)) return connectButton;
  return (
    <div className="flex items-center gap-2">
      {connectButton}
      <Button variant="ghost" size="sm" onClick={() => cancelLogin(service)}>
        {t('common.cancel')}
      </Button>
    </div>
  );
};

const AccountRow: React.FC<{
  service: AccountService;
  status: AccountStatus | undefined;
  failed: boolean;
  available: boolean;
}> = ({ service, status, failed, available }) => {
  const [failedAvatar, setFailedAvatar] = useState<string | null>(null);
  const avatar = status?.connected ? status.avatar_url : null;

  return (
    <div className="flex min-h-[56px] items-center justify-between gap-4 py-3">
      <div className="flex min-w-0 items-center gap-3">
        {avatar && avatar !== failedAvatar && (
          <img
            src={avatar}
            alt=""
            draggable={false}
            onError={() => setFailedAvatar(avatar)}
            className="h-9 w-9 flex-shrink-0 rounded-full object-cover"
          />
        )}
        <div className="flex min-w-0 flex-col">
          <span className="text-[13px] font-medium text-text-primary">{ACCOUNT_LABELS[service]}</span>
          <span
            className={`mt-0.5 flex items-center gap-1 text-xs ${
              status?.connected ? 'text-state-success' : 'text-text-muted'
            }`}
          >
            {status?.connected && <Check aria-hidden="true" className="h-3.5 w-3.5 flex-shrink-0" />}
            <span className="truncate select-text">{accountSummary(status, failed, available)}</span>
          </span>
        </div>
      </div>
      <AccountAction service={service} />
    </div>
  );
};

/** The accounts dialog, opened from the AniList tab. Settings → Account shows the same rows. */
export const AuthDialog: React.FC = () => {
  const authDialogOpen = useUIStore((s) => s.authDialogOpen);
  const setAuthDialogOpen = useUIStore((s) => s.setAuthDialogOpen);
  const { data: accounts, isError } = useAccountStatus();
  // This component is mounted for the whole life of the app, whether the dialog is open or not.
  useAccountEvents();

  return (
    <Modal isOpen={authDialogOpen} onClose={() => setAuthDialogOpen(false)} title={t('authDialog.title')} size="md">
      <p className="text-[13px] text-text-secondary">{t('authDialog.intro')}</p>
      <div className="mt-2 divide-y divide-border">
        {ACCOUNT_SERVICES.map((service) => (
          <AccountRow
            key={service}
            service={service}
            status={accounts?.[service]}
            failed={isError}
            available={isAccountAvailable(service, accounts)}
          />
        ))}
      </div>
    </Modal>
  );
};
