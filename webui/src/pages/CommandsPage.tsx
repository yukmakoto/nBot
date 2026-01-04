import { useMemo, useState, type ReactNode } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import toast from 'react-hot-toast';
import { Plus, Settings, Terminal, Trash2, X } from 'lucide-react';

import { api } from '../lib/api';
import { getApiErrorMessage } from '../lib/errors';

type CommandParam = {
  name: string;
  description: string;
  required: boolean;
  param_type?: string;
};

type Command = {
  id: string;
  name: string;
  aliases?: string[];
  pattern?: string | null;
  description: string;
  is_builtin: boolean;
  action: unknown;
  params?: CommandParam[];
  category?: string;
  config?: Record<string, unknown>;
};

type ActionKind = 'help' | 'plugin' | 'custom' | 'unknown';

const EMPTY_COMMANDS: Command[] = [];

function getActionKind(action: unknown): { kind: ActionKind; value?: string } {
  if (typeof action === 'string') {
    if (action === 'Help') return { kind: 'help' };
    return { kind: 'unknown' };
  }
  if (action && typeof action === 'object') {
    const record = action as Record<string, unknown>;
    if (typeof record.Plugin === 'string') return { kind: 'plugin', value: record.Plugin };
    if (typeof record.Custom === 'string') return { kind: 'custom', value: record.Custom };
  }
  return { kind: 'unknown' };
}

function splitAliases(value: string): string[] {
  return value
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean);
}

export function CommandsPage() {
  const queryClient = useQueryClient();
  const [createOpen, setCreateOpen] = useState(false);
  const [editTarget, setEditTarget] = useState<Command | null>(null);

  const commandsQuery = useQuery({
    queryKey: ['commands'],
    queryFn: async () => (await api.get('/commands')).data as Command[],
    refetchInterval: 2000,
  });

  const commands = commandsQuery.data ?? EMPTY_COMMANDS;

  const builtins = useMemo(() => commands.filter((c) => c.is_builtin), [commands]);
  const pluginCmds = useMemo(
    () => commands.filter((c) => getActionKind(c.action).kind === 'plugin'),
    [commands],
  );
  const customCmds = useMemo(
    () => commands.filter((c) => !c.is_builtin && getActionKind(c.action).kind === 'custom'),
    [commands],
  );

  return (
    <div className="space-y-6 pt-2">
      <div className="flex items-center justify-between gap-4">
        <div>
          <div className="flex items-center gap-4 mb-2">
            <div className="w-1.5 h-8 bg-brand rounded-full shadow-sm" />
            <h1 className="text-2xl font-black text-text-main tracking-tight font-brand">
              指令管理
            </h1>
          </div>
          <p className="text-sm font-bold text-text-main/60 pl-6">管理机器人指令与自定义命令</p>
        </div>
        <button className="btn-primary flex items-center gap-2" onClick={() => setCreateOpen(true)}>
          <Plus className="w-4 h-4" />
          添加指令
        </button>
      </div>

      <div className="space-y-8 pb-10">
        <Section
          title="内置指令"
          emptyIcon={<Terminal className="w-14 h-14 mx-auto mb-4 opacity-20" />}
          emptyLabel="暂无内置指令"
          items={builtins.map((cmd) => (
            <CommandRow
              key={cmd.id}
              command={cmd}
              canEdit
              onEdit={() => setEditTarget(cmd)}
            />
          ))}
        />

        <Section
          title="插件指令"
          emptyIcon={<Terminal className="w-14 h-14 mx-auto mb-4 opacity-20" />}
          emptyLabel="暂无插件指令"
          items={pluginCmds.map((cmd) => (
            <CommandRow key={cmd.id} command={cmd} canEdit={false} onEdit={() => {}} />
          ))}
        />

        <Section
          title="自定义指令"
          emptyIcon={<Terminal className="w-16 h-16 mx-auto mb-4 opacity-20" />}
          emptyLabel="暂无自定义指令"
          items={customCmds.map((cmd) => (
            <CommandRow
              key={cmd.id}
              command={cmd}
              canEdit
              onEdit={() => setEditTarget(cmd)}
            />
          ))}
        />
      </div>

      {createOpen ? (
        <CommandCreateModal
          onClose={() => setCreateOpen(false)}
          onCreated={() => queryClient.invalidateQueries({ queryKey: ['commands'] })}
        />
      ) : null}
      {editTarget ? (
        <CommandEditModal
          command={editTarget}
          onClose={() => setEditTarget(null)}
          onSaved={() => queryClient.invalidateQueries({ queryKey: ['commands'] })}
        />
      ) : null}
    </div>
  );
}

function Section({
  title,
  items,
  emptyIcon,
  emptyLabel,
}: {
  title: string;
  items: ReactNode[];
  emptyIcon: ReactNode;
  emptyLabel: string;
}) {
  return (
    <div>
      <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest mb-4 px-2">
        {title}
      </div>
      {items.length ? (
        <div className="space-y-3">{items}</div>
      ) : (
        <div className="text-center py-20 bg-brand-soft/50 rounded-[40px] border-2 border-dashed border-brand/10">
          {emptyIcon}
          <p className="font-black uppercase tracking-widest text-brand/40">{emptyLabel}</p>
        </div>
      )}
    </div>
  );
}

function CommandRow({
  command,
  canEdit,
  onEdit,
}: {
  command: Command;
  canEdit: boolean;
  onEdit: () => void;
}) {
  const action = getActionKind(command.action);
  const actionLabel =
    action.kind === 'help'
      ? '显示帮助'
      : action.kind === 'plugin'
        ? '插件动作'
        : action.kind === 'custom'
          ? '自定义动作'
          : '未知动作';

  return (
    <div className="bg-white rounded-[32px] p-7 hover:shadow-xl transition-all duration-500 group relative border border-brand-soft">
      <div className="flex items-center gap-6">
        <div className="w-14 h-14 rounded-2xl bg-brand-soft flex items-center justify-center text-brand shrink-0 shadow-inner">
          <Terminal className="w-7 h-7" />
        </div>
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-3 mb-1.5">
            <h3 className="text-lg font-black text-brand truncate">/{command.name}</h3>
            <div className="flex gap-2">
              {command.is_builtin ? (
                <span className="text-[10px] font-black px-2.5 py-0.5 rounded-full bg-emerald-50 text-emerald-500 shrink-0 uppercase">
                  内置
                </span>
              ) : (
                <span className="text-[10px] font-black px-2.5 py-0.5 rounded-full bg-orange-50 text-orange-500 shrink-0 uppercase">
                  机器人
                </span>
              )}
              {action.kind === 'plugin' ? (
                <span className="text-[10px] font-black px-2.5 py-0.5 rounded-full bg-sky-50 text-sky-500 shrink-0 uppercase">
                  插件
                </span>
              ) : (
                <span className="text-[10px] font-black px-2.5 py-0.5 rounded-full bg-brand/10 text-brand shrink-0 uppercase">
                  指令
                </span>
              )}
            </div>
          </div>
          <p className="text-sm text-text-main/60 truncate font-bold leading-relaxed">
            {command.description}
          </p>
          <div className="flex items-center gap-3 mt-2 text-[10px] text-brand/40 font-black uppercase tracking-widest">
            <span>{actionLabel}</span>
            {Array.isArray(command.params) && command.params.length ? (
              <>
                <span className="opacity-30">·</span>
                <span>{command.params.length} 参数</span>
              </>
            ) : null}
          </div>
        </div>
        {canEdit ? (
          <button
            className="p-2.5 rounded-2xl text-brand/20 hover:text-brand hover:bg-brand-soft transition-all"
            onClick={onEdit}
            title="编辑"
          >
            <Settings className="w-5 h-5" />
          </button>
        ) : null}
      </div>
    </div>
  );
}

function CommandCreateModal({
  onClose,
  onCreated,
}: {
  onClose: () => void;
  onCreated: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [aliases, setAliases] = useState('');
  const [pattern, setPattern] = useState('');
  const [actionValue, setActionValue] = useState('');
  const [params, setParams] = useState<CommandParam[]>([]);
  const canSave = name.trim() && description.trim();

  async function create() {
    if (!canSave || busy) return;
    setBusy(true);
    try {
      await api.post('/commands', {
        name: name.trim(),
        description: description.trim(),
        aliases: splitAliases(aliases),
        pattern: pattern.trim() ? pattern.trim() : null,
        action_value: actionValue.trim(),
        params,
      });
      toast.success('指令已创建');
      onCreated();
      onClose();
    } catch (e: unknown) {
      toast.error(getApiErrorMessage(e, '创建失败'));
    } finally {
      setBusy(false);
    }
  }

  function addParam() {
    setParams((prev) => [
      ...prev,
      {
        name: `param${prev.length + 1}`,
        description: '',
        required: false,
        param_type: 'string',
      },
    ]);
  }

  return (
    <div className="modal-backdrop" onClick={() => (!busy ? onClose() : null)}>
      <div className="modal-container max-w-2xl" onClick={(e) => e.stopPropagation()}>
        <div className="bg-brand-soft/50 px-8 py-6 border-b border-brand/10 flex items-center justify-between">
          <div className="font-black text-xl text-text-main uppercase tracking-tight">创建新指令</div>
          <button
            className="p-2 rounded-full hover:bg-brand/10 text-brand/40 hover:text-brand transition-all"
            onClick={onClose}
            disabled={busy}
            title="关闭"
          >
            <X className="w-6 h-6" />
          </button>
        </div>

        <div className="p-8 space-y-5 clean-scroll max-h-[70vh] overflow-auto">
          <div className="space-y-2">
            <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest ml-1">
              指令名称
            </div>
            <div className="flex">
              <div className="px-4 py-3 rounded-l-2xl bg-brand-soft/50 border border-brand-soft text-brand/50 font-mono font-black">
                /
              </div>
              <input
                className="flex-1 px-5 py-3 rounded-r-2xl border border-brand-soft bg-white text-sm font-bold text-text-main focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all"
                placeholder="help"
                value={name}
                onChange={(e) => setName(e.target.value)}
                disabled={busy}
              />
            </div>
          </div>

          <div className="space-y-2">
            <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest ml-1">
              描述
            </div>
            <textarea
              className="w-full px-5 py-3 rounded-2xl border border-brand-soft bg-white text-sm font-medium text-text-main focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all"
              rows={3}
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              disabled={busy}
            />
          </div>

          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            <div className="space-y-2">
              <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest ml-1">
                别名（逗号分隔）
              </div>
              <input
                className="w-full px-5 py-3 rounded-2xl border border-brand-soft bg-white text-sm font-bold text-text-main focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all"
                placeholder="菜单, 列表"
                value={aliases}
                onChange={(e) => setAliases(e.target.value)}
                disabled={busy}
              />
            </div>
            <div className="space-y-2">
              <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest ml-1">
                自定义正则（可选）
              </div>
              <input
                className="w-full px-5 py-3 rounded-2xl border border-brand-soft bg-white font-mono text-xs text-text-main focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all"
                placeholder="^/帮助\\s*(\\S+)?$"
                value={pattern}
                onChange={(e) => setPattern(e.target.value)}
                disabled={busy}
              />
            </div>
          </div>

          <div className="space-y-2">
            <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest ml-1">
              Action（action_value）
            </div>
            <input
              className="w-full px-5 py-3 rounded-2xl border border-brand-soft bg-white text-sm font-bold text-text-main focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all"
              placeholder="custom_action"
              value={actionValue}
              onChange={(e) => setActionValue(e.target.value)}
              disabled={busy}
            />
            <div className="text-xs text-text-main/60 font-medium">
              用于插件/运行时识别自定义动作（后端会保存为 CommandAction::Custom）。
            </div>
          </div>

          <div className="p-5 bg-brand-soft/30 border border-brand/10 rounded-3xl space-y-3">
            <div className="flex items-center justify-between">
              <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest">
                参数
              </div>
              <button className="btn-secondary" onClick={addParam} type="button" disabled={busy}>
                <Plus className="w-4 h-4" />
              </button>
            </div>
            {params.length ? (
              <div className="space-y-2">
                {params.map((p, idx) => (
                  <div
                    key={idx}
                    className="flex flex-col md:flex-row md:items-center gap-3 p-4 bg-white rounded-2xl border border-brand-soft"
                  >
                    <input
                      className="flex-1 px-4 py-2.5 rounded-2xl border border-brand-soft bg-white text-sm font-bold text-text-main focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all"
                      placeholder="参数名"
                      value={p.name}
                      onChange={(e) =>
                        setParams((prev) => {
                          const next = [...prev];
                          next[idx] = { ...next[idx], name: e.target.value };
                          return next;
                        })
                      }
                      disabled={busy}
                    />
                    <select
                      className="px-4 py-2.5 rounded-2xl border border-brand-soft bg-white text-sm font-black text-text-main focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all"
                      value={p.param_type ?? 'string'}
                      onChange={(e) =>
                        setParams((prev) => {
                          const next = [...prev];
                          next[idx] = { ...next[idx], param_type: e.target.value };
                          return next;
                        })
                      }
                      disabled={busy}
                    >
                      <option value="string">文本</option>
                      <option value="number">数字</option>
                      <option value="user">@用户</option>
                      <option value="group">群号</option>
                    </select>
                    <label className="flex items-center gap-2 text-xs font-bold text-text-main/70">
                      <input
                        type="checkbox"
                        checked={!!p.required}
                        onChange={(e) =>
                          setParams((prev) => {
                            const next = [...prev];
                            next[idx] = { ...next[idx], required: e.target.checked };
                            return next;
                          })
                        }
                        disabled={busy}
                      />
                      必填
                    </label>
                    <button
                      className="btn-danger-ghost flex items-center gap-2 justify-center"
                      onClick={() => setParams((prev) => prev.filter((_, i) => i !== idx))}
                      type="button"
                      disabled={busy}
                      title="删除参数"
                    >
                      <Trash2 className="w-4 h-4" />
                      删除
                    </button>
                  </div>
                ))}
              </div>
            ) : (
              <div className="text-xs text-text-main/50 font-bold">暂无参数</div>
            )}
          </div>
        </div>

        <div className="bg-brand-soft/10 px-8 py-6 flex justify-end gap-3 border-t border-brand-soft">
          <button className="btn-ghost" onClick={onClose} disabled={busy}>
            取消
          </button>
          <button className="btn-primary" onClick={create} disabled={busy || !canSave}>
            {busy ? '创建中...' : '创建'}
          </button>
        </div>
      </div>
    </div>
  );
}

function CommandEditModal({
  command,
  onClose,
  onSaved,
}: {
  command: Command;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [aliases, setAliases] = useState((command.aliases ?? []).join(', '));
  const [description, setDescription] = useState(command.description ?? '');
  const [pattern, setPattern] = useState(command.pattern ?? '');

  const isHelp = command.id === 'help' || getActionKind(command.action).kind === 'help';
  const currentMode = (command.config?.['mode'] as string | undefined) ?? 'text';
  const currentBg = (command.config?.['background_url'] as string | undefined) ?? '';

  const [helpMode, setHelpMode] = useState(currentMode);
  const [helpBg, setHelpBg] = useState(currentBg);

  async function save() {
    if (busy) return;
    setBusy(true);
    try {
      const updates: Record<string, unknown> = {
        aliases: splitAliases(aliases),
        description: description.trim(),
        pattern: pattern.trim() ? pattern.trim() : null,
      };
      if (isHelp) {
        updates.config = {
          mode: helpMode,
          background_url: helpBg.trim(),
        };
      }
      const resp = await api.put(`/commands/${encodeURIComponent(command.id)}`, updates);
      if (resp.data?.status === 'success') {
        toast.success('已保存');
        onSaved();
        onClose();
      } else {
        toast.error(resp.data?.message ?? '保存失败');
      }
    } catch (e: unknown) {
      toast.error(getApiErrorMessage(e, '保存失败'));
    } finally {
      setBusy(false);
    }
  }

  async function remove() {
    if (deleting || command.is_builtin) return;
    if (!confirm(`确认删除指令：/${command.name}（${command.id}）？`)) return;
    setDeleting(true);
    try {
      const resp = await api.delete(`/commands/${encodeURIComponent(command.id)}`);
      if (resp.data?.status === 'success') {
        toast.success('已删除');
        onSaved();
        onClose();
      } else {
        toast.error(resp.data?.message ?? '删除失败');
      }
    } catch (e: unknown) {
      toast.error(getApiErrorMessage(e, '删除失败'));
    } finally {
      setDeleting(false);
    }
  }

  return (
    <div className="modal-backdrop" onClick={() => (!busy && !deleting ? onClose() : null)}>
      <div className="modal-container max-w-2xl" onClick={(e) => e.stopPropagation()}>
        <div className="bg-brand-soft/50 px-8 py-6 border-b border-brand/10 flex items-center justify-between">
          <div className="min-w-0">
            <div className="font-black text-xl text-text-main truncate">编辑指令：/{command.name}</div>
            <div className="text-[10px] font-black uppercase tracking-widest text-brand/40 mt-1 truncate">
              {command.id}
            </div>
          </div>
          <button
            className="p-2 rounded-full hover:bg-brand/10 text-brand/40 hover:text-brand transition-all"
            onClick={onClose}
            disabled={busy || deleting}
            title="关闭"
          >
            <X className="w-6 h-6" />
          </button>
        </div>

        <div className="p-8 space-y-5 clean-scroll max-h-[70vh] overflow-auto">
          <div className="space-y-2">
            <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest ml-1">
              别名（逗号分隔）
            </div>
            <input
              className="w-full px-5 py-3 rounded-2xl border border-brand-soft bg-white text-sm font-bold text-text-main focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all"
              value={aliases}
              onChange={(e) => setAliases(e.target.value)}
              disabled={busy || deleting}
            />
          </div>

          <div className="space-y-2">
            <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest ml-1">
              描述
            </div>
            <textarea
              className="w-full px-5 py-3 rounded-2xl border border-brand-soft bg-white text-sm font-medium text-text-main focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all"
              rows={3}
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              disabled={busy || deleting}
            />
          </div>

          <div className="space-y-2">
            <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest ml-1">
              自定义正则（可选）
            </div>
            <input
              className="w-full px-5 py-3 rounded-2xl border border-brand-soft bg-white font-mono text-xs text-text-main focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all"
              value={pattern}
              onChange={(e) => setPattern(e.target.value)}
              disabled={busy || deleting}
            />
          </div>

          {isHelp ? (
            <div className="p-6 bg-brand-soft/30 border border-brand/10 rounded-3xl space-y-4">
              <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest">
                帮助指令配置
              </div>
              <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                <div className="space-y-2">
                  <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest ml-1">
                    显示模式
                  </div>
                  <select
                    className="w-full px-5 py-3 rounded-2xl border border-brand-soft bg-white text-sm font-black text-text-main focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all"
                    value={helpMode}
                    onChange={(e) => setHelpMode(e.target.value)}
                    disabled={busy || deleting}
                  >
                    <option value="text">文字帮助</option>
                    <option value="image">图片帮助</option>
                  </select>
                </div>
                <div className="space-y-2">
                  <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest ml-1">
                    背景图片 URL（可选）
                  </div>
                  <input
                    className="w-full px-5 py-3 rounded-2xl border border-brand-soft bg-white text-sm font-bold text-text-main focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all"
                    value={helpBg}
                    onChange={(e) => setHelpBg(e.target.value)}
                    disabled={busy || deleting || helpMode !== 'image'}
                  />
                </div>
              </div>
            </div>
          ) : null}
        </div>

        <div className="bg-brand-soft/10 px-8 py-6 flex justify-between gap-3 border-t border-brand-soft">
          {!command.is_builtin ? (
            <button className="btn-danger-ghost" onClick={remove} disabled={busy || deleting}>
              {deleting ? '删除中...' : '删除指令'}
            </button>
          ) : (
            <div />
          )}
          <div className="flex items-center gap-3">
            <button className="btn-ghost" onClick={onClose} disabled={busy || deleting}>
              取消
            </button>
            <button className="btn-primary" onClick={save} disabled={busy || deleting}>
              {busy ? '保存中...' : '保存更改'}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
