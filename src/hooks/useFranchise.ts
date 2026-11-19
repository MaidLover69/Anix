import { useEffect, useMemo, useState } from 'react';
import { useQueries } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { useAnimeDetails } from './useAniList';
import { MAX_ENTRIES, franchiseOrder, type OrderDetails } from '../lib/watchOrder';

/**
 * The watch order of the franchise `id` belongs to. Prequels and sequels are fetched one after
 * another (from the same cache as their own pages) until the chain ends. Pass `root` when the
 * anime's details are already at hand; otherwise they are fetched too.
 */
export function useFranchiseOrder(id: number, root?: OrderDetails | null) {
  const rootQuery = useAnimeDetails(root ? null : id);
  const first: OrderDetails | null = root ?? rootQuery.data ?? null;
  const [ids, setIds] = useState<number[]>([]);
  const results = useQueries({
    queries: ids.map((other) => ({
      queryKey: ['animeDetails', other],
      queryFn: () => invoke<OrderDetails>('fetch_anime_details', { id: other }), // i18n-ignore
      staleTime: 15 * 60 * 1000,
      retry: 1,
    })),
  });

  const known = useMemo(() => {
    const map = new Map<number, OrderDetails>();
    if (first) map.set(id, first);
    results.forEach((r, i) => r.data && map.set(ids[i], r.data));
    return map;
    // `results` is a new array each render; its data is what matters.
  }, [first, id, ids, results.map((r) => r.dataUpdatedAt).join()]);

  const order = useMemo(() => franchiseOrder(id, known), [id, known]);
  const loading = rootQuery.isFetching || results.some((r) => r.isFetching);

  useEffect(() => {
    const add = order.missing.filter((other) => other !== id && !ids.includes(other));
    if (add.length > 0 && ids.length < MAX_ENTRIES) setIds((prev) => [...prev, ...add].slice(0, MAX_ENTRIES));
  }, [order.missing, ids, id]);

  return { ...order, loading };
}
