import React from 'react';
import { createRoot } from 'react-dom/client';
import { HashRouter } from 'react-router-dom';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { Toaster } from 'react-hot-toast';

import App from './App';
import { AuthProvider } from './lib/AuthProvider';
import { SelectionProvider } from './lib/SelectionProvider';
import './styles/app.css';

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: false,
      refetchOnWindowFocus: false,
    },
  },
});

createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <AuthProvider>
        <SelectionProvider>
          <HashRouter>
            <App />
          </HashRouter>
          <Toaster position="top-right" />
        </SelectionProvider>
      </AuthProvider>
    </QueryClientProvider>
  </React.StrictMode>,
);
