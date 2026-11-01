import React from 'react';
import clsx from 'clsx';
import { Languages } from 'lucide-react';
import { CustomSelect } from '../CustomSelect';
import { Row } from '../settings/SettingsLayout';
import { errorMessage } from '../ui/ErrorState';
import { useSaveSettings, useSettings } from '../../hooks/useSettings';
import { useUIStore } from '../../stores/uiStore';
import {
  APP_LANGUAGES,
  SYSTEM_LANGUAGE,
  englishText,
  isAppLanguage,
  language,
  locale,
  systemLanguage,
  t,
  useLocale,
} from '../../i18n';

function nativeName(code: string): string {
  return APP_LANGUAGES.find((entry) => entry.code === code)?.name ?? code;
}

/** The language's name in the interface language, or '' when that is its own name. */
function localName(code: string): string {
  const own = nativeName(code);
  if (code === language()) return '';
  try {
    const local = new Intl.DisplayNames([locale()], { type: 'language' }).of(code === 'zh' ? 'zh-Hans' : code === 'pt' ? 'pt-BR' : code);
    return local && local !== own ? local : '';
  } catch {
    return '';
  }
}

/** "Deutsch — German" in English: each language by its own name, then in the interface language. */
function optionLabel(code: string): string {
  const local = localName(code);
  return local ? `${nativeName(code)} — ${local}` : nativeName(code);
}

/** The `app_language` value as the picker shows it: a known code, else "system". */
export function appLanguageValue(setting: unknown): string {
  return isAppLanguage(setting) ? setting : SYSTEM_LANGUAGE;
}

export const AppLanguageSelect: React.FC<{
  value: string;
  onChange: (value: string) => void;
  className?: string;
  'aria-labelledby'?: string;
  'aria-describedby'?: string;
}> = ({ value, onChange, className, ...aria }) => {
  useLocale();
  return (
    <CustomSelect
      {...aria}
      className={className ?? 'w-64'}
      value={appLanguageValue(value)}
      onChange={onChange}
      options={[
        { value: SYSTEM_LANGUAGE, label: t('appLanguage.system', { language: nativeName(systemLanguage()) }) },
        ...APP_LANGUAGES.map(({ code }) => ({ value: code, label: optionLabel(code), lang: code })),
      ]}
    />
  );
};

/**
 * The app languages as a grid of buttons, each written in its own language with its name in the
 * interface language under it. Used by the first-run setup.
 */
export const AppLanguageGrid: React.FC<{
  value: string;
  onChange: (value: string) => void;
  labelId: string;
  hintId?: string;
}> = ({ value, onChange, labelId, hintId }) => {
  useLocale();
  const current = appLanguageValue(value);
  const choices = [
    { code: SYSTEM_LANGUAGE, own: t('appLanguage.system', { language: nativeName(systemLanguage()) }), local: '' },
    ...APP_LANGUAGES.map(({ code, name }) => ({ code, own: name, local: localName(code) })),
  ];
  return (
    <div
      role="group"
      aria-labelledby={labelId}
      aria-describedby={hintId}
      className="grid grid-cols-[repeat(auto-fill,minmax(9rem,1fr))] gap-1.5"
    >
      {choices.map(({ code, own, local }) => {
        const pressed = code === current;
        return (
          <button
            key={code}
            type="button"
            aria-pressed={pressed}
            onClick={() => onChange(code)}
            className={clsx(
              'ui-theme-card flex min-w-0 flex-col items-start justify-center gap-0.5 rounded-md border px-3 py-2 text-start text-[13px] font-semibold text-text-primary cursor-pointer transition-[color,background-color,border-color,transform] motion-safe:active:scale-95',
              code === SYSTEM_LANGUAGE && 'col-span-2',
              pressed ? 'border-accent-500 bg-accent-tint' : 'border-border-strong bg-bg-card hover:bg-bg-input'
            )}
          >
            <span lang={code === SYSTEM_LANGUAGE ? undefined : code} className="max-w-full truncate">
              {own}
            </span>
            {/* Under the name, wrapping: beside it, long names ("Brazilian Portuguese") were cut at 960 px. */}
            {local && <span className="max-w-full text-[11px] font-normal leading-tight text-text-muted [overflow-wrap:anywhere]">{local}</span>}
          </button>
        );
      })}
    </div>
  );
};

/** The note that the translations are machine-made, in the interface language and in English. */
export const AiTranslationNote: React.FC<{ className?: string }> = ({ className }) => {
  const lang = useLocale();
  return (
    <p
      className={
        className ?? 'flex items-start gap-2 rounded-md ui-inset bg-bg-surface px-3 py-2 text-xs text-text-secondary'
      }
    >
      <Languages aria-hidden="true" className="mt-px h-3.5 w-3.5 flex-shrink-0 text-text-muted" />
      <span className="flex min-w-0 flex-col gap-0.5">
        <span>{t('appLanguage.aiNote')}</span>
        {lang !== 'en' && (
          <span lang="en" dir="ltr" className="text-text-muted">
            {englishText('appLanguage.aiNote')}
          </span>
        )}
      </span>
    </p>
  );
};

/** Settings › Appearance › Language. */
export const AppLanguageRow: React.FC = () => {
  const settings = useSettings().data;
  const save = useSaveSettings();
  const showToast = useUIStore((s) => s.showToast);
  return (
    <>
      <Row label={t('appLanguage.label')} hint={t('appLanguage.hint')} keywords="language translation" meme="appLang">
        {({ labelId, hintId }) => (
          <AppLanguageSelect
            aria-labelledby={labelId}
            aria-describedby={hintId}
            value={appLanguageValue(settings?.app_language)}
            onChange={(next) =>
              // mutateAsync: the row unmounts when the language switches, which drops per-call mutate callbacks.
              save
                .mutateAsync({ app_language: next })
                .catch((err) =>
                  showToast({ type: 'error', title: t('common.couldNotSaveSettings'), message: errorMessage(err) })
                )
            }
          />
        )}
      </Row>
      <AiTranslationNote />
    </>
  );
};
