import { useCallback, useEffect, useMemo, useRef } from 'react';
import { useQuery } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { errorMessage } from '../components/ui/ErrorState';
import { useUIStore } from '../stores/uiStore';
import { t } from '../i18n';

/** A site that refused the app's own browser window until the user passes its check. */
export interface SiteVerificationStatus {
  /** Key to pass to `open_site_verification`. */
  site: string;
  label: string;
  origin: string;
  /** Provider keys served by this site. */
  providers: string[];
}

interface SiteVerificationEvent {
  site: string;
  label: string;
}

interface SiteVerificationOptions {
  /** True while a failed source lookup is on screen; nothing is asked or listened to otherwise. */
  enabled: boolean;
  /** Changes with every failed lookup, so the list is read again after each one. */
  attempt: number;
  /** Only the site serving this provider is offered; null offers every site that needs it. */
  provider: string | null;
  /** Called when a site answers again after the user verified. */
  onVerified: () => void;
}

const NO_SITES: SiteVerificationStatus[] = [];

/**
 * Sites the user can verify to get a failed source lookup going again. `verify` opens the
 * site in the app's own window; the lookup is retried through `onVerified` once the site
 * answers there.
 */
export function useSiteVerification({ enabled, attempt, provider, onVerified }: SiteVerificationOptions) {
  const showToast = useUIStore((s) => s.showToast);

  const statusQuery = useQuery<SiteVerificationStatus[]>({
    queryKey: ['siteVerificationStatus', attempt],
    queryFn: () => invoke<SiteVerificationStatus[]>('get_site_verification_status'),
    enabled,
    staleTime: 0,
    gcTime: 0,
    retry: false,
  });
  const { refetch: refetchStatus } = statusQuery;

  const onVerifiedRef = useRef(onVerified);
  onVerifiedRef.current = onVerified;

  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    const unlisteners: Array<() => void> = [];
    const keep = (unlisten: () => void) => {
      // The effect may have been cleaned up before the listener was registered.
      if (cancelled) unlisten();
      else unlisteners.push(unlisten);
    };

    listen<SiteVerificationEvent>('site-verified', () => onVerifiedRef.current())
      .then(keep)
      .catch((err) => console.debug('[siteVerification] listen failed:', err));
    listen<SiteVerificationEvent>('site-verification-cancelled', (event) => {
      showToast({
        type: 'info',
        title: t('useSiteVerification.notCompleted'),
        message: t('useSiteVerification.stillRefuses', { site: event.payload.label }),
      });
      refetchStatus();
    })
      .then(keep)
      .catch((err) => console.debug('[siteVerification] listen failed:', err));

    return () => {
      cancelled = true;
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, [enabled, refetchStatus, showToast]);

  const verify = useCallback(
    async (site: string) => {
      try {
        await invoke('open_site_verification', { site });
      } catch (err) {
        showToast({
          type: 'error',
          title: t('useSiteVerification.openFailed'),
          message: errorMessage(err),
        });
      }
    },
    [showToast]
  );

  const pending = enabled ? statusQuery.data ?? NO_SITES : NO_SITES;
  const sites = useMemo(() => {
    if (!provider) return pending;
    const wanted = provider.toLowerCase();
    return pending.filter((site) => site.providers.some((key) => key.toLowerCase() === wanted));
  }, [pending, provider]);

  return { sites, verify };
}
