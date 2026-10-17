import React, { useId, useMemo } from 'react';
import { ArrowDown, ArrowUp, X } from 'lucide-react';
import { useQueryClient } from '@tanstack/react-query';
import { CustomSelect } from '../CustomSelect';
import { Button } from '../ui/Button';
import { Switch } from '../ui/Switch';
import { errorMessage } from '../ui/ErrorState';
import { useSaveSettings } from '../../hooks/useSettings';
import { usePlugins } from '../../hooks/usePlugins';
import { refreshAfterLanguageChange, useLanguagePrefs } from '../../hooks/useLanguagePrefs';
import { useUIStore } from '../../stores/uiStore';
import { KNOWN_LANGUAGES, languageName, type StreamCategory } from '../../lib/languages';
import type { AppSettings, PluginInfo } from '../../types';

const KEYS: Record<StreamCategory, 'preferred_sub_languages' | 'preferred_dub_languages'> = {
  sub: 'preferred_sub_languages',
  dub: 'preferred_dub_languages',
};

/** Codes the usable plugins serve for a category, by name. */
function offeredBy(plugins: PluginInfo[], category: StreamCategory): string[] {
  const codes = new Set<string>();
  for (const plugin of plugins) {
    if (!plugin.enabled || plugin.status !== 'ready') continue;
    for (const code of (category === 'dub' ? plugin.dub_languages : plugin.sub_languages) ?? []) codes.add(code);
  }
  return [...codes].sort((a, b) => languageName(a).localeCompare(languageName(b)));
}

const LanguageList: React.FC<{
  category: StreamCategory;
  label: string;
  hint: string;
  value: string[];
  offered: string[];
  onChange: (next: string[]) => void;
}> = ({ category, label, hint, value, offered, onChange }) => {
  const labelId = useId();
  const hintId = useId();

  const addOptions = useMemo(() => {
    const free = (code: string) => !value.includes(code);
    const offeredSet = new Set(offered);
    return [
      ...offered.filter(free).map((code) => ({
        value: code,
        label: languageName(code),
        group: 'Offered by your plugins',
      })),
      ...KNOWN_LANGUAGES.filter(({ code }) => free(code) && !offeredSet.has(code)).map(({ code, name }) => ({
        value: code,
        label: name,
        group: 'Other languages',
      })),
    ];
  }, [value, offered]);

  const move = (index: number, by: number) => {
    const next = [...value];
    const [code] = next.splice(index, 1);
    next.splice(index + by, 0, code);
    onChange(next);
  };

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
      <ol aria-labelledby={labelId} aria-describedby={hintId} className="flex flex-col gap-1.5">
        {value.map((code, index) => {
          const name = languageName(code);
          const servedBy = offered.includes(code);
          return (
            <li
              key={code}
              className="flex h-10 items-center justify-between gap-3 rounded-md ui-inset bg-bg-surface pl-3 pr-1"
            >
              <span className="flex min-w-0 items-baseline gap-2">
                <span className="w-4 flex-shrink-0 text-xs tabular-nums text-text-muted">{index + 1}.</span>
                <span className="truncate text-[13px] font-medium text-text-primary">{name}</span>
                <span className="text-xs text-text-muted">{code.toUpperCase()}</span>
                {!servedBy && (
                  <span className="truncate text-xs text-text-muted">
                    · no plugin offers {category === 'dub' ? 'this audio' : 'these subtitles'}
                  </span>
                )}
              </span>
              <span className="flex flex-shrink-0 items-center">
                <Button
                  icon
                  size="sm"
                  variant="ghost"
                  aria-label={`Prefer ${name} more`}
                  disabled={index === 0}
                  onClick={() => move(index, -1)}
                >
                  <ArrowUp aria-hidden="true" className="h-3.5 w-3.5" />
                </Button>
                <Button
                  icon
                  size="sm"
                  variant="ghost"
                  aria-label={`Prefer ${name} less`}
                  disabled={index === value.length - 1}
                  onClick={() => move(index, 1)}
                >
                  <ArrowDown aria-hidden="true" className="h-3.5 w-3.5" />
                </Button>
                <Button
                  icon
                  size="sm"
                  variant="ghost"
                  aria-label={`Remove ${name}`}
                  title={value.length === 1 ? 'At least one language is needed' : undefined}
                  disabled={value.length === 1}
                  onClick={() => onChange(value.filter((other) => other !== code))}
                >
                  <X aria-hidden="true" className="h-3.5 w-3.5" />
                </Button>
              </span>
            </li>
          );
        })}
      </ol>
      <CustomSelect
        aria-label={`Add a language to ${label.toLowerCase()}`}
        className="w-60"
        value=""
        placeholder="Add a language…"
        onChange={(code) => onChange([...value, code])}
        options={addOptions}
      />
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
        showToast({ type: 'error', title: 'Could not save settings', message: errorMessage(err) }),
    });

  return (
    <>
      <LanguageList
        category="sub"
        label="Subtitle languages"
        hint="For Sub: Japanese audio with subtitles in these languages. The first one is tried first, then the next."
        value={prefs.sub}
        offered={offeredBy(plugins, 'sub')}
        onChange={(next) => save({ [KEYS.sub]: next })}
      />
      <LanguageList
        category="dub"
        label="Dub languages"
        hint="For Dub: audio in these languages, tried in this order."
        value={prefs.dub}
        offered={offeredBy(plugins, 'dub')}
        onChange={(next) => save({ [KEYS.dub]: next })}
      />
      <div className="flex min-h-[56px] items-center justify-between gap-6 py-3">
        <div className="flex min-w-0 flex-col">
          <span id={strictLabelId} className="text-[13px] font-medium text-text-primary">
            Only use plugins in my languages
          </span>
          <span id={strictHintId} className="mt-0.5 text-xs text-text-muted">
            Auto and the episode lists leave other languages out. Off: they are tried after yours. A
            server you pick yourself always plays.
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
