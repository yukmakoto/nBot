import { Plus, Trash2 } from 'lucide-react';
import type { ReactNode } from 'react';

import { getByPath, setByPath, type ConfigValues } from '../lib/configSchema';
import type { ConfigSchemaItem } from '../lib/types';

function FieldShell({
  item,
  children,
}: {
  item: ConfigSchemaItem;
  children: ReactNode;
}) {
  return (
    <div className="p-5 rounded-2xl border border-brand-soft bg-white/70 hover:bg-white transition-all">
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0">
          <div className="font-black text-text-main">{item.label || item.key}</div>
          {item.description ? (
            <div className="text-xs text-text-main/60 font-bold mt-1 leading-relaxed">
              {item.description}
            </div>
          ) : null}
          <div className="text-[10px] font-black text-brand/30 uppercase tracking-widest mt-2 truncate">
            {item.key}
          </div>
        </div>
        <div className="shrink-0">{children}</div>
      </div>
    </div>
  );
}

function TextInput({
  value,
  onChange,
  disabled,
}: {
  value: string;
  onChange: (next: string) => void;
  disabled?: boolean;
}) {
  return (
    <input
      className="px-4 py-2 rounded-xl border border-brand-soft bg-white text-sm font-bold text-text-main focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all min-w-56"
      value={value}
      onChange={(e) => onChange(e.target.value)}
      disabled={disabled}
    />
  );
}

function NumberInput({
  value,
  onChange,
  disabled,
  min,
  max,
}: {
  value: number | null;
  onChange: (next: number | null) => void;
  disabled?: boolean;
  min?: number | null;
  max?: number | null;
}) {
  return (
    <input
      type="number"
      className="px-4 py-2 rounded-xl border border-brand-soft bg-white text-sm font-bold text-text-main focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all min-w-40"
      value={value === null ? '' : String(value)}
      onChange={(e) => {
        const raw = e.target.value.trim();
        if (!raw) {
          onChange(null);
          return;
        }
        const parsed = Number(raw);
        onChange(Number.isFinite(parsed) ? parsed : null);
      }}
      disabled={disabled}
      min={min ?? undefined}
      max={max ?? undefined}
    />
  );
}

function BooleanInput({
  value,
  onChange,
  disabled,
}: {
  value: boolean;
  onChange: (next: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <label className="inline-flex items-center gap-2 cursor-pointer select-none">
      <input
        type="checkbox"
        className="w-4 h-4 accent-brand"
        checked={value}
        onChange={(e) => onChange(e.target.checked)}
        disabled={disabled}
      />
      <span className="text-xs font-black text-text-main/70 uppercase tracking-widest">
        {value ? 'ON' : 'OFF'}
      </span>
    </label>
  );
}

function SelectInput({
  value,
  onChange,
  disabled,
  options,
}: {
  value: string;
  onChange: (next: string) => void;
  disabled?: boolean;
  options: { value: string; label: string }[];
}) {
  return (
    <select
      className="px-4 py-2 rounded-xl border border-brand-soft bg-white text-sm font-bold text-text-main focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all min-w-56"
      value={value}
      onChange={(e) => onChange(e.target.value)}
      disabled={disabled}
    >
      {options.map((opt) => (
        <option key={opt.value} value={opt.value}>
          {opt.label}
        </option>
      ))}
    </select>
  );
}

function ArrayEditor({
  value,
  onChange,
  disabled,
  itemType,
}: {
  value: unknown[];
  onChange: (next: unknown[]) => void;
  disabled?: boolean;
  itemType: string;
}) {
  const normalizedType = itemType.toLowerCase();

  function updateIndex(index: number, nextItem: unknown) {
    const next = [...value];
    next[index] = nextItem;
    onChange(next);
  }

  function removeIndex(index: number) {
    onChange(value.filter((_, i) => i !== index));
  }

  function add() {
    const nextItem =
      normalizedType === 'boolean' ? false : normalizedType === 'number' ? 0 : '';
    onChange([...value, nextItem]);
  }

  return (
    <div className="space-y-2 min-w-72">
      {value.length ? (
        value.map((item, idx) => (
          <div key={idx} className="flex items-center gap-2">
            {normalizedType === 'boolean' ? (
              <BooleanInput
                value={!!item}
                onChange={(v) => updateIndex(idx, v)}
                disabled={disabled}
              />
            ) : normalizedType === 'number' ? (
              <NumberInput
                value={typeof item === 'number' && Number.isFinite(item) ? item : null}
                onChange={(v) => updateIndex(idx, v)}
                disabled={disabled}
              />
            ) : (
              <TextInput
                value={typeof item === 'string' ? item : item == null ? '' : String(item)}
                onChange={(v) => updateIndex(idx, v)}
                disabled={disabled}
              />
            )}
            <button
              className="p-2 rounded-xl hover:bg-brand-soft text-red-400 hover:text-red-500 transition-all"
              onClick={() => removeIndex(idx)}
              disabled={disabled}
              title="删除"
              type="button"
            >
              <Trash2 className="w-4 h-4" />
            </button>
          </div>
        ))
      ) : (
        <div className="text-xs text-text-main/40 font-bold">暂无项目</div>
      )}

      <button
        className="btn-secondary inline-flex items-center gap-2"
        onClick={add}
        disabled={disabled}
        type="button"
      >
        <Plus className="w-4 h-4" />
        添加
      </button>
    </div>
  );
}

export function ConfigSchemaForm({
  schema,
  value,
  onChange,
  disabled,
}: {
  schema: ConfigSchemaItem[];
  value: ConfigValues;
  onChange: (next: ConfigValues) => void;
  disabled?: boolean;
}) {
  return (
    <div className="space-y-4">
      {schema.map((item) => {
        const key = item.key?.trim();
        if (!key) return null;

        const fieldType = (item.type ?? 'string').toLowerCase();
        const current = getByPath(value, key);

        if (fieldType === 'boolean') {
          return (
            <FieldShell key={key} item={item}>
              <BooleanInput
                value={!!current}
                onChange={(next) => onChange(setByPath(value, key, next))}
                disabled={disabled}
              />
            </FieldShell>
          );
        }

        if (fieldType === 'number') {
          const numeric =
            typeof current === 'number' && Number.isFinite(current) ? current : null;
          return (
            <FieldShell key={key} item={item}>
              <NumberInput
                value={numeric}
                onChange={(next) => onChange(setByPath(value, key, next))}
                disabled={disabled}
                min={item.min ?? null}
                max={item.max ?? null}
              />
            </FieldShell>
          );
        }

        if (fieldType === 'select') {
          const options = item.options?.filter((o) => o?.value) ?? [];
          const selected =
            typeof current === 'string' ? current : (options[0]?.value ?? '');
          return (
            <FieldShell key={key} item={item}>
              <SelectInput
                value={selected}
                onChange={(next) => onChange(setByPath(value, key, next))}
                options={options as { value: string; label: string }[]}
                disabled={disabled}
              />
            </FieldShell>
          );
        }

        if (fieldType === 'array') {
          const list = Array.isArray(current) ? current : [];
          return (
            <FieldShell key={key} item={item}>
              <ArrayEditor
                value={list}
                onChange={(next) => onChange(setByPath(value, key, next))}
                disabled={disabled}
                itemType={item.itemType ?? 'string'}
              />
            </FieldShell>
          );
        }

        const text = typeof current === 'string' ? current : current == null ? '' : String(current);
        return (
          <FieldShell key={key} item={item}>
            <TextInput
              value={text}
              onChange={(next) => onChange(setByPath(value, key, next))}
              disabled={disabled}
            />
          </FieldShell>
        );
      })}
    </div>
  );
}
