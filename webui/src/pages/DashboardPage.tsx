import { useQuery } from '@tanstack/react-query';
import { Cpu, Database, HardDrive, LayoutDashboard } from 'lucide-react';
import type { ReactNode } from 'react';

import { api } from '../lib/api';
import type { BotInstance } from '../lib/types';
import { useSelection } from '../lib/selection';

type SystemStats = {
  cpu_usage: number;
  memory_usage: number;
  disk_usage: number;
};

export function DashboardPage() {
  const { selectedBotId } = useSelection();

  const statusQuery = useQuery({
    queryKey: ['status'],
    queryFn: async () => (await api.get('/status')).data as BotInstance[],
    refetchInterval: 1000,
  });

  const statsQuery = useQuery({
    queryKey: ['system-stats'],
    queryFn: async () => (await api.get('/system/stats')).data as SystemStats,
    refetchInterval: 2000,
  });

  const bots = statusQuery.data ?? [];
  const onlineCount = bots.filter((b) => b.is_running).length;
  const selected = bots.find((b) => b.id === selectedBotId) ?? null;

  const stats = statsQuery.data ?? { cpu_usage: 0, memory_usage: 0, disk_usage: 0 };

  return (
    <div className="space-y-6 pt-2">
      <div className="card-md card-elevated relative overflow-hidden">
        <div className="absolute top-0 right-0 w-64 h-64 bg-brand-soft/50 rounded-full -translate-y-1/2 translate-x-1/2 blur-3xl" />
        <div className="relative flex items-center justify-between gap-6">
          <div className="min-w-0">
            <div className="flex items-center gap-3 mb-2">
              <div className="w-10 h-10 rounded-2xl bg-brand-soft flex items-center justify-center text-brand shadow-inner">
                <LayoutDashboard className="w-5 h-5" />
              </div>
              <h1 className="text-2xl font-black text-text-main tracking-tight truncate">
                {selected ? `Hello ${selected.name}！` : '欢迎使用 nBot'}
              </h1>
            </div>
            <div className="text-[10px] font-black text-brand/50 uppercase tracking-widest">
              {selected ? `实例 ID: ${selected.id}` : '当前还没有选择实例'}
            </div>
          </div>
          <div className="shrink-0 text-right">
            <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest">
              在线实例
            </div>
            <div className="text-3xl font-black text-text-main">{onlineCount}</div>
          </div>
        </div>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-3 gap-6">
        <StatCard label="CPU 使用率" value={stats.cpu_usage} icon={<Cpu className="w-5 h-5" />} />
        <StatCard
          label="内存占用"
          value={stats.memory_usage}
          icon={<Database className="w-5 h-5" />}
        />
        <StatCard
          label="磁盘占用"
          value={stats.disk_usage}
          icon={<HardDrive className="w-5 h-5" />}
        />
      </div>
    </div>
  );
}

function StatCard({
  label,
  value,
  icon,
}: {
  label: string;
  value: number;
  icon: ReactNode;
}) {
  const pct = Number.isFinite(value) ? Math.max(0, Math.min(100, value)) : 0;
  return (
    <div className="card-md card-elevated text-center">
      <div className="w-10 h-10 rounded-2xl bg-brand-soft flex items-center justify-center text-brand mx-auto mb-3 shadow-inner">
        {icon}
      </div>
      <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest mb-1">
        {label}
      </div>
      <div className="text-2xl font-black text-text-main">{pct.toFixed(1)}%</div>
    </div>
  );
}
