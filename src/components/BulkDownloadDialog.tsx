import React, { useId, useMemo, useRef, useState } from 'react';
import clsx from 'clsx';
import { Button, Chip, Modal, SegmentedControl, Switch, errorMessage } from './ui';
import { useUIStore } from '../stores/uiStore';
import { useEpisodes, useFillerEpisodes } from '../hooks/useProviders';
import { useSettings } from '../hooks/useSettings';
import { useWatchedEpisodes } from '../hooks/useLibrary';
import { useStreamingProviders } from '../hooks/usePlugins';
import { NoPluginsNotice } from './NoPluginsNotice';
import {
  isDownloadActive,
  newQueuedDownload,
  useAllDownloads,
  useCheckStorage,
  useDownloadFolder,
  useQueueDownloads,
} from '../hooks/useDownloads';
import { downloadLanguageLabel, nextUnwatched, pickEpisodes, storageState, type EpisodePick } from '../lib/downloadPicks';
import type { EpisodesResult, Media } from '../types';
import { CoverThumb } from './library/CoverThumb';
import { INPUT_CLASS, LABEL_CLASS, formatBytes } from './library/format';
import { t, tn } from '../i18n';

type Category = 'sub' | 'dub';

const qualityOptions = () => [
  { value: 'best', label: t('bulkDownloadDialog.best') },
  { value: '1080p', label: '1080p' },
  { value: '720p', label: '720p' },
  { value: '480p', label: '480p' },
];

/** Upper bound for one batch when the anime's episode count is not known. */
const MAX_BATCH_WITHOUT_COUNT = 100;
/** Space the backend wants left over after a download (`HEADROOM_BYTES`). */
const HEADROOM_BYTES = 1_048_576_000;

const DialogSection: React.FC<{ title: string; children: React.ReactNode }> = ({ title, children }) => {
  const id = useId();
  return (
    <section aria-labelledby={id} className="flex flex-col gap-2">
      <h3 id={id} className="text-[11px] font-semibold uppercase tracking-wider text-text-muted">
        {title}
      </h3>
      {children}
    </section>
  );
};

function highestEpisode(episodes: EpisodesResult | undefined, category: Category): number {
  let highest = 0;
  for (const provider of Object.values(episodes ?? {})) {
    for (const episode of provider?.[category] ?? []) {
      if (Number.isFinite(episode.number)) highest = Math.max(highest, Math.floor(episode.number));
    }
  }
  return highest;
}

export const BulkDownloadDialog: React.FC = () => {
  const isOpen = useUIStore((state) => state.bulkDownloadDialogOpen);
  const media = useUIStore((state) => state.activeMediaForDownload) as Media | null;
  const close = useUIStore((state) => state.closeBulkDownloadDialog);

  if (!isOpen || !media) return null;
  // Keyed by anime, and unmounted on close, so every opening starts from that anime's defaults.
  return <BulkDownloadForm key={media.id} media={media} onClose={close} />;
};

interface BulkDownloadFormProps {
  media: Media;
  onClose: () => void;
}

const BulkDownloadForm: React.FC<BulkDownloadFormProps> = ({ media, onClose }) => {
  const fieldId = useId();
  const showToast = useUIStore((state) => state.showToast);
  const { data: settings } = useSettings();
  const episodesQuery = useEpisodes(media.id, media.id_mal, media.title.romaji);
  const queueDownloads = useQueueDownloads();
  const folder = useDownloadFolder();
  const { label: providerLabel, none: noPlugins, installed: installedPlugins } = useStreamingProviders();
  const watched = useWatchedEpisodes(media.id).data ?? [];
  const filler = useFillerEpisodes(media.id_mal).data ?? [];
  const allDownloads = useAllDownloads().data;

  // null means "not chosen yet": the value then follows the settings / episode data as they load.
  const [categoryChoice, setCategoryChoice] = useState<Category | null>(null);
  const [qualityChoice, setQualityChoice] = useState<string | null>(null);
  const [pick, setPick] = useState<EpisodePick>('range');
  const [skipFiller, setSkipFiller] = useState(false);
  const [fromInput, setFromInput] = useState<string | null>(null);
  const [toInput, setToInput] = useState<string | null>(null);
  // Episodes the backend refused in the last attempt; the next attempt retries only these.
  const [failed, setFailed] = useState<{ episode: number; reason: string }[]>([]);
  const [queuedSoFar, setQueuedSoFar] = useState(0);
  const [submitError, setSubmitError] = useState<string | null>(null);
  // `isPending` only changes on the next render, so a ref is what stops a fast double click.
  const submittingRef = useRef(false);

  const title =
    media.title.english || media.title.user_preferred || media.title.romaji || 'Anime'; // i18n-ignore: stored as the series title
  const cover = media.cover_image.large || media.cover_image.extra_large || null;

  const episodesData = episodesQuery.data;
  const dubAvailable = !episodesData || highestEpisode(episodesData, 'dub') > 0;
  const category: Category =
    categoryChoice ?? (settings?.prefer_dub && dubAvailable ? 'dub' : 'sub');

  const quality =
    qualityChoice ??
    qualityOptions().find((option) => option.value === settings?.download_quality)?.value ??
    'best';

  // What the providers list for this audio is what can be downloaded; AniList's count caps it
  // and stands in while the provider lists load.
  const totalEpisodes = useMemo(() => {
    const listed = highestEpisode(episodesData, category);
    const catalog = media.episodes && media.episodes > 0 ? media.episodes : null;
    if (listed > 0) return catalog ? Math.min(listed, catalog) : listed;
    return catalog;
  }, [episodesData, category, media.episodes]);

  // Episodes of this anime in this audio that are saved, queued or downloading are not queued again.
  const ownRows = (allDownloads ?? []).filter(
    (row) => row.anilist_id === media.id && row.category.toLowerCase() === category
  );
  const savedEpisodes = ownRows.filter((row) => row.status === 'COMPLETED').map((row) => row.episode_num);
  const skippedEpisodes = ownRows
    .filter((row) => row.status === 'COMPLETED' || isDownloadActive(row))
    .map((row) => row.episode_num);
  const savedCount = new Set(savedEpisodes.map(Math.round)).size;

  const clamp = (value: number) =>
    Math.max(1, totalEpisodes ? Math.min(totalEpisodes, value) : value);
  const parse = (value: string): number | null => {
    const parsed = Number.parseInt(value, 10);
    return Number.isFinite(parsed) ? clamp(parsed) : null;
  };

  // Typing past the last episode snaps back to it; the lower bound is applied on blur so the
  // field can be cleared while typing.
  const capToTotal = (value: string): string => {
    const parsed = Number.parseInt(value, 10);
    return totalEpisodes && Number.isFinite(parsed) && parsed > totalEpisodes
      ? String(totalEpisodes)
      : value;
  };

  const fromValue = fromInput ?? '1';
  const toValue = toInput ?? String(totalEpisodes ?? 1);
  const from = parse(fromValue);
  const to = parse(toValue);
  const resumeFrom = nextUnwatched(watched, totalEpisodes);

  const episodes = pickEpisodes({
    pick,
    total: totalEpisodes,
    from,
    to,
    watched,
    saved: skippedEpisodes,
    filler,
    skipFiller,
  });
  const count = episodes.length;
  const inRange = pick === 'range' && from !== null && to !== null ? Math.abs(to - from) + 1 : count;
  const leftOut = Math.max(0, inRange - count);

  const rangeError =
    pick === 'range' && (from === null || to === null)
      ? t('bulkDownloadDialog.enterRange')
      : !totalEpisodes && count > MAX_BATCH_WITHOUT_COUNT
      ? t('bulkDownloadDialog.batchLimit', { count: MAX_BATCH_WITHOUT_COUNT })
      : count === 0
      ? pick === 'unwatched'
        ? t('bulkDownloadDialog.allWatchedOrSaved')
        : t('bulkDownloadDialog.allInRangeSaved')
      : null;

  // The saved preference only applies if it is a provider that actually lists this anime.
  const preferredProvider = settings?.preferred_provider;
  const wantsProvider = !!preferredProvider && preferredProvider !== 'auto';
  const matchedProvider = wantsProvider
    ? Object.keys(episodesData ?? {}).find(
        (key) => key.toLowerCase() === preferredProvider.toLowerCase()
      )
    : undefined;
  const provider = matchedProvider ?? 'auto';

  const storageQuery = useCheckStorage(quality === 'best' ? '1080p' : quality);
  const storage = storageQuery.data;
  const neededBytes = storage ? storage.needed_bytes * Math.max(1, count) : 0;
  const space = storage ? storageState(neededBytes, storage.free_bytes, HEADROOM_BYTES) : 'ok';
  const noSpace = space === 'full';

  const pending = queueDownloads.isPending;
  const retrying = failed.length > 0;
  const canSubmit = !noPlugins && !pending && !noSpace && (retrying || rangeError === null);

  const preferredLanguage =
    (category === 'dub' ? settings?.preferred_dub_languages : settings?.preferred_sub_languages)?.[0] ?? null;
  const languageLine = downloadLanguageLabel({
    category,
    audio_lang: category === 'dub' ? preferredLanguage : 'ja',
    sub_lang: category === 'sub' ? preferredLanguage : null,
  });

  const handleClose = () => {
    if (!pending) onClose();
  };

  // Changing what to download starts a new batch, so the retry list no longer applies.
  const startNewBatch = () => {
    setFailed([]);
    setSubmitError(null);
  };

  const choosePick = (next: EpisodePick, range?: { from: number; to: number | null }) => {
    startNewBatch();
    setPick(next);
    if (range) {
      setFromInput(String(range.from));
      setToInput(range.to === null ? null : String(range.to));
    }
  };

  const handleQueue = async () => {
    if (!canSubmit || submittingRef.current) return;
    const batch = retrying ? failed.map((failure) => failure.episode) : episodes;
    if (batch.length === 0) return;

    submittingRef.current = true;
    setSubmitError(null);

    const seriesCover = media.cover_image.extra_large || media.cover_image.large || null;
    const records = batch.map((episode) =>
      newQueuedDownload({
        anilistId: media.id,
        episodeNum: episode,
        episodeTitle: `Episode ${episode}`, // i18n-ignore: stored with the download
        seriesTitle: title,
        seriesCover,
        provider,
        category,
        quality,
      })
    );

    try {
      const result = await queueDownloads.mutateAsync(records);
      const total = queuedSoFar + result.queued;

      if (result.failures.length === 0) {
        showToast({
          type: 'success',
          title: t('bulkDownloadDialog.queuedToast', { count: total, title }),
          message: t('bulkDownloadDialog.trackThem'),
        });
        onClose();
        return;
      }

      setQueuedSoFar(total);
      setFailed(
        result.failures.map(({ record, reason }) => ({ episode: record.episode_num, reason }))
      );
    } catch (err) {
      setSubmitError(errorMessage(err, t('bulkDownloadDialog.couldNotQueue')));
    } finally {
      submittingRef.current = false;
    }
  };

  const episodeCountText = [
    totalEpisodes
      ? t('common.episodeCount', { count: totalEpisodes })
      : episodesQuery.isLoading && !noPlugins
      ? t('bulkDownloadDialog.checkingEpisodes')
      : t('bulkDownloadDialog.countUnknown'),
    savedCount > 0 ? t('bulkDownloadDialog.alreadySaved', { count: savedCount }) : null,
  ]
    .filter(Boolean)
    .join(' · ');

  const fraction = storage && storage.free_bytes > 0 ? Math.min(1, neededBytes / storage.free_bytes) : 0;

  return (
    <Modal
      isOpen
      onClose={handleClose}
      title={t('bulkDownloadDialog.title')}
      size="lg"
      footer={
        <>
          <Button variant="ghost" onClick={handleClose} disabled={pending}>
            {queuedSoFar > 0 ? t('common.close') : t('common.cancel')}
          </Button>
          <Button variant="primary" onClick={handleQueue} disabled={!canSubmit}>
            {pending
              ? t('bulkDownloadDialog.queuing')
              : noSpace
              ? t('bulkDownloadDialog.notEnoughSpace')
              : retrying
              ? t('bulkDownloadDialog.retryFailed', { count: failed.length })
              : count > 0
              ? t('bulkDownloadDialog.queueCount', { count })
              : t('bulkDownloadDialog.queueEpisodes')}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-5">
        {noPlugins && (
          <NoPluginsNotice
            installed={installedPlugins}
            detail={t('bulkDownloadDialog.noPluginsDetail')}
            onOpenSettings={onClose}
          />
        )}

        <div className="flex items-center gap-3 min-w-0">
          <CoverThumb src={cover} className="w-14 h-20 rounded-md flex-shrink-0" />
          <div className="flex flex-col min-w-0 gap-0.5">
            <span className="text-sm font-semibold text-text-primary truncate">{title}</span>
            <span className="text-xs text-text-muted">{episodeCountText}</span>
            {!noPlugins && (
              <span className="text-xs text-text-muted">
                {tn(
                  wantsProvider && !matchedProvider && episodesData
                    ? 'bulkDownloadDialog.providerNotListed'
                    : 'bulkDownloadDialog.provider',
                  {
                    provider: (
                      <span className="text-text-secondary">
                        {provider === 'auto' ? t('bulkDownloadDialog.automaticProvider') : providerLabel(provider)}
                      </span>
                    ),
                  },
                  { name: wantsProvider ? providerLabel(preferredProvider) : '' }
                )}
              </span>
            )}
          </div>
        </div>

        <DialogSection title={t('bulkDownloadDialog.audio')}>
          <SegmentedControl<Category>
            aria-label={t('bulkDownloadDialog.audio')}
            value={category}
            onChange={(value) => {
              startNewBatch();
              setCategoryChoice(value);
            }}
            options={[
              { value: 'sub', label: t('common.sub') },
              {
                value: 'dub',
                label: t('common.dub'),
                disabled: !dubAvailable,
                title: dubAvailable ? undefined : t('bulkDownloadDialog.noDub'),
              },
            ]}
          />
          <p className="text-xs text-text-muted">{t('bulkDownloadDialog.whenProviderHas', { language: languageLine })}</p>
        </DialogSection>

        <DialogSection title={t('bulkDownloadDialog.quality')}>
          <SegmentedControl
            aria-label={t('bulkDownloadDialog.quality')}
            value={quality}
            onChange={(value) => {
              startNewBatch();
              setQualityChoice(value);
            }}
            options={qualityOptions()}
          />
        </DialogSection>

        <DialogSection title={t('common.episodes')}>
          <div role="group" aria-label={t('bulkDownloadDialog.quickPicks')} className="flex flex-wrap gap-2">
            <Chip
              selected={pick === 'range' && from === 1 && to === (totalEpisodes ?? to)}
              onClick={() => choosePick('range', { from: 1, to: totalEpisodes })}
              disabled={pending}
            >
              {t('bulkDownloadDialog.all')}
            </Chip>
            <Chip selected={pick === 'unwatched'} onClick={() => choosePick('unwatched')} disabled={pending || !totalEpisodes}>
              {t('bulkDownloadDialog.unwatched')}
            </Chip>
            {resumeFrom !== null && (
              <Chip
                selected={pick === 'range' && from === resumeFrom && to === (totalEpisodes ?? to)}
                onClick={() => choosePick('range', { from: resumeFrom, to: totalEpisodes })}
                disabled={pending}
              >
                {t('bulkDownloadDialog.fromEpisodeNumber', { number: resumeFrom })}
              </Chip>
            )}
          </div>

          {pick === 'range' && (
            <div className="grid grid-cols-2 gap-3">
              <div>
                <label htmlFor={`${fieldId}-from`} className={LABEL_CLASS}>
                  {t('bulkDownloadDialog.fromEpisode')}
                </label>
                <input
                  id={`${fieldId}-from`}
                  type="number"
                  inputMode="numeric"
                  min={1}
                  max={totalEpisodes ?? undefined}
                  step={1}
                  value={fromValue}
                  onChange={(event) => {
                    startNewBatch();
                    setFromInput(capToTotal(event.target.value));
                  }}
                  onBlur={() => from !== null && setFromInput(String(from))}
                  disabled={pending}
                  className={`${INPUT_CLASS} tabular-nums`}
                />
              </div>
              <div>
                <label htmlFor={`${fieldId}-to`} className={LABEL_CLASS}>
                  {t('bulkDownloadDialog.toEpisode')}
                </label>
                <input
                  id={`${fieldId}-to`}
                  type="number"
                  inputMode="numeric"
                  min={1}
                  max={totalEpisodes ?? undefined}
                  step={1}
                  value={toValue}
                  onChange={(event) => {
                    startNewBatch();
                    setToInput(capToTotal(event.target.value));
                  }}
                  onBlur={() => to !== null && setToInput(String(to))}
                  disabled={pending}
                  className={`${INPUT_CLASS} tabular-nums`}
                />
              </div>
            </div>
          )}

          <div className="flex items-center justify-between gap-4">
            <span id={`${fieldId}-filler`} className="text-[13px] text-text-primary">
              {t('bulkDownloadDialog.skipFiller')}
              <span className="block text-xs text-text-muted">
                {filler.length > 0
                  ? t('bulkDownloadDialog.fillerKnown', { count: filler.length })
                  : t('bulkDownloadDialog.noFiller')}
              </span>
            </span>
            <Switch
              aria-labelledby={`${fieldId}-filler`}
              checked={skipFiller}
              disabled={pending || filler.length === 0}
              onChange={(checked) => {
                startNewBatch();
                setSkipFiller(checked);
              }}
            />
          </div>

          {rangeError && !retrying ? (
            <p className="text-xs text-state-warning">{rangeError}</p>
          ) : (
            <p className="text-xs text-text-muted" aria-live="polite">
              {t('bulkDownloadDialog.selected', { count })}
              {leftOut > 0 ? ` · ${t('bulkDownloadDialog.leftOut', { count: leftOut })}` : ''}
            </p>
          )}
        </DialogSection>

        <DialogSection title={t('bulkDownloadDialog.storage')}>
          {storage ? (
            <>
              <div
                role="meter"
                aria-label={t('bulkDownloadDialog.spaceNeeded')}
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={Math.round(fraction * 100)}
                aria-valuetext={t('bulkDownloadDialog.neededOfFree', {
                  needed: formatBytes(neededBytes) || t('bulkDownloadDialog.zeroSize'),
                  free: formatBytes(storage.free_bytes) || t('bulkDownloadDialog.zeroSize'),
                })}
                className="h-2 w-full overflow-hidden rounded-full bg-ink/10"
              >
                <div
                  className={clsx(
                    'h-full rounded-full motion-safe:transition-[width]',
                    space === 'full' ? 'bg-state-error' : space === 'tight' ? 'bg-state-warning' : 'bg-accent-500'
                  )}
                  style={{ width: `${Math.max(2, fraction * 100)}%` }}
                />
              </div>
              <p className={clsx('text-xs', space === 'full' ? 'text-state-error' : 'text-text-muted')}>
                {count > 0 ? `${t('bulkDownloadDialog.aboutNeeded', { size: formatBytes(neededBytes) })} · ` : ''}
                {t(
                  space === 'full'
                    ? 'bulkDownloadDialog.freeNotEnough'
                    : space === 'tight'
                    ? 'bulkDownloadDialog.freeTight'
                    : 'bulkDownloadDialog.free',
                  { size: formatBytes(storage.free_bytes) || t('bulkDownloadDialog.zeroSize') }
                )}
              </p>
              <div className="flex items-center justify-between gap-3">
                <span className="min-w-0 truncate text-xs text-text-muted select-text" title={storage.dir}>
                  {storage.dir}
                </span>
                <Button
                  variant="ghost"
                  size="sm"
                  disabled={pending || folder.change.isPending}
                  onClick={() =>
                    folder.change.mutate(undefined, {
                      onError: (err) =>
                        showToast({ type: 'error', title: t('bulkDownloadDialog.couldNotUseFolder'), message: errorMessage(err) }),
                    })
                  }
                >
                  {t('bulkDownloadDialog.changeFolder')}
                </Button>
              </div>
            </>
          ) : (
            <p className="text-xs text-text-muted">{storageQuery.isError ? t('bulkDownloadDialog.couldNotReadSpace') : t('bulkDownloadDialog.checkingSpace')}</p>
          )}
        </DialogSection>

        {retrying && (
          <div role="alert" className="p-3 rounded-md bg-state-error/10">
            <p className="text-[13px] font-medium text-state-error">
              {queuedSoFar > 0
                ? t('bulkDownloadDialog.queuedButFailed', { queued: queuedSoFar, count: failed.length })
                : t('bulkDownloadDialog.couldNotQueueCount', { count: failed.length })}
            </p>
            <ul className="mt-1.5 flex flex-col gap-1 max-h-28 overflow-y-auto text-xs text-text-secondary select-text">
              {failed.map(({ episode, reason }) => (
                <li key={episode} className="break-words">
                  {t('bulkDownloadDialog.failureLine', { episode, reason })}
                </li>
              ))}
            </ul>
          </div>
        )}

        {submitError && (
          <p role="alert" className="text-xs text-state-error select-text break-words">
            {submitError}
          </p>
        )}
      </div>
    </Modal>
  );
};
