import React, { useEffect, useState } from 'react';
import { Check, RefreshCw, Search } from 'lucide-react';
import clsx from 'clsx';
import { Button, Modal, Spinner, errorMessage } from '../ui';
import { CoverThumb } from './CoverThumb';
import { INPUT_CLASS, mediaFormatLabel } from './format';
import { t, tn } from '../../i18n';
import {
  candidateTitle,
  matchRequest,
  useMatchSearch,
  type LocalSeries,
  type MatchCandidate,
  type SeriesMatch,
} from '../../hooks/useLocalLibrary';

interface LocalMatchDialogProps {
  series: LocalSeries | null;
  match: SeriesMatch | undefined;
  onClose: () => void;
  onPick: (series: LocalSeries, candidate: MatchCandidate) => void;
  /** Forgets the match and searches again. */
  onResearch: (series: LocalSeries) => void;
  busy: boolean;
}

export function candidateDetails(candidate: MatchCandidate): string {
  const year = candidate.season_year ?? candidate.start_year;
  return [
    mediaFormatLabel(candidate.format),
    year ? String(year) : null,
    candidate.episodes ? t('common.episodeCount', { count: candidate.episodes }) : null,
  ]
    .filter(Boolean)
    .join(' · ');
}

const CandidateRow: React.FC<{
  candidate: MatchCandidate;
  current: boolean;
  disabled: boolean;
  onPick: () => void;
}> = ({ candidate, current, disabled, onPick }) => {
  const title = candidateTitle(candidate);
  const english = candidate.title.english && candidate.title.english !== title ? candidate.title.english : null;
  return (
    <li className={clsx('flex items-center gap-3 rounded-md p-2', current ? 'bg-accent-tint' : 'hover:bg-ink/[0.05]')}>
      <CoverThumb src={candidate.cover_image} className="h-16 w-11 flex-shrink-0 rounded-sm" />
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="truncate text-[13px] font-medium text-text-primary" title={title}>
          {title}
        </span>
        {english && (
          <span className="truncate text-xs text-text-secondary" title={english}>
            {english}
          </span>
        )}
        <span className="text-xs text-text-muted tabular-nums">
          {candidateDetails(candidate)}
          {candidate.score > 0 && ` · ${t('localMatchDialog.fit', { percent: Math.round(candidate.score * 100) })}`}
        </span>
      </div>
      {current ? (
        <span className="flex flex-shrink-0 items-center gap-1 text-xs text-accent-300">
          <Check className="h-3.5 w-3.5" aria-hidden="true" />
          {t('localMatchDialog.current')}
        </span>
      ) : (
        <Button size="sm" variant="neutral" disabled={disabled} onClick={onPick} className="flex-shrink-0">
          {t('localMatchDialog.useThis')}
        </Button>
      )}
    </li>
  );
};

/** Shows the search results a series was matched from and lets the user pick another title. */
export const LocalMatchDialog: React.FC<LocalMatchDialogProps> = ({
  series,
  match,
  onClose,
  onPick,
  onResearch,
  busy,
}) => {
  const [query, setQuery] = useState('');
  useEffect(() => {
    setQuery(series?.title ?? '');
  }, [series?.key, series?.title]);

  const request = series ? matchRequest(series) : null;
  const searchingOther = !!series && query.trim() !== series.title.trim();
  const search = useMatchSearch(searchingOther ? query : '', request);
  const currentId = match?.media?.id ?? null;

  const list: MatchCandidate[] = searchingOther ? search.data ?? [] : match?.candidates ?? [];

  return (
    <Modal
      isOpen={series !== null}
      onClose={onClose}
      title={t('localMatchDialog.title')}
      size="xl"
      header={
        series && (
          <p className="text-xs text-text-muted">
            {tn(
              series.season ? 'localMatchDialog.forSeriesSeason' : 'localMatchDialog.forSeries',
              { title: <span className="select-text text-text-secondary">{series.title}</span> },
              { count: series.files.length, season: series.season ?? '' }
            )}
          </p>
        )
      }
      footer={
        series && (
          <>
            <Button variant="ghost" disabled={busy} onClick={() => onResearch(series)}>
              <RefreshCw className="h-3.5 w-3.5" aria-hidden="true" />
              <span>{t('localMatchDialog.matchAgain')}</span>
            </Button>
            <Button variant="secondary" onClick={onClose}>
              {t('common.close')}
            </Button>
          </>
        )
      }
    >
      {series && (
        <div className="flex flex-col gap-3">
          <label htmlFor="local-match-search" className="sr-only">
            {t('localMatchDialog.searchAniList')}
          </label>
          <div className="relative">
            <Search
              className="pointer-events-none absolute start-3 top-1/2 h-4 w-4 -translate-y-1/2 text-text-muted"
              aria-hidden="true"
            />
            <input
              id="local-match-search"
              type="search"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder={t('localMatchDialog.searchAniList')}
              spellCheck={false}
              autoComplete="off"
              className={`${INPUT_CLASS} w-full ps-9`}
            />
          </div>

          {searchingOther && (search.typing || search.isFetching) ? (
            <div className="flex justify-center py-8">
              <Spinner label={t('localMatchDialog.searching')} className="text-text-muted" />
            </div>
          ) : searchingOther && search.isError ? (
            <p role="alert" className="py-6 text-center text-xs text-state-error select-text">
              {errorMessage(search.error)}
            </p>
          ) : list.length === 0 ? (
            <p className="py-6 text-center text-xs text-text-muted">
              {searchingOther
                ? search.debouncedQuery.length < 2
                  ? t('localMatchDialog.typeMore')
                  : t('localMatchDialog.nothingFound')
                : t('localMatchDialog.noResults')}
            </p>
          ) : (
            <ul className="flex max-h-[55vh] flex-col gap-1 overflow-y-auto">
              {list.map((candidate) => (
                <CandidateRow
                  key={candidate.id}
                  candidate={candidate}
                  current={candidate.id === currentId && match?.status === 'confirmed'}
                  disabled={busy}
                  onPick={() => onPick(series, candidate)}
                />
              ))}
            </ul>
          )}
        </div>
      )}
    </Modal>
  );
};
