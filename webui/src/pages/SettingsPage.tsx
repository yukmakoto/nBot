import { useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import toast from 'react-hot-toast';
import { Copy, Download, Eye, EyeOff, Info, RefreshCw, Shield, X } from 'lucide-react';

import { api } from '../lib/api';
import { useAuth } from '../lib/auth';
import { getApiErrorMessage, getApiErrorStatus } from '../lib/errors';

type SystemInfo = {
  version?: string;
  rust_version?: string;
  os?: string;
  arch?: string;
  data_dir?: string;
  uptime_secs?: number;
};

type DockerInfo = {
  available?: boolean;
  version?: string;
  containers_running?: number;
  containers_total?: number;
};

export function SettingsPage() {
  const systemQuery = useQuery({
    queryKey: ['system-info'],
    queryFn: async () => (await api.get('/system/info')).data as SystemInfo,
  });

  const dockerQuery = useQuery({
    queryKey: ['docker-info'],
    queryFn: async () => (await api.get('/docker/info')).data as DockerInfo,
  });

  return (
    <div className="space-y-6 pt-2">
      <div>
        <div className="flex items-center gap-4 mb-2">
          <div className="w-1.5 h-8 bg-brand rounded-full shadow-sm" />
          <h1 className="text-2xl font-black text-text-main tracking-tight font-brand">系统设置</h1>
        </div>
        <p className="text-sm font-bold text-text-main/60 pl-6">管理 nBot 全局配置与系统信息</p>
      </div>

      <ApiTokenCard />

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6 pb-10">
        <InfoCard
          title="系统信息"
          items={[
            ['系统', systemQuery.data?.os ?? '未知'],
            ['架构', systemQuery.data?.arch ?? '未知'],
            ['Rust', systemQuery.data?.rust_version ?? '未知'],
            ['数据目录', systemQuery.data?.data_dir ?? '未知'],
            ['运行时长', formatUptime(systemQuery.data?.uptime_secs ?? 0)],
          ]}
          loading={systemQuery.isLoading}
        />
        <InfoCard
          title="Docker 状态"
          items={[
            ['可用', dockerQuery.data?.available ? '是' : '否'],
            ['版本', dockerQuery.data?.version ?? '未知'],
            [
              '容器',
              `${dockerQuery.data?.containers_running ?? 0}/${dockerQuery.data?.containers_total ?? 0}`,
            ],
          ]}
          loading={dockerQuery.isLoading}
        />
      </div>

      <OfficialPluginsSyncCard />

      <ExportCard />
    </div>
  );
}

function ApiTokenCard() {
  const { token, setToken, clearToken } = useAuth();
  const [show, setShow] = useState(false);
  const [editing, setEditing] = useState(false);
  const [newToken, setNewToken] = useState('');
  const [busy, setBusy] = useState(false);

  const masked = useMemo(() => {
    if (!token) return '未设置';
    if (token.length <= 8) return '***';
    return `${token.slice(0, 4)}...${token.slice(-4)}`;
  }, [token]);

  async function copy() {
    if (!token) return;
    try {
      await navigator.clipboard.writeText(token);
      toast.success('Token 已复制到剪贴板');
    } catch {
      toast.error('复制失败：浏览器不支持或权限不足（HTTP 站点通常会被限制）');
    }
  }

  async function verifyAndSave() {
    const t = newToken.trim();
    if (t.length < 16) {
      toast.error('Token 太短：请粘贴完整的 API Token');
      return;
    }
    setBusy(true);
    try {
      await api.get('/status', { headers: { Authorization: `Bearer ${t}` } });
      setToken(t);
      setNewToken('');
      setEditing(false);
      toast.success('Token 已更新并验证成功');
    } catch (e: unknown) {
      const status = getApiErrorStatus(e);
      toast.error(
        status === 401 || status === 403 ? 'Token 无效' : getApiErrorMessage(e, '验证失败：无法连接后端'),
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="card-md">
      <div className="flex items-start justify-between gap-6">
        <div className="min-w-0">
          <div className="flex items-center gap-3 mb-2">
            <div className="w-10 h-10 rounded-2xl bg-brand-soft flex items-center justify-center text-brand shadow-inner">
              <Shield className="w-5 h-5" />
            </div>
            <div>
              <div className="font-black text-text-main text-lg">API Token</div>
              <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest">
                WebUI 鉴权
              </div>
            </div>
          </div>
          <div className="text-xs font-mono text-text-main/70 bg-brand-soft/30 border border-brand/10 rounded-2xl px-4 py-3">
            {show ? token ?? '未设置' : masked}
          </div>
        </div>

        <div className="shrink-0 flex items-center gap-2">
          <button className="btn-secondary" onClick={() => setShow((v) => !v)} disabled={!token}>
            {show ? <EyeOff className="w-4 h-4" /> : <Eye className="w-4 h-4" />}
          </button>
          <button className="btn-secondary" onClick={copy} disabled={!token}>
            <Copy className="w-4 h-4" />
          </button>
          <button
            className="btn-danger-ghost"
            onClick={() => {
              clearToken();
              toast.success('已清除 Token');
            }}
            disabled={!token}
          >
            <X className="w-4 h-4" />
          </button>
          <button className="btn-secondary" onClick={() => setEditing((v) => !v)}>
            {editing ? '取消' : '修改'}
          </button>
        </div>
      </div>

      {editing ? (
        <div className="mt-6 p-5 bg-brand-soft/30 border border-brand/10 rounded-3xl space-y-3">
          <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest ml-1">
            输入新 Token
          </div>
          <input
            className="w-full px-5 py-3 rounded-2xl border border-brand-soft bg-white text-sm font-bold text-text-main focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all"
            type="password"
            placeholder="粘贴新的 API Token..."
            value={newToken}
            onChange={(e) => setNewToken(e.target.value)}
            disabled={busy}
          />
          <div className="flex justify-end gap-3">
            <button className="btn-ghost" onClick={() => setEditing(false)} disabled={busy}>
              取消
            </button>
            <button className="btn-primary" onClick={verifyAndSave} disabled={busy || newToken.trim().length < 16}>
              {busy ? '验证中...' : '验证并保存'}
            </button>
          </div>
        </div>
      ) : null}
    </div>
  );
}

function InfoCard({
  title,
  items,
  loading,
}: {
  title: string;
  items: Array<[string, string]>;
  loading: boolean;
}) {
  return (
    <div className="card-md">
      <div className="flex items-center gap-3 mb-4">
        <div className="w-10 h-10 rounded-2xl bg-brand-soft flex items-center justify-center text-brand shadow-inner">
          <Info className="w-5 h-5" />
        </div>
        <div className="font-black text-text-main">{title}</div>
      </div>

      {loading ? (
        <div className="text-sm text-brand/50 font-black uppercase tracking-widest animate-pulse">
          加载中...
        </div>
      ) : (
        <div className="space-y-2">
          {items.map(([k, v]) => (
            <div key={k} className="flex items-center justify-between gap-4">
              <div className="text-xs font-black text-brand/40 uppercase tracking-widest">{k}</div>
              <div className="text-sm font-bold text-text-main/70 truncate">{v}</div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

type SyncOfficialPluginsReport = {
  installed: number;
  updated: number;
  skipped: number;
  failed: number;
};

function OfficialPluginsSyncCard() {
  const [busy, setBusy] = useState(false);
  const [forceUpdate, setForceUpdate] = useState(false);
  const [report, setReport] = useState<SyncOfficialPluginsReport | null>(null);

  async function sync() {
    if (busy) return;
    setBusy(true);
    try {
      const payload = forceUpdate ? { force_update: true } : {};
      const resp = await api.post('/market/sync', payload, { timeout: 120_000 });
      if (resp.data?.status !== 'success') {
        toast.error(resp.data?.message ?? '同步失败');
        return;
      }

      const next = resp.data?.report as SyncOfficialPluginsReport | undefined;
      if (!next) {
        toast.error('同步失败：后端返回无效数据');
        return;
      }

      setReport(next);
      toast.success(`同步完成：安装 ${next.installed} / 更新 ${next.updated} / 失败 ${next.failed}`);
    } catch (e: unknown) {
      toast.error(getApiErrorMessage(e, '同步失败'));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="card-md">
      <div className="flex items-start justify-between gap-6">
        <div className="min-w-0">
          <div className="font-black text-text-main text-lg mb-1">官方插件同步</div>
          <div className="text-xs text-text-main/60 font-bold">
            从插件市场同步官方插件（保留配置与启用状态）
          </div>
          {report ? (
            <div className="mt-3 text-xs font-bold text-text-main/60">
              上次结果：安装 {report.installed}，更新 {report.updated}，跳过 {report.skipped}，失败 {report.failed}
            </div>
          ) : null}
          <label className="mt-4 inline-flex items-center gap-2 text-xs font-bold text-text-main/60 select-none">
            <input
              type="checkbox"
              className="accent-brand"
              checked={forceUpdate}
              onChange={(e) => setForceUpdate(e.target.checked)}
              disabled={busy}
            />
            强制更新（即使版本未提升）
          </label>
        </div>

        <button className="btn-primary flex items-center gap-2" onClick={sync} disabled={busy}>
          <RefreshCw className={busy ? 'w-4 h-4 animate-spin' : 'w-4 h-4'} />
          {busy ? '同步中...' : '立即同步'}
        </button>
      </div>
    </div>
  );
}

function ExportCard() {
  const [busy, setBusy] = useState(false);

  async function exportData() {
    if (busy) return;
    setBusy(true);
    try {
      const resp = await api.get('/system/export', { responseType: 'blob' });
      const blob = new Blob([resp.data], {
        type: resp.headers['content-type'] ?? 'application/octet-stream',
      });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = 'nbot-export.zip';
      document.body.appendChild(a);
      a.click();
      a.remove();
      URL.revokeObjectURL(url);
      toast.success('导出已开始');
    } catch (e: unknown) {
      toast.error(getApiErrorMessage(e, '导出失败'));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="card-md">
      <div className="flex items-center justify-between gap-6">
        <div>
          <div className="font-black text-text-main text-lg mb-1">数据导出</div>
          <div className="text-xs text-text-main/60 font-bold">导出当前数据目录与运行态信息</div>
        </div>
        <button className="btn-primary flex items-center gap-2" onClick={exportData} disabled={busy}>
          <Download className="w-4 h-4" />
          {busy ? '导出中...' : '导出'}
        </button>
      </div>
    </div>
  );
}

function formatUptime(secs: number) {
  if (!secs) return '未知';
  if (secs > 86400) return `${Math.floor(secs / 86400)} 天 ${Math.floor((secs % 86400) / 3600)} 小时`;
  if (secs > 3600) return `${Math.floor(secs / 3600)} 小时 ${Math.floor((secs % 3600) / 60)} 分钟`;
  if (secs > 60) return `${Math.floor(secs / 60)} 分钟`;
  return `${secs} 秒`;
}
