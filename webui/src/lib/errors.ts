import axios from 'axios';

function isRecord(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === 'object' && !Array.isArray(value);
}

export function getApiErrorStatus(error: unknown): number | null {
  if (!axios.isAxiosError(error)) return null;
  const status = error.response?.status;
  return typeof status === 'number' ? status : null;
}

export function getApiErrorMessage(error: unknown, fallback: string): string {
  if (axios.isAxiosError(error)) {
    const data = error.response?.data;
    if (isRecord(data)) {
      const message = data.message;
      if (typeof message === 'string' && message.trim()) return message;
    }
    if (typeof error.message === 'string' && error.message.trim()) return error.message;
    return fallback;
  }

  if (error instanceof Error && typeof error.message === 'string' && error.message.trim()) {
    return error.message;
  }

  return fallback;
}

