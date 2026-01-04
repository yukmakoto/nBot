import { createContext, useContext } from 'react';

export const TOKEN_KEY = 'nbot_api_token';
export const UNAUTHORIZED_EVENT = 'nbot:unauthorized';

export type AuthContextValue = {
  token: string | null;
  setToken: (token: string) => void;
  clearToken: () => void;
};

export const AuthContext = createContext<AuthContextValue | null>(null);

export function useAuth() {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error('useAuth must be used within AuthProvider');
  return ctx;
}

