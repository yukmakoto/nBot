import { useEffect, useMemo, useRef, useState } from 'react';
import toast from 'react-hot-toast';
import { Copy, Pause, Play, ScrollText, Trash2 } from 'lucide-react';

import { api } from '../lib/api';
import { getApiErrorMessage } from '../lib/errors';

type LogLine = { id: number; line: string };
type LogsResponse = {
  cursor: number;
  next_cursor: number;
  truncated: boolean;
  lines: LogLine[];
};

export function LogsPage() {
  const [lines, setLines] = useState<LogLine[]>([]);
  const [cursor, setCursor] = useState<number | null>(null);
  const [paused, setPaused] = useState(false);
  const [follow, setFollow] = useState(true);
  const [filter, setFilter] = useState('');
  const [loading, setLoading] = useState(false);

  const scrollerRef = useRef<HTMLDivElement | null>(null);
  const cursorRef = useRef<number | null>(null);
  const pausedRef = useRef(false);
  const loadingRef = useRef(false);

  const filtered = useMemo(() => {
    const q = filter.trim().toLowerCase();
    if (!q) return lines;
    return lines.filter((l) => l.line.toLowerCase().includes(q));
  }, [filter, lines]);

  useEffect(() => {
    if (!follow) return;
    const el = scrollerRef.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
  }, [follow, filtered.length]);

  useEffect(() => {
    let alive = true;
    let timer: number | null = null;

    async function tick() {
      if (!alive || pausedRef.current) return;
      if (loadingRef.current) return;
      loadingRef.current = true;
      setLoading(true);
      try {
        const cur = cursorRef.current;
        const params: Record<string, unknown> = { limit: cur == null ? 600 : 1200 };
        if (cur != null) params.cursor = cur;
        const resp = await api.get('/system/logs', { params });
        const data = resp.data as LogsResponse;

        if (!alive) return;

        if (data.truncated && cur != null) {
          toast('日志已截断，已自动重置为最新内容');
          setLines(data.lines ?? []);
          const next = data.next_cursor ?? data.cursor ?? null;
          cursorRef.current = next;
          setCursor(next);
          return;
        }

        if (Array.isArray(data.lines) && data.lines.length) {
          setLines((prev) => {
            const lastId = prev.length ? prev[prev.length - 1]!.id : 0;
            const incoming = data.lines.filter((l) => l.id > lastId);
            return incoming.length ? [...prev, ...incoming] : prev;
          });
        }
        const next = data.next_cursor ?? data.cursor ?? null;
        cursorRef.current = next;
        setCursor(next);
      } catch (e: unknown) {
        if (alive) toast.error(getApiErrorMessage(e, '获取日志失败'));
      } finally {
        loadingRef.current = false;
        if (alive) setLoading(false);
      }
    }

    tick();
    timer = window.setInterval(tick, 1000);

    return () => {
      alive = false;
      if (timer) window.clearInterval(timer);
    };
  }, []);

  useEffect(() => {
    cursorRef.current = cursor;
  }, [cursor]);

  useEffect(() => {
    pausedRef.current = paused;
  }, [paused]);

  async function copy() {
    const text = filtered.map((l) => l.line).join('\n');
    if (!text) {
      toast.error('没有可复制的内容');
      return;
    }

    try {
      await navigator.clipboard.writeText(text);
      toast.success('已复制');
    } catch {
      try {
        const el = document.createElement('textarea');
        el.value = text;
        el.style.position = 'fixed';
        el.style.top = '0';
        el.style.left = '0';
        el.style.opacity = '0';
        document.body.appendChild(el);
        el.focus();
        el.select();
        const ok = document.execCommand('copy');
        document.body.removeChild(el);
        if (ok) toast.success('已复制');
        else toast.error('复制失败，请检查浏览器权限');
      } catch {
        toast.error('复制失败，请检查浏览器权限');
      }
    }
  }

  return (
    <div className="space-y-6 pt-2">
      <div className="flex items-center justify-between gap-4">
        <div>
          <div className="flex items-center gap-4 mb-2">
            <div className="w-1.5 h-8 bg-brand rounded-full shadow-sm" />
            <h1 className="text-2xl font-black text-text-main tracking-tight font-brand">日志</h1>
          </div>
          <p className="text-sm font-bold text-text-main/60 pl-6">实时查看后端运行日志</p>
        </div>

        <div className="flex items-center gap-2">
          <button className="btn-secondary flex items-center gap-2" onClick={copy}>
            <Copy className="w-4 h-4" />
            复制
          </button>
          <button
            className="btn-secondary flex items-center gap-2"
            onClick={() => setLines([])}
            title="清空本地视图（不会影响后端）"
          >
            <Trash2 className="w-4 h-4" />
            清屏
          </button>
          <button
            className="btn-primary flex items-center gap-2"
            onClick={() => setPaused((v) => !v)}
            title={paused ? '继续刷新' : '暂停刷新'}
          >
            {paused ? <Play className="w-4 h-4" /> : <Pause className="w-4 h-4" />}
            {paused ? '继续' : '暂停'}
          </button>
        </div>
      </div>

      <div className="card-md">
        <div className="flex flex-col md:flex-row md:items-center md:justify-between gap-3">
          <div className="flex items-center gap-3 min-w-0">
            <div className="w-10 h-10 rounded-2xl bg-brand-soft flex items-center justify-center text-brand shadow-inner">
              <ScrollText className="w-5 h-5" />
            </div>
            <div className="min-w-0">
              <div className="font-black text-text-main">后端日志</div>
              <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest truncate">
                {paused ? 'PAUSED' : loading ? 'SYNCING' : 'LIVE'} · {lines.length} 行
              </div>
            </div>
          </div>

          <div className="flex flex-wrap items-center gap-2">
            <label className="inline-flex items-center gap-2 text-xs font-black text-text-main/60">
              <input
                type="checkbox"
                className="w-4 h-4 accent-brand"
                checked={follow}
                onChange={(e) => setFollow(e.target.checked)}
              />
              自动滚动
            </label>
            <input
              className="px-4 py-2 rounded-xl border border-brand-soft bg-white text-sm font-bold text-text-main focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all min-w-56"
              placeholder="过滤关键字..."
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
            />
          </div>
        </div>

        <div
          ref={scrollerRef}
          className="mt-5 h-[65vh] overflow-y-auto clean-scroll rounded-[28px] border border-brand-soft bg-black/90 text-white/90 p-4 font-mono text-xs leading-relaxed"
        >
          {filtered.length ? (
            filtered.map((l) => (
              <div key={l.id} className="whitespace-pre-wrap break-words">
                {l.line}
              </div>
            ))
          ) : (
            <div className="text-white/50">暂无日志</div>
          )}
        </div>
      </div>
    </div>
  );
}
