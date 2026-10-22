/**
 * Languages of streams. A sub is Japanese audio with subtitles in a language, a dub is audio
 * in a language. Codes are ISO 639-1; the backend (`providers/languages.rs`) uses the same
 * table for matching, so labels normalise the same way on both sides.
 */
import type { PluginInfo } from '../types';
import { language, locale, t } from '../i18n';

interface LanguageEntry {
  code: string;
  name: string;
  /** Other spellings a site may label a track with, lower case. */
  aliases: string[];
}

const LANGUAGES: LanguageEntry[] = [
  { code: 'en', name: 'English', aliases: ['eng', 'inglés', 'ingles', 'anglais', 'englisch', 'inglese', 'inglês'] },
  { code: 'ja', name: 'Japanese', aliases: ['jpn', 'jap', '日本語', 'japonés', 'japonais', 'japanisch', 'giapponese'] },
  { code: 'es', name: 'Spanish', aliases: ['spa', 'español', 'espanol', 'castellano', 'latino', 'espagnol', 'spanisch', 'spagnolo', 'espanhol'] },
  { code: 'pt', name: 'Portuguese', aliases: ['por', 'português', 'portugues', 'portugués', 'brazilian', 'portuguais', 'portugiesisch', 'portoghese'] },
  { code: 'fr', name: 'French', aliases: ['fre', 'fra', 'français', 'francais', 'francés', 'französisch', 'francese', 'francês'] },
  { code: 'de', name: 'German', aliases: ['ger', 'deu', 'deutsch', 'alemán', 'aleman', 'allemand', 'tedesco', 'alemão'] },
  { code: 'it', name: 'Italian', aliases: ['ita', 'italiano', 'italien', 'italienisch'] },
  { code: 'ru', name: 'Russian', aliases: ['rus', 'русский', 'ruso', 'russe', 'russisch', 'russo'] },
  { code: 'ar', name: 'Arabic', aliases: ['ara', 'العربية', 'عربي', 'árabe', 'arabe', 'arabisch', 'arabo'] },
  { code: 'hi', name: 'Hindi', aliases: ['hin', 'हिन्दी', 'हिंदी'] },
  { code: 'id', name: 'Indonesian', aliases: ['ind', 'bahasa indonesia', 'indonesia'] },
  { code: 'ms', name: 'Malay', aliases: ['may', 'msa', 'bahasa melayu', 'melayu'] },
  { code: 'th', name: 'Thai', aliases: ['tha', 'ไทย'] },
  { code: 'vi', name: 'Vietnamese', aliases: ['vie', 'tiếng việt', 'tieng viet'] },
  { code: 'tr', name: 'Turkish', aliases: ['tur', 'türkçe', 'turkce'] },
  { code: 'pl', name: 'Polish', aliases: ['pol', 'polski'] },
  { code: 'nl', name: 'Dutch', aliases: ['dut', 'nld', 'nederlands'] },
  { code: 'sv', name: 'Swedish', aliases: ['swe', 'svenska'] },
  { code: 'no', name: 'Norwegian', aliases: ['nor', 'nob', 'nb', 'norsk', 'bokmål'] },
  { code: 'da', name: 'Danish', aliases: ['dan', 'dansk'] },
  { code: 'fi', name: 'Finnish', aliases: ['fin', 'suomi'] },
  { code: 'cs', name: 'Czech', aliases: ['cze', 'ces', 'čeština', 'cestina'] },
  { code: 'sk', name: 'Slovak', aliases: ['slo', 'slk', 'slovenčina'] },
  { code: 'hu', name: 'Hungarian', aliases: ['hun', 'magyar'] },
  { code: 'ro', name: 'Romanian', aliases: ['rum', 'ron', 'română', 'romana'] },
  { code: 'bg', name: 'Bulgarian', aliases: ['bul', 'български'] },
  { code: 'uk', name: 'Ukrainian', aliases: ['ukr', 'українська'] },
  { code: 'el', name: 'Greek', aliases: ['gre', 'ell', 'ελληνικά'] },
  { code: 'he', name: 'Hebrew', aliases: ['heb', 'iw', 'עברית'] },
  { code: 'fa', name: 'Persian', aliases: ['per', 'fas', 'farsi', 'فارسی'] },
  { code: 'ur', name: 'Urdu', aliases: ['urd', 'اردو'] },
  { code: 'bn', name: 'Bengali', aliases: ['ben', 'bangla', 'বাংলা'] },
  { code: 'ta', name: 'Tamil', aliases: ['tam', 'தமிழ்'] },
  { code: 'te', name: 'Telugu', aliases: ['tel', 'తెలుగు'] },
  { code: 'ml', name: 'Malayalam', aliases: ['mal', 'മലയാളം'] },
  { code: 'kn', name: 'Kannada', aliases: ['kan', 'ಕನ್ನಡ'] },
  { code: 'mr', name: 'Marathi', aliases: ['mar', 'मराठी'] },
  { code: 'zh', name: 'Chinese', aliases: ['chi', 'zho', '中文', '简体中文', '繁體中文', 'mandarin', 'chinese (simplified)', 'chinese (traditional)'] },
  { code: 'ko', name: 'Korean', aliases: ['kor', '한국어'] },
  { code: 'tl', name: 'Filipino', aliases: ['fil', 'tgl', 'tagalog'] },
  { code: 'hr', name: 'Croatian', aliases: ['hrv', 'hrvatski'] },
  { code: 'sr', name: 'Serbian', aliases: ['srp', 'српски', 'srpski'] },
  { code: 'ca', name: 'Catalan', aliases: ['cat', 'català'] },
  { code: 'lt', name: 'Lithuanian', aliases: ['lit', 'lietuvių'] },
  { code: 'lv', name: 'Latvian', aliases: ['lav', 'latviešu'] },
  { code: 'et', name: 'Estonian', aliases: ['est', 'eesti'] },
];

/** Every language the app knows, by English name. */
export const KNOWN_LANGUAGES: { code: string; name: string }[] = LANGUAGES.map(({ code, name }) => ({
  code,
  name,
})).sort((a, b) => a.name.localeCompare(b.name));

/** Every language the app knows, named and sorted in the interface language (for pickers). */
export function knownLanguages(): { code: string; name: string }[] {
  const lang = locale();
  return LANGUAGES.map(({ code }) => ({ code, name: languageLabel(code) })).sort((a, b) =>
    a.name.localeCompare(b.name, lang)
  );
}

export const DEFAULT_LANGUAGES = ['en'];

let displayNames: { locale: string; names: Intl.DisplayNames | null } | null = null;

/**
 * The name of a language code in the interface language, as it reads inside a sentence
 * ("anglais" in French), or the code in capitals when it is not known.
 */
export function languageName(code: string): string {
  const known = LANGUAGES.find((entry) => entry.code === code.toLowerCase());
  if (!known) return code.toUpperCase();
  if (language() === 'en') return known.name;
  if (displayNames?.locale !== locale()) {
    let names: Intl.DisplayNames | null = null;
    try {
      names = new Intl.DisplayNames([locale()], { type: 'language' });
    } catch {
      // Without Intl.DisplayNames the English name is shown.
    }
    displayNames = { locale: locale(), names };
  }
  const local = displayNames.names?.of(known.code);
  return local && local.toLowerCase() !== known.code ? local : known.name;
}

/** `languageName` starting with a capital, for lists and pickers ("Anglais"). */
export function languageLabel(code: string): string {
  const name = languageName(code);
  return name.charAt(0).toLocaleUpperCase(locale()) + name.slice(1);
}

function lookup(candidate: string): string | null {
  const text = candidate.trim();
  if (!text) return null;
  const found = LANGUAGES.find(
    (entry) => entry.code === text || entry.name.toLowerCase() === text || entry.aliases.includes(text)
  );
  return found ? found.code : null;
}

/**
 * The ISO 639-1 code for a language label ("English", "en-US", "Español", "pt-BR",
 * "English [CC]"), or null when it names no known language.
 */
export function normalizeLanguage(label: string | null | undefined): string | null {
  const text = (label ?? '').trim().toLowerCase();
  if (!text) return null;
  const whole = lookup(text);
  if (whole) return whole;
  const primary = text.split(/[-_]/)[0];
  if (primary.length >= 2 && primary.length <= 3 && primary.length < text.length) {
    const code = lookup(primary);
    if (code) return code;
  }
  const head = text.split(/[([\-,/|]/)[0];
  if (head.length < text.length) {
    const code = lookup(head);
    if (code) return code;
  }
  for (const word of text.split(/\s+/)) {
    const bare = word.replace(/^[^\p{L}\p{N}]+|[^\p{L}\p{N}]+$/gu, '');
    if ([...bare].length > 3) {
      const code = lookup(bare);
      if (code) return code;
    }
  }
  return null;
}

export type StreamCategory = 'sub' | 'dub';

/** "English subtitles" for a sub, "Hindi audio" for a dub. */
export function versionLabel(code: string, category: StreamCategory): string {
  return t(category === 'dub' ? 'languages.audioIn' : 'languages.subtitlesIn', { language: languageName(code) });
}

/** The languages a plugin serves for a category. */
export function pluginLanguages(plugin: PluginInfo | undefined, category: StreamCategory): string[] {
  if (!plugin) return [];
  return (category === 'dub' ? plugin.dub_languages : plugin.sub_languages) ?? [];
}

/** The language a provider is listed under: the most wanted one it serves, else its first. */
export function primaryLanguage(languages: string[], wanted: string[]): string | null {
  return wanted.find((code) => languages.includes(code)) ?? languages[0] ?? null;
}

export interface LanguageGroup<T> {
  /** ISO code, or null for providers that do not say. */
  language: string | null;
  label: string;
  items: T[];
}

/**
 * Splits providers into groups by the language they would be played in: groups of wanted
 * languages first (in preference order), then the others by name, then providers that do
 * not say. Items keep their order within a group.
 */
export function groupByLanguage<T>(
  items: T[],
  languagesOf: (item: T) => string[],
  wanted: string[],
  category: StreamCategory
): LanguageGroup<T>[] {
  const groups = new Map<string | null, T[]>();
  for (const item of items) {
    const language = primaryLanguage(languagesOf(item), wanted);
    const list = groups.get(language) ?? [];
    list.push(item);
    groups.set(language, list);
  }
  const rank = (language: string | null) => {
    if (language === null) return Number.MAX_SAFE_INTEGER;
    const position = wanted.indexOf(language);
    return position === -1 ? wanted.length : position;
  };
  return [...groups.entries()]
    .sort(
      ([a], [b]) =>
        rank(a) - rank(b) || languageName(a ?? '').localeCompare(languageName(b ?? ''))
    )
    .map(([language, list]) => ({
      language,
      label: language ? sourceLabel(language, category) : t('languages.notStated'),
      items: list,
    }));
}

/** A plugin's languages as one short line: "English, Spanish subtitles · Russian audio". */
export function pluginLanguageSummary(plugin: PluginInfo): string {
  const names = (codes: string[]) => codes.map(languageName).join(t('languages.listSeparator'));
  return [
    plugin.sub_languages?.length ? t('languages.subtitlesIn', { language: names(plugin.sub_languages) }) : null,
    plugin.dub_languages?.length ? t('languages.audioIn', { language: names(plugin.dub_languages) }) : null,
  ]
    .filter(Boolean)
    .join(' · ');
}

/**
 * How a source reads wherever one is listed (server list, player bar, directory, downloads,
 * alerts): "Japanese audio · English subtitles" for a sub, "Hindi audio" for a dub.
 */
export function sourceLabel(code: string | null | undefined, category: StreamCategory): string {
  if (category === 'dub') return code ? versionLabel(code, 'dub') : t('languages.dubbedAudio');
  return code
    ? t('languages.japaneseAudioWith', { subtitles: versionLabel(code, 'sub') })
    : t('languages.japaneseAudio');
}

/** "Subtitles only" / "Dub only" for a plugin that serves just one of them, else null. */
export function audioOnlyLabel(plugin: Pick<PluginInfo, 'sub_languages' | 'dub_languages'> | undefined): string | null {
  if (!plugin) return null;
  const subs = plugin.sub_languages?.length ?? 0;
  const dubs = plugin.dub_languages?.length ?? 0;
  if (subs > 0 && dubs === 0) return t('languages.subtitlesOnly');
  if (dubs > 0 && subs === 0) return t('languages.dubOnly');
  return null;
}

/** Language names for a list of codes, as the extension directory shows them: "English, Hindi". */
export function languageList(codes: string[]): string {
  return codes.map(languageLabel).join(t('languages.listSeparator'));
}

interface StreamFacts {
  streams: { quality: string | null; subtitle_variant?: string | null }[];
  subtitles: unknown[];
}

/**
 * What a source's first stream is like, when the plugin said: "Switchable subtitles · 1080p",
 * "Subtitles burnt in · 720p". Null when it said nothing about either.
 */
export function describeStream(sources: StreamFacts | null | undefined, category: StreamCategory): string | null {
  const stream = sources?.streams[0];
  if (!stream) return null;
  const subtitles =
    category === 'dub'
      ? null
      : sources.subtitles.length > 0 || stream.subtitle_variant === 's-sub'
        ? t('languages.switchableSubtitles')
        : stream.subtitle_variant === 'h-sub'
          ? t('languages.burntInSubtitles')
          : null;
  const resolution = /(\d{3,4})p\b/i.exec(stream.quality ?? '')?.[1];
  const parts = [subtitles, resolution ? `${resolution}p` : null].filter(Boolean);
  return parts.length > 0 ? parts.join(' · ') : null;
}
