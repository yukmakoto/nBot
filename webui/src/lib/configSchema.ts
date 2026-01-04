import type { ConfigSchemaItem } from './types';

export type ConfigValues = Record<string, unknown>;

function isRecord(value: unknown): value is ConfigValues {
  return !!value && typeof value === 'object' && !Array.isArray(value);
}

export function getByPath(obj: ConfigValues, path: string): unknown {
  const parts = path.split('.').filter(Boolean);
  if (!parts.length) return undefined;

  let cursor: unknown = obj;
  for (const part of parts) {
    if (!isRecord(cursor)) return undefined;
    cursor = cursor[part];
  }
  return cursor;
}

export function setByPath(obj: ConfigValues, path: string, value: unknown): ConfigValues {
  const parts = path.split('.').filter(Boolean);
  if (!parts.length) return obj;

  const [head, ...rest] = parts;
  if (!head) return obj;

  const next: ConfigValues = { ...obj };
  if (!rest.length) {
    next[head] = value;
    return next;
  }

  const child = obj[head];
  next[head] = setByPath(isRecord(child) ? child : {}, rest.join('.'), value);
  return next;
}

function defaultForItem(item: ConfigSchemaItem): unknown {
  if (item.default !== undefined) return item.default;

  const fieldType = (item.type ?? 'string').toLowerCase();
  if (fieldType === 'boolean') return false;
  if (fieldType === 'number') return 0;
  if (fieldType === 'array') return [];
  if (fieldType === 'select') return item.options?.[0]?.value ?? '';
  return '';
}

export function applySchemaDefaults(schema: ConfigSchemaItem[], input: unknown): ConfigValues {
  const base = isRecord(input) ? input : {};
  let result: ConfigValues = { ...base };

  for (const item of schema) {
    const key = item.key?.trim();
    if (!key) continue;
    if (getByPath(result, key) !== undefined) continue;
    result = setByPath(result, key, defaultForItem(item));
  }

  return result;
}

