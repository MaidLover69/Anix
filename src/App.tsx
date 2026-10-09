import React from 'react';
import { RouterProvider } from 'react-router-dom';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { router } from './router';

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 5 * 60 * 1000, // 5 minutes
      gcTime: 30 * 60 * 1000, // 30 minutes in memory
      retry: 2,
      retryDelay: (attemptIndex) => Math.min(1000 * 2 ** attemptIndex, 10000),
      refetchOnWindowFocus: false, // don't refetch on tab focus
      // refetchOnReconnect is left at its default; queries that must not restart on a
      // network blip (episode sources) opt out in their own hook.
      // Every query is an IPC call to the backend, which also serves local data, so none
      // of them should pause when the webview thinks it is offline.
      networkMode: 'always',
    },
    mutations: {
      networkMode: 'always',
    },
  },
});

export const App: React.FC = () => {
  return (
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
};
