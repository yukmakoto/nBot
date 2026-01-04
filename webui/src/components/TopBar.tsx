import { useEffect } from 'react';
import { useQuery } from '@tanstack/react-query';
import { ChevronDown, Shield, X } from 'lucide-react';
import toast from 'react-hot-toast';

import { api } from '../lib/api';
import { useAuth } from '../lib/auth';
import { useSelection } from '../lib/selection';

type BotInstance = {
  id: string;
  name: string;
  platform: string;
  is_running?: boolean;
  is_connected?: boolean;
};

export function TopBar() {
  const { clearToken } = useAuth();
  const { selectedBotId, setSelectedBotId } = useSelection();

  const statusQuery = useQuery({
    queryKey: ['status'],
    queryFn: async () => (await api.get('/status')).data as BotInstance[],
    refetchInterval: 1000,
  });

  const bots = statusQuery.data ?? [];
  const effectiveSelectedId =
    selectedBotId && bots.some((b) => b.id === selectedBotId)
      ? selectedBotId
      : bots[0]?.id ?? null;

  useEffect(() => {
    if (effectiveSelectedId !== selectedBotId) {
      setSelectedBotId(effectiveSelectedId);
    }
  }, [effectiveSelectedId, selectedBotId, setSelectedBotId]);

  return (
    <header className="px-6 py-5 flex items-center justify-between gap-4">
      <div className="flex items-center gap-3 min-w-0">
        <div className="w-1.5 h-8 bg-brand rounded-full shadow-sm" />
        <div className="min-w-0">
          <div className="font-black text-text-main text-lg tracking-tight truncate">
            nBot 控制台
          </div>
          <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest truncate">
            {bots.length ? `已发现 ${bots.length} 个实例` : '暂无实例'}
          </div>
        </div>
      </div>

      <div className="flex items-center gap-3">
        <div className="relative">
          <select
            className="appearance-none pl-4 pr-10 py-2.5 rounded-2xl bg-white/70 border border-brand-soft shadow-sm hover:border-brand/20 focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all text-sm font-black text-text-main"
            value={effectiveSelectedId ?? ''}
            onChange={(e) => setSelectedBotId(e.target.value || null)}
            disabled={!bots.length}
          >
            {bots.length ? (
              bots.map((b) => (
                <option key={b.id} value={b.id}>
                  {b.name} · {b.platform}
                </option>
              ))
            ) : (
              <option value="">未选择实例</option>
            )}
          </select>
          <ChevronDown className="w-4 h-4 absolute right-4 top-1/2 -translate-y-1/2 text-brand/40 pointer-events-none" />
        </div>

        <button
          className="btn-secondary flex items-center gap-2"
          onClick={() => {
            clearToken();
            toast.success('已清除 API Token');
          }}
          title="清除 Token"
        >
          <Shield className="w-4 h-4" />
          <span className="hidden sm:inline">Token</span>
          <X className="w-4 h-4 opacity-60" />
        </button>
      </div>
    </header>
  );
}
