import { useMemo, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import toast from 'react-hot-toast';
import { Cpu, RotateCw, Wrench } from 'lucide-react';

import { api } from '../lib/api';
import { getApiErrorMessage } from '../lib/errors';
import type { ToolInfo } from '../lib/types';

const EMPTY_TOOLS: ToolInfo[] = [];

export function ToolsPage() {
  const [tab, setTab] = useState<'tools' | 'infra'>('tools');

  const toolsQuery = useQuery({
    queryKey: ['tools'],
    queryFn: async () => (await api.get('/tools')).data as ToolInfo[],
    refetchInterval: 2000,
  });

  const tools = toolsQuery.data ?? EMPTY_TOOLS;
  const toolItems = useMemo(() => tools.filter((t) => t.kind !== 'infra'), [tools]);
  const infraItems = useMemo(() => tools.filter((t) => t.kind === 'infra'), [tools]);

  const list = tab === 'tools' ? toolItems : infraItems;

  return (
    <div className="space-y-6 pt-2">
      <div>
        <div className="flex items-center gap-4 mb-2">
          <div className="w-1.5 h-8 bg-brand rounded-full shadow-sm" />
          <h1 className="text-2xl font-black text-text-main tracking-tight font-brand">工具服务</h1>
        </div>
        <p className="text-sm font-bold text-text-main/60 pl-6">管理工具容器与基础设施服务</p>
      </div>

      <div className="flex gap-2">
        <button
          className={`flex items-center gap-2 px-5 py-2.5 rounded-2xl font-black text-xs uppercase tracking-widest transition-all ${
            tab === 'tools'
              ? 'bg-brand text-white shadow-lg shadow-brand/20'
              : 'bg-white text-brand/50 hover:bg-brand-soft border border-brand-soft'
          }`}
          onClick={() => setTab('tools')}
        >
          <Wrench className="w-4 h-4" />
          工具容器 <span className="ml-1 px-2 py-0.5 rounded-full bg-white/20 text-[10px]">{toolItems.length}</span>
        </button>
        <button
          className={`flex items-center gap-2 px-5 py-2.5 rounded-2xl font-black text-xs uppercase tracking-widest transition-all ${
            tab === 'infra'
              ? 'bg-brand text-white shadow-lg shadow-brand/20'
              : 'bg-white text-brand/50 hover:bg-brand-soft border border-brand-soft'
          }`}
          onClick={() => setTab('infra')}
        >
          <Cpu className="w-4 h-4" />
          基础设施 <span className="ml-1 px-2 py-0.5 rounded-full bg-white/20 text-[10px]">{infraItems.length}</span>
        </button>
      </div>

      <div className="space-y-4 pb-10">
        {toolsQuery.isLoading ? (
          <div className="flex items-center justify-center py-12">
            <div className="w-10 h-10 border-4 border-brand border-t-transparent rounded-full animate-spin" />
          </div>
        ) : !list.length ? (
          <div className="text-center py-20 bg-brand-soft/50 rounded-[32px] border-2 border-dashed border-brand/10">
            {tab === 'tools' ? (
              <Wrench className="w-14 h-14 mx-auto mb-4 opacity-20" />
            ) : (
              <Cpu className="w-14 h-14 mx-auto mb-4 opacity-20" />
            )}
            <p className="font-black uppercase tracking-widest text-brand/40">
              {tab === 'tools' ? '未发现工具容器' : '未发现基础设施'}
            </p>
          </div>
        ) : (
          list.map((tool) => <ToolCard key={tool.id} tool={tool} />)
        )}
      </div>
    </div>
  );
}

function ToolCard({ tool }: { tool: ToolInfo }) {
  const queryClient = useQueryClient();
  const [busy, setBusy] = useState<string | null>(null);

  async function call(action: string) {
    if (busy) return;
    setBusy(action);
    try {
      await api.post(`/tools/${encodeURIComponent(tool.id)}/${action}`);
      toast.success('操作已提交');
      await queryClient.invalidateQueries({ queryKey: ['tools'] });
    } catch (e: unknown) {
      toast.error(getApiErrorMessage(e, '操作失败'));
    } finally {
      setBusy(null);
    }
  }

  return (
    <div className="bg-white rounded-[32px] p-7 border border-brand-soft hover:shadow-xl transition-all duration-500">
      <div className="flex items-start justify-between gap-6">
        <div className="min-w-0">
          <div className="flex items-center gap-3 mb-2">
            <div className="w-12 h-12 rounded-2xl bg-brand-soft flex items-center justify-center text-brand shadow-inner">
              <Wrench className="w-6 h-6" />
            </div>
            <div className="min-w-0">
              <div className="font-black text-text-main text-lg truncate">{tool.name}</div>
              <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest truncate">
                {tool.status} · {tool.container_name}
              </div>
            </div>
          </div>
          <div className="text-sm text-text-main/60 font-bold leading-relaxed">
            {tool.description}
          </div>
          {tool.detail ? (
            <div className="mt-3 text-xs font-mono text-text-main/60 bg-brand-soft/40 border border-brand/10 rounded-2xl p-4">
              {tool.detail}
            </div>
          ) : null}
        </div>

        <div className="shrink-0 grid grid-cols-2 gap-2 w-[220px]">
          <button className="btn-secondary w-full" onClick={() => call('start')} disabled={!!busy}>
            启动
          </button>
          <button className="btn-secondary w-full" onClick={() => call('stop')} disabled={!!busy}>
            停止
          </button>
          <button
            className="btn-secondary w-full flex items-center justify-center gap-2"
            onClick={() => call('restart')}
            disabled={!!busy}
          >
            <RotateCw className="w-4 h-4" />
            重启
          </button>
          <button className="btn-secondary w-full" onClick={() => call('pull')} disabled={!!busy}>
            拉取
          </button>
          <button className="btn-secondary w-full col-span-2" onClick={() => call('recreate')} disabled={!!busy}>
            重建
          </button>
        </div>
      </div>
    </div>
  );
}
