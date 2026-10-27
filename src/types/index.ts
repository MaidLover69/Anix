import type { ThemeId } from '../theme/themes';
import type { TimeFormat } from '../lib/time';

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
    seasonYear?: number | null;
    isAdult?: boolean | null;
  };
}

/** One entry of `recommendations.nodes` in the details (AniList, or MyAnimeList while it is down). */
export interface RecommendationNode {
  mediaRecommendation: RelationEdge['node'] | null;
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
  recommendations?: { nodes: RecommendationNode[] } | null;
  studios?: { edges: { isMain: boolean; node: { id: number; name: string } }[] } | null;
  startDate?: FuzzyDate | null;
  endDate?: FuzzyDate | null;
  tags?: { name: string; rank: number | null; isMediaSpoiler: boolean | null }[] | null;
  /** The catalogue that answered while AniList was unavailable (absent for AniList's own answer). */
  servedBy?: string;
}

/** AniList's date with optional parts. */
export interface FuzzyDate {
  year?: number | null;
  month?: number | null;
  day?: number | null;
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
  /** ISO 639-1 code of the subtitle language (sub) or audio language (dub), when known. */
  language?: string | null;
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
  status: ListStatusValue;
  /** False while a status chosen here waits to be sent to the connected lists. */
  synced: boolean;
}

/** A watchlist status, in AniList vocabulary. */
export type ListStatusValue = 'CURRENT' | 'PLANNING' | 'COMPLETED' | 'PAUSED' | 'DROPPED';

/** The status shown for one anime and where it came from. */
export interface ListStatus {
  status: ListStatusValue | null;
  synced: boolean;
  source: 'anilist' | 'myanimelist' | 'local';
}

/** One entry of the notification centre. */
export interface Notice {
  id: number;
  /** 'airing', 'download', 'test' or another short word. */
  kind: string;
  title: string;
  body: string;
  /** An in-app route or an anix:// link. */
  link: string | null;
  read: boolean;
  /** Unix milliseconds. */
  created_at: number;
}

export interface BackupResult {
  path: string;
  summary: {
    counts: Record<string, number>;
    has_logins: boolean;
    logins_restored: boolean;
    search_history: unknown;
  };
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
  /** 0 in an estimated schedule when the number is not known (a long-running show). */
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
  /** ISO 639-1 audio language ('ja' for a sub), once the source is known. */
  audio_lang?: string | null;
  /** ISO 639-1 subtitle language of a sub, once the source is known. */
  sub_lang?: string | null;
}

export interface StorageCheck {
  ok: boolean;
  free_bytes: number;
  needed_bytes: number;
  /** Size of the disk the download folder is on; 0 when unknown. */
  total_bytes: number;
  /** The download folder. */
  dir: string;
}

export interface UpdateInfo {
  version: string;
  changelog: string;
  published_at: string;
  release_url: string;
  /** The release's AppImage, when the website lists it with a checksum. */
  appimage?: { url: string; sha256: string; size: number | null };
}

/** `get_update_install_support`: whether Install can replace the running AppImage. */
export interface UpdateInstallSupport {
  appimage: boolean;
  reason: string | null;
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
/** `auto`: the system DNS until a site looks blocked, then DNS over HTTPS for that site. */
export type DohMode = 'off' | 'on' | 'auto';
/** The server last picked for an anime, tried first next time. */
export interface RememberedSource {
  provider: string;
  category: 'sub' | 'dub';
  at: number;
}
/** Which anime catalogue is asked first; the others stay as fallbacks. */
export type CatalogueSource = 'auto' | 'anilist' | 'myanimelist' | 'kitsu' | 'shikimori' | 'comick';

// Every key here has a reader. `settings.json` also holds the account keys (`anilist_*`,
// `mal_*`), which only the backend reads and writes; see `AccountsStatus` for what the
// frontend is told about accounts.
export interface AppSettings {
  autoplay: boolean;
  /** 1.2.0's single switch; read once to fill `skip_intro` and `skip_outro`. */
  auto_skip_intro_outro: boolean;
  /** Jump past openings / endings (after a 3-second countdown) when skip times are known. */
  skip_intro: boolean;
  skip_outro: boolean;
  prefer_dub: boolean;
  /** 'auto' or a provider key (a key of the `get_all_episodes` result). */
  preferred_provider: string;
  external_player: ExternalPlayer;
  hide_adult_content: boolean;
  release_notifications: boolean;
  /** 1.2.0's switch; `doh_mode` replaces it and keeps its value. */
  enable_doh: boolean;
  doh_mode: DohMode;
  doh_provider: DohProvider;
  catalogue_source: CatalogueSource;
  download_quality: string;
  download_dir: string;
  /** A built-in theme id (see `theme/themes.ts`). */
  theme: ThemeId;
  /** Closing the window hides it in the tray instead of quitting. */
  minimize_to_tray: boolean;
  /** An autostart entry starts the app hidden in the tray at login. */
  start_with_desktop: boolean;
  /** ISO 639-1 codes of the subtitle languages wanted with Japanese audio, most wanted first. */
  preferred_sub_languages: string[];
  /** ISO 639-1 codes of the dub audio languages wanted, most wanted first. */
  preferred_dub_languages: string[];
  /** Auto only asks plugins in the languages above. */
  only_preferred_languages: boolean;
  /** Where the title bar puts the window buttons: as the desktop does, or always left/right. */
  window_buttons: WindowButtonsSetting;
  /** The release the update window last opened for by itself (`shouldPromptUpdate`). */
  update_prompted_version?: string;
  /** The desktop's accent colour replaces the theme's (not in Manga, which has no colour). */
  use_system_accent: boolean;
  /** Mascot, kaomoji, friendlier wording and the completed stamp. */
  playful_touches: boolean;
  /** Hours after an air time during which extensions are asked whether the episode is out. */
  release_check_hours: number;
  /** "Alerts while Anix is closed": a windowless `--background` process is started at login. */
  background_alerts: boolean;
  /** Which engine plays streams; a change applies after a restart. */
  player_engine: PlayerEngineSetting;
  /** Embedded mpv's hardware decoding: copy-back or full interop after the self-test, always copy-back, or software. */
  hwdec_mode: HwdecModeSetting;
  /** Embedded mpv's read-ahead buffer, MB and seconds. */
  player_buffer_mb: number;
  player_readahead_secs: number;
  /** Embedded mpv keeps its cache on disk (in the app's cache folder), up to `player_disk_cache_mb`. */
  player_disk_cache: boolean;
  player_disk_cache_mb: number;
  /** Home row ids in the order chosen in Settings › Appearance; rows missing here follow. */
  home_row_order?: string[];
  /** Plugin ids in the order Auto tries them (Settings › Sources); plugins missing here follow. */
  provider_order?: string[];
  /** Settings › Plugins: test the enabled plugins in the background about once a day. */
  plugin_health_checks?: boolean;
  /** The server last picked per anime (AniList id), tried first on its next episodes. */
  remembered_sources?: Record<string, RememberedSource>;
  /** Settings › Appearance: blur the stills of episodes not watched yet (spoiler guard). */
  blur_unwatched_art?: boolean;
  /** Settings › Appearance: a custom accent colour (#rrggbb); the system accent wins while that is on. */
  accent_color?: string | null;
  /** Settings › Appearance: how strongly text and hairlines stand out. */
  contrast?: ContrastLevel;
  /** Settings › Appearance: the root font size every rem-based size follows. */
  ui_size?: UiSize;
  /** Settings › Appearance: clock times in 12 or 24 hours, or as the system writes them. */
  time_format?: TimeFormat;
  /** Settings › Appearance: an anime's art behind the app (`null`: none). */
  background_art?: BackgroundArt | null;
  /** Settings › Appearance › Language: an interface language code, or 'system' (missing = 'system'). */
  app_language?: string;
  /** The app version the setup screen was last shown in (missing: never, so it opens). */
  setup_seen_version?: string;
}

/** 'external' opens every episode in the external player chosen in Settings. */
export type PlayerEngineSetting = 'web' | 'mpv' | 'external';
export type HwdecModeSetting = 'auto' | 'copy' | 'off';

export type WindowButtonsSetting = 'desktop' | 'left' | 'right';

export type ContrastLevel = 'standard' | 'medium' | 'high';
export type UiSize = 'small' | 'medium' | 'large';
export type { TimeFormat };

/** The art chosen on an anime's page ("Use as background"), and how it is drawn. */
export interface BackgroundArt {
  url: string;
  title: string;
  /** 'blur' softens it; 'plain' only dims it (cheaper to draw). */
  style: 'blur' | 'plain';
}

export type WindowButtonKind = 'minimize' | 'maximize' | 'close';

/** `get_desktop_appearance` result. */
export interface DesktopAppearance {
  button_layout: { left: WindowButtonKind[]; right: WindowButtonKind[] };
  button_layout_source: 'gnome' | 'kde' | 'default';
  /** `#rrggbb` */
  accent_color: string | null;
  accent_source: 'portal' | 'gnome' | 'kde' | null;
}

/** `get_watched_history` row: one watched episode. */
export interface WatchedEpisodeRow {
  anilist_id: number;
  title: string;
  cover: string | null;
  episode_number: number;
  /** 0 or 1 when the length is not known. */
  duration_ms: number;
  updated_at: number;
}

/** `fetch_anilist_stats` result. */
export interface AniListStatsEntry {
  media_id: number;
  title: string;
  cover: string | null;
  status: string;
  /** 0..100; 0 = not scored. */
  score: number;
  progress: number;
  repeat: number;
  episodes: number | null;
  /** Minutes per episode. */
  duration: number | null;
  format: string | null;
  genres: string[];
  studios: string[];
  completed_year: number | null;
  /** Unix seconds. */
  updated_at: number;
}

export interface AniListStats {
  entries: AniListStatsEntry[];
  voice_actors: { id: number; name: string; image: string | null; count: number }[];
  voice_actor_titles: number;
}

export type AccountService = 'anilist' | 'mal' | 'comick';

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
  comick: AccountStatus;
  /** False when this build has no Comick client id: Comick cannot be connected. */
  comick_available: boolean;
}

/** One anime of the connected Comick library (`get_comick_library`). */
export interface ComickLibraryEntry {
  hid: string;
  title: string;
  /** 1 watching, 2 completed, 3 on hold, 4 dropped, 5 plan to watch. */
  status: number;
  /** Saved episode as text ("12", "12.5"), or null. */
  progress: { number: string | null } | null;
  rating: number | null;
  cover_url: string | null;
  total_units: number | null;
  url: string | null;
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
  /** ISO 639-1 codes of the subtitle languages it serves (with Japanese audio). */
  sub_languages: string[];
  /** ISO 639-1 codes of the audio languages it serves dubs in. */
  dub_languages: string[];
  servers: string[];
  /** The plugin has settings of its own (`get_plugin_config`). */
  configurable: boolean;
}

/** One setting of a plugin (`get_plugin_config`). */
export interface PluginConfigField {
  name: string;
  label: string;
  /** `switch` values are "true" or "false". */
  type: 'text' | 'switch' | 'select';
  default: string;
  value: string;
  options?: { value: string; label: string }[];
}

export interface PluginConfig {
  fields: PluginConfigField[];
  requires_config: boolean;
  configured: boolean;
}

export * from './system';
