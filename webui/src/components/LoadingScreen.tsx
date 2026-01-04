import { LoaderCircle } from 'lucide-react';

export function LoadingScreen({ label }: { label: string }) {
  return (
    <div className="h-full min-h-screen flex flex-col items-center justify-center gap-6">
      <div className="w-16 h-16 rounded-[32px] bg-white/70 border border-brand-soft shadow-sm flex items-center justify-center">
        <LoaderCircle className="w-8 h-8 text-brand animate-spin" />
      </div>
      <div className="text-brand font-black text-sm uppercase tracking-widest animate-pulse">
        {label}
      </div>
    </div>
  );
}

