import React, { useEffect, useId, useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { Modal } from '../ui/Modal';
import { Button } from '../ui/Button';
import { errorMessage } from '../ui/ErrorState';
import { LanguageChoices, offeredBy } from '../settings/LanguagePreferences';
import { AiTranslationNote, AppLanguageSelect, appLanguageValue } from './AppLanguage';
import { useAppVersion, useSaveSettings, useSettings } from '../../hooks/useSettings';
import { refreshAfterLanguageChange, useLanguagePrefs } from '../../hooks/useLanguagePrefs';
import { usePlugins } from '../../hooks/usePlugins';
import { useEmbeddedMpvStatus } from '../../hooks/useEmbeddedMpv';
import { useUIStore } from '../../stores/uiStore';
import { applyLanguageSetting, t, useLocale } from '../../i18n';
import { needsSetup } from '../../lib/version';

interface Draft {
  appLanguage: string;
  sub: string[];
  dub: string[];
}

const Field: React.FC<{
  label: string;
  hint: string;
  children: (ids: { labelId: string; hintId: string }) => React.ReactNode;
}> = ({ label, hint, children }) => {
  const labelId = useId();
  const hintId = useId();
  return (
    <section className="flex flex-col gap-2">
      <div className="flex flex-col">
        <h3 id={labelId} className="text-[13px] font-medium text-text-primary">
          {label}
        </h3>
        <p id={hintId} className="mt-0.5 text-xs text-text-muted">
          {hint}
        </p>
      </div>
      {children({ labelId, hintId })}
    </section>
  );
};

/**
 * Opens over the app on a first start and once after each upgrade (see `needsSetup`): the
 * interface language and the first, second and third choice of subtitle and dub languages,
 * prefilled with what is saved. Skip (or Escape) keeps everything and does not ask again for
 * this version.
 */
export const SetupDialog: React.FC = () => {
  useLocale();
  const settings = useSettings().data;
  const version = useAppVersion().data;
  const prefs = useLanguagePrefs();
  const plugins = usePlugins().data ?? [];
  const save = useSaveSettings();
  const queryClient = useQueryClient();
  const showToast = useUIStore((s) => s.showToast);
  const [dismissed, setDismissed] = useState(false);
  const [draft, setDraft] = useState<Draft | null>(null);
  const engine = useEmbeddedMpvStatus();
  // "Is the video player showing correctly?" counts down on its own; it goes first.
  const engineAsks = engine.isPending || engine.data?.ask_confirm === true;

  const open =
    !dismissed && !engineAsks && !!settings && !!version && needsSetup(settings.setup_seen_version, version);

  useEffect(() => {
    if (open && draft === null) {
      setDraft({ appLanguage: appLanguageValue(settings?.app_language), sub: prefs.sub, dub: prefs.dub });
    }
  }, [open, draft, settings?.app_language, prefs.sub, prefs.dub]);

  const introId = useId();
  if (!open || !draft || !version) return null;

  const finish = (changes: Record<string, unknown>) => {
    setDismissed(true);
    save.mutate(
      { ...changes, setup_seen_version: version },
      {
        onSuccess: () => refreshAfterLanguageChange(queryClient),
        onError: (err) =>
          showToast({ type: 'error', title: t('common.couldNotSaveSettings'), message: errorMessage(err) }),
      }
    );
  };

  const skip = () => {
    // The language picked here was only a preview.
    void applyLanguageSetting(settings?.app_language);
    finish({});
  };

  const saveChoices = () =>
    finish({
      app_language: draft.appLanguage,
      preferred_sub_languages: draft.sub,
      preferred_dub_languages: draft.dub,
    });

  return (
    <Modal
      isOpen
      onClose={skip}
      size="xl"
      title={t('setup.title')}
      describedBy={introId}
      header={<p id={introId} className="mt-1 text-[13px] text-text-secondary select-text">{t('setup.intro', { version })}</p>}
      footer={
        <>
          <Button variant="ghost" onClick={skip}>
            {t('setup.skip')}
          </Button>
          <Button variant="primary" data-autofocus onClick={saveChoices}>
            {t('setup.save')}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-6">
        <Field label={t('setup.appLanguage')} hint={t('setup.appLanguageHint')}>
          {({ labelId, hintId }) => (
            <div className="flex flex-col gap-2">
              <AppLanguageSelect
                aria-labelledby={labelId}
                aria-describedby={hintId}
                className="w-full max-w-sm"
                value={draft.appLanguage}
                onChange={(next) => {
                  setDraft({ ...draft, appLanguage: next });
                  void applyLanguageSetting(next);
                }}
              />
              <AiTranslationNote />
            </div>
          )}
        </Field>
        <Field label={t('setup.subLanguages')} hint={t('setup.subLanguagesHint')}>
          {({ labelId, hintId }) => (
            <LanguageChoices
              category="sub"
              labelId={labelId}
              hintId={hintId}
              value={draft.sub}
              offered={offeredBy(plugins, 'sub')}
              onChange={(sub) => setDraft({ ...draft, sub })}
            />
          )}
        </Field>
        <Field label={t('setup.dubLanguages')} hint={t('setup.dubLanguagesHint')}>
          {({ labelId, hintId }) => (
            <LanguageChoices
              category="dub"
              labelId={labelId}
              hintId={hintId}
              value={draft.dub}
              offered={offeredBy(plugins, 'dub')}
              onChange={(dub) => setDraft({ ...draft, dub })}
            />
          )}
        </Field>
        <p className="text-xs text-text-muted">{t('setup.later')}</p>
      </div>
    </Modal>
  );
};
