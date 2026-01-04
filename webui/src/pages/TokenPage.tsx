import { useMemo, useState } from 'react';
import toast from 'react-hot-toast';
import { Eye, EyeOff, KeyRound, LoaderCircle } from 'lucide-react';

import { api, API_BASE_KEY, isTauriRuntime, setApiBaseUrl } from '../lib/api';
import { useAuth } from '../lib/auth';
import { getApiErrorStatus } from '../lib/errors';

export function TokenPage({ initialError }: { initialError?: string }) {
  const { setToken } = useAuth();

  const tauri = isTauriRuntime();
  const [showEndpoint, setShowEndpoint] = useState(() => tauri || !!localStorage.getItem(API_BASE_KEY));
  const [endpoint, setEndpoint] = useState(() => {
    const stored = localStorage.getItem(API_BASE_KEY);
    if (stored) return stored;
    return tauri ? 'http://127.0.0.1:32100' : '';
  });

  const [value, setValue] = useState('');
  const [busy, setBusy] = useState(false);
  const [show, setShow] = useState(false);
  const [error, setError] = useState<string | null>(initialError ?? null);

  const canSubmit = useMemo(() => value.trim().length >= 16 && !busy, [value, busy]);

  async function verifyAndSave() {
    const token = value.trim();
    if (token.length < 16) {
      setError('Token 太短：请粘贴完整的 API Token');
      return;
    }

    setError(null);

    const baseResult = setApiBaseUrl(endpoint);
    if (!baseResult.ok) {
      setError(baseResult.error);
      return;
    }

    setBusy(true);
    try {
      await api.get('/status', { headers: { Authorization: `Bearer ${token}` } });
      setToken(token);
      toast.success('Token 验证成功');
    } catch (e: unknown) {
      const status = getApiErrorStatus(e);
      if (status === 401 || status === 403) {
        setError('Token 无效：请确认填写的是当前实例的 API Token');
      } else {
        setError('无法连接后端：请确认后端已启动，并检查端口/网络');
      }
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="min-h-screen flex items-center justify-center p-6">
      <div className="w-full max-w-lg rounded-[40px] bg-white/80 backdrop-blur-xl border border-white/60 shadow-xl overflow-hidden">
        <div className="px-10 py-10 bg-gradient-to-br from-brand-soft to-white">
          <div className="flex items-center gap-4">
            <div className="w-14 h-14 rounded-[24px] bg-white border border-brand/10 shadow-inner overflow-hidden flex items-center justify-center">
              <img src="/nbot_logo.png" alt="nBot" className="w-10 h-10 object-contain" />
            </div>
            <div className="min-w-0">
              <div className="text-2xl font-black text-text-main tracking-tight">nBot WebUI</div>
              <div className="text-[10px] font-black text-brand/50 uppercase tracking-widest">
                请输入 API Token 以继续
              </div>
            </div>
          </div>
        </div>

        <div className="px-10 py-10 space-y-5">
          {showEndpoint ? (
            <div className="space-y-2">
              <div className="flex items-center justify-between gap-3">
                <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest ml-1">
                  后端地址（可选）
                </div>
                {!tauri ? (
                  <button
                    className="text-[10px] font-black text-brand/40 hover:text-brand uppercase tracking-widest transition"
                    type="button"
                    onClick={() => setShowEndpoint(false)}
                    disabled={busy}
                  >
                    隐藏
                  </button>
                ) : null}
              </div>
              <input
                className="w-full px-5 py-3 rounded-2xl border border-brand-soft bg-white focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all text-text-main font-bold"
                placeholder="http://127.0.0.1:32100"
                value={endpoint}
                onChange={(e) => setEndpoint(e.target.value)}
                disabled={busy}
              />
              <div className="text-[10px] text-text-main/50 font-bold px-1">
                示例：<code className="font-mono">http://127.0.0.1:32100</code>（自动补全 <code className="font-mono">/api</code>）
              </div>
            </div>
          ) : (
            <button
              className="btn-ghost w-full"
              type="button"
              onClick={() => setShowEndpoint(true)}
              disabled={busy}
            >
              连接远程/本地后端（高级）
            </button>
          )}

          <div className="space-y-2">
            <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest ml-1">
              API Token
            </div>
            <div className="relative">
              <input
                className="w-full pl-5 pr-14 py-4 rounded-2xl border border-brand-soft bg-white focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all text-text-main font-bold"
                type={show ? 'text' : 'password'}
                placeholder="粘贴 token（默认在 data/state/api_token.txt）"
                value={value}
                onChange={(e) => setValue(e.target.value)}
                disabled={busy}
              />
              <button
                className="absolute right-4 top-1/2 -translate-y-1/2 p-2 rounded-xl hover:bg-brand-soft text-brand/50 hover:text-brand transition-all"
                type="button"
                onClick={() => setShow((v) => !v)}
                disabled={busy}
                title={show ? '隐藏' : '显示'}
              >
                {show ? <EyeOff className="w-5 h-5" /> : <Eye className="w-5 h-5" />}
              </button>
            </div>
          </div>

          {error ? (
            <div className="p-4 bg-red-50 border border-red-100 rounded-2xl text-red-600 text-xs font-bold">
              {error}
            </div>
          ) : null}

          <div className="pt-2 flex items-center gap-3">
            <button
              className="btn-primary flex-1 flex items-center justify-center gap-2"
              type="button"
              onClick={verifyAndSave}
              disabled={!canSubmit}
            >
              {busy ? (
                <LoaderCircle className="w-4 h-4 animate-spin" />
              ) : (
                <KeyRound className="w-4 h-4" />
              )}
              {busy ? '验证中...' : '验证并进入'}
            </button>
            <a
              className="btn-secondary"
              href="https://github.com/yukmakoto/nBot"
              target="_blank"
              rel="noreferrer"
            >
              GitHub
            </a>
          </div>

          <div className="text-xs text-text-main/60 font-medium">
            提示：首次启动会在 <code className="font-mono">data/state/api_token.txt</code> 生成 token。
          </div>
        </div>
      </div>
    </div>
  );
}
