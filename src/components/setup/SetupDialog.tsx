import React, { useEffect, useId, useRef, useState, useSyncExternalStore } from 'react';
import { ChevronLeft, ChevronRight } from 'lucide-react';
import { router } from '../../router';
import { useQueryClient } from '@tanstack/react-query';
import clsx from 'clsx';
import { Modal } from '../ui/Modal';
import { Button } from '../ui/Button';
import { errorMessage } from '../ui/ErrorState';
import { LanguageChoices, offeredBy } from '../settings/LanguagePreferences';
import { AiTranslationNote, AppLanguageGrid, appLanguageValue } from './AppLanguage';
import { useAppVersion, useSaveSettings, useSettings } from '../../hooks/useSettings';
import { refreshAfterLanguageChange, useLanguagePrefs } from '../../hooks/useLanguagePrefs';
import { usePlugins } from '../../hooks/usePlugins';
import { useEmbeddedMpvStatus } from '../../hooks/useEmbeddedMpv';
import { useUIStore } from '../../stores/uiStore';
import { applyLanguageSetting, formatNumber, t, useLocale } from '../../i18n';
import { needsSetup } from '../../lib/version';
import { Meme, MemeImage } from '../meme/Meme';

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

const STEP_COUNT = 3;

/**
 * Opens over the app on a first start and once after each upgrade (see `needsSetup`), in three
 * short steps: the interface language, then the first, second and third choice of subtitle
 * languages, then of dub languages, prefilled with what is saved. Skip (or Escape) is there on
 * every step; it keeps everything and does not ask again for this version.
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
  const [step, setStep] = useState(0);
  const primaryRef = useRef<HTMLButtonElement>(null);
  const engine = useEmbeddedMpvStatus();
  // "Is the video player showing correctly?" counts down on its own; it goes first.
  const engineAsks = engine.isPending || engine.data?.ask_confirm === true;

  // Not over the player: it would catch the player's keys. It opens once the user leaves it.
  const onPlayer = useSyncExternalStore(router.subscribe, () => router.state.location.pathname.startsWith('/watch'));

  const open =
    !dismissed && !engineAsks && !onPlayer && !!settings && !!version && needsSetup(settings.setup_seen_version, version);

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

  const last = step === STEP_COUNT - 1;
  const stepLabel = t('setup.stepOf', { step: formatNumber(step + 1), count: formatNumber(STEP_COUNT) });

  return (
    <Modal
      isOpen
      onClose={skip}
      size="xl"
      title={<Meme spot="pickLangs" plain={t('setup.title')} />}
      describedBy={introId}
      header={
        <>
          <MemeImage name="kamina-believe" className="meme-dlg-img" />
          <p id={introId} className="mt-1 text-[13px] text-text-secondary select-text">{t('setup.intro', { version })}</p>
          <div className="mt-3 flex items-center gap-3">
            <div
              role="progressbar"
              aria-label={stepLabel}
              aria-valuemin={1}
              aria-valuemax={STEP_COUNT}
              aria-valuenow={step + 1}
              className="flex flex-1 gap-1.5"
            >
              {Array.from({ length: STEP_COUNT }, (_, i) => (
                <span key={i} className="h-1 flex-1 overflow-hidden rounded-full bg-ink/[0.12]">
                  <span
                    className={clsx(
                      'block h-full rounded-full bg-accent-500 transition-[width] duration-300',
                      i <= step ? 'w-full' : 'w-0'
                    )}
                  />
                </span>
              ))}
            </div>
            <span aria-live="polite" className="flex-shrink-0 text-xs text-text-muted">
              {stepLabel}
            </span>
          </div>
        </>
      }
      footer={
        <>
          <Button key="skip" variant="ghost" onClick={skip}>
            {t('setup.skip')}
          </Button>
          {step > 0 && (
            <Button
              key="back"
              variant="neutral"
              onClick={() => {
                setStep(step - 1);
                // Back leaves the first step without its own button; keep the focus in the dialog.
                if (step === 1) primaryRef.current?.focus();
              }}
            >
              <ChevronLeft aria-hidden="true" className="h-4 w-4 rtl:-scale-x-100" />
              {t('common.back')}
            </Button>
          )}
          <Button
            key="primary"
            ref={primaryRef}
            variant="primary"
            data-autofocus
            onClick={last ? saveChoices : () => setStep(step + 1)}
          >
            {last ? t('setup.save') : t('setup.next')}
            {!last && <ChevronRight aria-hidden="true" className="h-4 w-4 rtl:-scale-x-100" />}
          </Button>
        </>
      }
    >
      <div className="flex min-h-[16rem] flex-col gap-4">
        {/* Every step sits in the same grid cell, the others hidden, so the dialog keeps the
            tallest step's height and does not jump between steps. */}
        <div className="grid">
          {[
            <Field label={t('setup.appLanguage')} hint={t('setup.appLanguageHint')}>
              {({ labelId, hintId }) => (
                <div className="flex flex-col gap-3">
                  <AppLanguageGrid
                    labelId={labelId}
                    hintId={hintId}
                    value={draft.appLanguage}
                    onChange={(next) => {
                      setDraft({ ...draft, appLanguage: next });
                      // A preview: Arabic turns the dialog right to left at once.
                      void applyLanguageSetting(next);
                    }}
                  />
                  <AiTranslationNote />
                </div>
              )}
            </Field>,
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
            </Field>,
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
            </Field>,
          ].map((content, i) => (
            <div
              key={i}
              aria-hidden={i !== step || undefined}
              className={clsx('flex flex-col [grid-area:1/1]', i === step ? 'animate-fade-in' : 'invisible')}
            >
              {content}
            </div>
          ))}
        </div>
        <p className="mt-auto text-xs text-text-muted">{t('setup.later')}</p>
      </div>
    </Modal>
  );
};
