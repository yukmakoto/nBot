import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import toast from 'react-hot-toast';
import {
  ChevronDown,
  ChevronUp,
  Database,
  Eye,
  EyeOff,
  MessageSquare,
  Plus,
  Save,
  Search,
  Tag,
  Trash2,
  Zap,
} from 'lucide-react';

import { api } from '../lib/api';
import { getApiErrorMessage } from '../lib/errors';

type LLMProvider = {
  id: string;
  name: string;
  type: string;
  api_key: string;
  base_url?: string;
  models?: string[];
};

type LibraryModel = {
  model_id: string;
  provider_id: string;
  enabled?: boolean;
};

type ModelMapping = {
  provider: string;
  model: string;
};

type LlmConfigResponse = {
  status: string;
  providers: LLMProvider[];
  model_library: LibraryModel[];
  mappings: Record<string, ModelMapping>;
  default_model: string;
  tavily_api_key: string;
};

type TabKey = 'providers' | 'library' | 'mapping' | 'websearch' | 'chat';

export function LlmPage() {
  const [tab, setTab] = useState<TabKey>('providers');
  const [providers, setProviders] = useState<LLMProvider[]>([]);
  const [modelLibrary, setModelLibrary] = useState<LibraryModel[]>([]);
  const [mappings, setMappings] = useState<Record<string, ModelMapping>>({});
  const [defaultAlias, setDefaultAlias] = useState('default');
  const [tavilyKey, setTavilyKey] = useState('');
  const [saving, setSaving] = useState(false);
  const [loadedOnce, setLoadedOnce] = useState(false);

  const configQuery = useQuery({
    queryKey: ['llm-config'],
    queryFn: async () => (await api.get('/llm/config')).data as LlmConfigResponse,
    refetchOnWindowFocus: false,
  });

  useEffect(() => {
    if (loadedOnce) return;
    if (configQuery.data?.status !== 'success') return;
    setProviders(configQuery.data.providers ?? []);
    setModelLibrary(configQuery.data.model_library ?? []);
    setMappings(configQuery.data.mappings ?? {});
    setDefaultAlias(configQuery.data.default_model ?? 'default');
    setTavilyKey(configQuery.data.tavily_api_key ?? '');
    setLoadedOnce(true);
  }, [configQuery.data, loadedOnce]);

  const enabledModels = useMemo(
    () => modelLibrary.filter((m) => m.enabled !== false),
    [modelLibrary],
  );

  async function saveConfig() {
    if (saving) return;
    if (!defaultAlias.trim()) {
      toast.error('默认别名不能为空');
      return;
    }
    setSaving(true);
    try {
      const resp = await api.put('/llm/config', {
        providers,
        model_library: modelLibrary,
        mappings,
        default_model: defaultAlias.trim(),
        tavily_api_key: tavilyKey.trim(),
      });
      if (resp.data?.status === 'success') {
        toast.success('配置已保存');
    } else {
      toast.error(resp.data?.message ?? '保存失败');
    }
    } catch (e: unknown) {
      toast.error(getApiErrorMessage(e, '保存失败'));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="space-y-6 pt-2">
      <div className="flex items-center justify-between gap-4">
        <div>
          <div className="flex items-center gap-4 mb-2">
            <div className="w-1.5 h-8 bg-brand rounded-full shadow-sm" />
            <h1 className="text-2xl font-black text-text-main tracking-tight font-brand">
              对话服务
            </h1>
          </div>
          <p className="text-sm font-bold text-text-main/60 pl-6">配置 LLM 供应商、模型库与对话测试</p>
        </div>
        {tab !== 'chat' ? (
          <button
            className="btn-primary flex items-center gap-2"
            onClick={saveConfig}
            disabled={saving}
          >
            <Save className="w-4 h-4" />
            {saving ? '保存中...' : '保存配置'}
          </button>
        ) : null}
      </div>

      {configQuery.isLoading ? (
        <div className="flex items-center justify-center py-20">
          <div className="w-12 h-12 border-4 border-brand border-t-transparent rounded-full animate-spin" />
        </div>
      ) : configQuery.isError ? (
        <div className="card-md">
          <div className="text-sm font-bold text-red-500">
            {(configQuery.error as Error).message || '加载失败'}
          </div>
        </div>
      ) : (
        <div className="grid grid-cols-1 xl:grid-cols-[320px_minmax(0,1fr)] gap-6 pb-10">
          <div className="bg-white/70 border border-white/60 shadow-sm backdrop-blur-xl rounded-[32px] p-4">
            <TabButton
              active={tab === 'providers'}
              icon={<Zap className="w-5 h-5" />}
              label="供应商"
              count={String(providers.length)}
              onClick={() => setTab('providers')}
            />
            <TabButton
              active={tab === 'library'}
              icon={<Database className="w-5 h-5" />}
              label="模型库"
              count={String(enabledModels.length)}
              onClick={() => setTab('library')}
            />
            <TabButton
              active={tab === 'mapping'}
              icon={<Tag className="w-5 h-5" />}
              label="别名映射"
              count={String(Object.keys(mappings).length)}
              onClick={() => setTab('mapping')}
            />
            <TabButton
              active={tab === 'websearch'}
              icon={<Search className="w-5 h-5" />}
              label="联网搜索"
              count={tavilyKey.trim() ? '1' : ''}
              onClick={() => setTab('websearch')}
            />
            <TabButton
              active={tab === 'chat'}
              icon={<MessageSquare className="w-5 h-5" />}
              label="对话测试"
              count=""
              onClick={() => setTab('chat')}
            />
          </div>

          <div className="min-h-[520px]">
            {tab === 'providers' ? (
              <ProvidersTab
                providers={providers}
                setProviders={setProviders}
                modelLibrary={modelLibrary}
                setModelLibrary={setModelLibrary}
              />
            ) : tab === 'library' ? (
              <ModelLibraryTab
                providers={providers}
                modelLibrary={modelLibrary}
                setModelLibrary={setModelLibrary}
              />
            ) : tab === 'mapping' ? (
              <MappingTab
                providers={providers}
                enabledModels={enabledModels}
                mappings={mappings}
                setMappings={setMappings}
                defaultAlias={defaultAlias}
                setDefaultAlias={setDefaultAlias}
              />
            ) : tab === 'websearch' ? (
              <WebSearchTab tavilyKey={tavilyKey} setTavilyKey={setTavilyKey} />
            ) : (
              <ChatTestTab providers={providers} enabledModels={enabledModels} />
            )}
          </div>
        </div>
      )}
    </div>
  );
}

function TabButton({
  active,
  icon,
  label,
  count,
  onClick,
}: {
  active: boolean;
  icon: ReactNode;
  label: string;
  count: string;
  onClick: () => void;
}) {
  return (
    <button
      className={
        active
          ? 'w-full flex items-center justify-between p-4 rounded-2xl bg-brand-soft text-brand shadow-sm transition-all mb-1'
          : 'w-full flex items-center justify-between p-4 rounded-2xl text-text-main/60 hover:bg-brand-soft/50 hover:text-brand transition-all mb-1'
      }
      onClick={onClick}
    >
      <div className="flex items-center gap-4">
        <div className={active ? 'text-brand' : 'text-text-main/20'}>{icon}</div>
        <span className="font-black text-sm uppercase tracking-widest">{label}</span>
      </div>
      {count ? (
        <span
          className={
            active
              ? 'text-[10px] font-black px-2 py-0.5 rounded-full bg-brand text-white uppercase tracking-tight'
              : 'text-[10px] font-black px-2 py-0.5 rounded-full bg-slate-100 text-slate-400 uppercase tracking-tight'
          }
        >
          {count}
        </span>
      ) : null}
    </button>
  );
}

function ProvidersTab({
  providers,
  setProviders,
  modelLibrary,
  setModelLibrary,
}: {
  providers: LLMProvider[];
  setProviders: (next: LLMProvider[]) => void;
  modelLibrary: LibraryModel[];
  setModelLibrary: (next: LibraryModel[]) => void;
}) {
  function addProvider() {
    const suffix = crypto.randomUUID ? crypto.randomUUID().slice(0, 8) : String(Date.now());
    setProviders([
      ...providers,
      {
        id: `provider_${suffix}`,
        name: '新供应商',
        type: 'openai',
        api_key: '',
        base_url: 'https://api.openai.com/v1',
        models: [],
      },
    ]);
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <div className="w-1.5 h-6 bg-brand rounded-full" />
          <h2 className="text-xl font-black text-text-main">供应商管理</h2>
        </div>
        <button className="btn-primary flex items-center gap-2" onClick={addProvider}>
          <Plus className="w-4 h-4" />
          添加供应商
        </button>
      </div>

      {providers.length ? (
        <div className="space-y-4">
          {providers.map((p, idx) => (
            <ProviderCard
              key={p.id}
              idx={idx}
              providers={providers}
              setProviders={setProviders}
              modelLibrary={modelLibrary}
              setModelLibrary={setModelLibrary}
            />
          ))}
        </div>
      ) : (
        <div className="bg-white rounded-[32px] p-16 border-2 border-dashed border-brand-soft text-center">
          <div className="w-20 h-20 bg-brand-soft rounded-full mx-auto mb-4 flex items-center justify-center text-brand/30">
            <Zap className="w-10 h-10" />
          </div>
          <div className="font-black text-text-main mb-2">暂无供应商</div>
          <div className="text-xs text-text-main/50 font-medium">
            点击右上角添加您的第一个 AI 服务供应商
          </div>
        </div>
      )}
    </div>
  );
}

function ProviderCard({
  idx,
  providers,
  setProviders,
  modelLibrary,
  setModelLibrary,
}: {
  idx: number;
  providers: LLMProvider[];
  setProviders: (next: LLMProvider[]) => void;
  modelLibrary: LibraryModel[];
  setModelLibrary: (next: LibraryModel[]) => void;
}) {
  const provider = providers[idx]!;
  const [expanded, setExpanded] = useState(false);
  const [showKey, setShowKey] = useState(false);
  const [testing, setTesting] = useState(false);
  const [fetching, setFetching] = useState(false);

  function update(next: Partial<LLMProvider>) {
    const list = [...providers];
    list[idx] = { ...list[idx], ...next };
    setProviders(list);
  }

  function remove() {
    setProviders(providers.filter((_, i) => i !== idx));
  }

  async function test() {
    if (testing) return;
    const key = (provider.api_key ?? '').trim();
    if (!key) {
      toast.error('请先填写 API Key');
      return;
    }
    setTesting(true);
    try {
      const resp = await api.post('/llm/test', {
        provider: provider.type ?? 'openai',
        api_key: key,
        base_url: (provider.base_url ?? '').trim(),
      });
      if (resp.data?.status === 'success') toast.success(resp.data?.message ?? '连接成功');
      else toast.error(resp.data?.message ?? '连接失败');
    } catch (e: unknown) {
      toast.error(getApiErrorMessage(e, '请求失败'));
    } finally {
      setTesting(false);
    }
  }

  async function fetchModels() {
    if (fetching) return;
    const key = (provider.api_key ?? '').trim();
    if (!key) {
      toast.error('请先填写 API Key');
      return;
    }
    setFetching(true);
    try {
      const resp = await api.post('/llm/models', {
        provider: provider.type ?? 'openai',
        api_key: key,
        base_url: (provider.base_url ?? '').trim(),
      });
      if (resp.data?.status === 'success') {
        const models = (resp.data?.models ?? []) as string[];
        update({ models });
        toast.success(`已获取 ${models.length} 个模型`);
      } else {
        toast.error(resp.data?.message ?? '获取模型失败');
      }
    } catch (e: unknown) {
      toast.error(getApiErrorMessage(e, '获取模型失败'));
    } finally {
      setFetching(false);
    }
  }

  function addToLibrary() {
    const models = provider.models ?? [];
    if (!models.length) return;
    const exists = new Set(modelLibrary.map((m) => `${m.provider_id}::${m.model_id}`));
    const toAdd = models
      .filter((m) => m && !exists.has(`${provider.id}::${m}`))
      .map((m) => ({ provider_id: provider.id, model_id: m, enabled: true }));
    if (!toAdd.length) {
      toast('模型库已包含这些模型');
      return;
    }
    setModelLibrary([...modelLibrary, ...toAdd]);
    toast.success(`已加入 ${toAdd.length} 个模型到模型库`);
  }

  return (
    <div className="bg-white rounded-[28px] border border-brand-soft shadow-sm overflow-hidden">
      <div className="p-6 flex items-center justify-between gap-4">
        <div className="min-w-0">
          <input
            className="font-black text-lg text-text-main bg-transparent outline-none w-full"
            value={provider.name}
            onChange={(e) => update({ name: e.target.value })}
          />
          <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest truncate">
            {(provider.type ?? 'openai').toUpperCase()} · {provider.id}
          </div>
        </div>
        <div className="flex items-center gap-2 shrink-0">
          <button className="btn-secondary" onClick={test} disabled={testing}>
            {testing ? '测试中...' : '连通测试'}
          </button>
          <button
            className="p-2.5 rounded-2xl text-brand/30 hover:text-brand hover:bg-brand-soft transition-all"
            onClick={() => setExpanded((v) => !v)}
            title={expanded ? '收起' : '展开'}
          >
            {expanded ? <ChevronUp className="w-5 h-5" /> : <ChevronDown className="w-5 h-5" />}
          </button>
        </div>
      </div>

      {expanded ? (
        <div className="px-6 pb-6 pt-5 border-t border-brand-soft/50 space-y-4">
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            <div className="space-y-2">
              <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest ml-1">
                接口类型
              </div>
              <select
                className="w-full px-5 py-3 rounded-2xl border border-brand-soft bg-white text-sm font-black text-text-main focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all"
                value={provider.type ?? 'openai'}
                onChange={(e) => update({ type: e.target.value })}
              >
                <option value="openai">OpenAI 兼容</option>
                <option value="claude">Claude</option>
              </select>
              <div className="text-xs text-text-main/60 font-medium">
                说明：当前后端仅对 <code className="font-mono">claude</code> 做专用适配，其余按 OpenAI 兼容处理。
              </div>
            </div>
            <div className="space-y-2">
              <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest ml-1">
                Base URL
              </div>
              <input
                className="w-full px-5 py-3 rounded-2xl border border-brand-soft bg-white text-sm font-bold text-text-main focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all"
                value={provider.base_url ?? ''}
                onChange={(e) => update({ base_url: e.target.value })}
                placeholder={provider.type === 'claude' ? 'https://api.anthropic.com' : 'https://api.openai.com/v1'}
              />
            </div>
          </div>

          <div className="space-y-2">
            <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest ml-1">
              API Key
            </div>
            <div className="relative">
              <input
                className="w-full px-5 py-3 pr-12 rounded-2xl border border-brand-soft bg-white text-sm font-bold text-text-main focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all"
                type={showKey ? 'text' : 'password'}
                value={provider.api_key ?? ''}
                onChange={(e) => update({ api_key: e.target.value })}
                placeholder={provider.type === 'claude' ? 'sk-ant-...' : 'sk-...'}
              />
              <button
                className="absolute right-3 top-1/2 -translate-y-1/2 p-2 text-brand/30 hover:text-brand transition-colors"
                onClick={() => setShowKey((v) => !v)}
                type="button"
                title={showKey ? '隐藏' : '显示'}
              >
                {showKey ? <EyeOff className="w-5 h-5" /> : <Eye className="w-5 h-5" />}
              </button>
            </div>
          </div>

          <div className="flex flex-wrap items-center gap-3">
            <button className="btn-secondary" onClick={fetchModels} disabled={fetching}>
              {fetching ? '获取中...' : '获取模型列表'}
            </button>
            <button className="btn-secondary" onClick={addToLibrary} disabled={!provider.models?.length}>
              加入模型库
            </button>
            <button className="btn-danger-ghost" onClick={remove}>
              删除供应商
            </button>
          </div>

          {provider.models?.length ? (
            <div className="mt-2 p-4 bg-brand-soft/30 border border-brand/10 rounded-3xl">
              <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest mb-3">
                已获取模型（{provider.models.length}）
              </div>
              <div className="flex flex-wrap gap-2">
                {provider.models.slice(0, 24).map((m) => (
                  <span
                    key={m}
                    className="text-[10px] font-black px-3 py-1.5 rounded-full bg-white border border-brand-soft text-text-main/70"
                  >
                    {m}
                  </span>
                ))}
                {provider.models.length > 24 ? (
                  <span className="text-[10px] font-black px-3 py-1.5 rounded-full bg-white border border-brand-soft text-text-main/40">
                    +{provider.models.length - 24}
                  </span>
                ) : null}
              </div>
            </div>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

function ModelLibraryTab({
  providers,
  modelLibrary,
  setModelLibrary,
}: {
  providers: LLMProvider[];
  modelLibrary: LibraryModel[];
  setModelLibrary: (next: LibraryModel[]) => void;
}) {
  const [providerId, setProviderId] = useState('');
  const [modelId, setModelId] = useState('');

  const grouped = useMemo(() => {
    const map = new Map<string, LibraryModel[]>();
    for (const m of modelLibrary) {
      const key = m.provider_id || 'unknown';
      const list = map.get(key) ?? [];
      list.push(m);
      map.set(key, list);
    }
    return map;
  }, [modelLibrary]);

  function addModel() {
    const pid = providerId.trim();
    const mid = modelId.trim();
    if (!pid || !mid) return;
    const exists = modelLibrary.some((m) => m.provider_id === pid && m.model_id === mid);
    if (exists) {
      toast('模型已存在');
      return;
    }
    setModelLibrary([...modelLibrary, { provider_id: pid, model_id: mid, enabled: true }]);
    setModelId('');
  }

  function toggleEnabled(provider_id: string, model_id: string) {
    setModelLibrary(
      modelLibrary.map((m) =>
        m.provider_id === provider_id && m.model_id === model_id
          ? { ...m, enabled: m.enabled === false }
          : m,
      ),
    );
  }

  function removeModel(provider_id: string, model_id: string) {
    setModelLibrary(modelLibrary.filter((m) => !(m.provider_id === provider_id && m.model_id === model_id)));
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center gap-3">
        <div className="w-1.5 h-6 bg-brand rounded-full" />
        <h2 className="text-xl font-black text-text-main">模型库</h2>
      </div>

      <div className="bg-white rounded-[28px] border border-brand-soft shadow-sm p-6 space-y-4">
        <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest">添加模型</div>
        <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
          <select
            className="px-5 py-3 rounded-2xl border border-brand-soft bg-white text-sm font-black text-text-main focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all"
            value={providerId}
            onChange={(e) => setProviderId(e.target.value)}
          >
            <option value="">选择供应商...</option>
            {providers.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name} · {p.id}
              </option>
            ))}
          </select>
          <input
            className="px-5 py-3 rounded-2xl border border-brand-soft bg-white text-sm font-bold text-text-main focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all"
            value={modelId}
            onChange={(e) => setModelId(e.target.value)}
            placeholder="gpt-4o / claude-3-5-sonnet..."
          />
          <button className="btn-primary" onClick={addModel} disabled={!providerId.trim() || !modelId.trim()}>
            添加
          </button>
        </div>
      </div>

      <div className="space-y-4">
        {Array.from(grouped.entries()).map(([pid, list]) => {
          const providerName = providers.find((p) => p.id === pid)?.name ?? pid;
          const enabledCount = list.filter((m) => m.enabled !== false).length;
          return (
            <div key={pid} className="bg-white rounded-[28px] border border-brand-soft shadow-sm overflow-hidden">
              <div className="p-5 bg-brand-soft/30 border-b border-brand-soft flex items-center justify-between gap-4">
                <div className="min-w-0">
                  <div className="font-black text-text-main truncate">{providerName}</div>
                  <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest truncate">
                    {enabledCount}/{list.length} 已启用 · {pid}
                  </div>
                </div>
              </div>
              <div className="p-5 space-y-2">
                {list.map((m) => (
                  <div
                    key={`${m.provider_id}::${m.model_id}`}
                    className={
                      m.enabled === false
                        ? 'flex items-center justify-between gap-4 p-4 rounded-2xl border border-slate-100 bg-slate-50 opacity-70'
                        : 'flex items-center justify-between gap-4 p-4 rounded-2xl border border-brand-soft bg-brand-soft/20'
                    }
                  >
                    <div className="min-w-0">
                      <div className="font-black text-text-main truncate">{m.model_id}</div>
                      <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest truncate">
                        {m.enabled === false ? '禁用' : '启用'}
                      </div>
                    </div>
                    <div className="flex items-center gap-2">
                      <button className="btn-secondary" onClick={() => toggleEnabled(m.provider_id, m.model_id)}>
                        {m.enabled === false ? '启用' : '禁用'}
                      </button>
                      <button className="btn-danger-ghost" onClick={() => removeModel(m.provider_id, m.model_id)} title="删除">
                        <Trash2 className="w-4 h-4" />
                      </button>
                    </div>
                  </div>
                ))}
              </div>
            </div>
          );
        })}

        {!modelLibrary.length ? (
          <div className="text-center py-20 bg-brand-soft/50 rounded-[32px] border-2 border-dashed border-brand/10">
            <Database className="w-14 h-14 mx-auto mb-4 opacity-20" />
            <p className="font-black uppercase tracking-widest text-brand/40">暂无模型</p>
          </div>
        ) : null}
      </div>
    </div>
  );
}

function MappingTab({
  providers,
  enabledModels,
  mappings,
  setMappings,
  defaultAlias,
  setDefaultAlias,
}: {
  providers: LLMProvider[];
  enabledModels: LibraryModel[];
  mappings: Record<string, ModelMapping>;
  setMappings: (next: Record<string, ModelMapping>) => void;
  defaultAlias: string;
  setDefaultAlias: (next: string) => void;
}) {
  const [newAlias, setNewAlias] = useState('');
  const [newValue, setNewValue] = useState('');

  const options = useMemo(() => {
    return enabledModels.map((m) => {
      const providerName = providers.find((p) => p.id === m.provider_id)?.name ?? m.provider_id;
      return {
        value: `${m.provider_id}||${m.model_id}`,
        label: `${m.model_id} (${providerName})`,
      };
    });
  }, [enabledModels, providers]);

  function addMapping() {
    const alias = newAlias.trim();
    if (!alias) return;
    if (mappings[alias]) {
      toast.error('别名已存在');
      return;
    }
    const [pid, mid] = newValue.split('||');
    if (!pid || !mid) return;
    setMappings({ ...mappings, [alias]: { provider: pid, model: mid } });
    setNewAlias('');
    setNewValue('');
  }

  function updateMapping(alias: string, value: string) {
    const [pid, mid] = value.split('||');
    if (!pid || !mid) return;
    setMappings({ ...mappings, [alias]: { provider: pid, model: mid } });
  }

  function remove(alias: string) {
    const next = { ...mappings };
    delete next[alias];
    setMappings(next);
    if (defaultAlias === alias) {
      const fallback = Object.keys(next)[0] ?? 'default';
      setDefaultAlias(fallback);
    }
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center gap-3">
        <div className="w-1.5 h-6 bg-brand rounded-full" />
        <h2 className="text-xl font-black text-text-main">别名映射</h2>
      </div>

      <div className="bg-brand-soft/50 rounded-2xl p-5 border border-brand/10 text-xs text-text-main/70 font-medium">
        别名映射允许你用自定义名称（如 <span className="font-mono">default</span> /{' '}
        <span className="font-mono">fast</span>）引用具体模型，便于随时切换。
      </div>

      <div className="bg-white rounded-[28px] border border-brand-soft shadow-sm p-6 space-y-4">
        <div className="flex items-center justify-between">
          <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest">默认别名</div>
          <div className="text-sm font-black text-text-main">{defaultAlias}</div>
        </div>

        {Object.keys(mappings).length ? (
          <div className="space-y-2">
            {Object.entries(mappings).map(([alias, mapping]) => {
              const current = `${mapping.provider}||${mapping.model}`;
              const providerName = providers.find((p) => p.id === mapping.provider)?.name ?? mapping.provider;
              return (
                <div
                  key={alias}
                  className="grid grid-cols-1 md:grid-cols-[200px_minmax(0,1fr)_160px_180px] gap-3 items-center p-4 bg-brand-soft/30 rounded-2xl border border-transparent hover:border-brand-soft transition-all"
                >
                  <div className="font-black text-text-main truncate">{alias}</div>
                  <select
                    className="w-full min-w-0 px-4 py-2.5 rounded-2xl border border-brand-soft bg-white text-sm font-bold text-text-main focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all"
                    value={current}
                    onChange={(e) => updateMapping(alias, e.target.value)}
                    disabled={!options.length}
                  >
                    {options.length ? (
                      options.map((o) => (
                        <option key={o.value} value={o.value}>
                          {o.label}
                        </option>
                      ))
                    ) : (
                      <option value={current}>{current}</option>
                    )}
                  </select>
                  <div className="min-w-0 text-[11px] font-black text-brand/60 bg-white rounded-xl px-3 py-2 truncate">
                    {providerName}
                  </div>
                  <div className="flex justify-end gap-2">
                    {alias === defaultAlias ? (
                      <span className="text-[10px] font-black px-2.5 py-1 bg-brand text-white rounded-full uppercase tracking-tighter">
                        DEFAULT
                      </span>
                    ) : (
                      <button className="btn-secondary" onClick={() => setDefaultAlias(alias)} title="设为默认">
                        设为默认
                      </button>
                    )}
                    <button className="btn-danger-ghost" onClick={() => remove(alias)} title="删除映射">
                      <Trash2 className="w-4 h-4" />
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
        ) : (
          <div className="text-center py-10 text-brand/30 font-bold">暂无别名映射</div>
        )}

        <div className="mt-6 p-5 bg-brand-soft/30 rounded-2xl border border-dashed border-brand-soft space-y-3">
          <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest">添加新别名</div>
          <div className="grid grid-cols-1 md:grid-cols-[240px_minmax(0,1fr)_140px] gap-3">
            <input
              className="px-5 py-3 rounded-2xl border border-brand-soft bg-white text-sm font-bold text-text-main focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all"
              placeholder="alias (e.g. default, fast)"
              value={newAlias}
              onChange={(e) => setNewAlias(e.target.value)}
            />
            <select
              className="w-full min-w-0 px-5 py-3 rounded-2xl border border-brand-soft bg-white text-sm font-bold text-text-main focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all"
              value={newValue}
              onChange={(e) => setNewValue(e.target.value)}
              disabled={!options.length}
            >
              <option value="">选择模型...</option>
              {options.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
            </select>
            <button
              className="btn-primary w-full px-4 whitespace-nowrap"
              onClick={addMapping}
              disabled={!newAlias.trim() || !newValue || !options.length}
            >
              创建映射
            </button>
          </div>
          {!options.length ? (
            <div className="text-xs text-text-main/60 font-medium">
              暂无可选模型：请先在「模型库」启用至少一个模型。
            </div>
          ) : null}
        </div>
      </div>
    </div>
  );
}

function WebSearchTab({
  tavilyKey,
  setTavilyKey,
}: {
  tavilyKey: string;
  setTavilyKey: (next: string) => void;
}) {
  const [show, setShow] = useState(false);
  const [testing, setTesting] = useState(false);

  async function test() {
    const key = tavilyKey.trim();
    if (!key || testing) return;
    setTesting(true);
    try {
      const resp = await api.post('/llm/tavily/test', { api_key: key });
      if (resp.data?.status === 'success') {
        toast.success(resp.data?.message ?? '连接成功');
      } else {
        toast.error(resp.data?.message ?? '连接失败');
      }
    } catch (e: unknown) {
      toast.error(getApiErrorMessage(e, '请求失败'));
    } finally {
      setTesting(false);
    }
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center gap-3">
        <div className="w-1.5 h-6 bg-brand rounded-full" />
        <h2 className="text-xl font-black text-text-main">联网搜索</h2>
      </div>

      <div className="bg-sky-50/50 rounded-2xl p-5 border border-sky-100 text-xs text-text-main/70 font-medium">
        后端提供 Tavily API Key 连通性测试。填入 Key 并保存后，插件可按约定使用联网搜索能力。
      </div>

      <div className="bg-white rounded-[28px] border border-brand-soft shadow-sm overflow-hidden">
        <div className="p-6 space-y-5">
          <div className="flex items-center gap-4">
            <div className="w-12 h-12 rounded-2xl bg-gradient-to-br from-violet-500 to-purple-600 flex items-center justify-center text-white shadow-lg shadow-violet-500/20">
              <Search className="w-6 h-6" />
            </div>
            <div>
              <div className="font-black text-text-main text-lg">Tavily Search</div>
              <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest">AI 搜索引擎</div>
            </div>
          </div>

          <div className="space-y-2">
            <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest ml-1">
              API Key
            </div>
            <div className="relative">
              <input
                className="w-full px-5 py-3 pr-12 rounded-2xl border border-brand-soft bg-brand-soft/30 text-sm font-bold text-text-main focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all"
                type={show ? 'text' : 'password'}
                placeholder="tvly-..."
                value={tavilyKey}
                onChange={(e) => setTavilyKey(e.target.value)}
              />
              <button
                className="absolute right-3 top-1/2 -translate-y-1/2 p-2 text-brand/30 hover:text-brand transition-colors"
                onClick={() => setShow((v) => !v)}
                type="button"
                title={show ? '隐藏' : '显示'}
              >
                {show ? <EyeOff className="w-5 h-5" /> : <Eye className="w-5 h-5" />}
              </button>
            </div>
          </div>

          <div className="flex items-center justify-between pt-2">
            <a
              className="text-xs text-brand hover:text-brand-hover font-bold transition-colors"
              href="https://tavily.com/"
              target="_blank"
              rel="noreferrer"
            >
              获取 Tavily API Key
            </a>
            <button className="btn-secondary" onClick={test} disabled={testing || !tavilyKey.trim()}>
              {testing ? '测试中...' : '连通测试'}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

function ChatTestTab({
  providers,
  enabledModels,
}: {
  providers: LLMProvider[];
  enabledModels: LibraryModel[];
}) {
  const [selected, setSelected] = useState('');
  const [input, setInput] = useState('');
  const [sending, setSending] = useState(false);
  const [messages, setMessages] = useState<Array<{ role: 'user' | 'assistant'; content: string }>>([]);

  const options = useMemo(() => {
    return enabledModels.map((m) => {
      const providerName = providers.find((p) => p.id === m.provider_id)?.name ?? m.provider_id;
      return {
        value: `${m.provider_id}||${m.model_id}`,
        label: `${m.model_id} (${providerName})`,
      };
    });
  }, [enabledModels, providers]);

  async function send() {
    const text = input.trim();
    if (!text || sending) return;
    const [pid, mid] = selected.split('||');
    if (!pid || !mid) {
      toast.error('请先选择模型');
      return;
    }
    const provider = providers.find((p) => p.id === pid);
    if (!provider) {
      toast.error('供应商不存在');
      return;
    }
    if (!provider.api_key?.trim()) {
      toast.error('供应商缺少 API Key');
      return;
    }

    setSending(true);
    setMessages((prev) => [...prev, { role: 'user', content: text }]);
    setInput('');

    try {
      const resp = await api.post('/llm/chat', {
        provider: provider.type ?? 'openai',
        api_key: provider.api_key.trim(),
        base_url: (provider.base_url ?? '').trim(),
        model: mid,
        messages: [{ role: 'user', content: text }],
      });
      if (resp.data?.status === 'success') {
        setMessages((prev) => [...prev, { role: 'assistant', content: resp.data?.content ?? '' }]);
      } else {
        const msg = resp.data?.message ?? '请求失败';
        toast.error(msg);
        setMessages((prev) => [...prev, { role: 'assistant', content: `❌ ${msg}` }]);
      }
    } catch (e: unknown) {
      const msg = getApiErrorMessage(e, '网络错误');
      toast.error(msg);
      setMessages((prev) => [...prev, { role: 'assistant', content: `❌ ${msg}` }]);
    } finally {
      setSending(false);
    }
  }

  return (
    <div className="space-y-6 h-full flex flex-col">
      <div className="flex items-center gap-3">
        <div className="w-1.5 h-6 bg-brand rounded-full" />
        <h2 className="text-xl font-black text-text-main">对话测试</h2>
      </div>

      <div className="bg-white rounded-[28px] border border-brand-soft shadow-sm p-6 space-y-4">
        <div className="grid grid-cols-1 md:grid-cols-[minmax(0,1fr)_240px] gap-3">
          <select
            className="px-5 py-3 rounded-2xl border border-brand-soft bg-white text-sm font-bold text-text-main focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all"
            value={selected}
            onChange={(e) => setSelected(e.target.value)}
            disabled={!options.length}
          >
            <option value="">选择模型...</option>
            {options.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </select>
          <button className="btn-secondary" onClick={() => setMessages([])} disabled={!messages.length || sending}>
            清空对话
          </button>
        </div>
        {!options.length ? (
          <div className="text-xs text-text-main/60 font-medium">
            暂无可用模型：请先在「模型库」启用至少一个模型，并确保供应商填写了 API Key。
          </div>
        ) : null}
      </div>

      <div className="flex-1 bg-white rounded-[28px] border border-brand-soft shadow-sm overflow-hidden flex flex-col min-h-[420px]">
        <div className="flex-1 p-6 overflow-y-auto clean-scroll space-y-4 bg-brand-soft/10">
          {messages.length ? (
            messages.map((m, idx) => (
              <div key={idx} className={m.role === 'user' ? 'flex justify-end' : 'flex justify-start'}>
                <div
                  className={
                    m.role === 'user'
                      ? 'max-w-[75%] bg-brand text-white px-5 py-3 rounded-2xl rounded-tr-md shadow-sm whitespace-pre-wrap text-sm font-medium'
                      : 'max-w-[75%] bg-white px-5 py-3 rounded-2xl rounded-tl-md shadow-sm border border-brand-soft whitespace-pre-wrap text-sm font-medium'
                  }
                >
                  {m.content}
                </div>
              </div>
            ))
          ) : (
            <div className="flex flex-col items-center justify-center h-full text-brand/30">
              <MessageSquare className="w-12 h-12 opacity-40" />
              <div className="mt-4 font-black text-sm">开始对话测试</div>
              <div className="text-xs mt-1 opacity-70">从模型库选择模型，然后发送消息</div>
            </div>
          )}
          {sending ? (
            <div className="flex justify-start">
              <div className="bg-white px-5 py-3 rounded-2xl rounded-tl-md shadow-sm border border-brand-soft text-brand/50 text-sm font-bold">
                思考中...
              </div>
            </div>
          ) : null}
        </div>

        <div className="p-4 border-t border-brand-soft bg-white">
          <div className="flex items-end gap-3">
            <textarea
              className="flex-1 px-5 py-3 rounded-2xl border border-brand-soft bg-brand-soft/30 text-sm font-medium text-text-main focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all resize-none"
              rows={2}
              placeholder="输入消息..."
              value={input}
              onChange={(e) => setInput(e.target.value)}
              disabled={sending}
            />
            <button
              className="btn-primary flex items-center gap-2"
              onClick={send}
              disabled={sending || !input.trim() || !selected}
            >
              <Zap className="w-4 h-4" />
              发送
            </button>
          </div>
          <div className="text-[10px] text-text-main/40 font-bold mt-2 px-1">
            调用后端 <code className="font-mono">/api/llm/chat</code> 测试模型连通性与响应。
          </div>
        </div>
      </div>
    </div>
  );
}
