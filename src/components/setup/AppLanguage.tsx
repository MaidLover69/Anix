import React from 'react';
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

/** "Deutsch — German" in English: each language by its own name, then in the interface language. */
function optionLabel(code: string): string {
  const own = nativeName(code);
  if (code === language()) return own;
  try {
    const local = new Intl.DisplayNames([locale()], { type: 'language' }).of(code === 'zh' ? 'zh-Hans' : code === 'pt' ? 'pt-BR' : code);
    return local && local !== own ? `${own} — ${local}` : own;
  } catch {
    return own;
  }
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
      <Row label={t('appLanguage.label')} hint={t('appLanguage.hint')} keywords="language translation">
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
