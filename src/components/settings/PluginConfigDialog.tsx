import React, { useEffect, useId, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { Modal } from '../ui/Modal';
import { Button } from '../ui/Button';
import { Switch } from '../ui/Switch';
import { Spinner } from '../ui/Spinner';
import { ErrorState, errorMessage } from '../ui/ErrorState';
import { CustomSelect } from '../CustomSelect';
import { useUIStore } from '../../stores/uiStore';
import { t } from '../../i18n';
import type { PluginConfig, PluginConfigField, PluginInfo } from '../../types';

/** What the backend keeps of one value (`userconfig.rs`). */
const MAX_VALUE_CHARS = 4096;

/** Why a value cannot be saved, or null when it can. */
export function fieldProblem(field: PluginConfigField, value: string): string | null {
  if (field.type === 'select' && field.options?.length && !field.options.some((o) => o.value === value)) {
    return t('pluginConfigDialog.chooseOption');
  }
  if (field.type === 'switch' && value !== 'true' && value !== 'false') return t('pluginConfigDialog.turnOnOrOff');
  if ([...value].length > MAX_VALUE_CHARS) return t('pluginConfigDialog.maxChars', { count: MAX_VALUE_CHARS });
  return null;
}

const Field: React.FC<{
  field: PluginConfigField;
  value: string;
  problem: string | null;
  onChange: (value: string) => void;
}> = ({ field, value, problem, onChange }) => {
  const id = useId();
  const described = problem ? `${id}-problem` : field.default ? `${id}-default` : undefined;
  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-center justify-between gap-3">
        <label id={`${id}-label`} htmlFor={id} className="text-[13px] font-medium text-text-primary">
          {field.label}
        </label>
        {field.type === 'switch' && (
          <Switch
            id={id}
            aria-labelledby={`${id}-label`}
            aria-describedby={described}
            checked={value === 'true'}
            onChange={(checked) => onChange(checked ? 'true' : 'false')}
          />
        )}
      </div>
      {field.type === 'select' && (
        <CustomSelect
          aria-labelledby={`${id}-label`}
          aria-describedby={described}
          value={value}
          onChange={onChange}
          options={(field.options ?? []).map((o) => ({ value: o.value, label: o.label || o.value }))}
        />
      )}
      {field.type === 'text' && (
        <input
          id={id}
          type="text"
          autoComplete="off"
          spellCheck={false}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          aria-invalid={problem ? true : undefined}
          aria-describedby={described}
          className="h-9 rounded-md border border-border bg-bg-input px-3 text-[13px] text-text-primary placeholder:text-text-muted select-text"
        />
      )}
      {problem ? (
        <p id={`${id}-problem`} role="alert" className="text-xs text-state-error">
          {problem}
        </p>
      ) : (
        field.default &&
        field.type === 'text' && (
          <p id={`${id}-default`} className="break-words text-xs text-text-muted select-text">
            {t('pluginConfigDialog.default', { value: field.default })}
          </p>
        )
      )}
    </div>
  );
};

/** Settings › Plugins › Settings: the fields a plugin's manifest declares in `userConfig`. */
export const PluginConfigDialog: React.FC<{ plugin: PluginInfo | null; onClose: () => void }> = ({
  plugin,
  onClose,
}) => {
  const queryClient = useQueryClient();
  const showToast = useUIStore((s) => s.showToast);
  const id = plugin?.id ?? '';
  const name = plugin ? plugin.name.trim() || plugin.id : '';
  const config = useQuery<PluginConfig>({
    queryKey: ['pluginConfig', id],
    queryFn: () => invoke<PluginConfig>('get_plugin_config', { id }), // i18n-ignore (generic call, not text)
    enabled: !!plugin,
    staleTime: 0,
    gcTime: 0,
  });
  const [values, setValues] = useState<Record<string, string>>({});
  const [checked, setChecked] = useState(false);

  useEffect(() => {
    if (config.data) setValues(Object.fromEntries(config.data.fields.map((f) => [f.name, f.value])));
    setChecked(false);
  }, [config.data]);

  const save = useMutation({
    mutationFn: (next: Record<string, string>) => invoke<void>('set_plugin_config', { id, values: next }), // i18n-ignore (generic call, not text)
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['plugins'] });
      showToast({ type: 'success', title: t('pluginConfigDialog.saved', { name }) });
      onClose();
    },
    onError: (err) => showToast({ type: 'error', title: t('pluginConfigDialog.couldNotSave', { name }), message: errorMessage(err) }),
  });

  const fields = config.data?.fields ?? [];
  const problems = Object.fromEntries(fields.map((f) => [f.name, fieldProblem(f, values[f.name] ?? '')]));
  const invalid = fields.some((f) => problems[f.name]);

  const handleSubmit = (event: React.FormEvent) => {
    event.preventDefault();
    setChecked(true);
    if (invalid || save.isPending) return;
    save.mutate(values);
  };

  return (
    <Modal
      isOpen={plugin !== null}
      onClose={() => {
        if (!save.isPending) onClose();
      }}
      title={t('pluginConfigDialog.title', { name })}
      size="md"
      footer={
        <>
          <Button variant="ghost" onClick={onClose} disabled={save.isPending}>
            {t('common.cancel')}
          </Button>
          <Button
            variant="primary"
            type="submit"
            form="plugin-config-form"
            disabled={save.isPending || !config.data || fields.length === 0}
          >
            {save.isPending && <Spinner size="sm" label={t('pluginConfigDialog.saving')} />}
            {t('common.save')}
          </Button>
        </>
      }
    >
      {config.isPending ? (
        <div aria-busy="true" className="flex items-center gap-2 py-4 text-xs text-text-muted">
          <Spinner size="sm" label={t('pluginConfigDialog.loading')} />
          {t('pluginConfigDialog.loadingEllipsis')}
        </div>
      ) : config.isError ? (
        <ErrorState compact title={t('pluginConfigDialog.loadFailed')} error={config.error} onRetry={() => config.refetch()} />
      ) : fields.length === 0 ? (
        <p className="text-[13px] text-text-secondary">{t('pluginConfigDialog.noSettings')}</p>
      ) : (
        <form id="plugin-config-form" onSubmit={handleSubmit} noValidate className="flex flex-col gap-4">
          {config.data?.requires_config && !config.data.configured && (
            <p className="text-xs text-text-secondary">{t('pluginConfigDialog.startsOnceSaved')}</p>
          )}
          {fields.map((field) => (
            <Field
              key={field.name}
              field={field}
              value={values[field.name] ?? ''}
              problem={checked ? problems[field.name] : null}
              onChange={(value) => setValues((current) => ({ ...current, [field.name]: value }))}
            />
          ))}
          <p className="text-xs text-text-muted">{t('pluginConfigDialog.savingRestarts')}</p>
        </form>
      )}
    </Modal>
  );
};
