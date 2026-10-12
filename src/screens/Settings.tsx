import React, { useEffect, useId, useRef, useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import { Puzzle, RefreshCw, Trash2 } from 'lucide-react';
import { useQueryClient } from '@tanstack/react-query';
import { CustomSelect } from '../components/CustomSelect';
import { ThemePicker } from '../components/ThemePicker';
import { useTheme } from '../hooks/useTheme';
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
import { ACCOUNT_LABELS, ACCOUNT_SERVICES, useAccountStatus } from '../hooks/useAccounts';
import { AccountAction, accountSummary } from '../components/AuthDialog';
import { useUIStore } from '../stores/uiStore';
import type { AppSettings, DohProvider, ExternalPlayer, PluginInfo, PluginRepo } from '../types';

// Episodes always open in the built-in player; this picks the program used outside it.
const PLAYER_OPTIONS: { value: ExternalPlayer; label: string }[] = [
  { value: 'mpv', label: 'mpv' },
  { value: 'vlc', label: 'VLC' },
];

const DOH_OPTIONS: { value: DohProvider; label: string }[] = [
  { value: 'cloudflare', label: 'Cloudflare (1.1.1.1)' },
  { value: 'google', label: 'Google (8.8.8.8)' },
  { value: 'adguard', label: 'AdGuard DNS' },
];

function isOneOf<T extends string>(value: unknown, options: { value: T }[]): value is T {
  return options.some((o) => o.value === value);
}

const Section: React.FC<{
  title: string;
  children: React.ReactNode;
  /** Set on a section that can be linked to; it then takes focus when the link is followed. */
  sectionRef?: React.Ref<HTMLElement>;
}> = ({ title, children, sectionRef }) => {
  const titleId = useId();
  return (
    <section
      ref={sectionRef}
      aria-labelledby={titleId}
      tabIndex={sectionRef ? -1 : undefined}
      className="flex scroll-mt-4 flex-col gap-2 rounded-xl outline-none focus-visible:ring-2 focus-visible:ring-accent-500/60 focus-visible:ring-offset-4 focus-visible:ring-offset-bg-page"
    >
      <h2 id={titleId} className="text-[11px] font-semibold uppercase tracking-wider text-text-muted">
        {title}
      </h2>
      <div className="divide-y divide-border rounded-xl ui-card bg-bg-card px-4">{children}</div>
    </section>
  );
};

/** A labelled row; `children` receives the ids to tie the control to its label and hint. */
const Row: React.FC<{
  label: string;
  hint?: string;
  /** Puts the control under the label, for one that needs the full width. */
  stacked?: boolean;
  children: (ids: { labelId: string; hintId: string | undefined }) => React.ReactNode;
}> = ({ label, hint, stacked = false, children }) => {
  const labelId = useId();
  const hintId = useId();
  return (
    <div
      className={
        stacked
          ? 'flex flex-col gap-3 py-3'
          : 'flex min-h-[56px] items-center justify-between gap-6 py-3'
      }
    >
      <div className="flex min-w-0 flex-col">
        <span id={labelId} className="text-[13px] font-medium text-text-primary">
          {label}
        </span>
        {hint && (
          <span id={hintId} className="mt-0.5 text-xs text-text-muted">
            {hint}
          </span>
        )}
      </div>
      <div className={stacked ? 'flex w-full' : 'flex flex-shrink-0 items-center'}>
        {children({ labelId, hintId: hint ? hintId : undefined })}
      </div>
    </div>
  );
};

/** The link to send to `add_plugin_repo`, or why what was typed cannot be one. */
function parseRepoLink(input: string): { url: string } | { problem: string } {
  const text = input.trim();
  if (!text) return { problem: 'Paste a repository link first.' };
  const url = /^[a-z][a-z0-9+.-]*:\/\//i.test(text) ? text : `https://${text}`;
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return { problem: 'This does not look like a link. It should start with https://' };
  }
  if (parsed.protocol !== 'https:' && parsed.protocol !== 'http:') {
    return { problem: 'The link has to start with https://' };
  }
  if (!parsed.hostname.includes('.') || /\s/.test(text)) {
    return { problem: 'This does not look like a link. It should start with https://' };
  }
  const isGitHub = /^(www\.)?github\.com$/i.test(parsed.hostname);
  if (isGitHub && parsed.pathname.split('/').filter(Boolean).length < 2) {
    return { problem: 'A GitHub link needs the owner and the repository: https://github.com/owner/repository' };
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
  return count === 1 ? '1 plugin' : `${count} plugins`;
}

function updatedLabel(updatedAtMs: number): string {
  if (!Number.isFinite(updatedAtMs) || updatedAtMs <= 0) return 'Not downloaded yet';
  return `Updated ${new Date(updatedAtMs).toLocaleString(undefined, {
    dateStyle: 'medium',
    timeStyle: 'short',
  })}`;
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

const PluginRow: React.FC<{
  plugin: PluginInfo;
  onToggle: (plugin: PluginInfo, enabled: boolean) => void;
}> = ({ plugin, onToggle }) => {
  const nameId = useId();
  const name = plugin.name.trim() || plugin.id;
  const version = plugin.version.trim();
  const meta = [
    version ? (/^v/i.test(version) ? version : `v${version}`) : null,
    plugin.author.trim() ? `by ${plugin.author.trim()}` : null,
    plugin.lang.trim() ? plugin.lang.trim().toUpperCase() : null,
  ]
    .filter(Boolean)
    .join(' · ');
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
          </p>
          {plugin.description.trim() && (
            <p className="mt-0.5 line-clamp-2 text-xs leading-[18px] text-text-muted select-text">
              {plugin.description.trim()}
            </p>
          )}
        </div>
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
            This plugin could not be loaded{plugin.enabled ? ', so it is not used' : ''}
          </p>
          <p className="mt-0.5 max-h-24 overflow-y-auto whitespace-pre-wrap break-words text-xs text-text-secondary select-text">
            {plugin.error?.trim() || 'The plugin did not report a reason.'}
          </p>
        </div>
      )}
    </li>
  );
};

/** Settings → Plugins: the repositories streaming plugins are downloaded from, and their plugins. */
const PluginsSection: React.FC = () => {
  const showToast = useUIStore((s) => s.showToast);
  const [searchParams] = useSearchParams();
  const linkedTo = searchParams.get('section') === 'plugins';
  const sectionRef = useRef<HTMLElement>(null);

  // Followed from a "no plugin" state elsewhere in the app. This runs after the layout has
  // put a newly opened screen at the top.
  useEffect(() => {
    const section = sectionRef.current;
    if (!linkedTo || !section) return;
    section.focus({ preventScroll: true });
    section.scrollIntoView({ block: 'start' });
  }, [linkedTo]);

  const reposQuery = usePluginRepos();
  const pluginsQuery = usePlugins();
  const repos = reposQuery.data ?? [];
  const plugins = pluginsQuery.data ?? [];

  const addRepo = useAddPluginRepo();
  const removeRepo = useRemovePluginRepo();
  const refreshRepos = useRefreshPluginRepos();
  const setPluginEnabled = useSetPluginEnabled();

  const fieldId = useId();
  const [link, setLink] = useState('');
  const [linkError, setLinkError] = useState<string | null>(null);
  const [repoToRemove, setRepoToRemove] = useState<PluginRepo | null>(null);

  const handleAdd = (event: React.FormEvent) => {
    event.preventDefault();
    if (addRepo.isPending) return;
    const parsed = parseRepoLink(link);
    if ('problem' in parsed) {
      setLinkError(parsed.problem);
      return;
    }
    if (repos.some((repo) => repoLinkKey(repo.url) === repoLinkKey(parsed.url))) {
      setLinkError('This repository is already added. Use Refresh all to pick up new versions.');
      return;
    }
    setLinkError(null);
    addRepo.mutate(parsed.url, {
      onSuccess: (repo) => {
        setLink('');
        const name = repo.name?.trim() || 'the repository';
        if (repo.error) {
          showToast({ type: 'warning', title: `Added ${name}, with a problem`, message: repo.error });
        } else {
          showToast({
            type: 'success',
            title: `Added ${name}`,
            message: `${pluginCountLabel(repo.plugin_count)} installed.`,
          });
        }
      },
      onError: (err) => setLinkError(errorMessage(err, 'The repository could not be added.')),
    });
  };

  const handleRefresh = () => {
    refreshRepos.mutate(undefined, {
      onSuccess: (updated) => {
        const failed = updated.filter((repo) => repo.error).length;
        if (failed > 0) {
          showToast({
            type: 'warning',
            title:
              failed === 1
                ? '1 repository could not be refreshed'
                : `${failed} repositories could not be refreshed`,
            message: 'The reason is shown next to it.',
          });
        } else {
          showToast({ type: 'success', title: 'Plugins are up to date' });
        }
      },
      onError: (err) =>
        showToast({ type: 'error', title: 'Could not refresh the plugins', message: errorMessage(err) }),
    });
  };

  const handleToggle = (plugin: PluginInfo, enabled: boolean) => {
    setPluginEnabled.mutate(
      { id: plugin.id, enabled },
      {
        onError: (err) =>
          showToast({
            type: 'error',
            title: `Could not turn ${plugin.name.trim() || plugin.id} ${enabled ? 'on' : 'off'}`,
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
        showToast({ type: 'info', title: `Removed ${name}` });
      },
      onError: (err) =>
        showToast({ type: 'error', title: `Could not remove ${name}`, message: errorMessage(err) }),
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
        <PluginRow key={plugin.id} plugin={plugin} onToggle={handleToggle} />
      ))}
    </ul>
  );

  return (
    <Section title="Plugins" sectionRef={sectionRef}>
      <form onSubmit={handleAdd} noValidate className="py-3">
        <label htmlFor={fieldId} className="text-[13px] font-medium text-text-primary">
          Add a plugin repository
        </label>
        <div className="mt-2 flex items-center gap-2">
          <input
            id={fieldId}
            type="url"
            inputMode="url"
            autoComplete="off"
            spellCheck={false}
            placeholder="Paste the link of a plugin list"
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
            {addRepo.isPending && <Spinner size="sm" label="Adding the repository" />}
            {addRepo.isPending ? 'Adding…' : 'Add'}
          </Button>
        </div>
        <div id={`${fieldId}-help`} className="mt-1.5 text-xs">
          {linkError ? (
            <p role="alert" className="break-words text-state-error select-text">
              {linkError}
            </p>
          ) : addRepo.isPending ? (
            <p className="text-text-muted">Downloading the repository and its plugins…</p>
          ) : (
            <p className="text-text-muted">
              The link from the Anix website, a Google Drive share link, a GitHub repository, or a direct link to a plugin list.
            </p>
          )}
        </div>
      </form>

      {loading ? (
        <div aria-busy="true" className="flex items-center gap-2 py-4 text-xs text-text-muted">
          <Spinner size="sm" label="Loading plugins" />
          Loading plugins…
        </div>
      ) : reposQuery.isError && nothingAdded ? (
        <ErrorState
          compact
          title="The plugin repositories could not be loaded"
          error={reposQuery.error ?? undefined}
          onRetry={() => {
            reposQuery.refetch();
            pluginsQuery.refetch();
          }}
        />
      ) : nothingAdded ? (
        <EmptyState
          className="py-10"
          icon={<Puzzle aria-hidden="true" />}
          title="No plugins yet"
          description="Streaming sources come from plugins. Paste the link of a plugin repository above to install the ones it offers. Until then anime and episode lists still show, but nothing can be played."
        />
      ) : (
        <>
          <Row
            label="Repositories"
            hint={`${repos.length === 1 ? '1 repository' : `${repos.length} repositories`}, ${pluginCountLabel(
              plugins.length
            )}`}
          >
            {() => (
              <Button
                variant="secondary"
                size="sm"
                onClick={handleRefresh}
                disabled={refreshRepos.isPending || repos.length === 0}
              >
                {refreshRepos.isPending ? (
                  <Spinner size="sm" label="Refreshing plugins" />
                ) : (
                  <RefreshCw aria-hidden="true" className="h-3.5 w-3.5" />
                )}
                Refresh all
              </Button>
            )}
          </Row>

          {pluginsQuery.isError && (
            <div role="alert" className="flex items-center justify-between gap-4 py-3">
              <p className="min-w-0 break-words text-xs text-state-error select-text">
                The plugin list could not be loaded: {errorMessage(pluginsQuery.error)}
              </p>
              <Button variant="ghost" size="sm" onClick={() => pluginsQuery.refetch()}>
                Retry
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
                    aria-label={`Remove ${repo.name?.trim() || repo.url}`}
                    onClick={() => setRepoToRemove(repo)}
                  >
                    <Trash2 aria-hidden="true" className="h-3.5 w-3.5" />
                    Remove
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
                        This repository has no streaming plugins.
                      </p>
                    )}
              </div>
            );
          })}

          {strayPlugins.length > 0 && (
            <div className="py-3">
              <p className="text-[13px] font-medium text-text-primary">Other plugins</p>
              <p className="mt-0.5 text-xs text-text-muted">Installed, but not part of a repository above.</p>
              {pluginList(strayPlugins)}
            </div>
          )}
        </>
      )}

      <Modal
        isOpen={repoToRemove !== null}
        onClose={() => {
          if (!removeRepo.isPending) setRepoToRemove(null);
        }}
        title="Remove this repository?"
        size="md"
        footer={
          <>
            <Button variant="ghost" onClick={() => setRepoToRemove(null)} disabled={removeRepo.isPending}>
              Cancel
            </Button>
            <Button variant="danger" onClick={handleRemove} disabled={removeRepo.isPending}>
              {removeRepo.isPending && <Spinner size="sm" label="Removing the repository" />}
              Remove
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
                ? `Its ${pluginCountLabel(repoToRemove.plugin_count)} will be removed and can no longer be used for streaming or downloads.`
                : 'It will no longer be checked for plugins.'}{' '}
              You can add the repository again at any time.
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
        title="Could not load settings"
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
          title: 'Could not save settings',
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
          title: 'Could not check for updates',
          message: errorMessage(result.error),
        });
      } else if (result.data) {
        setUpdateDialogOpen(true);
      } else {
        showToast({
          type: 'info',
          title: 'No newer release found',
          message: appVersion ? `You are running version ${appVersion}.` : undefined,
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

  return (
    <div className="flex max-w-2xl flex-col gap-8 pb-16 animate-fade-in">
      <h1 className="text-xl font-semibold text-text-primary">Settings</h1>

      <Section title="Appearance">
        <Row label="Theme" hint="Changes the look of the whole app. The player stays dark over the video." stacked>
          {({ labelId, hintId }) => (
            <ThemePicker
              aria-labelledby={labelId}
              aria-describedby={hintId}
              value={theme}
              onChange={(next) =>
                setTheme(next, (err) =>
                  showToast({
                    type: 'error',
                    title: 'Could not save settings',
                    message: errorMessage(err),
                  })
                )
              }
            />
          )}
        </Row>
      </Section>

      <Section title="Playback">
        <Row label="Autoplay" hint="Start playing as soon as an episode opens.">
          {({ labelId, hintId }) => (
            <Switch
              aria-labelledby={labelId}
              aria-describedby={hintId}
              checked={settings.autoplay}
              onChange={(checked) => updateSetting('autoplay', checked)}
            />
          )}
        </Row>
        <Row label="Skip intros and outros" hint="Jump past openings and endings when skip times are known.">
          {({ labelId, hintId }) => (
            <Switch
              aria-labelledby={labelId}
              aria-describedby={hintId}
              checked={settings.auto_skip_intro_outro}
              onChange={(checked) => updateSetting('auto_skip_intro_outro', checked)}
            />
          )}
        </Row>
        <Row label="Preferred audio" hint="Used when an anime has no watch history yet.">
          {() => (
            <SegmentedControl
              aria-label="Preferred audio"
              value={settings.prefer_dub ? 'dub' : 'sub'}
              onChange={(value) => updateSetting('prefer_dub', value === 'dub')}
              options={[
                { value: 'sub', label: 'Sub' },
                { value: 'dub', label: 'Dub' },
              ]}
            />
          )}
        </Row>
        <Row
          label="External player"
          hint="Opens local files, finished downloads and “Open in external player”. It must be installed."
        >
          {({ labelId }) => (
            <CustomSelect
              aria-labelledby={labelId}
              className="w-52"
              value={externalPlayer}
              onChange={(value) => {
                if (isOneOf(value, PLAYER_OPTIONS)) updateSetting('external_player', value);
              }}
              options={PLAYER_OPTIONS}
            />
          )}
        </Row>
      </Section>

      <Section title="Sources">
        <Row
          label="Preferred provider"
          hint={
            providers.none
              ? 'Providers come from plugins. Enable one under Plugins to choose it here.'
              : 'Tried first for streams and downloads. Auto tries every enabled plugin.'
          }
        >
          {({ labelId }) => (
            <CustomSelect
              aria-labelledby={labelId}
              className="w-52"
              disabled={providers.none}
              value={preferredProvider}
              onChange={(value) => updateSetting('preferred_provider', value)}
              options={providers.options}
            />
          )}
        </Row>
        <Row label="Hide adult content" hint="Leave 18+ titles out of search, rails and the schedule.">
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

      <PluginsSection />

      <Section title="Network">
        <Row
          label="DNS over HTTPS"
          hint="Resolve provider domains through an encrypted resolver instead of your ISP's DNS."
        >
          {({ labelId, hintId }) => (
            <Switch
              aria-labelledby={labelId}
              aria-describedby={hintId}
              checked={settings.enable_doh}
              onChange={(checked) => updateSetting('enable_doh', checked)}
            />
          )}
        </Row>
        <Row label="Resolver">
          {({ labelId }) => (
            <CustomSelect
              aria-labelledby={labelId}
              className="w-52"
              disabled={!settings.enable_doh}
              value={dohProvider}
              onChange={(value) => {
                if (isOneOf(value, DOH_OPTIONS)) updateSetting('doh_provider', value);
              }}
              options={DOH_OPTIONS}
            />
          )}
        </Row>
      </Section>

      <Section title="Notifications">
        <Row
          label="Release notifications"
          hint="Show a desktop notification when a new episode airs for an anime you have a bell on. Turning this off keeps your bells."
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
      </Section>

      <Section title="Account">
        {ACCOUNT_SERVICES.map((service) => (
          <Row
            key={service}
            label={ACCOUNT_LABELS[service]}
            hint={accountSummary(accountsQuery.data?.[service], accountsQuery.isError)}
          >
            {() => <AccountAction service={service} />}
          </Row>
        ))}
      </Section>

      <Section title="About">
        <Row label="Anix" hint={appVersion ? `Version ${appVersion}` : undefined}>
          {() => (
            <Button variant="secondary" size="sm" onClick={handleCheckUpdate} disabled={isCheckingUpdate}>
              {isCheckingUpdate && <Spinner size="sm" label="Checking for updates" />}
              Check for updates
            </Button>
          )}
        </Row>
      </Section>
    </div>
  );
};
