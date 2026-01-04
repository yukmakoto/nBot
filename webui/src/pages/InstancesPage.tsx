import { useMemo, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import toast from 'react-hot-toast';
import { useNavigate } from 'react-router-dom';
import {
  Boxes,
  ChevronRight,
  Copy,
  FileText,
  LoaderCircle,
  LogIn,
  Plus,
  Settings,
  X,
} from 'lucide-react';

import { api } from '../lib/api';
import { getApiErrorMessage } from '../lib/errors';
import type { BotInstance } from '../lib/types';

const EMPTY_BOTS: BotInstance[] = [];

export function InstancesPage() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();

  const statusQuery = useQuery({
    queryKey: ['status'],
    queryFn: async () => (await api.get('/status')).data as BotInstance[],
    refetchInterval: 1000,
  });

  const bots = statusQuery.data ?? EMPTY_BOTS;
  const grouped = useMemo(() => groupByPlatform(bots), [bots]);
  const platforms = useMemo(() => sortPlatforms(Object.keys(grouped)), [grouped]);

  const [pending, setPending] = useState<Record<string, boolean>>({});
  const [createOpen, setCreateOpen] = useState(false);
  const [logsTarget, setLogsTarget] = useState<{ id: string; name: string } | null>(null);
  const [copyTarget, setCopyTarget] = useState<{ id: string; name: string } | null>(null);

  async function toggleBot(bot: BotInstance) {
    const id = bot.id;
    const isRunning = !!bot.is_running;
    if (pending[id] !== undefined) return;
    const targetRun = !isRunning;
    setPending((m) => ({ ...m, [id]: targetRun }));

    try {
      const platform = (bot.platform ?? '').toLowerCase();
      if (platform === 'discord') {
        await api.put(`/bots/${encodeURIComponent(id)}/discord`, { is_running: targetRun });
      } else {
        await api.post('/docker/action', { id, action: targetRun ? 'start' : 'stop' });
      }
    } catch (e: unknown) {
      toast.error(getApiErrorMessage(e, '操作失败'));
    } finally {
      setTimeout(() => {
        setPending((m) => {
          const next = { ...m };
          delete next[id];
          return next;
        });
      }, 10_000);
    }
  }

  async function triggerLogin(bot: BotInstance) {
    try {
      const resp = await api.post(`/bots/${encodeURIComponent(bot.id)}/login`);
      const status = resp.data?.status as string | undefined;
      const message = resp.data?.message as string | undefined;
      if (status === 'error') {
        toast.error(message ?? '触发登录失败');
        return;
      }

      const qr = resp.data?.qr as string | null | undefined;
      const qrImage = resp.data?.qr_image as string | null | undefined;
      if (qr) {
        toast.success('已获取登录二维码，请扫码登录');
        queryClient.setQueryData(['napcat-qr'], { qr, qr_image: qrImage ?? null });
      } else {
        toast('已触发登录：请稍候等待二维码更新');
      }
      await queryClient.invalidateQueries({ queryKey: ['napcat-qr'] });
    } catch (e: unknown) {
      toast.error(getApiErrorMessage(e, '触发登录失败'));
    }
  }

  return (
    <div className="space-y-6 pt-2">
      <div className="flex items-center justify-between gap-4">
        <div>
          <div className="flex items-center gap-4 mb-2">
            <div className="w-1.5 h-8 bg-brand rounded-full shadow-sm" />
            <h1 className="text-2xl font-black text-text-main tracking-tight font-brand">
              实例管理
            </h1>
          </div>
          <p className="text-sm font-bold text-text-main/60 pl-6">
            管理机器人适配器与消息平台连接
          </p>
        </div>
        <button className="btn-primary flex items-center gap-2" onClick={() => setCreateOpen(true)}>
          <Plus className="w-4 h-4" />
          添加实例
        </button>
      </div>

      {!bots.length ? (
        <div className="card-md text-center py-16">
          <div className="w-16 h-16 rounded-[28px] bg-brand-soft flex items-center justify-center text-brand mx-auto mb-4 shadow-inner">
            <Boxes className="w-7 h-7" />
          </div>
          <div className="text-xl font-black text-text-main mb-2">还没有创建任何实例</div>
          <div className="text-sm font-bold text-text-main/60 mb-6">
            点击下方按钮创建一个实例，然后扫码登录即可开始使用
          </div>
          <button className="btn-primary inline-flex items-center gap-2" onClick={() => setCreateOpen(true)}>
            <Plus className="w-4 h-4" />
            创建实例
          </button>
        </div>
      ) : (
        <div className="space-y-8 pb-10">
          {platforms.map((platform) => (
            <PlatformSection
              key={platform}
              platform={platform}
              bots={grouped[platform] ?? []}
              pending={pending}
              onToggle={toggleBot}
              onLogs={(id, name) => setLogsTarget({ id, name })}
              onLogin={triggerLogin}
              onCopy={(id, name) => setCopyTarget({ id, name })}
              onConfig={(id) => navigate(`/instances/${encodeURIComponent(id)}`)}
            />
          ))}
        </div>
      )}

      {createOpen ? <CreateInstanceModal onClose={() => setCreateOpen(false)} /> : null}
      {logsTarget ? (
        <LogsModal id={logsTarget.id} name={logsTarget.name} onClose={() => setLogsTarget(null)} />
      ) : null}
      {copyTarget ? (
        <CopyBotModal id={copyTarget.id} name={copyTarget.name} onClose={() => setCopyTarget(null)} />
      ) : null}
    </div>
  );
}

function PlatformSection({
  platform,
  bots,
  pending,
  onToggle,
  onLogs,
  onLogin,
  onCopy,
  onConfig,
}: {
  platform: string;
  bots: BotInstance[];
  pending: Record<string, boolean>;
  onToggle: (bot: BotInstance) => void;
  onLogs: (id: string, name: string) => void;
  onLogin: (bot: BotInstance) => void;
  onCopy: (id: string, name: string) => void;
  onConfig: (id: string) => void;
}) {
  const [collapsed, setCollapsed] = useState(false);

  return (
    <div>
      <div
        className="flex items-center gap-4 mb-6 select-none cursor-pointer group"
        onClick={() => setCollapsed((v) => !v)}
      >
        <div className="flex items-center gap-3 bg-white/80 backdrop-blur-md px-6 py-3 rounded-[24px] shadow-sm border border-white/50 group-hover:bg-white transition-all group-hover:shadow-md group-hover:-translate-y-0.5 duration-300">
          <div className="w-10 h-10 rounded-2xl bg-brand-soft flex items-center justify-center text-brand shadow-inner">
            <Boxes className="w-5 h-5" />
          </div>
          <h2 className="font-black text-xl tracking-tight text-text-main">{platform}</h2>
          <span className="text-[10px] font-black bg-brand-soft text-brand px-2.5 py-1 rounded-full uppercase">
            {bots.length}
          </span>
          <div
            className={`ml-2 transition-transform duration-500 text-brand/20 group-hover:text-brand ${
              collapsed ? '-rotate-90' : 'rotate-90'
            }`}
          >
            <ChevronRight className="w-5 h-5" />
          </div>
        </div>
        <div className="flex-1 h-0.5 bg-gradient-to-r from-brand-soft to-transparent opacity-50 rounded-full" />
      </div>

      {!collapsed ? (
        <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-6">
          {bots.map((bot) => (
            <InstanceCard
              key={bot.id}
              bot={bot}
              pending={pending[bot.id] !== undefined}
              onToggle={() => onToggle(bot)}
              onLogs={() => onLogs(bot.container_id ?? bot.id, bot.name)}
              onLogin={() => onLogin(bot)}
              onCopy={() => onCopy(bot.id, bot.name)}
              onConfig={() => onConfig(bot.id)}
            />
          ))}
        </div>
      ) : null}
    </div>
  );
}

function InstanceCard({
  bot,
  pending,
  onToggle,
  onLogs,
  onLogin,
  onCopy,
  onConfig,
}: {
  bot: BotInstance;
  pending: boolean;
  onToggle: () => void;
  onLogs: () => void;
  onLogin: () => void;
  onCopy: () => void;
  onConfig: () => void;
}) {
  const isRunning = !!bot.is_running;
  const isConnected = !!bot.is_connected;
  return (
    <div className={`card-md card-elevated relative overflow-hidden ${pending ? 'opacity-80' : ''}`}>
      <div className="absolute -right-10 -top-10 w-40 h-40 bg-brand-soft/60 rounded-full blur-2xl" />
      <div className="relative flex flex-col gap-4">
        <div className="flex items-start justify-between gap-4">
          <div className="min-w-0">
            <div className="flex items-center gap-2">
              <div className="font-black text-text-main text-lg truncate">{bot.name}</div>
              {isConnected ? <div className="w-2.5 h-2.5 rounded-full bg-sky-400 shadow-sky-200 shadow-sm animate-pulse" /> : null}
            </div>
            <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest truncate">
              {bot.platform} · {bot.id}
            </div>
            {isRunning ? (
              <div className="mt-2 text-[10px] text-brand font-bold flex items-center gap-1">
                <LoaderCircle className="w-3 h-3 animate-spin" />
                运行中
              </div>
            ) : (
              <div className="mt-2 text-[10px] text-text-main/40 font-bold uppercase tracking-widest">
                已停止
              </div>
            )}
          </div>

          <button
            className={`px-4 py-2 rounded-2xl font-black text-xs uppercase tracking-widest transition-all border ${
              isRunning
                ? 'bg-brand text-white border-brand/10 shadow-lg shadow-brand/20 hover:bg-brand-hover'
                : 'bg-white text-brand border-brand-soft hover:bg-brand-soft'
            } ${pending ? 'pointer-events-none' : ''}`}
            onClick={onToggle}
            title={isRunning ? '停止' : '启动'}
          >
            {pending ? '处理中' : isRunning ? '停止' : '启动'}
          </button>
        </div>

        <div className="flex flex-wrap gap-2">
          {isRunning ? (
            <button className="btn-secondary flex items-center gap-2" onClick={onLogs}>
              <FileText className="w-4 h-4" />
              日志
            </button>
          ) : null}
          {isRunning && !isConnected && bot.platform.toLowerCase() !== 'discord' ? (
            <button className="btn-secondary flex items-center gap-2 text-amber-600" onClick={onLogin}>
              <LogIn className="w-4 h-4" />
              登录
            </button>
          ) : null}
          <button className="btn-secondary flex items-center gap-2" onClick={onCopy}>
            <Copy className="w-4 h-4" />
            复制
          </button>
          <button className="btn-secondary flex items-center gap-2" onClick={onConfig}>
            <Settings className="w-4 h-4" />
            配置
          </button>
        </div>
      </div>
    </div>
  );
}

function CreateInstanceModal({ onClose }: { onClose: () => void }) {
  const [name, setName] = useState('');
  const [platform, setPlatform] = useState('QQ');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function create() {
    const n = name.trim();
    if (!n) return;
    setBusy(true);
    setError(null);
    try {
      const resp = await api.post('/bots', { name: n, platform });
      const status = resp.data?.status;
      if (status === 'accepted') {
        toast.success('创建任务已提交，稍候会出现在列表中');
      } else {
        toast.success('创建成功');
      }
      onClose();
    } catch (e: unknown) {
      setError(getApiErrorMessage(e, '创建失败'));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="modal-backdrop" onClick={() => (!busy ? onClose() : null)}>
      <div className="modal-container max-w-lg" onClick={(e) => e.stopPropagation()}>
        <div className="px-8 py-6 bg-brand-soft/50 border-b border-brand/10 flex items-center justify-between">
          <div>
            <div className="font-black text-xl text-text-main uppercase tracking-tight">创建实例</div>
            <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest">
              创建一个新的机器人适配器
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

        <div className="p-8 space-y-6">
          <div className="space-y-2">
            <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest ml-1">
              名称
            </div>
            <input
              className="w-full px-5 py-3 rounded-2xl border border-brand-soft focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all text-text-main font-bold"
              value={name}
              onChange={(e) => setName(e.target.value)}
              disabled={busy}
              placeholder="例如：QQ Bot 1"
            />
          </div>

          <div className="space-y-2">
            <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest ml-1">
              平台
            </div>
            <select
              className="w-full px-5 py-3 rounded-2xl border border-brand-soft bg-white focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all text-text-main font-bold"
              value={platform}
              onChange={(e) => setPlatform(e.target.value)}
              disabled={busy}
            >
              <option value="QQ">QQ（NapCat OneBot）</option>
              <option value="Discord">Discord（进程内）</option>
            </select>
          </div>

          {error ? (
            <div className="p-4 bg-red-50 border border-red-100 rounded-2xl text-red-600 text-xs font-bold">
              {error}
            </div>
          ) : null}
        </div>

        <div className="px-8 py-6 bg-brand-soft/20 border-t border-brand-soft flex justify-end gap-4">
          <button className="btn-ghost" onClick={onClose} disabled={busy}>
            取消
          </button>
          <button className="btn-primary" onClick={create} disabled={busy || !name.trim()}>
            {busy ? '创建中...' : '立即创建'}
          </button>
        </div>
      </div>
    </div>
  );
}

function LogsModal({ id, name, onClose }: { id: string; name: string; onClose: () => void }) {
  const logsQuery = useQuery({
    queryKey: ['docker-logs', id],
    queryFn: async () => {
      const resp = await api.get('/docker/logs', { params: { id } });
      return (resp.data?.logs ?? '') as string;
    },
    refetchInterval: 1500,
  });

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal-container max-w-4xl" onClick={(e) => e.stopPropagation()}>
        <div className="px-8 py-6 bg-brand-soft/50 border-b border-brand/10 flex items-center justify-between">
          <div className="min-w-0">
            <div className="font-black text-xl text-text-main uppercase tracking-tight truncate">
              日志
            </div>
            <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest truncate">
              {name} · {id}
            </div>
          </div>
          <button
            className="p-2 rounded-full hover:bg-brand/10 text-brand/40 hover:text-brand transition-all"
            onClick={onClose}
            title="关闭"
          >
            <X className="w-6 h-6" />
          </button>
        </div>
        <div className="p-6">
          <pre className="clean-scroll whitespace-pre-wrap text-xs font-mono bg-brand-soft/30 border border-brand-soft rounded-2xl p-5 h-[60vh] overflow-auto">
            {logsQuery.isLoading ? '加载中...' : logsQuery.data ?? ''}
          </pre>
        </div>
      </div>
    </div>
  );
}

function CopyBotModal({ id, name, onClose }: { id: string; name: string; onClose: () => void }) {
  const [newName, setNewName] = useState(`${name}_copy`);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function copy() {
    const v = newName.trim();
    if (!v) return;
    setBusy(true);
    setError(null);
    try {
      await api.post(`/bots/${encodeURIComponent(id)}/copy`, { new_name: v });
      toast.success('克隆成功');
      onClose();
    } catch (e: unknown) {
      setError(getApiErrorMessage(e, '复制失败'));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="modal-backdrop" onClick={() => (!busy ? onClose() : null)}>
      <div className="modal-container max-w-md" onClick={(e) => e.stopPropagation()}>
        <div className="px-8 py-6 bg-brand-soft/50 border-b border-brand/10 flex items-center justify-between">
          <div className="min-w-0">
            <div className="font-black text-xl text-text-main uppercase tracking-tight truncate">
              克隆实例
            </div>
            <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest truncate">
              源实例：{name}
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
        <div className="p-8 space-y-6">
          <div className="space-y-2">
            <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest ml-1">
              新实例名称
            </div>
            <input
              className="w-full px-5 py-3 rounded-2xl border border-brand-soft focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all text-text-main font-bold"
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
              disabled={busy}
            />
          </div>

          {error ? (
            <div className="p-4 bg-red-50 border border-red-100 rounded-2xl text-red-600 text-xs font-bold">
              {error}
            </div>
          ) : null}
        </div>
        <div className="px-8 py-6 bg-brand-soft/20 border-t border-brand-soft flex justify-end gap-4">
          <button className="btn-ghost" onClick={onClose} disabled={busy}>
            取消
          </button>
          <button className="btn-primary" onClick={copy} disabled={busy || !newName.trim()}>
            {busy ? '克隆中...' : '确认克隆'}
          </button>
        </div>
      </div>
    </div>
  );
}

function groupByPlatform(bots: BotInstance[]) {
  const map: Record<string, BotInstance[]> = {};
  for (const b of bots) {
    const key = b.platform || 'Unknown';
    map[key] ??= [];
    map[key].push(b);
  }
  return map;
}

function sortPlatforms(list: string[]) {
  return list.sort((a, b) => {
    if (a.toLowerCase() === 'qq') return -1;
    if (b.toLowerCase() === 'qq') return 1;
    return a.localeCompare(b);
  });
}
