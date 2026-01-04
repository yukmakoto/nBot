import { type ReactNode } from 'react';

import { Sidebar } from './Sidebar';
import { TopBar } from './TopBar';
import { TaskDock } from './TaskDock';
import { QrModal } from './QrModal';

export function AppLayout({ children }: { children: ReactNode }) {
  return (
    <div className="min-h-screen h-full flex bg-[var(--color-bg-body)]">
      <div className="pointer-events-none fixed inset-0 overflow-hidden">
        <div className="absolute -top-40 -right-40 w-[520px] h-[520px] bg-brand-soft/60 rounded-full blur-3xl" />
        <div className="absolute -bottom-48 -left-48 w-[560px] h-[560px] bg-brand-soft/60 rounded-full blur-3xl" />
      </div>

      <Sidebar />

      <div className="flex-1 min-w-0 flex flex-col relative">
        <TopBar />
        <main className="flex-1 min-h-0 overflow-y-auto clean-scroll px-6 pb-10">
          {children}
        </main>
      </div>

      <TaskDock />
      <QrModal />
    </div>
  );
}
