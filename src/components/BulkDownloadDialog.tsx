import React, { useId, useMemo, useRef, useState } from 'react';
import { Button, Modal, SegmentedControl, errorMessage } from './ui';
import { useUIStore } from '../stores/uiStore';
import { useEpisodes } from '../hooks/useProviders';
import { useSettings } from '../hooks/useSettings';
import { useStreamingProviders } from '../hooks/usePlugins';
import { NoPluginsNotice } from './NoPluginsNotice';
import { newQueuedDownload, useCheckStorage, useQueueDownloads } from '../hooks/useDownloads';
import type { EpisodesResult, Media } from '../types';
import { CoverThumb } from './library/CoverThumb';
import { INPUT_CLASS, LABEL_CLASS, formatBytes } from './library/format';

type Category = 'sub' | 'dub';

const QUALITY_OPTIONS = [
  { value: 'best', label: 'Best' },
  { value: '1080p', label: '1080p' },
  { value: '720p', label: '720p' },
  { value: '480p', label: '480p' },
];

/** Upper bound for one batch when the anime's episode count is not known. */
const MAX_BATCH_WITHOUT_COUNT = 100;

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
  const { label: providerLabel, none: noPlugins, installed: installedPlugins } = useStreamingProviders();

  // null means "not chosen yet": the value then follows the settings / episode data as they load.
  const [categoryChoice, setCategoryChoice] = useState<Category | null>(null);
  const [qualityChoice, setQualityChoice] = useState<string | null>(null);
  const [fromInput, setFromInput] = useState<string | null>(null);
  const [toInput, setToInput] = useState<string | null>(null);
  // Episodes the backend refused in the last attempt; the next attempt retries only these.
  const [failed, setFailed] = useState<{ episode: number; reason: string }[]>([]);
  const [queuedSoFar, setQueuedSoFar] = useState(0);
  const [submitError, setSubmitError] = useState<string | null>(null);
  // `isPending` only changes on the next render, so a ref is what stops a fast double click.
  const submittingRef = useRef(false);

  const title =
    media.title.english || media.title.user_preferred || media.title.romaji || 'Anime';
  const cover = media.cover_image.large || media.cover_image.extra_large || null;

  const episodesData = episodesQuery.data;
  const dubAvailable = !episodesData || highestEpisode(episodesData, 'dub') > 0;
  const category: Category =
    categoryChoice ?? (settings?.prefer_dub && dubAvailable ? 'dub' : 'sub');

  const quality =
    qualityChoice ??
    QUALITY_OPTIONS.find((option) => option.value === settings?.download_quality)?.value ??
    'best';

  // What the providers list for this audio is what can be downloaded; AniList's count caps it
  // and stands in while the provider lists load.
  const totalEpisodes = useMemo(() => {
    const listed = highestEpisode(episodesData, category);
    const catalog = media.episodes && media.episodes > 0 ? media.episodes : null;
    if (listed > 0) return catalog ? Math.min(listed, catalog) : listed;
    return catalog;
  }, [episodesData, category, media.episodes]);

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
  const start = from !== null && to !== null ? Math.min(from, to) : null;
  const end = from !== null && to !== null ? Math.max(from, to) : null;
  const count = start !== null && end !== null ? end - start + 1 : 0;

  const rangeError =
    count === 0
      ? 'Enter the first and last episode to download.'
      : !totalEpisodes && count > MAX_BATCH_WITHOUT_COUNT
      ? `The episode count for this anime is unknown, so one batch is limited to ${MAX_BATCH_WITHOUT_COUNT} episodes.`
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
  const lowSpace = !!storage && (!storage.ok || storage.free_bytes < neededBytes);

  const pending = queueDownloads.isPending;
  const retrying = failed.length > 0;
  const canSubmit = !noPlugins && !pending && (retrying || rangeError === null);

  const handleClose = () => {
    if (!pending) onClose();
  };

  // Changing what to download starts a new batch, so the retry list no longer applies.
  const startNewBatch = () => {
    setFailed([]);
    setSubmitError(null);
  };

  const handleQueue = async () => {
    if (!canSubmit || submittingRef.current) return;
    const episodes: number[] = [];
    if (retrying) {
      episodes.push(...failed.map((failure) => failure.episode));
    } else if (start !== null && end !== null) {
      for (let episode = start; episode <= end; episode++) episodes.push(episode);
    }
    if (episodes.length === 0) return;

    submittingRef.current = true;
    setSubmitError(null);

    const seriesCover = media.cover_image.extra_large || media.cover_image.large || null;
    const records = episodes.map((episode) =>
      newQueuedDownload({
        anilistId: media.id,
        episodeNum: episode,
        episodeTitle: `Episode ${episode}`,
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
          title: `Queued ${total} ${total === 1 ? 'episode' : 'episodes'} of "${title}"`,
          message: 'Track them in Library, on the Downloads tab.',
        });
        onClose();
        return;
      }

      setQueuedSoFar(total);
      setFailed(
        result.failures.map(({ record, reason }) => ({ episode: record.episode_num, reason }))
      );
    } catch (err) {
      setSubmitError(errorMessage(err, 'The downloads could not be queued.'));
    } finally {
      submittingRef.current = false;
    }
  };

  const episodeCountText = totalEpisodes
    ? `${totalEpisodes} ${totalEpisodes === 1 ? 'episode' : 'episodes'} available`
    : episodesQuery.isLoading && !noPlugins
    ? 'Checking available episodes…'
    : 'Episode count unknown';

  return (
    <Modal
      isOpen
      onClose={handleClose}
      title="Download episodes"
      size="md"
      footer={
        <>
          <Button variant="ghost" onClick={handleClose} disabled={pending}>
            {queuedSoFar > 0 ? 'Close' : 'Cancel'}
          </Button>
          <Button variant="primary" onClick={handleQueue} disabled={!canSubmit}>
            {pending
              ? 'Queuing…'
              : retrying
              ? `Retry ${failed.length} failed`
              : count > 0
              ? `Queue ${count} ${count === 1 ? 'episode' : 'episodes'}`
              : 'Queue episodes'}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-5">
        {noPlugins && (
          <NoPluginsNotice
            installed={installedPlugins}
            detail="Downloads are fetched through streaming plugins. Add or turn one on in Settings, then queue the episodes."
            onOpenSettings={onClose}
          />
        )}

        <div className="flex items-center gap-3 min-w-0">
          <CoverThumb src={cover} className="w-10 h-14 rounded-md flex-shrink-0" />
          <div className="flex flex-col min-w-0 gap-0.5">
            <span className="text-[13px] font-medium text-text-primary truncate">{title}</span>
            <span className="text-xs text-text-muted">{episodeCountText}</span>
          </div>
        </div>

        <div className="grid grid-cols-2 gap-3">
          <div>
            <label htmlFor={`${fieldId}-from`} className={LABEL_CLASS}>
              From episode
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
              To episode
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
          {rangeError && !retrying && (
            <p className="col-span-2 text-xs text-state-warning">{rangeError}</p>
          )}
        </div>

        <div className="flex flex-wrap items-end gap-x-6 gap-y-4">
          <div>
            <span className={LABEL_CLASS}>Audio</span>
            <SegmentedControl<Category>
              aria-label="Audio"
              value={category}
              onChange={(value) => {
                startNewBatch();
                setCategoryChoice(value);
              }}
              options={[
                { value: 'sub', label: 'Sub' },
                {
                  value: 'dub',
                  label: 'Dub',
                  disabled: !dubAvailable,
                  title: dubAvailable ? undefined : 'No provider lists a dub for this anime',
                },
              ]}
            />
          </div>
          <div>
            <span className={LABEL_CLASS}>Quality</span>
            <SegmentedControl
              aria-label="Quality"
              value={quality}
              onChange={(value) => {
                startNewBatch();
                setQualityChoice(value);
              }}
              options={QUALITY_OPTIONS}
            />
          </div>
        </div>

        <div className="flex flex-col gap-1 text-xs text-text-muted">
          {!noPlugins && (
            <p>
              Provider:{' '}
              <span className="text-text-secondary">
                {provider === 'auto' ? 'automatic (the first one that works)' : providerLabel(provider)}
              </span>
              {wantsProvider && !matchedProvider && episodesData
                ? ` — your preferred provider "${providerLabel(preferredProvider)}" does not list this anime.`
                : ''}
            </p>
          )}
          {storage && (
            <p className={lowSpace ? 'text-state-error' : undefined}>
              {formatBytes(storage.free_bytes) || '0 MB'} free in the download folder
              {count > 0 ? ` · about ${formatBytes(neededBytes)} needed` : ''}
              {lowSpace ? ' — there may not be enough space.' : ''}
            </p>
          )}
        </div>

        {retrying && (
          <div role="alert" className="p-3 rounded-md bg-state-error/10">
            <p className="text-[13px] font-medium text-state-error">
              {queuedSoFar > 0
                ? `Queued ${queuedSoFar}, but ${failed.length} could not be queued`
                : failed.length === 1
                ? '1 episode could not be queued'
                : `${failed.length} episodes could not be queued`}
            </p>
            <ul className="mt-1.5 flex flex-col gap-1 max-h-28 overflow-y-auto text-xs text-text-secondary select-text">
              {failed.map(({ episode, reason }) => (
                <li key={episode} className="break-words">
                  Episode {episode}: {reason}
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
