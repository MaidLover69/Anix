import React, { useId, useState } from 'react';
import { Puzzle, RefreshCw, Trash2 } from 'lucide-react';
import { useQueryClient } from '@tanstack/react-query';
import { CustomSelect } from '../components/CustomSelect';
import { ThemePicker } from '../components/ThemePicker';
import { AppearanceExtras } from '../components/settings/AppearanceExtras';
import { HomeRowOrder } from '../components/settings/HomeRowOrder';
import { Row, Section, SettingsShell } from '../components/settings/SettingsLayout';
import { DownloadSettings } from '../components/settings/DownloadSettings';
import { SystemSection } from '../components/settings/SystemSection';
import { BackupSection } from '../components/settings/BackupSection';
import { ReleaseCheckSettings } from '../components/settings/ReleaseCheckSettings';
import { LanguagePreferences } from '../components/settings/LanguagePreferences';
import { AppLanguageRow } from '../components/setup/AppLanguage';
import { audioOnlyLabel, pluginLanguageSummary } from '../lib/languages';
import { HealthBadge, PluginDirectory, UpdateBadge } from '../components/settings/PluginTools';
import { PluginConfigDialog } from '../components/settings/PluginConfigDialog';
import { ServerOrder } from '../components/settings/ServerOrder';
import { usePluginHealth, usePluginUpdates, PLUGIN_UPDATES_KEY, type HealthState } from '../hooks/usePluginHealth';
import { useTheme } from '../hooks/useTheme';
import { useSiteVerification } from '../hooks/useSiteVerification';
import { Button } from '../components/ui/Button';
import { Switch } from '../components/ui/Switch';
import { Modal } from '../components/ui/Modal';
import { EmptyState } from '../components/ui/EmptyState';
import { SegmentedControl } from '../components/ui/SegmentedControl';
import { Spinner } from '../components/ui/Spinner';
import { ErrorState, errorMessage } from '../components/ui/ErrorState';
import {
  useSettings,
  useSaveSettings,
  useCheckUpdate,
  useAppVersion,
  AUTO_PROVIDER,
} from '../hooks/useSettings';
import {
  useAddPluginRepo,
  usePluginRepos,
  usePlugins,
  useRefreshPluginRepos,
  useRemovePluginRepo,
  useSetPluginEnabled,
  useStreamingProviders,
} from '../hooks/usePlugins';
import { invalidateAdultFilteredQueries } from '../hooks/useAniList';
import { ACCOUNT_LABELS, ACCOUNT_SERVICES, isAccountAvailable, useAccountStatus } from '../hooks/useAccounts';
import { AccountAction, accountSummary } from '../components/AuthDialog';
import { useUIStore } from '../stores/uiStore';
import type { AppSettings, CatalogueSource, DohMode, DohProvider, ExternalPlayer, PluginInfo, PluginRepo, PluginUpdate } from '../types';
import { PlayerEngineSettings } from '../components/settings/PlayerEngineSettings';
import { locale, t } from '../i18n';

// Episodes always open in the built-in player; this picks the program used outside it.
const PLAYER_OPTIONS: { value: ExternalPlayer; label: string }[] = [
  { value: 'mpv', label: 'mpv' },
  { value: 'vlc', label: 'VLC' },
];

// Functions, not constants: the labels follow the interface language.
const dohModeOptions = (): { value: DohMode; label: string }[] => [
  { value: 'auto', label: t('settings.dohAutomatic') },
  { value: 'on', label: t('common.on') },
  { value: 'off', label: t('common.off') },
];

const DOH_OPTIONS: { value: DohProvider; label: string }[] = [
  { value: 'cloudflare', label: 'Cloudflare (1.1.1.1)' },
  { value: 'google', label: 'Google (8.8.8.8)' },
  { value: 'adguard', label: 'AdGuard DNS' },
];

const catalogueOptions = (): { value: CatalogueSource; label: string }[] => [
  { value: 'auto', label: t('settings.catalogueAuto') },
  { value: 'anilist', label: t('settings.preferAniList') },
  { value: 'myanimelist', label: 'MyAnimeList' },
  { value: 'kitsu', label: 'Kitsu' },
  { value: 'shikimori', label: 'Shikimori' },
  { value: 'comick', label: 'Comick' },
];

function isOneOf<T extends string>(value: unknown, options: { value: T }[]): value is T {
  return options.some((o) => o.value === value);
}

/** The link to send to `add_plugin_repo`, or why what was typed cannot be one. */
function parseRepoLink(input: string): { url: string } | { problem: string } {
  const text = input.trim();
  if (!text) return { problem: t('settings.pasteLinkFirst') };
  const url = /^[a-z][a-z0-9+.-]*:\/\//i.test(text) ? text : `https://${text}`;
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return { problem: t('settings.notALink') };
  }
  if (parsed.protocol !== 'https:' && parsed.protocol !== 'http:') {
    return { problem: t('settings.mustStartHttps') };
  }
  if (!parsed.hostname.includes('.') || /\s/.test(text)) {
    return { problem: t('settings.notALink') };
  }
  const isGitHub = /^(www\.)?github\.com$/i.test(parsed.hostname);
  if (isGitHub && parsed.pathname.split('/').filter(Boolean).length < 2) {
    return { problem: t('settings.githubNeedsRepo') };
  }
  return { url };
}

/** Two spellings of the same repository link compare equal. */
function repoLinkKey(url: string): string {
  return url
    .trim()
    .toLowerCase()
    .replace(/^https?:\/\/(www\.)?/, '')
    .replace(/\.git$/, '')
    .replace(/\/+$/, '');
}

function pluginCountLabel(count: number): string {
  return t('settings.pluginCount', { count });
}

function updatedLabel(updatedAtMs: number): string {
  if (!Number.isFinite(updatedAtMs) || updatedAtMs <= 0) return t('settings.notDownloadedYet');
  return t('settings.updatedAt', {
    date: new Date(updatedAtMs).toLocaleString(locale(), {
      dateStyle: 'medium',
      timeStyle: 'short',
    }),
  });
}

const PluginIcon: React.FC<{ src: string | null }> = ({ src }) => {
  const [failedSrc, setFailedSrc] = useState<string | null>(null);
  return (
    <span className="flex h-9 w-9 flex-shrink-0 items-center justify-center overflow-hidden rounded-md bg-ink/[0.06] text-text-muted">
      {src && failedSrc !== src ? (
        <img
          src={src}
          alt=""
          loading="lazy"
          referrerPolicy="no-referrer"
          onError={() => setFailedSrc(src)}
          className="h-full w-full object-cover"
        />
      ) : (
        <Puzzle aria-hidden="true" className="h-4 w-4" />
      )}
    </span>
  );
};

/**
 * "Verify" for a plugin whose test hit a site check. Mounted per result, so each new test
 * reads the list of blocked sites again; shows nothing when the site is not listed.
 */
const PluginVerifyButton: React.FC<{ provider: string; name: string; onVerified: () => void }> = ({
  provider,
  name,
  onVerified,
}) => {
  const { sites, verify } = useSiteVerification({ enabled: true, attempt: 0, provider, onVerified });
  const site = sites[0];
  if (!site) return null;
  return (
    <Button
      variant="secondary"
      size="sm"
      className="mt-0.5"
      aria-label={t('settings.verifyAt', { name, site: site.label })}
      onClick={() => verify(site.site)}
    >
      {t('settings.verify')}
    </Button>
  );
};

const PluginRow: React.FC<{
  plugin: PluginInfo;
  onToggle: (plugin: PluginInfo, enabled: boolean) => void;
  health?: HealthState;
  update?: PluginUpdate;
  onTest?: (plugin: PluginInfo) => void;
  onConfigure?: (plugin: PluginInfo) => void;
}> = ({ plugin, onToggle, health, update, onTest, onConfigure }) => {
  const nameId = useId();
  const name = plugin.name.trim() || plugin.id;
  const version = plugin.version.trim();
  const meta = [
    version ? (/^v/i.test(version) ? version : `v${version}`) : null,
    plugin.author.trim() ? t('settings.byAuthor', { author: plugin.author.trim() }) : null,
  ]
    .filter(Boolean)
    .join(' · ');
  const languages = pluginLanguageSummary(plugin);
  const only = audioOnlyLabel(plugin);
  const failed = plugin.status === 'error';

  return (
    <li className="rounded-md ui-inset bg-bg-surface px-3 py-2.5">
      <div className="flex items-start gap-3">
        <PluginIcon src={plugin.icon} />
        <div className="min-w-0 flex-1">
          <p className="flex flex-wrap items-baseline gap-x-2">
            <span id={nameId} className="text-[13px] font-medium text-text-primary select-text">
              {name}
            </span>
            {meta && <span className="text-xs text-text-muted select-text">{meta}</span>}
            <UpdateBadge update={update} name={name} />
            <HealthBadge state={health} />
          </p>
          {plugin.description.trim() && (
            <p className="mt-0.5 line-clamp-2 text-xs leading-[18px] text-text-muted select-text">
              {plugin.description.trim()}
            </p>
          )}
          {(languages || only) && (
            <p className="mt-0.5 text-xs text-text-secondary select-text">
              {[languages, only].filter(Boolean).join(' · ')}
            </p>
          )}
        </div>
        {onTest && plugin.enabled && health && health !== 'testing' && health.status === 'verification' && (
          <PluginVerifyButton provider={plugin.id} name={name} onVerified={() => onTest(plugin)} />
        )}
        {onConfigure && plugin.configurable && (
          <Button
            variant="ghost"
            size="sm"
            className="mt-0.5"
            aria-label={t('settings.settingsOf', { name })}
            onClick={() => onConfigure(plugin)}
          >
            {t('common.settings')}
          </Button>
        )}
        {onTest && plugin.enabled && (
          <Button
            variant="ghost"
            size="sm"
            className="mt-0.5"
            aria-label={t('settings.testName', { name })}
            disabled={health === 'testing'}
            onClick={() => onTest(plugin)}
          >
            {t('settings.test')}
          </Button>
        )}
        <Switch
          aria-labelledby={nameId}
          className="mt-1.5"
          checked={plugin.enabled}
          onChange={(checked) => onToggle(plugin, checked)}
        />
      </div>
      {failed && (
        <div role="alert" className="mt-2 rounded-md bg-state-error/10 px-3 py-2">
          <p className="text-xs font-medium text-state-error">
            {t(plugin.enabled ? 'settings.loadFailedNotUsed' : 'settings.loadFailed')}
          </p>
          <p className="mt-0.5 max-h-24 overflow-y-auto whitespace-pre-wrap break-words text-xs text-text-secondary select-text">
            {plugin.error?.trim() || t('settings.noReason')}
          </p>
        </div>
      )}
      {health && health !== 'testing' && health.error && health.status !== 'ok' && (
        <p className="mt-2 break-words text-xs text-text-muted select-text">{t('settings.testResult', { error: health.error })}</p>
      )}
    </li>
  );
};

/** Settings → Plugins: the repositories streaming plugins are downloaded from, and their plugins. */
const PluginsSection: React.FC = () => {
  const showToast = useUIStore((s) => s.showToast);
  const reposQuery = usePluginRepos();
  const pluginsQuery = usePlugins();
  const repos = reposQuery.data ?? [];
  const plugins = pluginsQuery.data ?? [];

  const addRepo = useAddPluginRepo();
  const removeRepo = useRemovePluginRepo();
  const refreshRepos = useRefreshPluginRepos();
  const setPluginEnabled = useSetPluginEnabled();
  const queryClient = useQueryClient();
  const pluginHealth = usePluginHealth();
  const updatesQuery = usePluginUpdates(repos.length > 0);
  const updates = new Map((updatesQuery.data ?? []).map((u) => [u.id, u]));

  const fieldId = useId();
  const [link, setLink] = useState('');
  const [linkError, setLinkError] = useState<string | null>(null);
  const [repoToRemove, setRepoToRemove] = useState<PluginRepo | null>(null);
  const [configuring, setConfiguring] = useState<PluginInfo | null>(null);
  const settings = useSettings().data;
  const saveSettings = useSaveSettings();

  const handleAdd = (event: React.FormEvent) => {
    event.preventDefault();
    if (addRepo.isPending) return;
    const parsed = parseRepoLink(link);
    if ('problem' in parsed) {
      setLinkError(parsed.problem);
      return;
    }
    if (repos.some((repo) => repoLinkKey(repo.url) === repoLinkKey(parsed.url))) {
      setLinkError(t('settings.alreadyAdded'));
      return;
    }
    setLinkError(null);
    addRepo.mutate(parsed.url, {
      onSuccess: (repo) => {
        setLink('');
        const name = repo.name?.trim();
        if (repo.error) {
          showToast({
            type: 'warning',
            title: name ? t('settings.addedNameWithProblem', { name }) : t('settings.addedRepoWithProblem'),
            message: repo.error,
          });
        } else {
          showToast({
            type: 'success',
            title: name ? t('settings.addedName', { name }) : t('settings.addedRepo'),
            message: t('settings.pluginsInstalled', { count: repo.plugin_count }),
          });
        }
      },
      onError: (err) => setLinkError(errorMessage(err, t('settings.couldNotAddRepo'))),
    });
  };

  const handleRefresh = () => {
    refreshRepos.mutate(undefined, {
      onSuccess: (updated) => {
        queryClient.invalidateQueries({ queryKey: PLUGIN_UPDATES_KEY });
        const failed = updated.filter((repo) => repo.error).length;
        if (failed > 0) {
          showToast({
            type: 'warning',
            title: t('settings.reposNotRefreshed', { count: failed }),
            message: t('settings.reasonShown'),
          });
        } else {
          showToast({ type: 'success', title: t('settings.pluginsUpToDate') });
        }
      },
      onError: (err) =>
        showToast({ type: 'error', title: t('settings.couldNotRefresh'), message: errorMessage(err) }),
    });
  };

  const handleToggle = (plugin: PluginInfo, enabled: boolean) => {
    setPluginEnabled.mutate(
      { id: plugin.id, enabled },
      {
        onError: (err) =>
          showToast({
            type: 'error',
            title: t(enabled ? 'settings.couldNotTurnOn' : 'settings.couldNotTurnOff', { name: plugin.name.trim() || plugin.id }),
            message: errorMessage(err),
          }),
      }
    );
  };

  const handleRemove = () => {
    if (!repoToRemove || removeRepo.isPending) return;
    const name = repoToRemove.name?.trim() || repoToRemove.url;
    removeRepo.mutate(repoToRemove.url, {
      onSuccess: () => {
        setRepoToRemove(null);
        showToast({ type: 'info', title: t('settings.removedName', { name }) });
      },
      onError: (err) =>
        showToast({ type: 'error', title: t('settings.couldNotRemove', { name }), message: errorMessage(err) }),
    });
  };

  const repoUrls = new Set(repos.map((repo) => repo.url));
  // Plugins whose repository is not in the list are still shown, so none is out of reach.
  const strayPlugins = plugins.filter((plugin) => !repoUrls.has(plugin.repo_url));
  const loading = reposQuery.isPending || pluginsQuery.isPending;
  const nothingAdded = repos.length === 0 && plugins.length === 0;

  const pluginList = (list: PluginInfo[]) => (
    <ul className="mt-3 flex flex-col gap-2">
      {list.map((plugin) => (
        <PluginRow
          key={plugin.id}
          plugin={plugin}
          onToggle={handleToggle}
          health={pluginHealth.results[plugin.id]}
          update={updates.get(plugin.id)}
          onTest={(p) => pluginHealth.test(p.id)}
          onConfigure={setConfiguring}
        />
      ))}
    </ul>
  );

  return (
    <Section title={t('settingsLayout.plugins')} id="plugins">
      <form onSubmit={handleAdd} noValidate className="py-3">
        <label htmlFor={fieldId} className="text-[13px] font-medium text-text-primary">
          {t('settings.addRepo')}
        </label>
        <div className="mt-2 flex items-center gap-2">
          <input
            id={fieldId}
            type="url"
            inputMode="url"
            autoComplete="off"
            spellCheck={false}
            placeholder={t('settings.repoPlaceholder')}
            value={link}
            onChange={(e) => {
              setLink(e.target.value);
              if (linkError) setLinkError(null);
            }}
            disabled={addRepo.isPending}
            aria-invalid={linkError ? true : undefined}
            aria-describedby={`${fieldId}-help`}
            className="h-9 min-w-0 flex-1 rounded-md border border-border bg-bg-input px-3 text-[13px] text-text-primary placeholder:text-text-muted select-text disabled:opacity-60"
          />
          <Button type="submit" variant="primary" disabled={addRepo.isPending || !link.trim()}>
            {addRepo.isPending && <Spinner size="sm" label={t('settings.addingRepo')} />}
            {addRepo.isPending ? t('settings.adding') : t('common.add')}
          </Button>
        </div>
        <div id={`${fieldId}-help`} className="mt-1.5 text-xs">
          {linkError ? (
            <p role="alert" className="break-words text-state-error select-text">
              {linkError}
            </p>
          ) : addRepo.isPending ? (
            <p className="text-text-muted">{t('settings.downloadingRepo')}</p>
          ) : (
            <p className="text-text-muted">
              {t('settings.repoLinkHelp')}
            </p>
          )}
        </div>
      </form>

      {loading ? (
        <div aria-busy="true" className="flex items-center gap-2 py-4 text-xs text-text-muted">
          <Spinner size="sm" label={t('settings.loadingPlugins')} />
          {t('settings.loadingPluginsEllipsis')}
        </div>
      ) : reposQuery.isError && nothingAdded ? (
        <ErrorState
          compact
          title={t('settings.reposLoadFailed')}
          error={reposQuery.error ?? undefined}
          onRetry={() => {
            reposQuery.refetch();
            pluginsQuery.refetch();
          }}
        />
      ) : nothingAdded ? (
        <>
        <EmptyState
          className="py-10"
          icon={<Puzzle aria-hidden="true" />}
          title={t('settings.noPlugins')}
          description={t('settings.noPluginsHint')}
        />
        <PluginDirectory repos={repos} />
        </>
      ) : (
        <>
          <Row
            label={t('settings.repositories')}
            hint={`${t('settings.repositoryCount', { count: repos.length })}, ${pluginCountLabel(plugins.length)}`}
          >
            {() => (
              <div className="flex items-center gap-2">
              <Button
                variant="ghost"
                size="sm"
                onClick={() => pluginHealth.testAll(plugins.filter((p) => p.enabled).map((p) => p.id))}
                disabled={pluginHealth.testingAll || !plugins.some((p) => p.enabled)}
              >
                {pluginHealth.testingAll && <Spinner size="sm" label={t('settings.testingPlugins')} />}
                {t('settings.testAll')}
              </Button>
              <Button
                variant="secondary"
                size="sm"
                onClick={handleRefresh}
                disabled={refreshRepos.isPending || repos.length === 0}
              >
                {refreshRepos.isPending ? (
                  <Spinner size="sm" label={t('settings.refreshingPlugins')} />
                ) : (
                  <RefreshCw aria-hidden="true" className="h-3.5 w-3.5" />
                )}
                {t('settings.refreshAll')}
              </Button>
              </div>
            )}
          </Row>

          <Row
            label={t('settings.backgroundChecks')}
            hint={t('settings.backgroundChecksHint')}
          >
            {({ labelId, hintId }) => (
              <Switch
                aria-labelledby={labelId}
                aria-describedby={hintId}
                checked={settings?.plugin_health_checks !== false}
                onChange={(checked) =>
                  saveSettings.mutate(
                    { plugin_health_checks: checked },
                    {
                      onError: (err) =>
                        showToast({ type: 'error', title: t('common.couldNotSaveSettings'), message: errorMessage(err) }),
                    }
                  )
                }
              />
            )}
          </Row>

          {pluginsQuery.isError && (
            <div role="alert" className="flex items-center justify-between gap-4 py-3">
              <p className="min-w-0 break-words text-xs text-state-error select-text">
                {t('settings.pluginListFailed', { error: errorMessage(pluginsQuery.error) })}
              </p>
              <Button variant="ghost" size="sm" onClick={() => pluginsQuery.refetch()}>
                {t('common.retry')}
              </Button>
            </div>
          )}

          {repos.map((repo) => {
            const own = plugins.filter((plugin) => plugin.repo_url === repo.url);
            return (
              <div key={repo.url} className="py-3">
                <div className="flex items-start justify-between gap-4">
                  <div className="min-w-0">
                    <p className="truncate text-[13px] font-medium text-text-primary select-text">
                      {repo.name?.trim() || repo.url}
                    </p>
                    <p className="mt-0.5 truncate text-xs text-text-muted select-text">{repo.url}</p>
                    <p className="mt-0.5 text-xs text-text-muted">
                      {pluginCountLabel(repo.plugin_count)} · {updatedLabel(repo.updated_at_ms)}
                    </p>
                  </div>
                  <Button
                    variant="ghost"
                    size="sm"
                    aria-label={t('settings.removeName', { name: repo.name?.trim() || repo.url })}
                    onClick={() => setRepoToRemove(repo)}
                  >
                    <Trash2 aria-hidden="true" className="h-3.5 w-3.5" />
                    {t('common.remove')}
                  </Button>
                </div>
                {repo.error && (
                  <p
                    role="alert"
                    className="mt-2 whitespace-pre-wrap break-words rounded-md bg-state-error/10 px-3 py-2 text-xs text-state-error select-text"
                  >
                    {repo.error}
                  </p>
                )}
                {own.length > 0
                  ? pluginList(own)
                  : !repo.error &&
                    !pluginsQuery.isError && (
                      <p className="mt-2 text-xs text-text-muted">
                        {t('settings.noStreamingPlugins')}
                      </p>
                    )}
              </div>
            );
          })}

          <PluginDirectory repos={repos} />

          {strayPlugins.length > 0 && (
            <div className="py-3">
              <p className="text-[13px] font-medium text-text-primary">{t('settings.otherPlugins')}</p>
              <p className="mt-0.5 text-xs text-text-muted">{t('settings.otherPluginsHint')}</p>
              {pluginList(strayPlugins)}
            </div>
          )}
        </>
      )}

      <PluginConfigDialog plugin={configuring} onClose={() => setConfiguring(null)} />

      <Modal
        isOpen={repoToRemove !== null}
        onClose={() => {
          if (!removeRepo.isPending) setRepoToRemove(null);
        }}
        title={t('settings.removeRepoTitle')}
        size="md"
        footer={
          <>
            <Button variant="ghost" onClick={() => setRepoToRemove(null)} disabled={removeRepo.isPending}>
              {t('common.cancel')}
            </Button>
            <Button variant="danger" onClick={handleRemove} disabled={removeRepo.isPending}>
              {removeRepo.isPending && <Spinner size="sm" label={t('settings.removingRepo')} />}
              {t('common.remove')}
            </Button>
          </>
        }
      >
        {repoToRemove && (
          <div className="flex flex-col gap-2 text-[13px] text-text-secondary">
            <p className="break-words font-medium text-text-primary select-text">
              {repoToRemove.name?.trim() || repoToRemove.url}
            </p>
            <p>
              {repoToRemove.plugin_count > 0
                ? t('settings.removeRepoPlugins', { count: repoToRemove.plugin_count })
                : t('settings.removeRepoNoPlugins')}{' '}
              {t('settings.addAgain')}
            </p>
          </div>
        )}
      </Modal>
    </Section>
  );
};

export const Settings: React.FC = () => {
  const queryClient = useQueryClient();
  const providers = useStreamingProviders();
  const settingsQuery = useSettings();
  const settings = settingsQuery.data;
  const saveSettingsMutation = useSaveSettings();

  const setUpdateDialogOpen = useUIStore((s) => s.setUpdateDialogOpen);
  const showToast = useUIStore((s) => s.showToast);

  const { refetch: checkUpdateNow } = useCheckUpdate();
  const { data: appVersion } = useAppVersion();
  const [isCheckingUpdate, setIsCheckingUpdate] = useState(false);

  const accountsQuery = useAccountStatus();
  const { theme, setTheme } = useTheme();

  if (settingsQuery.isLoading) {
    return (
      <div aria-busy="true" className="flex max-w-2xl flex-col gap-8 pb-16">
        <div className="h-7 w-32 rounded-md shimmer-placeholder" />
        {Array.from({ length: 3 }).map((_, i) => (
          <div key={i} className="h-40 rounded-xl shimmer-placeholder" />
        ))}
      </div>
    );
  }

  if (!settings) {
    return (
      <ErrorState
        title={t('settings.couldNotLoad')}
        error={settingsQuery.error ?? undefined}
        onRetry={() => settingsQuery.refetch()}
      />
    );
  }

  const updateSetting = <K extends keyof AppSettings>(
    key: K,
    value: AppSettings[K],
    onSaved?: () => void
  ) => {
    saveSettingsMutation.mutate({ [key]: value } as Partial<AppSettings>, {
      onSuccess: onSaved,
      onError: (err) =>
        showToast({
          type: 'error',
          title: t('common.couldNotSaveSettings'),
          message: errorMessage(err),
        }),
    });
  };

  const handleCheckUpdate = async () => {
    setIsCheckingUpdate(true);
    try {
      const result = await checkUpdateNow();
      if (result.isError) {
        showToast({
          type: 'error',
          title: t('settings.couldNotCheckUpdates'),
          message: errorMessage(result.error),
        });
      } else if (result.data) {
        setUpdateDialogOpen(true);
      } else {
        showToast({
          type: 'info',
          title: t('settings.noNewerRelease'),
          message: appVersion ? t('settings.runningVersion', { version: appVersion }) : undefined,
        });
      }
    } finally {
      setIsCheckingUpdate(false);
    }
  };

  // A stored key that is not an enabled plugin (removed, switched off, or a name from an
  // older build) behaves as Auto, so it is shown as Auto.
  const preferredProvider = isOneOf(settings.preferred_provider, providers.options)
    ? settings.preferred_provider
    : AUTO_PROVIDER;
  // Older settings files hold 'builtin', which has always launched mpv.
  const externalPlayer = settings.external_player === 'vlc' ? 'vlc' : 'mpv';
  const dohProvider = isOneOf(settings.doh_provider, DOH_OPTIONS) ? settings.doh_provider : 'cloudflare';
  const dohMode = isOneOf(settings.doh_mode, dohModeOptions()) ? settings.doh_mode : 'auto';
  const catalogueSource = isOneOf(settings.catalogue_source, catalogueOptions()) ? settings.catalogue_source : 'auto';

  return (
    <SettingsShell>
        <Section title={t('settingsLayout.appearance')} id="appearance">
          <AppLanguageRow />
          <Row label={t('settings.theme')} hint={t('settings.themeHint')} stacked>
            {({ labelId, hintId }) => (
              <ThemePicker
                aria-labelledby={labelId}
                aria-describedby={hintId}
                value={theme}
                onChange={(next) =>
                  setTheme(next, (err) =>
                    showToast({
                      type: 'error',
                      title: t('common.couldNotSaveSettings'),
                      message: errorMessage(err),
                    })
                  )
                }
              />
            )}
          </Row>
          <AppearanceExtras settings={settings} onChange={(key, value) => updateSetting(key, value)} />
          <HomeRowOrder value={settings.home_row_order} onChange={(order) => updateSetting('home_row_order', order)} />
        </Section>

        <Section title={t('settingsLayout.playback')} id="playback">
          <Row label={t('settings.autoplay')} hint={t('settings.autoplayHint')}>
            {({ labelId, hintId }) => (
              <Switch
                aria-labelledby={labelId}
                aria-describedby={hintId}
                checked={settings.autoplay}
                onChange={(checked) => updateSetting('autoplay', checked)}
              />
            )}
          </Row>
          <Row label={t('settings.skipIntros')} hint={t('settings.skipIntrosHint')}>
            {({ labelId, hintId }) => (
              <Switch
                aria-labelledby={labelId}
                aria-describedby={hintId}
                checked={settings.skip_intro}
                onChange={(checked) => updateSetting('skip_intro', checked)}
              />
            )}
          </Row>
          <Row label={t('settings.skipEndings')} hint={t('settings.skipEndingsHint')}>
            {({ labelId, hintId }) => (
              <Switch
                aria-labelledby={labelId}
                aria-describedby={hintId}
                checked={settings.skip_outro}
                onChange={(checked) => updateSetting('skip_outro', checked)}
              />
            )}
          </Row>
          <Row label={t('settings.preferredAudio')} hint={t('settings.preferredAudioHint')}>
            {() => (
              <SegmentedControl
                aria-label={t('settings.preferredAudio')}
                value={settings.prefer_dub ? 'dub' : 'sub'}
                onChange={(value) => updateSetting('prefer_dub', value === 'dub')}
                options={[
                  { value: 'sub', label: t('common.sub') },
                  { value: 'dub', label: t('common.dub') },
                ]}
              />
            )}
          </Row>
          <Row
            label={t('settings.externalPlayer')}
            hint={t('settings.externalPlayerHint')}
          >
            {({ labelId, hintId }) => (
              <CustomSelect
                aria-labelledby={labelId}
                aria-describedby={hintId}
                className="w-52"
                value={externalPlayer}
                onChange={(value) => {
                  if (isOneOf(value, PLAYER_OPTIONS)) updateSetting('external_player', value);
                }}
                options={PLAYER_OPTIONS}
              />
            )}
          </Row>
          <PlayerEngineSettings settings={settings} onChange={updateSetting} />
        </Section>

        <Section title={t('settingsLayout.sources')} id="sources">
          <Row
            label={t('settings.preferredProvider')}
            hint={providers.none ? t('settings.providerNoneHint') : t('settings.providerHint')}
          >
            {({ labelId, hintId }) => (
              <CustomSelect
                aria-labelledby={labelId}
                aria-describedby={hintId}
                className="w-52"
                disabled={providers.none}
                value={preferredProvider}
                onChange={(value) => updateSetting('preferred_provider', value)}
                options={providers.options}
              />
            )}
          </Row>
          <ServerOrder value={settings.provider_order} onChange={(order) => updateSetting('provider_order', order)} />
          <LanguagePreferences />
          <Row label={t('settings.hideAdult')} hint={t('settings.hideAdultHint')}>
            {({ labelId, hintId }) => (
              <Switch
                aria-labelledby={labelId}
                aria-describedby={hintId}
                checked={settings.hide_adult_content}
                onChange={(checked) =>
                  // The backend applies the filter; lists fetched under the old value are stale.
                  updateSetting('hide_adult_content', checked, () =>
                    invalidateAdultFilteredQueries(queryClient)
                  )
                }
              />
            )}
          </Row>
        </Section>

        <DownloadSettings />

        <SystemSection />

        <Section title={t('settingsLayout.network')} id="network">
          <Row
            label="DNS over HTTPS" // i18n-ignore: protocol name
            hint={t('settings.dohHint')}
          >
            {({ labelId, hintId }) => (
              <CustomSelect
                aria-labelledby={labelId}
                aria-describedby={hintId}
                className="w-52"
                value={dohMode}
                onChange={(value) => {
                  if (isOneOf(value, dohModeOptions())) updateSetting('doh_mode', value);
                }}
                options={dohModeOptions()}
              />
            )}
          </Row>
          <Row label={t('settings.resolver')}>
            {({ labelId, hintId }) => (
              <CustomSelect
                aria-labelledby={labelId}
                aria-describedby={hintId}
                className="w-52"
                disabled={dohMode === 'off'}
                value={dohProvider}
                onChange={(value) => {
                  if (isOneOf(value, DOH_OPTIONS)) updateSetting('doh_provider', value);
                }}
                options={DOH_OPTIONS}
              />
            )}
          </Row>
          <Row
            label={t('settings.catalogueSource')}
            hint={t('settings.catalogueSourceHint')}
          >
            {({ labelId, hintId }) => (
              <CustomSelect
                aria-labelledby={labelId}
                aria-describedby={hintId}
                className="w-52"
                value={catalogueSource}
                onChange={(value) => {
                  if (isOneOf(value, catalogueOptions())) updateSetting('catalogue_source', value);
                }}
                options={catalogueOptions()}
              />
            )}
          </Row>
        </Section>

        <Section title={t('settingsLayout.notifications')} id="notifications">
          <Row
            label={t('settings.releaseNotifications')}
            hint={t('settings.releaseNotificationsHint')}
          >
            {({ labelId, hintId }) => (
              <Switch
                aria-labelledby={labelId}
                aria-describedby={hintId}
                checked={settings.release_notifications}
                onChange={(checked) => updateSetting('release_notifications', checked)}
              />
            )}
          </Row>
          <ReleaseCheckSettings />
        </Section>

        <Section title={t('settingsLayout.account')} id="account">
          {ACCOUNT_SERVICES.map((service) => (
            <Row
              key={service}
              label={ACCOUNT_LABELS[service]}
              hint={accountSummary(
                accountsQuery.data?.[service],
                accountsQuery.isError,
                isAccountAvailable(service, accountsQuery.data),
              )}
            >
              {() => <AccountAction service={service} />}
            </Row>
          ))}
        </Section>

        <BackupSection />

        <Section title={t('settingsLayout.about')} id="about">
          <Row label="Anix" hint={appVersion ? t('settings.version', { version: appVersion }) : undefined}>
            {() => (
              <Button variant="secondary" size="sm" onClick={handleCheckUpdate} disabled={isCheckingUpdate}>
                {isCheckingUpdate && <Spinner size="sm" label={t('settings.checkingUpdates')} />}
                {t('settings.checkForUpdates')}
              </Button>
            )}
          </Row>
        </Section>

        {/* Last: with many plugins this list is long and would bury the sections above. */}
        <PluginsSection />
    </SettingsShell>
  );
};
