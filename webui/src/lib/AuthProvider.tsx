import React, { useCallback, useEffect, useMemo, useState } from 'react';

import { AuthContext, TOKEN_KEY, UNAUTHORIZED_EVENT, type AuthContextValue } from './auth';

function normalizeToken(token: string | null): string | null {
  if (!token) return null;
  const trimmed = token.trim();
  return trimmed ? trimmed : null;
}

function loadTokenFromStorage(): string | null {
  return normalizeToken(localStorage.getItem(TOKEN_KEY));
}

function saveTokenToStorage(token: string | null) {
  if (!token) {
    localStorage.removeItem(TOKEN_KEY);
    return;
  }
  localStorage.setItem(TOKEN_KEY, token);
}

export function AuthProvider({ children }: { children: React.ReactNode }) {
  const [token, setTokenState] = useState<string | null>(() => loadTokenFromStorage());

  const setToken = useCallback((value: string) => {
    const normalized = normalizeToken(value);
    saveTokenToStorage(normalized);
    setTokenState(normalized);
  }, []);

  const clearToken = useCallback(() => {
    saveTokenToStorage(null);
    setTokenState(null);
  }, []);

  useEffect(() => {
    function onUnauthorized() {
      clearToken();
    }
    window.addEventListener(UNAUTHORIZED_EVENT, onUnauthorized);
    return () => window.removeEventListener(UNAUTHORIZED_EVENT, onUnauthorized);
  }, [clearToken]);

  useEffect(() => {
    function onStorage(e: StorageEvent) {
      if (e.key === TOKEN_KEY) {
        setTokenState(normalizeToken(e.newValue));
      }
    }
    window.addEventListener('storage', onStorage);
    return () => window.removeEventListener('storage', onStorage);
  }, []);

  const value = useMemo<AuthContextValue>(
    () => ({
      token,
      setToken,
      clearToken,
    }),
    [token, setToken, clearToken],
  );

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}
