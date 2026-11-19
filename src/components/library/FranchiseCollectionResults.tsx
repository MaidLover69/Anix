/**
 * Library › Playlists › a franchise collection: the watch order of one franchise, built from
 * the details of its entries. When some are not known (relations a catalogue does not give, or
 * every catalogue down) it says so instead of showing a short list as if it were complete.
 */
import React from 'react';
import { Link } from 'react-router-dom';
import { ListOrdered } from 'lucide-react';
import { ShimmerImage } from '../ShimmerImage';
import { Spinner } from '../ui/Spinner';
import { EmptyState } from '../ui';
import { mediaFormatLabel } from './format';
import { useFranchiseOrder } from '../../hooks/useFranchise';
import { SavedNote } from '../SavedNote';
import { t } from '../../i18n';

export const FranchiseCollectionResults: React.FC<{ animeId: number }> = ({ animeId }) => {
  const { entries, state, loading } = useFranchiseOrder(animeId);

  if (entries.length === 0) {
    return loading ? (
      <Spinner label={t('watchOrder.findingMore')} className="text-text-muted" />
    ) : (
      <EmptyState
        icon={<ListOrdered className="w-8 h-8" aria-hidden="true" />}
        title={t('smartCollection.franchise')}
        description={t('franchise.unavailable')}
      />
    );
  }
  return (
    <div className="flex flex-col gap-4">
      <SavedNote kind="details" id={animeId} />
      <ol className="grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-x-4 gap-y-6">
        {entries.map(({ node, number, side, current }) => (
          <li key={node.id}>
            <Link to={`/detail/${node.id}`} draggable={false} className="flex flex-col gap-2" aria-current={current ? 'page' : undefined}>
              <ShimmerImage src={node.coverImage?.large || node.coverImage?.extraLarge || null} alt="" className="aspect-[2/3] w-full rounded-md" />
              <span className="text-[13px] font-medium leading-[18px] text-text-primary line-clamp-2">
                {number}. {node.title?.english || node.title?.userPreferred || node.title?.romaji || t('watchOrder.untitled')}
              </span>
              <span className="text-xs text-text-muted">
                {[side ? t('watchOrder.sideStory') : null, mediaFormatLabel(node.format ?? null), node.seasonYear].filter(Boolean).join(' · ')}
              </span>
            </Link>
          </li>
        ))}
      </ol>
      {(state === 'partial' || state === 'unavailable') && !loading && <p className="text-xs text-text-muted">{t('franchise.partial')}</p>}
      {loading && <Spinner size="sm" label={t('watchOrder.findingMore')} className="text-text-muted" />}
    </div>
  );
};
