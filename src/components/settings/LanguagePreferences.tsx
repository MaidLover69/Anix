import React, { useId } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { CustomSelect } from '../CustomSelect';
import { Switch } from '../ui/Switch';
import { errorMessage } from '../ui/ErrorState';
import { useSaveSettings } from '../../hooks/useSettings';
import { usePlugins } from '../../hooks/usePlugins';
import { refreshAfterLanguageChange, useLanguagePrefs } from '../../hooks/useLanguagePrefs';
import { useUIStore } from '../../stores/uiStore';
import { knownLanguages, languageLabel, type StreamCategory } from '../../lib/languages';
import { LANGUAGE_SLOTS, withChoice } from '../../lib/languageChoice';
import { locale, t } from '../../i18n';
import type { AppSettings, PluginInfo } from '../../types';

const KEYS: Record<StreamCategory, 'preferred_sub_languages' | 'preferred_dub_languages'> = {
  sub: 'preferred_sub_languages',
  dub: 'preferred_dub_languages',
};

/** Codes the usable plugins serve for a category, by name. */
export function offeredBy(plugins: PluginInfo[], category: StreamCategory): string[] {
  const codes = new Set<string>();
  for (const plugin of plugins) {
    if (!plugin.enabled || plugin.status !== 'ready') continue;
    for (const code of (category === 'dub' ? plugin.dub_languages : plugin.sub_languages) ?? []) codes.add(code);
  }
  const lang = locale();
  return [...codes].sort((a, b) => languageLabel(a).localeCompare(languageLabel(b), lang));
}

/** Three pickers (first, second, third choice) for one category's ordered language list. */
export const LanguageChoices: React.FC<{
  category: StreamCategory;
  value: string[];
  offered: string[];
  onChange: (next: string[]) => void;
  /** Ids of the visible label and hint the group is named and described by. */
  labelId: string;
  hintId?: string;
}> = ({ category, value, offered, onChange, labelId, hintId }) => {
  const choiceLabels = [t('languagePrefs.firstChoice'), t('languagePrefs.secondChoice'), t('languagePrefs.thirdChoice')];
  const options = [
    ...offered.map((code) => ({ value: code, label: languageLabel(code), group: t('languagePrefs.offeredGroup') })),
    ...knownLanguages()
      .filter(({ code }) => !offered.includes(code))
      .map(({ code, name }) => ({
        value: code,
        label: name,
        group: offered.length > 0 ? t('languagePrefs.otherGroup') : undefined,
      })),
  ];
  const extra = value.slice(LANGUAGE_SLOTS);
  const missing = (code: string) =>
    offered.length > 0 && code !== '' && !offered.includes(code)
      ? t(category === 'dub' ? 'languagePrefs.noPluginAudio' : 'languagePrefs.noPluginSubtitles')
      : null;

  return (
    <div role="group" aria-labelledby={labelId} aria-describedby={hintId} className="flex flex-col gap-2">
      <div className="grid grid-cols-1 gap-2 min-[720px]:grid-cols-3">
        {choiceLabels.map((choice, slot) => {
          const code = value[slot] ?? '';
          const note = missing(code);
          const choiceId = `${labelId}-${slot}`;
          return (
            <div key={slot} className="flex min-w-0 flex-col gap-1">
              <span id={choiceId} className="text-xs text-text-muted">
                {choice}
              </span>
              <CustomSelect
                aria-labelledby={`${labelId} ${choiceId}`}
                className="w-full"
                value={code}
                placeholder={t('languagePrefs.none')}
                onChange={(next) => onChange(withChoice(value, slot, next))}
                options={slot === 0 ? options : [{ value: '', label: t('languagePrefs.none') }, ...options]}
              />
              {note && <span className="text-xs text-text-muted">{note}</span>}
            </div>
          );
        })}
      </div>
      {extra.length > 0 && (
        <span className="text-xs text-text-muted">
          {t('languagePrefs.alsoTried', { languages: extra.map(languageLabel).join(t('languages.listSeparator')) })}
        </span>
      )}
    </div>
  );
};

const CategoryRow: React.FC<{
  category: StreamCategory;
  label: string;
  hint: string;
  value: string[];
  offered: string[];
  onChange: (next: string[]) => void;
}> = ({ label, hint, ...choices }) => {
  const labelId = useId();
  const hintId = useId();
  return (
    <div className="flex flex-col gap-3 py-3">
      <div className="flex min-w-0 flex-col">
        <span id={labelId} className="text-[13px] font-medium text-text-primary">
          {label}
        </span>
        <span id={hintId} className="mt-0.5 text-xs text-text-muted">
          {hint}
        </span>
      </div>
      <LanguageChoices labelId={labelId} hintId={hintId} {...choices} />
    </div>
  );
};

/** Settings › Sources: which languages streams are wanted in. */
export const LanguagePreferences: React.FC = () => {
  const queryClient = useQueryClient();
  const prefs = useLanguagePrefs();
  const plugins = usePlugins().data ?? [];
  const saveSettings = useSaveSettings();
  const showToast = useUIStore((s) => s.showToast);
  const strictLabelId = useId();
  const strictHintId = useId();

  const save = (changes: Partial<AppSettings>) =>
    saveSettings.mutate(changes, {
      onSuccess: () => refreshAfterLanguageChange(queryClient),
      onError: (err) =>
        showToast({ type: 'error', title: t('common.couldNotSaveSettings'), message: errorMessage(err) }),
    });

  return (
    <>
      <CategoryRow
        category="sub"
        label={t('languagePrefs.subLabel')}
        hint={t('languagePrefs.subHint')}
        value={prefs.sub}
        offered={offeredBy(plugins, 'sub')}
        onChange={(next) => save({ [KEYS.sub]: next })}
      />
      <CategoryRow
        category="dub"
        label={t('languagePrefs.dubLabel')}
        hint={t('languagePrefs.dubHint')}
        value={prefs.dub}
        offered={offeredBy(plugins, 'dub')}
        onChange={(next) => save({ [KEYS.dub]: next })}
      />
      <div className="flex min-h-[56px] items-center justify-between gap-6 py-3">
        <div className="flex min-w-0 flex-col">
          <span id={strictLabelId} className="text-[13px] font-medium text-text-primary">
            {t('languagePrefs.strictLabel')}
          </span>
          <span id={strictHintId} className="mt-0.5 text-xs text-text-muted">
            {t('languagePrefs.strictHint')}
          </span>
        </div>
        <Switch
          aria-labelledby={strictLabelId}
          aria-describedby={strictHintId}
          checked={prefs.strict}
          onChange={(checked) => save({ only_preferred_languages: checked })}
        />
      </div>
    </>
  );
};
