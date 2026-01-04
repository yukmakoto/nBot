import { Navigate, Route, Routes } from 'react-router-dom';
import { useQuery } from '@tanstack/react-query';

import { api } from './lib/api';
import { useAuth } from './lib/auth';
import { getApiErrorStatus } from './lib/errors';
import { AppLayout } from './components/AppLayout';
import { LoadingScreen } from './components/LoadingScreen';
import { TokenPage } from './pages/TokenPage';

import { DashboardPage } from './pages/DashboardPage';
import { InstancesPage } from './pages/InstancesPage';
import { InstanceConfigPage } from './pages/InstanceConfigPage';
import { DatabasesPage } from './pages/DatabasesPage';
import { PluginsPage } from './pages/PluginsPage';
import { LlmPage } from './pages/LlmPage';
import { ToolsPage } from './pages/ToolsPage';
import { CommandsPage } from './pages/CommandsPage';
import { RelationsPage } from './pages/RelationsPage';
import { SettingsPage } from './pages/SettingsPage';
import { LogsPage } from './pages/LogsPage';

export default function App() {
  const { token } = useAuth();

  const statusQuery = useQuery({
    queryKey: ['status', token ?? ''],
    queryFn: async () => (await api.get('/status')).data,
    retry: (failureCount, error: unknown) => {
      const status = getApiErrorStatus(error);
      if (status === 401 || status === 403) return false;
      return failureCount < 2;
    },
    refetchInterval: (query) => (query.state.data ? 1000 : false),
  });

  if (!statusQuery.isSuccess) {
    const status = getApiErrorStatus(statusQuery.error);
    const unauthorized = status === 401 || status === 403;
    const initialError =
      statusQuery.isError && !unauthorized
        ? '后端不可用，请确认后端已启动并检查地址/端口。'
        : token && unauthorized
          ? 'API Token 无效或已过期，请重新填写。'
          : undefined;

    if (statusQuery.isLoading && token) {
      return <LoadingScreen label="正在连接后端..." />;
    }

    return <TokenPage initialError={initialError} />;
  }

  return (
    <AppLayout>
      <Routes>
        <Route path="/" element={<DashboardPage />} />
        <Route path="/instances" element={<InstancesPage />} />
        <Route path="/instances/:id" element={<InstanceConfigPage />} />
        <Route path="/databases" element={<DatabasesPage />} />
        <Route path="/plugins" element={<PluginsPage />} />
        <Route path="/llm" element={<LlmPage />} />
        <Route path="/tools" element={<ToolsPage />} />
        <Route path="/commands" element={<CommandsPage />} />
        <Route path="/logs" element={<LogsPage />} />
        <Route path="/relations" element={<RelationsPage />} />
        <Route path="/settings" element={<SettingsPage />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </AppLayout>
  );
}
