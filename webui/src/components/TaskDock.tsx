import { useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import toast from 'react-hot-toast';
import { CheckCircle2, ChevronDown, ChevronUp, LoaderCircle, Trash2, XCircle } from 'lucide-react';

import { api } from '../lib/api';
import { getApiErrorMessage } from '../lib/errors';
import type { BackgroundTask } from '../lib/types';

function normalizeState(s: string): 'running' | 'success' | 'error' | 'unknown' {
  if (s === 'running' || s === 'success' || s === 'error') return s;
  return 'unknown';
}

const EMPTY_TASKS: BackgroundTask[] = [];

export function TaskDock() {
  const tasksQuery = useQuery({
    queryKey: ['tasks'],
    queryFn: async () => (await api.get('/tasks')).data as BackgroundTask[],
    refetchInterval: 1000,
  });

  const tasks = tasksQuery.data ?? EMPTY_TASKS;
  const runningCount = useMemo(
    () => tasks.filter((t) => normalizeState(String(t.state)) === 'running').length,
    [tasks],
  );

  const sessionKey = useMemo(() => {
    if (!tasks.length) return 'none';
    let latest = tasks[0]!;
    for (const task of tasks) {
      if (task.created_at > latest.created_at) latest = task;
    }
    return `${latest.id}:${latest.created_at}`;
  }, [tasks]);

  const [collapsedKey, setCollapsedKey] = useState<string | null>(null);
  const open = collapsedKey !== sessionKey;

  if (!tasks.length) return null;

  return (
    <div className="fixed bottom-6 right-6 z-[60] w-[360px] max-w-[calc(100vw-3rem)]">
      <div className="bg-white/80 backdrop-blur-xl border border-white/60 shadow-xl rounded-[28px] overflow-hidden">
        <button
          className="w-full px-5 py-4 flex items-center justify-between gap-3 hover:bg-brand-soft/40 transition-all"
          onClick={() => setCollapsedKey((prev) => (prev === sessionKey ? null : sessionKey))}
          type="button"
        >
          <div className="flex items-center gap-3 min-w-0">
            <div className="w-10 h-10 rounded-2xl bg-brand-soft flex items-center justify-center text-brand shadow-inner">
              <LoaderCircle className={runningCount ? 'w-5 h-5 animate-spin' : 'w-5 h-5'} />
            </div>
            <div className="min-w-0 text-left">
              <div className="font-black text-text-main truncate">后台任务</div>
              <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest truncate">
                {runningCount ? `运行中 ${runningCount} 个` : `共 ${tasks.length} 个`}
              </div>
            </div>
          </div>
          <div className="text-brand/40">
            {open ? <ChevronDown className="w-5 h-5" /> : <ChevronUp className="w-5 h-5" />}
          </div>
        </button>

        {open ? (
          <div className="p-4 pt-0 space-y-3 max-h-[55vh] overflow-auto clean-scroll">
            {tasks.map((t) => (
              <TaskRow key={t.id} task={t} />
            ))}
          </div>
        ) : null}
      </div>
    </div>
  );
}

function TaskRow({ task }: { task: BackgroundTask }) {
  const [busy, setBusy] = useState(false);

  const state = normalizeState(String(task.state));
  const title = task.title || task.kind || task.id;
  const detail = task.detail || task.error || '';

  const progress = task.progress ?? null;
  const pct =
    progress && progress.total > 0 ? Math.round((progress.current / progress.total) * 100) : null;

  async function dismiss() {
    if (busy) return;
    setBusy(true);
    try {
      const resp = await api.delete(`/tasks/${encodeURIComponent(task.id)}`);
      if (resp.data?.status !== 'success') {
        toast.error(resp.data?.message ?? '删除失败');
      }
    } catch (e: unknown) {
      toast.error(getApiErrorMessage(e, '删除失败'));
    } finally {
      setBusy(false);
    }
  }

  const statusBadge =
    state === 'running' ? (
      <span className="inline-flex items-center gap-1 text-[10px] font-black px-2.5 py-1 rounded-full bg-brand-soft text-brand uppercase tracking-tight">
        <LoaderCircle className="w-3.5 h-3.5 animate-spin" />
        RUNNING
      </span>
    ) : state === 'success' ? (
      <span className="inline-flex items-center gap-1 text-[10px] font-black px-2.5 py-1 rounded-full bg-emerald-50 text-emerald-600 uppercase tracking-tight">
        <CheckCircle2 className="w-3.5 h-3.5" />
        SUCCESS
      </span>
    ) : state === 'error' ? (
      <span className="inline-flex items-center gap-1 text-[10px] font-black px-2.5 py-1 rounded-full bg-red-50 text-red-600 uppercase tracking-tight">
        <XCircle className="w-3.5 h-3.5" />
        ERROR
      </span>
    ) : (
      <span className="inline-flex items-center gap-1 text-[10px] font-black px-2.5 py-1 rounded-full bg-slate-50 text-slate-500 uppercase tracking-tight">
        UNKNOWN
      </span>
    );

  return (
    <div className="bg-white rounded-2xl border border-brand-soft p-4 hover:shadow-md transition-all">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex items-center gap-2 mb-1.5">
            <div className="font-black text-text-main truncate">{title}</div>
            {statusBadge}
          </div>
          {detail ? (
            <div className="text-xs text-text-main/60 font-bold whitespace-pre-wrap">{detail}</div>
          ) : null}
        </div>
        <button
          className="p-2 rounded-2xl text-brand/20 hover:text-red-500 hover:bg-red-50 transition-all disabled:opacity-50"
          onClick={dismiss}
          disabled={busy}
          title="删除任务"
        >
          <Trash2 className="w-5 h-5" />
        </button>
      </div>

      {progress ? (
        <div className="mt-3">
          <div className="flex items-center justify-between text-[10px] font-black text-brand/40 uppercase tracking-widest">
            <span>{progress.label}</span>
            <span>
              {progress.current}/{progress.total}
              {pct !== null ? ` · ${pct}%` : ''}
            </span>
          </div>
          <div className="mt-2 h-2 rounded-full bg-brand-soft overflow-hidden">
            <div
              className="h-full bg-brand"
              style={{ width: `${Math.max(0, Math.min(100, pct ?? 0))}%` }}
            />
          </div>
        </div>
      ) : null}
    </div>
  );
}
