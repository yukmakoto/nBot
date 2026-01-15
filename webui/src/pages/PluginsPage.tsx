import { useEffect, useMemo, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import toast from 'react-hot-toast';
import {
  Download,
  Package,
  Save,
  Search,
  Settings,
  Store,
  Trash2,
  X,
} from 'lucide-react';

import { api } from '../lib/api';
import { getApiErrorMessage } from '../lib/errors';
import { ConfigSchemaForm } from '../components/ConfigSchemaForm';
import { applySchemaDefaults } from '../lib/configSchema';
import type { ConfigSchemaItem, InstalledPlugin, MarketPlugin } from '../lib/types';

const EMPTY_INSTALLED: InstalledPlugin[] = [];
const EMPTY_MARKET: MarketPlugin[] = [];

export function PluginsPage() {
  const queryClient = useQueryClient();
  const [tab, setTab] = useState<'installed' | 'market'>('installed');
  const [category, setCategory] = useState<'modules' | 'plugins'>('modules');
  const [query, setQuery] = useState('');
  const [configTarget, setConfigTarget] = useState<InstalledPlugin | null>(null);

  const installedQuery = useQuery({
    queryKey: ['plugins-installed'],
    queryFn: async () => (await api.get('/plugins/installed')).data as InstalledPlugin[],
    refetchInterval: 2000,
  });

  const marketQuery = useQuery({
    queryKey: ['plugins-market'],
    queryFn: async () => (await api.get('/market/plugins')).data as MarketPlugin[],
    enabled: tab === 'market',
  });

  const installed = installedQuery.data ?? EMPTY_INSTALLED;
  const market = marketQuery.data ?? EMPTY_MARKET;

  const installedVersions = useMemo(() => {
    const map = new Map<string, string>();
    for (const p of installed) {
      const id = p?.manifest?.id;
      if (!id) continue;
      map.set(id, p.manifest.version ?? '');
    }
    return map;
  }, [installed]);

  const filteredMarket = useMemo(() => {
    const q = query.trim().toLowerCase();
    return market.filter((p) => {
      const kind = (p.plugin_type ?? '').trim().toLowerCase();
      const isModule = kind === 'module';
      const typeMatch = category === 'modules' ? isModule : !isModule;
      const searchMatch =
        !q ||
        p.name.toLowerCase().includes(q) ||
        p.description.toLowerCase().includes(q) ||
        p.author.toLowerCase().includes(q);
      return typeMatch && searchMatch;
    });
  }, [market, query, category]);

  return (
    <div className="space-y-6 pt-2">
      <div>
        <div className="flex items-center gap-4 mb-2">
          <div className="w-1.5 h-8 bg-brand rounded-full shadow-sm" />
          <h1 className="text-2xl font-black text-text-main tracking-tight font-brand">插件中心</h1>
        </div>
        <p className="text-sm font-bold text-text-main/60 pl-6">扩展机器人功能与插件管理</p>
      </div>

      <div className="flex items-center gap-2">
        <button
          className={
            tab === 'installed'
              ? 'px-4 py-2 rounded-xl bg-brand text-white font-bold text-sm transition shadow-lg shadow-brand/20'
              : 'px-4 py-2 rounded-xl bg-brand-soft text-brand hover:bg-brand/10 font-bold text-sm transition'
          }
          onClick={() => setTab('installed')}
        >
          <Package className="w-4 h-4 inline mr-2" />
          已安装 <span className="ml-2 text-xs opacity-70">({installed.length})</span>
        </button>
        <button
          className={
            tab === 'market'
              ? 'px-4 py-2 rounded-xl bg-brand text-white font-bold text-sm transition shadow-lg shadow-brand/20'
              : 'px-4 py-2 rounded-xl bg-brand-soft text-brand hover:bg-brand/10 font-bold text-sm transition'
          }
          onClick={() => setTab('market')}
        >
          <Store className="w-4 h-4 inline mr-2" />
          插件市场
        </button>
      </div>

      {tab === 'installed' ? (
        <div className="space-y-3 pb-10">
          {installed.map((p) => (
            <InstalledRow key={p.manifest.id} plugin={p} onConfig={() => setConfigTarget(p)} />
          ))}
          {!installed.length ? (
            <div className="text-center py-12 text-brand/20 bg-brand-soft/50 rounded-3xl border-2 border-dashed border-brand-soft">
              <Package className="w-12 h-12 mx-auto mb-4 opacity-30" />
              <p className="font-bold">暂无已安装插件</p>
              <p className="text-xs mt-2 opacity-70">前往插件市场探索更多功能</p>
            </div>
          ) : null}
        </div>
      ) : (
        <div className="space-y-4 pb-10">
          <div className="flex items-center gap-4">
            <button
              className={
                category === 'modules'
                  ? 'text-sm font-bold text-brand border-b-2 border-brand pb-1'
                  : 'text-sm font-bold text-brand/40 hover:text-brand pb-1'
              }
              onClick={() => setCategory('modules')}
            >
              功能模块
            </button>
            <button
              className={
                category === 'plugins'
                  ? 'text-sm font-bold text-brand border-b-2 border-brand pb-1'
                  : 'text-sm font-bold text-brand/40 hover:text-brand pb-1'
              }
              onClick={() => setCategory('plugins')}
            >
              扩展插件
            </button>
          </div>

          <p className="text-xs text-brand/60 font-medium">
            {category === 'modules'
              ? '功能模块提供独立的新功能，可直接启用'
              : '扩展插件基于已有模块开发，需要先安装对应模块'}
          </p>

          <div className="relative">
            <Search className="w-4 h-4 absolute left-4 top-1/2 -translate-y-1/2 text-brand/40" />
            <input
              className="w-full pl-11 pr-4 py-3 rounded-2xl border border-brand-soft bg-white focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all text-text-main font-bold"
              placeholder="搜索插件..."
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </div>

          {marketQuery.isLoading ? (
            <div className="flex items-center justify-center py-12">
              <div className="w-10 h-10 border-4 border-brand border-t-transparent rounded-full animate-spin" />
            </div>
          ) : (
            <div className="space-y-3">
              {filteredMarket.map((p) => (
                <MarketRow
                  key={p.id}
                  plugin={p}
                  isModule={category === 'modules'}
                  installedVersion={installedVersions.get(p.id)}
                />
              ))}
              {!filteredMarket.length ? (
                <div className="text-center py-12 text-brand/20 bg-brand-soft/50 rounded-3xl border-2 border-dashed border-brand-soft">
                  <Store className="w-12 h-12 mx-auto mb-4 opacity-30" />
                  <p className="font-bold">{category === 'modules' ? '暂无可用模块' : '暂无可用插件'}</p>
                </div>
              ) : null}
            </div>
          )}
        </div>
      )}

      {configTarget ? (
        <PluginConfigModal
          plugin={configTarget}
          onClose={() => setConfigTarget(null)}
          onSaved={() => queryClient.invalidateQueries({ queryKey: ['plugins-installed'] })}
        />
      ) : null}
    </div>
  );
}

function InstalledRow({ plugin, onConfig }: { plugin: InstalledPlugin; onConfig: () => void }) {
  const queryClient = useQueryClient();
  const [busy, setBusy] = useState<'toggle' | 'uninstall' | null>(null);
  const enabled = !!plugin.enabled;
  const hasConfig = !!plugin.manifest.configSchema?.length;

  async function toggle() {
    if (busy) return;
    setBusy('toggle');
    try {
      const action = enabled ? 'disable' : 'enable';
      const resp = await api.post(`/plugins/${encodeURIComponent(plugin.manifest.id)}/${action}`);
      if (resp.data?.status !== 'success') {
        toast.error(resp.data?.message ?? '操作失败');
        return;
      }
      toast.success(enabled ? '已禁用插件' : '已启用插件');
      await queryClient.invalidateQueries({ queryKey: ['plugins-installed'] });
    } catch (e: unknown) {
      toast.error(getApiErrorMessage(e, '操作失败'));
    } finally {
      setBusy(null);
    }
  }

  async function uninstall() {
    if (busy) return;
    if (!confirm(`确认卸载插件：${plugin.manifest.name}（${plugin.manifest.id}）？`)) return;
    setBusy('uninstall');
    try {
      const resp = await api.delete(`/plugins/${encodeURIComponent(plugin.manifest.id)}`);
      if (resp.data?.status !== 'success') {
        toast.error(resp.data?.message ?? '卸载失败');
        return;
      }
      toast.success('已卸载插件');
      await queryClient.invalidateQueries({ queryKey: ['plugins-installed'] });
    } catch (e: unknown) {
      toast.error(getApiErrorMessage(e, '卸载失败'));
    } finally {
      setBusy(null);
    }
  }

  const type = (plugin.manifest.type ?? 'plugin').trim().toLowerCase();
  const typeLabel =
    type === 'module' ? '模块' : type === 'platform' ? '平台' : type === 'plugin' || type === 'bot' ? '插件' : '扩展';

  return (
    <div className="bg-white rounded-[32px] p-7 hover:shadow-xl transition-all duration-500 group relative border border-brand-soft">
      <div className="flex items-center gap-6">
        <div className="w-14 h-14 rounded-2xl bg-brand-soft flex items-center justify-center text-brand shrink-0 shadow-inner">
          <Package className="w-7 h-7" />
        </div>
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-3 mb-1.5">
            <h3 className="text-lg font-black text-text-main truncate">{plugin.manifest.name}</h3>
            <span className="text-[10px] font-black px-2.5 py-0.5 rounded-full shrink-0 uppercase bg-brand-soft text-brand/60">
              {typeLabel}
            </span>
            {plugin.manifest.builtin ? (
              <span className="text-[10px] font-black px-2.5 py-0.5 rounded-full shrink-0 uppercase bg-emerald-50 text-emerald-500">
                内置
              </span>
            ) : null}
          </div>
          <p className="text-sm text-text-main/60 truncate font-bold leading-relaxed">
            {plugin.manifest.description}
          </p>
          <div className="text-[11px] text-brand/50 font-black mt-2">
            v{plugin.manifest.version} · {plugin.manifest.author}
          </div>
        </div>

        <div className="flex items-center gap-3 shrink-0">
          {hasConfig ? (
            <button
              className="p-2.5 rounded-2xl text-brand/30 hover:text-brand hover:bg-brand-soft transition-all disabled:opacity-50"
              onClick={onConfig}
              disabled={busy === 'uninstall'}
              title="配置"
            >
              <Settings className="w-5 h-5" />
            </button>
          ) : null}
          <button
            className={
              enabled
                ? 'px-4 py-2.5 rounded-2xl bg-brand text-white font-black text-xs uppercase tracking-widest shadow-lg shadow-brand/20 hover:bg-brand-hover transition-all disabled:opacity-60'
                : 'px-4 py-2.5 rounded-2xl bg-brand-soft text-brand font-black text-xs uppercase tracking-widest hover:bg-brand/10 transition-all disabled:opacity-60'
            }
            onClick={toggle}
            disabled={busy !== null}
          >
            {enabled ? '已启用' : '已禁用'}
          </button>
          <button
            className="p-2.5 rounded-2xl text-brand/10 hover:text-red-500 hover:bg-red-50 transition-all disabled:opacity-50"
            onClick={uninstall}
            disabled={busy !== null}
            title="卸载"
          >
            <Trash2 className="w-5 h-5" />
          </button>
        </div>
      </div>
    </div>
  );
}

function MarketRow({
  plugin,
  isModule,
  installedVersion,
}: {
  plugin: MarketPlugin;
  isModule: boolean;
  installedVersion?: string;
}) {
  const queryClient = useQueryClient();
  const [busy, setBusy] = useState(false);
  const installed = Boolean(installedVersion);

  async function install() {
    if (busy) return;
    if (installed) return;
    setBusy(true);
    try {
      const resp = await api.post('/market/install', { plugin_id: plugin.id, source: 'market' });
      if (resp.data?.status !== 'success') {
        toast.error(resp.data?.message ?? '安装失败');
        return;
      }
      toast.success('安装成功');
      await queryClient.invalidateQueries({ queryKey: ['plugins-installed'] });
    } catch (e: unknown) {
      toast.error(getApiErrorMessage(e, '安装失败'));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="bg-white rounded-2xl border border-brand-soft p-5 hover:shadow-lg transition-all duration-300">
      <div className="flex items-center gap-5">
        <div
          className={
            isModule
              ? 'w-12 h-12 rounded-xl bg-brand-soft flex items-center justify-center text-brand shrink-0'
              : 'w-12 h-12 rounded-xl bg-sky-50 flex items-center justify-center text-sky-500 shrink-0'
          }
        >
          {isModule ? <Store className="w-6 h-6" /> : <Package className="w-6 h-6" />}
        </div>
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 mb-1">
            <h3 className="font-bold text-text-main truncate">{plugin.name}</h3>
            <span
              className={
                isModule
                  ? 'text-[10px] font-black px-2 py-0.5 rounded-full shrink-0 uppercase bg-brand/10 text-brand'
                  : 'text-[10px] font-black px-2 py-0.5 rounded-full shrink-0 uppercase bg-sky-100 text-sky-600'
              }
            >
              {isModule ? '模块' : '插件'}
            </span>
          </div>
          <p className="text-sm text-text-main/70 truncate font-medium">{plugin.description}</p>
          <p className="text-xs text-brand/40 mt-1 font-bold">
            v{plugin.version} · {plugin.author}
          </p>
          {installed && installedVersion !== plugin.version ? (
            <p className="text-xs text-brand/40 mt-1 font-bold">本地版本：v{installedVersion}</p>
          ) : null}
        </div>
        <div className="flex items-center gap-5 shrink-0">
          <span className="text-xs text-brand/20 flex items-center gap-1.5 font-bold">
            <Download className="w-4 h-4" />
            {plugin.downloads ?? 0}
          </span>
          <button
            className={
              isModule
                ? 'px-5 py-2.5 rounded-xl bg-brand hover:bg-brand-hover active:scale-95 disabled:bg-brand-soft text-white font-bold text-sm transition shadow-lg shadow-brand/20 flex items-center gap-2'
                : 'px-5 py-2.5 rounded-xl bg-sky-500 hover:bg-sky-600 active:scale-95 disabled:bg-sky-100 text-white font-bold text-sm transition shadow-lg shadow-sky-100 flex items-center gap-2'
            }
            onClick={install}
            disabled={busy || installed}
          >
            {busy ? (
              <div className="w-4 h-4 border-2 border-white border-t-transparent rounded-full animate-spin" />
            ) : installed ? (
              <Package className="w-4 h-4" />
            ) : (
              <Download className="w-4 h-4" />
            )}
            {busy ? '安装中' : installed ? '已安装' : '安装'}
          </button>
        </div>
      </div>
    </div>
  );
}

function PluginConfigModal({
  plugin,
  onClose,
  onSaved,
}: {
  plugin: InstalledPlugin;
  onClose: () => void;
  onSaved: () => void;
}) {
  const schema: ConfigSchemaItem[] = plugin.manifest.configSchema ?? [];
  const hasSchema = schema.length > 0;
  const [mode, setMode] = useState<'form' | 'json'>(() => (hasSchema ? 'form' : 'json'));
  const [values, setValues] = useState(() => applySchemaDefaults(schema, plugin.manifest.config));
  const [valuesText, setValuesText] = useState(() => JSON.stringify(values, null, 2));
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (mode !== 'json') return;
    setValuesText(JSON.stringify(values, null, 2));
  }, [mode, values]);

  const parsed = useMemo(() => {
    try {
      return { ok: true, value: JSON.parse(valuesText) };
    } catch (e: unknown) {
      return { ok: false, error: e instanceof Error ? e.message : 'JSON 解析失败' };
    }
  }, [valuesText]);

  function switchTo(next: 'form' | 'json') {
    if (busy) return;
    if (next === mode) return;
    if (next === 'form') {
      if (!parsed.ok) {
        toast.error('JSON 解析失败，无法切回表单');
        return;
      }
      setValues(applySchemaDefaults(schema, parsed.value));
    }
    setMode(next);
  }

  async function save() {
    const payload = mode === 'json' ? (parsed.ok ? parsed.value : null) : values;
    if (payload === null) return;
    setBusy(true);
    try {
      const resp = await api.post(`/plugins/${encodeURIComponent(plugin.manifest.id)}/config`, payload);
      if (resp.data?.status === 'success') {
        toast.success('配置已保存');
        onSaved();
        onClose();
    } else {
      toast.error(resp.data?.message ?? '保存失败');
    }
    } catch (e: unknown) {
      toast.error(getApiErrorMessage(e, '保存失败'));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="modal-backdrop" onClick={() => (!busy ? onClose() : null)}>
      <div
        className="modal-container max-w-3xl flex flex-col max-h-[calc(100vh-2rem)]"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="bg-brand-soft/50 px-8 py-6 border-b border-brand/10 flex items-center justify-between">
          <div className="min-w-0">
            <div className="text-xl font-black text-text-main truncate">{plugin.manifest.name}</div>
            <div className="text-[10px] font-black uppercase tracking-widest text-brand/40 mt-1">
              插件参数配置
            </div>
          </div>
          <button
            className="p-2 rounded-full hover:bg-brand/10 text-brand/40 hover:text-brand transition-all"
            onClick={onClose}
            disabled={busy}
            title="关闭"
          >
            <X className="w-6 h-6" />
          </button>
        </div>

        <div className="p-8 space-y-6 overflow-y-auto clean-scroll flex-1">
          {hasSchema ? (
            <div className="flex items-center justify-between gap-4">
              <div className="flex items-center gap-2">
                <button
                  className={
                    mode === 'form'
                      ? 'px-4 py-2 rounded-xl bg-brand text-white font-bold text-xs uppercase tracking-widest shadow-lg shadow-brand/20'
                      : 'px-4 py-2 rounded-xl bg-brand-soft text-brand/60 hover:text-brand hover:bg-brand/10 font-bold text-xs uppercase tracking-widest'
                  }
                  onClick={() => switchTo('form')}
                  disabled={busy}
                  type="button"
                >
                  表单
                </button>
                <button
                  className={
                    mode === 'json'
                      ? 'px-4 py-2 rounded-xl bg-brand text-white font-bold text-xs uppercase tracking-widest shadow-lg shadow-brand/20'
                      : 'px-4 py-2 rounded-xl bg-brand-soft text-brand/60 hover:text-brand hover:bg-brand/10 font-bold text-xs uppercase tracking-widest'
                  }
                  onClick={() => switchTo('json')}
                  disabled={busy}
                  type="button"
                >
                  JSON
                </button>
              </div>
              <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest">
                schema · {schema.length}
              </div>
            </div>
          ) : null}

          {mode === 'form' && hasSchema ? (
            <ConfigSchemaForm
              schema={schema}
              value={values}
              onChange={setValues}
              disabled={busy}
            />
          ) : (
            <div className="space-y-2">
              <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest ml-1">
                配置 JSON
              </div>
              <textarea
                className="w-full h-[45vh] px-5 py-4 rounded-2xl border border-brand-soft bg-white font-mono text-xs text-text-main focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all clean-scroll"
                value={valuesText}
                onChange={(e) => setValuesText(e.target.value)}
                disabled={busy}
              />
              {!parsed.ok ? (
                <div className="p-3 bg-red-50 border border-red-100 rounded-2xl text-red-600 text-xs font-bold">
                  {parsed.error}
                </div>
              ) : null}
            </div>
          )}
        </div>

        <div className="bg-brand-soft/10 px-8 py-6 flex justify-end gap-3 border-t border-brand-soft">
          <button className="btn-ghost" onClick={onClose} disabled={busy}>
            取消
          </button>
          <button className="btn-primary flex items-center gap-2" onClick={save} disabled={busy || !parsed.ok}>
            <Save className="w-4 h-4" />
            {busy ? '保存中...' : '保存配置'}
          </button>
        </div>
      </div>
    </div>
  );
}
