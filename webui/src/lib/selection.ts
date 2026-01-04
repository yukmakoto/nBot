import { createContext, useContext } from 'react';

export type SelectionContextValue = {
  selectedBotId: string | null;
  setSelectedBotId: (id: string | null) => void;
};

export const SelectionContext = createContext<SelectionContextValue | null>(null);

export function useSelection() {
  const ctx = useContext(SelectionContext);
  if (!ctx) throw new Error('useSelection must be used within SelectionProvider');
  return ctx;
}

