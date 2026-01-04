import React, { useEffect, useMemo, useState } from 'react';

import { SelectionContext, type SelectionContextValue } from './selection';

const SELECTED_BOT_KEY = 'nbot_selected_bot_id';

function normalize(id: string | null): string | null {
  if (!id) return null;
  const trimmed = id.trim();
  return trimmed ? trimmed : null;
}

export function SelectionProvider({ children }: { children: React.ReactNode }) {
  const [selectedBotId, setSelectedBotIdState] = useState<string | null>(() =>
    normalize(localStorage.getItem(SELECTED_BOT_KEY)),
  );

  const setSelectedBotId = (id: string | null) => {
    const normalized = normalize(id);
    if (!normalized) {
      localStorage.removeItem(SELECTED_BOT_KEY);
      setSelectedBotIdState(null);
      return;
    }
    localStorage.setItem(SELECTED_BOT_KEY, normalized);
    setSelectedBotIdState(normalized);
  };

  useEffect(() => {
    function onStorage(e: StorageEvent) {
      if (e.key === SELECTED_BOT_KEY) {
        setSelectedBotIdState(normalize(e.newValue));
      }
    }
    window.addEventListener('storage', onStorage);
    return () => window.removeEventListener('storage', onStorage);
  }, []);

  const value = useMemo<SelectionContextValue>(
    () => ({ selectedBotId, setSelectedBotId }),
    [selectedBotId],
  );

  return <SelectionContext.Provider value={value}>{children}</SelectionContext.Provider>;
}
