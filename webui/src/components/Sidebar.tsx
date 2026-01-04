import { useState, type ReactNode } from 'react';
import { NavLink } from 'react-router-dom';
import {
  Boxes,
  Database,
  LayoutDashboard,
  Menu,
  MessageSquare,
  Puzzle,
  ScrollText,
  Settings,
  Terminal,
  Users,
  Wrench,
  X,
} from 'lucide-react';
import clsx from 'clsx';

type NavItem = {
  to: string;
  label: string;
  icon: ReactNode;
};

const navItems: NavItem[] = [
  { to: '/', label: '仪表盘', icon: <LayoutDashboard className="w-5 h-5" /> },
  { to: '/instances', label: '实例管理', icon: <Boxes className="w-5 h-5" /> },
  { to: '/databases', label: '数据库', icon: <Database className="w-5 h-5" /> },
  { to: '/plugins', label: '插件中心', icon: <Puzzle className="w-5 h-5" /> },
  { to: '/llm', label: '对话服务', icon: <MessageSquare className="w-5 h-5" /> },
  { to: '/tools', label: '工具服务', icon: <Wrench className="w-5 h-5" /> },
  { to: '/commands', label: '指令管理', icon: <Terminal className="w-5 h-5" /> },
  { to: '/logs', label: '日志', icon: <ScrollText className="w-5 h-5" /> },
  { to: '/relations', label: '好友/群组', icon: <Users className="w-5 h-5" /> },
  { to: '/settings', label: '系统设置', icon: <Settings className="w-5 h-5" /> },
];

export function Sidebar() {
  const [mobileOpen, setMobileOpen] = useState(false);

  return (
    <>
      <button
        className="lg:hidden fixed top-4 left-4 z-50 p-3 bg-white/80 backdrop-blur-xl rounded-2xl shadow-lg border border-brand-soft text-brand hover:bg-brand-soft transition-all"
        onClick={() => setMobileOpen(true)}
        aria-label="打开菜单"
      >
        <Menu className="w-6 h-6" />
      </button>

      <div
        className={clsx(
          'lg:hidden fixed inset-0 z-40 bg-black/40 backdrop-blur-sm transition-opacity',
          mobileOpen ? 'opacity-100' : 'opacity-0 pointer-events-none',
        )}
        onClick={() => setMobileOpen(false)}
      />

      <aside
        className={clsx(
          'lg:hidden fixed inset-y-0 left-0 z-50 w-72 p-6 transition-transform duration-300',
          mobileOpen ? 'translate-x-0' : '-translate-x-full',
        )}
      >
        <div className="rounded-[32px] bg-white/80 border border-white/60 shadow-sm backdrop-blur-xl p-6 flex flex-col gap-6 h-full">
          <div className="flex items-center justify-between gap-3">
            <div className="flex items-center gap-3 min-w-0">
              <div className="w-12 h-12 rounded-[20px] bg-brand-soft border border-brand/10 shadow-inner overflow-hidden flex items-center justify-center">
                <img src="/nbot_logo.png" alt="nBot" className="w-9 h-9 object-contain" />
              </div>
              <div className="min-w-0">
                <div className="font-black text-text-main text-lg leading-tight truncate">nBot</div>
                <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest truncate">
                  WebUI
                </div>
              </div>
            </div>
            <button
              className="p-2 rounded-2xl text-brand/40 hover:text-brand hover:bg-brand-soft transition-all"
              onClick={() => setMobileOpen(false)}
              aria-label="关闭菜单"
            >
              <X className="w-6 h-6" />
            </button>
          </div>

          <nav className="flex flex-col gap-2">
            {navItems.map((item) => (
              <NavLink
                key={item.to}
                to={item.to}
                onClick={() => setMobileOpen(false)}
                className={({ isActive }) =>
                  clsx(
                    'flex items-center gap-3 px-4 py-3 rounded-2xl border transition-all select-none',
                    isActive
                      ? 'bg-brand text-white border-brand/10 shadow-lg shadow-brand/20'
                      : 'bg-white/70 border-brand-soft hover:bg-brand-soft hover:border-brand/20 text-text-main',
                  )
                }
                end={item.to === '/'}
              >
                <span className="shrink-0">{item.icon}</span>
                <span className="font-black text-sm tracking-tight truncate">{item.label}</span>
              </NavLink>
            ))}
          </nav>

          <div className="mt-auto pt-4 border-t border-brand-soft">
            <a
              className="btn-ghost w-full text-center block"
              href="https://github.com/yukmakoto/nBot"
              target="_blank"
              rel="noreferrer"
            >
              GitHub
            </a>
          </div>
        </div>
      </aside>

      <aside className="w-72 shrink-0 hidden lg:flex flex-col p-6">
        <div className="rounded-[32px] bg-white/70 border border-white/60 shadow-sm backdrop-blur-xl p-6 flex flex-col gap-6 flex-1">
          <div className="flex items-center gap-3">
            <div className="w-12 h-12 rounded-[20px] bg-brand-soft border border-brand/10 shadow-inner overflow-hidden flex items-center justify-center">
              <img src="/nbot_logo.png" alt="nBot" className="w-9 h-9 object-contain" />
            </div>
            <div className="min-w-0">
              <div className="font-black text-text-main text-lg leading-tight truncate">nBot</div>
              <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest truncate">
                WebUI
              </div>
            </div>
          </div>

          <nav className="flex flex-col gap-2">
            {navItems.map((item) => (
              <NavLink
                key={item.to}
                to={item.to}
                className={({ isActive }) =>
                  clsx(
                    'flex items-center gap-3 px-4 py-3 rounded-2xl border transition-all select-none',
                    isActive
                      ? 'bg-brand text-white border-brand/10 shadow-lg shadow-brand/20'
                      : 'bg-white/70 border-brand-soft hover:bg-brand-soft hover:border-brand/20 text-text-main',
                  )
                }
                end={item.to === '/'}
              >
                <span className="shrink-0">{item.icon}</span>
                <span className="font-black text-sm tracking-tight truncate">{item.label}</span>
              </NavLink>
            ))}
          </nav>

          <div className="mt-auto pt-4 border-t border-brand-soft">
            <a
              className="btn-ghost w-full text-center block"
              href="https://github.com/yukmakoto/nBot"
              target="_blank"
              rel="noreferrer"
            >
              GitHub
            </a>
          </div>
        </div>
      </aside>
    </>
  );
}
