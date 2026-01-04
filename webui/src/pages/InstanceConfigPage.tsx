import { useEffect, useMemo, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import toast from 'react-hot-toast';
import { useNavigate, useParams } from 'react-router-dom';
import { ArrowLeft, Save, Search, Settings, Trash2, X } from 'lucide-react';

import { api } from '../lib/api';
import { getApiErrorMessage } from '../lib/errors';

type BotModuleOverride = {
  enabled?: boolean | null;
  config?: unknown;
};

type BotDetail = {
  id: string;
  name: string;
  platform: string;
  is_connected?: boolean;
  is_running?: boolean;
  modules_config?: Record<string, BotModuleOverride>;
};

type EffectiveModule = {
  id: string;
  name: string;
  description: string;
  icon?: string;
  enabled: boolean;
  builtin?: boolean;
  config: unknown;
};

export function InstanceConfigPage() {
  const params = useParams();
  const botId = params.id ? decodeURIComponent(params.id) : '';
  const navigate = useNavigate();
  const queryClient = useQueryClient();

  const botQuery = useQuery({
    queryKey: ['bot', botId],
    enabled: !!botId,
    queryFn: async () => {
      const resp = await api.get(`/bots/${encodeURIComponent(botId)}`);
      if (resp.data?.status !== 'success') {
        throw new Error(resp.data?.message ?? '获取机器人信息失败');
      }
      return resp.data.bot as BotDetail;
    },
    refetchInterval: 1000,
  });

  const modulesQuery = useQuery({
    queryKey: ['bot-modules', botId],
    enabled: !!botId,
    queryFn: async () => {
      const resp = await api.get(`/bots/${encodeURIComponent(botId)}/modules`);
      if (resp.data?.status !== 'success') {
        throw new Error(resp.data?.message ?? '获取模块列表失败');
      }
      return (resp.data.modules ?? []) as EffectiveModule[];
    },
    refetchInterval: 2000,
  });

  const bot = botQuery.data ?? null;
  const overrides = bot?.modules_config ?? {};

  const [name, setName] = useState('');
  const [search, setSearch] = useState('');
  const [saving, setSaving] = useState(false);
  const [configTarget, setConfigTarget] = useState<EffectiveModule | null>(null);

  useEffect(() => {
    if (bot?.name) setName(bot.name);
  }, [bot?.name]);

  useEffect(() => {
    if (botQuery.error) toast.error((botQuery.error as Error).message);
  }, [botQuery.error]);
  useEffect(() => {
    if (modulesQuery.error) toast.error((modulesQuery.error as Error).message);
  }, [modulesQuery.error]);

  const filteredModules = useMemo(() => {
    const q = search.trim().toLowerCase();
    const list = modulesQuery.data ?? [];
    if (!q) return list;
    return list.filter(
      (m) => m.name.toLowerCase().includes(q) || m.description.toLowerCase().includes(q) || m.id.toLowerCase().includes(q),
    );
  }, [modulesQuery.data, search]);

  async function saveName() {
    const value = name.trim();
    if (!value || !botId) return;
    if (saving) return;
    setSaving(true);
    try {
      const resp = await api.put(`/bots/${encodeURIComponent(botId)}`, { name: value });
      if (resp.data?.status === 'success') {
        toast.success('已保存');
        await queryClient.invalidateQueries({ queryKey: ['status'] });
        await queryClient.invalidateQueries({ queryKey: ['bot', botId] });
    } else {
      toast.error(resp.data?.message ?? '保存失败');
    }
    } catch (e: unknown) {
      toast.error(getApiErrorMessage(e, '保存失败'));
    } finally {
      setSaving(false);
    }
  }

  async function deleteBot() {
    if (!botId) return;
    if (!confirm(`确认删除实例：${bot?.name ?? botId}（${botId}）？`)) return;
    try {
      const resp = await api.delete(`/bots/${encodeURIComponent(botId)}`);
      if (resp.data?.status === 'success') {
        toast.success('已删除');
        await queryClient.invalidateQueries({ queryKey: ['status'] });
        navigate('/instances', { replace: true });
    } else {
      toast.error(resp.data?.message ?? '删除失败');
    }
    } catch (e: unknown) {
      toast.error(getApiErrorMessage(e, '删除失败'));
    }
  }

  async function toggleModule(m: EffectiveModule) {
    const newEnabled = !m.enabled;
    try {
      await api.put(`/bots/${encodeURIComponent(botId)}/module`, {
        module_id: m.id,
        enabled: newEnabled,
      });
      toast.success(newEnabled ? '已启用模块' : '已禁用模块');
      await queryClient.invalidateQueries({ queryKey: ['bot-modules', botId] });
      await queryClient.invalidateQueries({ queryKey: ['bot', botId] });
    } catch (e: unknown) {
      toast.error(getApiErrorMessage(e, '操作失败'));
    }
  }

  if (!botId) {
    return (
      <div className="card-md">
        <div className="font-black text-text-main">无效的实例 ID</div>
      </div>
    );
  }

  return (
    <div className="space-y-6 pt-2">
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0">
          <div className="flex items-center gap-4 mb-2">
            <button
              className="p-2.5 rounded-2xl hover:bg-brand-soft text-brand/40 hover:text-brand transition-all active:scale-95"
              onClick={() => navigate('/instances')}
              title="返回"
            >
              <ArrowLeft className="w-5 h-5" />
            </button>
            <div className="w-1.5 h-8 bg-brand rounded-full shadow-sm" />
            <h1 className="text-2xl font-black text-text-main tracking-tight font-brand truncate">
              实例配置
            </h1>
          </div>
          <p className="text-sm font-bold text-text-main/60 pl-[68px] truncate">
            {bot ? `${bot.name} · ${bot.platform} · ${bot.id}` : botId}
          </p>
        </div>
        <button className="btn-danger-ghost flex items-center gap-2" onClick={deleteBot}>
          <Trash2 className="w-4 h-4" />
          删除实例
        </button>
      </div>

      <div className="card-md space-y-4">
        <div className="flex items-center justify-between gap-4">
          <div>
            <div className="font-black text-text-main text-lg">基础信息</div>
            <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest mt-1">
              修改实例名称
            </div>
          </div>
          <button className="btn-primary flex items-center gap-2" onClick={saveName} disabled={saving || !name.trim() || name.trim() === bot?.name}>
            <Save className="w-4 h-4" />
            {saving ? '保存中...' : '保存'}
          </button>
        </div>
        <input
          className="w-full px-5 py-3 rounded-2xl border border-brand-soft bg-white text-sm font-bold text-text-main focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="实例名称"
          disabled={botQuery.isLoading}
        />
      </div>

      <div className="card-md">
        <div className="flex items-center justify-between gap-4 mb-6">
          <div>
            <div className="font-black text-text-main text-lg">功能模块</div>
            <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest mt-1">
              为当前实例启用/禁用模块，并配置覆盖参数
            </div>
          </div>
          <div className="relative w-72 max-w-full">
            <Search className="w-4 h-4 absolute left-4 top-1/2 -translate-y-1/2 text-brand/30" />
            <input
              className="w-full pl-11 pr-4 py-3 rounded-2xl border border-brand-soft bg-white/70 hover:bg-white focus:bg-white focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all text-sm font-bold text-text-main"
              placeholder="搜索模块..."
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
          </div>
        </div>

        {modulesQuery.isLoading ? (
          <div className="flex items-center justify-center py-16">
            <div className="w-12 h-12 border-4 border-brand border-t-transparent rounded-full animate-spin" />
          </div>
        ) : filteredModules.length ? (
          <div className="space-y-3">
            {filteredModules.map((m) => {
              const hasOverride = overrides[m.id] !== undefined;
              return (
                <div
                  key={m.id}
                  className="flex items-center justify-between gap-4 p-4 rounded-2xl hover:bg-brand-soft/40 transition-all border border-transparent hover:border-brand-soft"
                >
                  <div className="min-w-0">
                    <div className="flex items-center gap-3">
                      <div className="font-black text-text-main truncate">{m.name}</div>
                      {m.builtin ? (
                        <span className="text-[10px] font-black px-2.5 py-0.5 rounded-full bg-emerald-50 text-emerald-600 uppercase tracking-tight">
                          系统
                        </span>
                      ) : null}
                      {hasOverride ? (
                        <span className="text-[10px] font-black px-2.5 py-0.5 rounded-full bg-brand-soft text-brand uppercase tracking-tight">
                          覆盖
                        </span>
                      ) : null}
                      <span
                        className={
                          m.enabled
                            ? 'text-[10px] font-black px-2.5 py-0.5 rounded-full bg-brand text-white uppercase tracking-tight'
                            : 'text-[10px] font-black px-2.5 py-0.5 rounded-full bg-slate-100 text-slate-500 uppercase tracking-tight'
                        }
                      >
                        {m.enabled ? 'ON' : 'OFF'}
                      </span>
                    </div>
                    <div className="text-xs text-text-main/60 font-bold truncate mt-1">
                      {m.description}
                    </div>
                    <div className="text-[10px] font-black text-brand/30 uppercase tracking-widest mt-1">
                      {m.id}
                    </div>
                  </div>
                  <div className="flex items-center gap-2 shrink-0">
                    <button className="btn-secondary" onClick={() => setConfigTarget(m)} title="配置">
                      <Settings className="w-4 h-4" />
                    </button>
                    <button
                      className={m.enabled ? 'btn-secondary' : 'btn-primary'}
                      onClick={() => toggleModule(m)}
                      title={m.enabled ? '禁用' : '启用'}
                    >
                      {m.enabled ? '禁用' : '启用'}
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
        ) : (
          <div className="text-center py-20 text-brand/40 font-black uppercase tracking-widest">
            未找到模块
          </div>
        )}
      </div>

      {configTarget && bot ? (
        <ModuleConfigModal
          botId={botId}
          module={configTarget}
          hasOverride={overrides[configTarget.id] !== undefined}
          onClose={() => setConfigTarget(null)}
        />
      ) : null}
    </div>
  );
}

function ModuleConfigModal({
  botId,
  module,
  hasOverride,
  onClose,
}: {
  botId: string;
  module: EffectiveModule;
  hasOverride: boolean;
  onClose: () => void;
}) {
  const queryClient = useQueryClient();
  const [busy, setBusy] = useState(false);
  const [text, setText] = useState(() => JSON.stringify(module.config ?? {}, null, 2));

  const parsed = useMemo(() => {
    try {
      return { ok: true as const, value: JSON.parse(text) };
    } catch (e: unknown) {
      return { ok: false as const, error: e instanceof Error ? e.message : 'JSON 解析失败' };
    }
  }, [text]);

  async function save() {
    if (!parsed.ok || busy) return;
    setBusy(true);
    try {
      const resp = await api.put(`/bots/${encodeURIComponent(botId)}/module`, {
        module_id: module.id,
        config: parsed.value,
      });
      if (resp.data?.status === 'success') {
        toast.success('配置已保存');
        await queryClient.invalidateQueries({ queryKey: ['bot-modules', botId] });
        await queryClient.invalidateQueries({ queryKey: ['bot', botId] });
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

  async function clearOverride() {
    if (busy) return;
    if (!confirm(`确认清除模块覆盖：${module.name}（${module.id}）？`)) return;
    setBusy(true);
    try {
      const resp = await api.delete(`/bots/${encodeURIComponent(botId)}/module/${encodeURIComponent(module.id)}`);
      if (resp.data?.status === 'success') {
        toast.success('已清除覆盖');
        await queryClient.invalidateQueries({ queryKey: ['bot-modules', botId] });
        await queryClient.invalidateQueries({ queryKey: ['bot', botId] });
        onClose();
      } else {
        toast.error(resp.data?.message ?? '操作失败');
      }
    } catch (e: unknown) {
      toast.error(getApiErrorMessage(e, '操作失败'));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="modal-backdrop" onClick={() => (!busy ? onClose() : null)}>
      <div className="modal-container max-w-3xl" onClick={(e) => e.stopPropagation()}>
        <div className="bg-brand-soft/50 px-8 py-6 border-b border-brand/10 flex items-center justify-between">
          <div className="min-w-0">
            <div className="text-xl font-black text-text-main truncate">{module.name}</div>
            <div className="text-[10px] font-black uppercase tracking-widest text-brand/40 mt-1 truncate">
              {module.id} · 模块配置（覆盖）
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

        <div className="p-8 space-y-4">
          <div className="text-xs text-text-main/60 font-bold">
            说明：此处编辑的是对当前实例生效的模块配置覆盖（会与全局模块配置合并）。
          </div>
          <textarea
            className="w-full h-[45vh] px-5 py-4 rounded-2xl border border-brand-soft bg-white font-mono text-xs text-text-main focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all clean-scroll"
            value={text}
            onChange={(e) => setText(e.target.value)}
            disabled={busy}
          />
          {!parsed.ok ? (
            <div className="p-3 bg-red-50 border border-red-100 rounded-2xl text-red-600 text-xs font-bold">
              {parsed.error}
            </div>
          ) : null}
        </div>

        <div className="bg-brand-soft/10 px-8 py-6 flex justify-between gap-3 border-t border-brand-soft">
          {hasOverride ? (
            <button className="btn-danger-ghost" onClick={clearOverride} disabled={busy}>
              清除覆盖
            </button>
          ) : (
            <div />
          )}
          <div className="flex items-center gap-3">
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
    </div>
  );
}
