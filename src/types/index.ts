export interface MediaTitle {
  romaji: string | null;
  english: string | null;
  native: string | null;
  user_preferred: string | null;
}

export interface CoverImage {
  extra_large: string | null;
  large: string | null;
  color: string | null;
}

export interface NextAiringEpisode {
  id: number;
  airing_at: number;
  time_until_airing: number;
  episode: number;
}

export interface Trailer {
  id: string | null;
  site: string | null;
  thumbnail: string | null;
}

export interface CharacterName {
  full: string | null;
  native: string | null;
}

export interface CharacterImage {
  large: string | null;
  medium: string | null;
}

export interface Character {
  id: number;
  name: CharacterName | null;
  image: CharacterImage | null;
}

export interface VoiceActor {
  id: number;
  name: CharacterName | null;
  image: CharacterImage | null;
  languageV2?: string | null;
}

export interface CharacterEdge {
  role: string | null;
  node: Character | null;
  voiceActors?: VoiceActor[];
}

export interface RelationEdge {
  relationType: string | null;
  node: {
    id: number;
    idMal?: number | null;
    title: {
      romaji: string | null;
      english: string | null;
      native: string | null;
      userPreferred: string | null;
    };
    coverImage: {
      large: string | null;
      extraLarge: string | null;
      color: string | null;
    };
    format: string | null;
    status: string | null;
    averageScore: number | null;
  };
}

export interface Media {
  id: number;
  id_mal: number | null;
  idMal?: number | null;
  imdb_id?: string | null;
  title: MediaTitle;
  cover_image: CoverImage;
  coverImage?: {
    large?: string | null;
    extraLarge?: string | null;
    medium?: string | null;
    color?: string | null;
  };
  banner_image: string | null;
  bannerImage?: string | null;
  description: string | null;
  synopsis_mal?: string | null;
  synopsis_imdb?: string | null;
  format: string | null;
  status: string | null;
  episodes: number | null;
  duration: number | null;
  season: string | null;
  season_year: number | null;
  seasonYear?: number | null;
  average_score: number | null; // AniList score (e.g. 87)
  averageScore?: number | null;
  score_mal?: number | null;    // MAL rating (e.g. 8.71)
  rating_imdb?: string | null;  // IMDb rating (e.g. "8.9")
  mean_score: number | null;
  meanScore?: number | null;
  popularity: number | null;
  favourites: number | null;
  genres: string[];
  is_adult: boolean;
  isAdult?: boolean;
  next_airing_episode?: NextAiringEpisode | null;
  nextAiringEpisode?: {
    id: number;
    airingAt: number;
    timeUntilAiring: number;
    episode: number;
  } | null;
  trailer?: Trailer | null;
  characters?: {
    edges: CharacterEdge[];
  } | null;
  relations?: {
    edges: RelationEdge[];
  } | null;
}

/** `fetch_anime_details` result: the raw AniList media object (camelCase). */
export interface AnimeDetails {
  id: number;
  idMal: number | null;
  title: {
    romaji: string | null;
    english: string | null;
    native: string | null;
    userPreferred: string | null;
  };
  coverImage: {
    large?: string | null;
    extraLarge?: string | null;
    color?: string | null;
  } | null;
  bannerImage: string | null;
  description: string | null;
  format: string | null;
  status: string | null;
  episodes: number | null;
  duration: number | null;
  season: string | null;
  seasonYear: number | null;
  averageScore: number | null;
  meanScore: number | null;
  popularity: number | null;
  favourites: number | null;
  isAdult: boolean | null;
  genres: string[] | null;
  nextAiringEpisode: {
    id: number;
    airingAt: number;
    timeUntilAiring: number;
    episode: number;
  } | null;
  trailer: Trailer | null;
  characters: { edges: CharacterEdge[] } | null;
  relations: { edges: RelationEdge[] } | null;
}

/** `fetch_mal_anime_synopsis` result (Jikan). */
export interface MalAnimeDetails {
  mal_id: number;
  title: string;
  title_english: string | null;
  title_japanese: string | null;
  synopsis: string | null;
  score: number | null;
  episodes: number | null;
}

export interface StreamItem {
  url: string;
  stream_type: string; // 'hls' | 'mp4' | 'dash'
  quality: string | null;
  audio?: string | null;
  subtitle_variant?: 'h-sub' | 's-sub' | null;
  referer?: string | null;
  origin?: string | null;
  headers?: Record<string, string> | null;
  is_active?: boolean;
}

export interface SubtitleItem {
  url: string;
  language: string;
  format: string;
  is_default?: boolean | null;
}

export interface SkipTimes {
  intro_start: number | null;
  intro_end: number | null;
  outro_start: number | null;
  outro_end: number | null;
}

export interface SourcesResult {
  streams: StreamItem[];
  subtitles: SubtitleItem[];
  skip: SkipTimes | null;
  /** Key of the provider that served `streams` (a key of the `get_all_episodes` result). */
  provider: string | null;
}

export interface EpisodeItem {
  number: number;
  pipe_id: string;
  title: string | null;
  image: string | null;
  synopsis?: string | null;
  filler: boolean;
}

export interface ProviderEpisodes {
  sub: EpisodeItem[];
  dub: EpisodeItem[];
}

export type EpisodesResult = Record<string, ProviderEpisodes>;

export interface HistoryEntry {
  anilist_id: number;
  title: string;
  cover: string | null;
  episode_number: number;
  episode_title: string | null;
  provider: string;
  category: 'sub' | 'dub' | string;
  position_ms: number;
  duration_ms: number;
  updated_at: number;
  from_remote: boolean;
}

export interface WatchlistEntry {
  anilist_id: number;
  title: string;
  cover: string | null;
  format: string | null;
  average_score: number | null;
  added_at: number;
}

export interface MediaListEntry {
  id: number;
  progress: number;
  score: number;
  status: string | null;
  title: string | null;
  cover: string | null;
  format_str: string | null;
  total_episodes: number | null;
  average_score: number | null;
}

export interface Viewer {
  id: number;
  name: string;
  avatar_url: string | null;
  anime_count: number;
  episodes_watched: number;
  minutes_watched: number;
  mean_score: number;
}

export interface AiringEntry {
  id: number;
  airing_at: number;
  episode: number;
  media_id: number;
  media_title: string;
  cover_image: string | null;
  banner_image: string | null;
  format: string | null;
  duration: number | null;
  average_score: number | null;
}

export interface NotificationPreference {
  media_id: number;
  enabled: boolean;
  media_title: string;
  cover_image: string | null;
}

export type DownloadStatus = 'QUEUED' | 'DOWNLOADING' | 'COMPLETED' | 'FAILED';

export interface DownloadRecord {
  id: string;
  anilist_id: number;
  episode_num: number;
  episode_title: string | null;
  series_title: string;
  series_cover: string | null;
  provider: string;
  category: string;
  quality: string;
  status: DownloadStatus | (string & {});
  /** 0.0–1.0 */
  progress: number;
  file_path: string | null;
  file_size: number | null;
  duration_s: number | null;
  error_msg: string | null;
  /** Unix time in milliseconds */
  created_at: number;
  /** Unix time in milliseconds */
  updated_at: number;
}

export interface StorageCheck {
  ok: boolean;
  free_bytes: number;
  needed_bytes: number;
}

export interface UpdateInfo {
  version: string;
  changelog: string;
  published_at: string;
  release_url: string;
}

export interface HomeData {
  trending: Media[];
  popular: Media[];
  top_rated: Media[];
  newest: Media[];
  continue_watching: HistoryEntry[];
  /** Set when AniList could not be reached and only local data is present. */
  error?: string | null;
}

export type ExternalPlayer = 'builtin' | 'mpv' | 'vlc';
export type DohProvider = 'cloudflare' | 'google' | 'adguard';

// Every key here has a reader. `settings.json` also holds the account keys (`anilist_*`,
// `mal_*`), which only the backend reads and writes; see `AccountsStatus` for what the
// frontend is told about accounts.
export interface AppSettings {
  autoplay: boolean;
  auto_skip_intro_outro: boolean;
  prefer_dub: boolean;
  /** 'auto' or a provider key (a key of the `get_all_episodes` result). */
  preferred_provider: string;
  external_player: ExternalPlayer;
  hide_adult_content: boolean;
  release_notifications: boolean;
  enable_doh: boolean;
  doh_provider: DohProvider;
  download_quality: string;
  download_dir: string;
}

export type AccountService = 'anilist' | 'mal';

/** What the frontend knows about one account. Tokens stay in the backend. */
export interface AccountStatus {
  connected: boolean;
  name: string | null;
  avatar_url: string | null;
}

/** `get_account_status` result. */
export interface AccountsStatus {
  anilist: AccountStatus;
  mal: AccountStatus;
}

/** `account-login` event: how a sign-in window ended. */
export interface AccountLoginEvent {
  service: AccountService;
  outcome: 'connected' | 'cancelled' | 'failed';
  name: string | null;
  message: string | null;
}

/** `account-changed` event: an account was connected, disconnected or signed out. */
export interface AccountChangedEvent {
  service: AccountService;
  connected: boolean;
  /** Set when the service ended the session rather than the user. */
  message: string | null;
}

/** `account-synced` event: a sync changed the remote list entry of one anime. */
export interface AccountSyncedEvent {
  service: AccountService;
  anilist_id: number;
}

/** What happened to the MyAnimeList copy of an entry saved from the AniList tab. */
export type MalMirror =
  | { state: 'not_connected' | 'no_mal_id' | 'saved' }
  | { state: 'failed'; message: string };

/** `save_anilist_entry` result. */
export interface SavedListEntry {
  entry: MediaListEntry;
  mal: MalMirror;
}

export interface AnimeThemeEntry {
  theme_type: string; // "OP" or "ED"
  sequence: number | null;
  slug: string;
  song_title: string | null;
  artist_name: string | null;
  episodes: string | null;
  video_url: string | null;
  audio_url: string | null;
  resolution: number | null;
}

export interface LocalAnimeFile {
  file_path: string;
  file_name: string;
  file_size: number;
  parsed_title: string;
  parsed_episode: number | null;
}

export interface CustomPlaylist {
  id: string;
  name: string;
  description: string | null;
  /** Unix time; older rows are in seconds, newer ones in milliseconds */
  created_at: number;
  updated_at: number;
  item_count: number;
  covers: string[];
}

export interface CustomPlaylistItem {
  id: string;
  playlist_id: string;
  anilist_id: number;
  mal_id: number | null;
  episode_num: number;
  episode_title: string | null;
  series_title: string;
  series_cover: string | null;
  category: string;
  sort_order: number;
  added_at: number;
}

export interface ExternalPlaylistItem {
  url: string;
  title: string;
  referer?: string | null;
}

/** A plugin repository the user added in Settings (`list_plugin_repos`). */
export interface PluginRepo {
  /** The link as it was added; also the argument of `remove_plugin_repo`. */
  url: string;
  name: string;
  plugin_count: number;
  /** Unix milliseconds of the last successful download. */
  updated_at_ms: number;
  error: string | null;
}

export type PluginStatus = 'ready' | 'disabled' | 'error';

/** A streaming plugin (`list_plugins`). Its `id` is the provider key used everywhere else. */
export interface PluginInfo {
  id: string;
  name: string;
  version: string;
  author: string;
  description: string;
  icon: string | null;
  lang: string;
  repo_url: string;
  enabled: boolean;
  status: PluginStatus;
  error: string | null;
  supports_dub: boolean;
  servers: string[];
}
