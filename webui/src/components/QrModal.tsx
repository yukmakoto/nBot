import { useEffect, useMemo, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import toast from 'react-hot-toast';
import { Copy, QrCode, X } from 'lucide-react';

import { api } from '../lib/api';

type QrResponse = { qr: string | null; qr_image?: string | null };

function normalizeQr(value: unknown): string | null {
  if (typeof value !== 'string') return null;
  const trimmed = value.trim();
  if (!trimmed) return null;
  if (trimmed.startsWith('http://') || trimmed.startsWith('https://') || trimmed.startsWith('data:image')) {
    return trimmed;
  }
  return null;
}

function normalizeQrImage(value: unknown): string | null {
  if (typeof value !== 'string') return null;
  const trimmed = value.trim();
  if (!trimmed) return null;
  if (trimmed.startsWith('data:image')) return trimmed;
  if (trimmed.startsWith('http://') || trimmed.startsWith('https://')) return trimmed;
  return null;
}

export function QrModal() {
  const queryClient = useQueryClient();
  const qrQuery = useQuery({
    queryKey: ['napcat-qr'],
    queryFn: async () => (await api.get('/napcat/qr')).data as QrResponse,
    refetchInterval: 1000,
  });

  const qr = useMemo(() => normalizeQr(qrQuery.data?.qr), [qrQuery.data?.qr]);
  const qrImage = useMemo(() => normalizeQrImage(qrQuery.data?.qr_image), [qrQuery.data?.qr_image]);

  if (!qr) return null;

  async function dismiss() {
    queryClient.setQueryData(['napcat-qr'], { qr: null, qr_image: null } satisfies QrResponse);
    try {
      await api.delete('/napcat/qr');
    } catch {
      // Ignore - UI already dismissed.
    }
  }

  return <QrModalInner qr={qr} qrImage={qrImage} onClose={dismiss} />;
}

function QrModalInner({
  qr,
  qrImage,
  onClose,
}: {
  qr: string;
  qrImage: string | null;
  onClose: () => void;
}) {
  const [imageFailed, setImageFailed] = useState(false);

  useEffect(() => {
    setImageFailed(false);
  }, [qr, qrImage]);
  const qrValue = qr;
  const displaySrc =
    imageFailed
      ? null
      : qrValue.startsWith('data:image')
        ? qrValue
        : qrImage
          ? qrImage
          : qrValue.startsWith('http://') || qrValue.startsWith('https://')
            ? qrValue
            : null;

  async function copy() {
    const text = qrValue;
    try {
      if (window.isSecureContext && navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(text);
        toast.success('二维码链接已复制');
        return;
      }
    } catch {}

    try {
      const textarea = document.createElement('textarea');
      textarea.value = text;
      textarea.setAttribute('readonly', 'true');
      textarea.style.position = 'fixed';
      textarea.style.left = '-9999px';
      textarea.style.top = '-9999px';
      document.body.appendChild(textarea);
      textarea.focus();
      textarea.select();
      const ok = document.execCommand('copy');
      document.body.removeChild(textarea);
      if (ok) {
        toast.success('二维码链接已复制');
        return;
      }
    } catch {}

    try {
      window.prompt('复制此链接', text);
      return;
    } catch {}

    toast.error('复制失败：请手动复制链接');
  }

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal-container max-w-md" onClick={(e) => e.stopPropagation()}>
        <div className="bg-brand-soft/50 px-8 py-6 border-b border-brand/10 flex items-center justify-between">
          <div className="flex items-center gap-3 min-w-0">
            <div className="w-10 h-10 rounded-2xl bg-white flex items-center justify-center text-brand shadow-inner">
              <QrCode className="w-5 h-5" />
            </div>
            <div className="min-w-0">
              <div className="font-black text-xl text-text-main truncate">扫码登录</div>
              <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest truncate">
                NapCat WebUI 二维码
              </div>
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

        <div className="p-8 space-y-5">
          <div className="bg-white rounded-[24px] border border-brand-soft p-4 flex items-center justify-center">
            {displaySrc ? (
              <img
                src={displaySrc}
                alt="QR"
                className="w-64 h-64 object-contain"
                onError={() => setImageFailed(true)}
              />
            ) : (
              <div className="w-64 h-64 flex items-center justify-center">
                {imageFailed ? (
                  <div className="text-center text-text-main/60 font-bold space-y-2">
                    <div>二维码图片加载失败</div>
                    {qrValue.startsWith('http') ? (
                      <div className="text-xs font-semibold">可点击“打开网页”或“复制链接”。</div>
                    ) : null}
                  </div>
                ) : (
                  <div className="w-10 h-10 border-4 border-brand border-t-transparent rounded-full animate-spin" />
                )}
              </div>
            )}
          </div>

          <div className="flex items-center justify-between gap-3">
            <button className="btn-secondary flex items-center gap-2" onClick={copy}>
              <Copy className="w-4 h-4" />
              复制链接
            </button>
            {qrValue.startsWith('http') ? (
              <a className="btn-secondary" href={qrValue} target="_blank" rel="noreferrer">
                打开网页
              </a>
            ) : null}
          </div>

          <div className="text-xs text-text-main/60 font-bold leading-relaxed">
            提示：如果二维码长期不刷新，可在「实例管理」里点击“登录”重新触发。
          </div>
        </div>
      </div>
    </div>
  );
}
