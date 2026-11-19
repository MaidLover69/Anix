# API the RED tests of #6 (rest of collections) expect

Tests: `collections_v142_tests.rs`, the `studio_*` tests at the end of `catalogue_tests.rs`, `scripts/collections-test.mjs`.
Every database change only adds (new column, new table rows). Texts need i18n keys in all locales (list in the script).

## Rust, `anix_lib`
- `catalogue::model::SearchQuery.studio: Option<String>` (name; trimmed; blank = none). `keeps` does not look at it.
- `catalogue::Endpoints.shikimori_studios: String` (default `https://shikimori.io/api/studios`, a JSON array of `{id, name, filtered_name}`); the test helper sets it to `<base>/shiki_studios`.
- AniList `search` with a studio: ONE request `Studio(search: $studio, ...) { media(...) { nodes {..} } }`, variable `studio`; the other filters (genre, format, status, score, year, season, adult) are applied in the app with `SearchQuery::keeps`; `Studio: null` = empty list.
- Shikimori `search` with a studio: looks the name up (case-insensitive, `name` or `filtered_name`) in `shikimori_studios`, sends `animes(studio: $studio, ...)` with the id as a string; ids mapped as for any Shikimori answer; unknown studio = empty list (Ok).
- MAL, Kitsu, Comick, Jikan, IMDb: `model::unsupported(..)` for a studio query before any request. Both AniList and Shikimori failing gives the usual offline error.
- `offline::Kind::Collection` (key = playlist id): never aged out by `prune`, large `max_rows`; `prune` must not delete it (its stray-kind DELETE list needs updating).
- `offline::store_from_at(pool, kind, key, &Value, source: Option<&str>, now)` and `store_from(..)` (now = clock); new column `anilist_cache.source TEXT` added by migration (rows without it still load).
- `offline::load_saved(pool, kind, key, now) -> Result<Option<SavedPage>>`; `SavedPage { value: Value, saved_at: i64, source: Option<String>, age_s: i64 /* >= 0 */ }`.
- `offline::query_key(&SearchQuery) -> String`: equals `search_key(..)` for a plain query (old rows still match), differs by studio (trim + case-insensitive), min_score, year, season, exclude genres.
- `models::WatchlistEntry.episodes: Option<i32>` (`#[serde(default)]`); new column `watchlist.episodes INTEGER`; `db::get_watchlist` returns it.
- `db::set_watchlist_episodes(pool, anilist_id: i32, Option<i32>) -> Result<()>`: updates an existing row only; `None` never erases a known count. (`add_to_watchlist` command also takes `episodes`.)
- `accounts::sync::waiting_changes(pool) -> Result<usize, AppError>`: number of distinct anime with a list change not yet sent (`watchlist.synced = 0` or a row in `account_sync_queue`).
- New module `image_cache` (register in `lib.rs`): consts `DEFAULT_LIMIT_MB = 200`, `MIN_LIMIT_MB`, `MAX_LIMIT_MB` (200 strictly between), `SETTING_KEY = "image_cache_mb"`; `limit_bytes(&Value) -> u64` (MiB; default on missing / wrong type; clamped);
  `used_bytes(dir) -> u64`; `prune(dir, limit_bytes) -> io::Result<PruneReport { removed: usize, freed: u64, kept_bytes: u64 }>` (oldest modified first; missing dir = nothing);
  `path_for(dir, url) -> PathBuf` (stable hash name directly inside `dir`, no URL text in the path); `save(dir, url, bytes, limit_bytes) -> io::Result<PathBuf>` (writes, then prunes, keeps the new file); `find(dir, url) -> Option<PathBuf>`.
- Settings key `image_cache_mb` (number, MB) in `settings.json`; default 200 in `src/hooks/useSettings.ts` and typed in `src/types/index.ts`; Settings > System row `settings.imageCacheLimit`.
- Migration test uses the local copy `testing/anix-v15/db-1.4.x/` (copied to a temp dir; skipped with a message when absent).

## TypeScript, `src/lib`
- `smartRule.ts`: `studio` is a kept rule key (<= 80 chars, same `clean`); `RuleSearch.studio?: string`; `ruleForFranchise(id: number): string` (`franchise=<id>`, throws for a non positive-integer id); `franchiseFromRule(rule): number | null` (positive integer <= 1e12 only); a franchise rule yields an empty search from `searchFromRule`.
- `searchArgs.ts` (new): `searchInvokeArgs(params): Record<string, unknown>` = exactly the `search_anime` arguments (`query, genres, format, status, sort, page, perPage, excludeGenres, minScore, year, season, studio`; empty = `null`; sort default `POPULARITY_DESC`, page 1, perPage 24). `useSearchAnime` and Discover use it; `SearchParams.studio?`; the Rust command `search_anime` takes `studio: Option<String>` and keys the saved copy with `query_key`.
- `watchOrder.ts`: `franchiseOrder(currentId, details: Map<number, OrderDetails>) -> { state: 'complete' | 'partial' | 'unavailable', entries: OrderEntry[], missing: number[] }`. unavailable = no details of the anime (missing = [id]); partial = relations unknown (null/absent) or chain ends not fetched; complete = relations read and nothing missing. Reuses `watchOrder`.
- `collage.ts` (new): `collageCovers(covers: (string|null|undefined)[] | null | undefined, max = 4): string[]` (skips blank, one use per url, first come first used, input untouched); `PlaylistCollage` uses it.
- `savedNote.ts` (new): `savedAge(ageSeconds): { unit: 'now'|'minutes'|'hours'|'days', count }` (< 60 s and NaN/negative = now).
- i18n keys, all locales: `discover.studio`, `discover.studioPlaceholder`, `smartCollection.studio`, `smartCollection.franchise`, `franchise.partial`, `franchise.unavailable`, `offline.savedPage`, `offline.savedPageNoSource`, `offline.ageNow`, `offline.ageMinutes`, `offline.ageHours`, `offline.ageDays`, `offline.changesWaiting`, `settings.imageCacheLimit`, `settings.imageCacheLimitHint`.
