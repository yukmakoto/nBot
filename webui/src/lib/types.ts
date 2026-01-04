export type BotInstance = {
  id: string;
  name: string;
  platform: string;
  is_connected?: boolean;
  is_running?: boolean;
  container_id?: string | null;
  ws_port?: number | null;
  webui_port?: number | null;
  qq_id?: string | null;
  linked_database?: string | null;
};

export type DatabaseInstance = {
  id: string;
  name: string;
  db_type: string;
  container_id?: string | null;
  host_port: number;
  internal_port: number;
  username: string;
  password: string;
  database_name: string;
  is_running?: boolean;
};

export type ConfigSelectOption = { value: string; label: string };

export type ConfigSchemaItem = {
  key: string;
  type: string;
  label: string;
  description?: string | null;
  default?: unknown;
  options?: ConfigSelectOption[] | null;
  itemType?: string | null;
  min?: number | null;
  max?: number | null;
};

export type PluginManifest = {
  id: string;
  name: string;
  version: string;
  description: string;
  author: string;
  type?: string;
  builtin?: boolean;
  commands?: string[];
  configSchema?: ConfigSchemaItem[];
  config?: unknown;
};

export type InstalledPlugin = {
  manifest: PluginManifest;
  enabled?: boolean;
  path?: string;
};

export type MarketPlugin = {
  id: string;
  name: string;
  version: string;
  description: string;
  author: string;
  downloads?: number;
  plugin_type?: string;
};

export type ToolInfo = {
  id: string;
  name: string;
  description: string;
  kind?: string;
  container_name: string;
  status: string;
  ports?: number[];
  detail?: string | null;
};

export type TaskProgress = {
  current: number;
  total: number;
  label: string;
};

export type TaskState = 'running' | 'success' | 'error';

export type BackgroundTask = {
  id: string;
  kind: string;
  title: string;
  state: TaskState;
  progress?: TaskProgress | null;
  detail?: string | null;
  result?: unknown;
  error?: string | null;
  created_at: number;
  updated_at: number;
};

