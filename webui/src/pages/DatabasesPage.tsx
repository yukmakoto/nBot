import { useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import toast from 'react-hot-toast';
import { Database, Globe, Link2, Trash2, X } from 'lucide-react';

import { api } from '../lib/api';
import { getApiErrorMessage } from '../lib/errors';
import type { BotInstance, DatabaseInstance } from '../lib/types';

const EMPTY_DATABASES: DatabaseInstance[] = [];
const EMPTY_BOTS: BotInstance[] = [];

export function DatabasesPage() {
  const [createOpen, setCreateOpen] = useState(false);

  const dbQuery = useQuery({
    queryKey: ['databases'],
    queryFn: async () => (await api.get('/databases')).data as DatabaseInstance[],
    refetchInterval: 2000,
  });

  const statusQuery = useQuery({
    queryKey: ['status'],
    queryFn: async () => (await api.get('/status')).data as BotInstance[],
    refetchInterval: 1000,
  });

  const databases = dbQuery.data ?? EMPTY_DATABASES;
  const bots = statusQuery.data ?? EMPTY_BOTS;

  const linkedBotByDbId = useMemo(() => {
    const map = new Map<string, BotInstance>();
    for (const bot of bots) {
      if (bot.linked_database) map.set(bot.linked_database, bot);
    }
    return map;
  }, [bots]);

  return (
    <div className="space-y-6 pt-2">
      <div className="flex items-center justify-between gap-4">
        <div>
          <div className="flex items-center gap-4 mb-2">
            <div className="w-1.5 h-8 bg-brand rounded-full shadow-sm" />
            <h1 className="text-2xl font-black text-text-main tracking-tight font-brand">数据库</h1>
          </div>
          <p className="text-sm font-bold text-text-main/60 pl-6">管理机器人数据存储服务</p>
        </div>
        <button className="btn-primary" onClick={() => setCreateOpen(true)}>
          添加数据库
        </button>
      </div>

      <div className="space-y-4 pb-10">
        {databases.map((db) => (
          <DatabaseRow
            key={db.id}
            db={db}
            linkedBot={linkedBotByDbId.get(db.id) ?? null}
            bots={bots}
          />
        ))}

        {!databases.length ? (
          <div className="text-center py-16 text-brand/20 bg-brand-soft/50 rounded-[32px] border-2 border-dashed border-brand/10">
            <Database className="w-14 h-14 mx-auto mb-4 opacity-20" />
            <p className="font-black uppercase tracking-widest">暂无数据库</p>
            <p className="text-xs mt-2 opacity-70">点击“添加数据库”创建一个</p>
          </div>
        ) : null}
      </div>

      {createOpen ? <CreateDatabaseModal onClose={() => setCreateOpen(false)} /> : null}
    </div>
  );
}

function DatabaseRow({
  db,
  linkedBot,
  bots,
}: {
  db: DatabaseInstance;
  linkedBot: BotInstance | null;
  bots: BotInstance[];
}) {
  const [showCredentials, setShowCredentials] = useState(false);
  const [linkOpen, setLinkOpen] = useState(false);

  async function deleteDb() {
    if (!confirm(`确认删除数据库：${db.name}（${db.id}）？`)) return;
    try {
      await api.delete(`/databases/${encodeURIComponent(db.id)}`);
      toast.success('删除成功');
    } catch (e: unknown) {
      toast.error(getApiErrorMessage(e, '删除失败'));
    }
  }

  async function unlink() {
    if (!linkedBot) return;
    try {
      await api.post('/bots/link-database', { bot_id: linkedBot.id, database_id: null });
      toast.success('已解除关联');
    } catch (e: unknown) {
      toast.error(getApiErrorMessage(e, '解除失败'));
    }
  }

  const typeLabel =
    db.db_type === 'postgres' ? 'PostgreSQL' : db.db_type === 'mysql' ? 'MySQL' : db.db_type === 'redis' ? 'Redis' : 'Database';

  return (
    <>
      <div className="bg-white rounded-[24px] border border-brand-soft p-6 hover:shadow-xl transition-all duration-300">
        <div className="flex items-center gap-5">
          <div className="w-14 h-14 rounded-2xl bg-brand-soft flex items-center justify-center text-brand shrink-0 shadow-inner">
            <Database className="w-7 h-7" />
          </div>

          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-2 mb-1.5">
              <h3 className="font-black text-text-main text-lg truncate">{db.name}</h3>
              <span className="text-[10px] font-black px-2.5 py-1 rounded-full uppercase tracking-tighter bg-brand-soft text-brand">
                {typeLabel}
              </span>
            </div>
            <div className="flex flex-wrap items-center gap-4 text-xs text-text-main/60 font-medium">
              <span className="flex items-center gap-1.5">
                <Globe className="w-4 h-4" />
                localhost:{db.host_port}
              </span>
              <span className="opacity-30">·</span>
              <span className="font-mono">{db.username}</span>
              <button
                className="text-brand hover:text-brand-hover font-black uppercase tracking-tight ml-2 transition-colors"
                onClick={() => setShowCredentials((v) => !v)}
              >
                {showCredentials ? '隐藏信息' : '显示信息'}
              </button>
            </div>

            {showCredentials ? (
              <div className="mt-4 p-4 bg-brand-soft/50 rounded-2xl text-xs font-mono text-text-main space-y-2 border border-brand/10">
                <div className="flex items-center gap-2">
                  <span className="font-black text-brand/40 w-16 uppercase">密码:</span>
                  <span>{db.password}</span>
                </div>
                <div className="flex items-center gap-2">
                  <span className="font-black text-brand/40 w-16 uppercase">库名:</span>
                  <span>{db.database_name}</span>
                </div>
              </div>
            ) : null}
          </div>

          <div className="flex items-center gap-3 shrink-0">
            {linkedBot ? (
              <div className="flex items-center gap-2 bg-brand-soft/40 border border-brand/10 rounded-2xl px-4 py-2">
                <span className="text-xs font-black text-text-main">{linkedBot.name}</span>
                <button className="btn-danger-ghost" onClick={unlink} title="解除关联">
                  解除
                </button>
              </div>
            ) : (
              <button className="btn-secondary flex items-center gap-2" onClick={() => setLinkOpen(true)}>
                <Link2 className="w-4 h-4" />
                关联实例
              </button>
            )}

            <button className="btn-danger-ghost flex items-center gap-2" onClick={deleteDb} title="删除数据库">
              <Trash2 className="w-4 h-4" />
              删除
            </button>
          </div>
        </div>
      </div>

      {linkOpen ? (
        <LinkBotModal dbId={db.id} bots={bots} onClose={() => setLinkOpen(false)} />
      ) : null}
    </>
  );
}

function LinkBotModal({
  dbId,
  bots,
  onClose,
}: {
  dbId: string;
  bots: BotInstance[];
  onClose: () => void;
}) {
  const [selectedBotId, setSelectedBotId] = useState<string>('');

  async function link() {
    if (!selectedBotId) return;
    try {
      await api.post('/bots/link-database', { bot_id: selectedBotId, database_id: dbId });
      toast.success('已关联');
      onClose();
    } catch (e: unknown) {
      toast.error(getApiErrorMessage(e, '关联失败'));
    }
  }

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal-container max-w-md" onClick={(e) => e.stopPropagation()}>
        <div className="bg-brand-soft/50 px-8 py-6 border-b border-brand/10 flex items-center justify-between">
          <div className="font-black text-xl text-text-main uppercase tracking-tight">关联机器人实例</div>
          <button
            className="p-2 rounded-full hover:bg-brand/10 text-brand/40 hover:text-brand transition-all"
            onClick={onClose}
            title="关闭"
          >
            <X className="w-6 h-6" />
          </button>
        </div>
        <div className="p-8 space-y-3">
          {bots.length ? (
            bots.map((b) => (
              <button
                key={b.id}
                className={
                  selectedBotId === b.id
                    ? 'w-full p-4 rounded-2xl border-4 border-brand bg-brand-soft text-left transition-all shadow-lg shadow-brand/20'
                    : 'w-full p-4 rounded-2xl border-2 border-brand-soft bg-white hover:border-brand/20 text-left transition-all'
                }
                onClick={() => setSelectedBotId(b.id)}
              >
                <div className="font-black text-text-main text-lg">{b.name}</div>
                <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest mt-1">
                  {b.platform} · {b.id}
                </div>
              </button>
            ))
          ) : (
            <div className="text-center text-brand/40 py-8 font-bold">暂无可用机器人</div>
          )}
        </div>
        <div className="bg-brand-soft/20 px-8 py-6 flex justify-end gap-4 border-t border-brand-soft">
          <button className="btn-ghost" onClick={onClose}>
            取消
          </button>
          <button className="btn-primary" onClick={link} disabled={!selectedBotId}>
            确认关联
          </button>
        </div>
      </div>
    </div>
  );
}

function CreateDatabaseModal({ onClose }: { onClose: () => void }) {
  const [name, setName] = useState('');
  const [dbType, setDbType] = useState('postgres');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function create() {
    const n = name.trim();
    if (!n) return;
    setBusy(true);
    setError(null);
    try {
      await api.post('/databases', { name: n, db_type: dbType });
      toast.success('创建成功');
      onClose();
    } catch (e: unknown) {
      setError(getApiErrorMessage(e, '创建失败'));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="modal-backdrop" onClick={() => (!busy ? onClose() : null)}>
      <div className="modal-container max-w-md" onClick={(e) => e.stopPropagation()}>
        <div className="bg-brand-soft/50 px-8 py-6 border-b border-brand/10 flex items-center justify-between">
          <div className="font-black text-xl text-text-main uppercase tracking-tight">新建数据库</div>
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
              数据库名称
            </div>
            <input
              className="w-full px-5 py-3 rounded-2xl border border-brand-soft focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all text-text-main font-bold"
              placeholder="my_database"
              value={name}
              onChange={(e) => setName(e.target.value)}
              disabled={busy}
            />
          </div>

          <div className="space-y-2">
            <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest ml-1">
              存储引擎
            </div>
            <select
              className="w-full px-5 py-3 rounded-2xl border border-brand-soft bg-white focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all text-text-main font-bold"
              value={dbType}
              onChange={(e) => setDbType(e.target.value)}
              disabled={busy}
            >
              <option value="postgres">PostgreSQL</option>
              <option value="mysql">MySQL</option>
              <option value="redis">Redis</option>
            </select>
          </div>

          {error ? (
            <div className="p-4 bg-red-50 border border-red-100 rounded-2xl text-red-600 text-xs font-bold">
              {error}
            </div>
          ) : null}
        </div>
        <div className="bg-brand-soft/20 px-8 py-6 flex justify-end gap-4 border-t border-brand-soft">
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
