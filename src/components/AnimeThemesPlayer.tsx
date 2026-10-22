import React, { useState } from 'react';
import { Music, Video, Volume2 } from 'lucide-react';
import { useAnimeThemes } from '../hooks/useAniList';
import { Button } from './ui/Button';
import { Modal } from './ui/Modal';
import type { AnimeThemeEntry } from '../types';
import { t } from '../i18n';

interface AnimeThemesPlayerProps {
  malId?: number | null;
  animeTitle: string;
}

type MediaMode = 'video' | 'audio';

function themeKey(theme: AnimeThemeEntry): string {
  return `${theme.theme_type}-${theme.slug}-${theme.video_url ?? theme.audio_url ?? ''}`;
}

const ThemeRow: React.FC<{
  theme: AnimeThemeEntry;
  onPlay: (theme: AnimeThemeEntry, mode: MediaMode) => void;
}> = ({ theme, onPlay }) => {
  const songTitle = theme.song_title || t('animeThemesPlayer.themeSong');
  return (
    <li className="flex items-center gap-3 py-2">
      <span className="w-14 flex-shrink-0 truncate whitespace-nowrap text-[11px] font-semibold tabular-nums text-text-muted" title={theme.slug}>
        {theme.slug}
      </span>
      <div className="flex min-w-0 flex-1 flex-col">
        <span className="truncate text-[13px] font-medium text-text-primary select-text">{songTitle}</span>
        {theme.artist_name && (
          <span className="truncate text-xs text-text-muted select-text">{theme.artist_name}</span>
        )}
      </div>
      <div className="flex flex-shrink-0 items-center gap-1">
        {theme.video_url && (
          <Button
            icon
            variant="ghost"
            size="sm"
            aria-label={t('animeThemesPlayer.watchVideoFor', { title: songTitle })}
            title={t('animeThemesPlayer.watchVideo')}
            onClick={() => onPlay(theme, 'video')}
          >
            <Video aria-hidden="true" className="h-4 w-4" />
          </Button>
        )}
        {theme.audio_url && (
          <Button
            icon
            variant="ghost"
            size="sm"
            aria-label={t('animeThemesPlayer.listenTo', { title: songTitle })}
            title={t('animeThemesPlayer.listen')}
            onClick={() => onPlay(theme, 'audio')}
          >
            <Volume2 aria-hidden="true" className="h-4 w-4" />
          </Button>
        )}
      </div>
    </li>
  );
};

const ThemeList: React.FC<{
  label: string;
  themes: AnimeThemeEntry[];
  onPlay: (theme: AnimeThemeEntry, mode: MediaMode) => void;
}> = ({ label, themes, onPlay }) => {
  if (themes.length === 0) return null;
  return (
    <div className="flex min-w-0 flex-col">
      <h3 className="text-[11px] font-semibold uppercase tracking-wider text-text-muted">{label}</h3>
      <ul className="mt-1 divide-y divide-border">
        {themes.map((theme) => (
          <ThemeRow key={themeKey(theme)} theme={theme} onPlay={onPlay} />
        ))}
      </ul>
    </div>
  );
};

export const AnimeThemesPlayer: React.FC<AnimeThemesPlayerProps> = ({ malId, animeTitle }) => {
  const { data: themes = [], isLoading } = useAnimeThemes(malId);
  const [active, setActive] = useState<{ theme: AnimeThemeEntry; mode: MediaMode } | null>(null);

  if (isLoading || themes.length === 0) return null;

  const openings = themes.filter((theme) => theme.theme_type === 'OP');
  const endings = themes.filter((theme) => theme.theme_type === 'ED');
  const handlePlay = (theme: AnimeThemeEntry, mode: MediaMode) => setActive({ theme, mode });

  const activeVideo = active?.mode === 'video' ? active.theme.video_url : null;
  const activeAudio = active ? active.theme.audio_url || active.theme.video_url : null;

  return (
    <section className="flex flex-col gap-3">
      <h2 className="text-xl font-semibold text-text-primary">{t('animeThemesPlayer.heading')}</h2>

      <div className="grid grid-cols-1 gap-x-8 gap-y-4 lg:grid-cols-2">
        <ThemeList label={t('animeThemesPlayer.openings')} themes={openings} onPlay={handlePlay} />
        <ThemeList label={t('animeThemesPlayer.endings')} themes={endings} onPlay={handlePlay} />
      </div>

      <Modal
        isOpen={!!active}
        onClose={() => setActive(null)}
        size="2xl"
        title={active ? `${active.theme.slug} · ${active.theme.song_title || t('animeThemesPlayer.themeSong')}` : t('animeThemesPlayer.themeSong')}
        header={
          <p className="truncate text-xs text-text-muted select-text">
            {active?.theme.artist_name || animeTitle}
          </p>
        }
      >
        {activeVideo ? (
          <video
            src={activeVideo}
            controls
            autoPlay
            className="aspect-video w-full rounded-xl bg-black object-contain"
          />
        ) : (
          <div className="flex flex-col items-center gap-4 py-8">
            <Music aria-hidden="true" className="h-10 w-10 text-text-muted" />
            {activeAudio && <audio src={activeAudio} controls autoPlay className="w-full max-w-md" />}
          </div>
        )}
      </Modal>
    </section>
  );
};
