import React, { useEffect } from 'react';
import { RouterProvider } from 'react-router-dom';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { router } from './router';
import { useThemeSync } from './hooks/useTheme';
import { useSettings } from './hooks/useSettings';
import { SetupDialog } from './components/setup/SetupDialog';
import { applyLanguageSetting, useLocale } from './i18n';

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

/** Renders nothing; it only has to live inside the query client. */
const ThemeSync: React.FC = () => {
  useThemeSync();
  return null;
};

/**
 * The screens, re-mounted when the interface language changes so every text is drawn again
 * in it (the route and the query cache are kept).
 */
const LocalizedApp: React.FC = () => {
  const settings = useSettings();
  const appLanguage = settings.data?.app_language;
  const settingsLoaded = settings.isSuccess;
  useEffect(() => {
    if (settingsLoaded) void applyLanguageSetting(appLanguage);
  }, [settingsLoaded, appLanguage]);
  const lang = useLocale();
  return <RouterProvider key={lang} router={router} />;
};

export const App: React.FC = () => {
  return (
    <QueryClientProvider client={queryClient}>
      <ThemeSync />
      <LocalizedApp />
      <SetupDialog />
    </QueryClientProvider>
  );
};
