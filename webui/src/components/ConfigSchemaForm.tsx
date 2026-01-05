import { ChevronDown, Plus, Trash2 } from 'lucide-react';
import { useEffect, useRef, useState, type ReactNode } from 'react';

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
      <div className="flex flex-wrap items-start gap-4">
        <div className="flex-1 min-w-0">
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
        <div className="shrink-0 max-w-full">{children}</div>
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
  const [open, setOpen] = useState(false);
  const boxRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!open) return;
    function onDocClick(ev: MouseEvent) {
      const el = boxRef.current;
      if (!el) return;
      if (ev.target instanceof Node && el.contains(ev.target)) return;
      setOpen(false);
    }
    document.addEventListener('mousedown', onDocClick);
    return () => document.removeEventListener('mousedown', onDocClick);
  }, [open]);

  const currentLabel =
    options.find((o) => o.value === value)?.label ??
    options.find((o) => o.value === value)?.value ??
    value;

  return (
    <div ref={boxRef} className="relative w-72 max-w-[60vw] min-w-0">
      <button
        type="button"
        className="w-full px-4 py-2 rounded-xl border border-brand-soft bg-white text-sm font-bold text-text-main focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all inline-flex items-center justify-between gap-2 disabled:opacity-50 disabled:cursor-not-allowed"
        onClick={() => (!disabled ? setOpen((v) => !v) : null)}
        disabled={disabled}
      >
        <span className="truncate" title={currentLabel}>
          {currentLabel}
        </span>
        <ChevronDown
          className={open ? 'w-4 h-4 text-text-main/50 rotate-180 transition-transform' : 'w-4 h-4 text-text-main/50 transition-transform'}
        />
      </button>

      {open ? (
        <div className="absolute left-0 right-0 top-full mt-2 rounded-xl border border-brand-soft bg-white overflow-hidden z-50">
          <div className="max-h-64 overflow-y-auto clean-scroll">
            {options.map((opt) => (
              <button
                key={opt.value}
                type="button"
                className={
                  opt.value === value
                    ? 'w-full text-left px-4 py-2 text-sm font-black bg-brand-soft text-brand truncate'
                    : 'w-full text-left px-4 py-2 text-sm font-bold hover:bg-brand-soft/60 text-text-main truncate'
                }
                onClick={() => {
                  onChange(opt.value);
                  setOpen(false);
                }}
                title={opt.label}
              >
                {opt.label}
              </button>
            ))}
          </div>
        </div>
      ) : null}
    </div>
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

function JsonInput({
  value,
  onChange,
  disabled,
}: {
  value: unknown;
  onChange: (next: unknown) => void;
  disabled?: boolean;
}) {
  const [draft, setDraft] = useState(() => {
    if (value == null) return '{}';
    if (typeof value === 'string') return value;
    try {
      return JSON.stringify(value, null, 2);
    } catch {
      return '{}';
    }
  });
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (typeof value === 'string') {
      setDraft(value);
      setError(null);
      return;
    }
    try {
      setDraft(value == null ? '{}' : JSON.stringify(value, null, 2));
      setError(null);
    } catch {
      setDraft('{}');
      setError(null);
    }
  }, [value]);

  function commit(text: string) {
    const trimmed = text.trim();
    if (!trimmed) {
      onChange({});
      setError(null);
      return;
    }

    try {
      const parsed = JSON.parse(trimmed) as unknown;
      if (parsed == null || typeof parsed !== 'object' || Array.isArray(parsed)) {
        setError('必须是 JSON 对象，例如 {"help":5}');
        return;
      }
      onChange(parsed);
      setError(null);
    } catch {
      setError('JSON 格式错误');
    }
  }

  return (
    <div className="min-w-72">
      <textarea
        className="w-full px-4 py-2 rounded-xl border border-brand-soft bg-white text-sm font-mono font-bold text-text-main focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all min-h-24"
        value={draft}
        onChange={(e) => {
          setDraft(e.target.value);
          setError(null);
        }}
        onBlur={() => commit(draft)}
        disabled={disabled}
        spellCheck={false}
      />
      {error ? <div className="mt-1 text-xs font-bold text-red-500">{error}</div> : null}
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

        if (fieldType === 'object') {
          const obj =
            current && typeof current === 'object' && !Array.isArray(current) ? current : {};
          return (
            <FieldShell key={key} item={item}>
              <JsonInput
                value={obj}
                onChange={(next) => onChange(setByPath(value, key, next))}
                disabled={disabled}
              />
            </FieldShell>
          );
        }

        if (current && typeof current === 'object' && !Array.isArray(current)) {
          return (
            <FieldShell key={key} item={item}>
              <JsonInput
                value={current}
                onChange={(next) => onChange(setByPath(value, key, next))}
                disabled={disabled}
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
