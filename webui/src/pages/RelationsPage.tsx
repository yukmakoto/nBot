import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import toast from 'react-hot-toast';
import { MessageSquare, Search, Users, X } from 'lucide-react';

import { api } from '../lib/api';
import { getApiErrorMessage } from '../lib/errors';
import type { BotInstance } from '../lib/types';
import { useSelection } from '../lib/selection';

type FriendInfo = {
  user_id: number;
  nickname: string;
  remark: string;
};

type GroupInfo = {
  group_id: number;
  group_name: string;
  member_count: number;
};

type GroupMemberInfo = {
  user_id: number;
  nickname: string;
  card: string;
  role: string;
  join_time: number;
  last_sent_time: number;
};

type ChatSegment =
  | { type: 'text'; text: string }
  | { type: 'image'; url: string }
  | { type: 'face' }
  | { type: 'at'; qq: string }
  | { type: 'reply' };

type ChatMessage = {
  message_id: number;
  time: number;
  sender_id: number;
  sender_name: string;
  segments: ChatSegment[];
  is_self: boolean;
};

type ChatTarget =
  | { kind: 'friend'; id: number; name: string }
  | { kind: 'group'; id: number; name: string };

export function RelationsPage() {
  const { selectedBotId } = useSelection();
  const [tab, setTab] = useState<'friends' | 'groups'>('friends');
  const [query, setQuery] = useState('');
  const [chatTarget, setChatTarget] = useState<ChatTarget | null>(null);
  const [membersTarget, setMembersTarget] = useState<GroupInfo | null>(null);

  const statusQuery = useQuery({
    queryKey: ['status'],
    queryFn: async () => (await api.get('/status')).data as BotInstance[],
    refetchInterval: 1000,
  });

  const bots = statusQuery.data ?? [];
  const selectedBot = bots.find((b) => b.id === selectedBotId) ?? null;
  const botName = selectedBot?.name ?? '未选择';

  const friendsQuery = useQuery({
    queryKey: ['relations', 'friends', selectedBotId],
    enabled: !!selectedBotId && tab === 'friends',
    queryFn: async () => {
      const resp = await api.get('/relations/friends', { params: { bot_id: selectedBotId } });
      if (resp.data?.status !== 'success') {
        throw new Error(resp.data?.message ?? '获取好友列表失败');
      }
      return (resp.data?.friends ?? []) as FriendInfo[];
    },
  });

  const groupsQuery = useQuery({
    queryKey: ['relations', 'groups', selectedBotId],
    enabled: !!selectedBotId && tab === 'groups',
    queryFn: async () => {
      const resp = await api.get('/relations/groups', { params: { bot_id: selectedBotId } });
      if (resp.data?.status !== 'success') {
        throw new Error(resp.data?.message ?? '获取群列表失败');
      }
      return (resp.data?.groups ?? []) as GroupInfo[];
    },
  });

  useEffect(() => {
    const err = (tab === 'friends' ? friendsQuery.error : groupsQuery.error) as Error | null;
    if (err) toast.error(err.message);
  }, [friendsQuery.error, groupsQuery.error, tab]);

  const filteredFriends = useMemo(() => {
    const q = query.trim().toLowerCase();
    const list = friendsQuery.data ?? [];
    if (!q) return list;
    return list.filter(
      (f) =>
        f.nickname.toLowerCase().includes(q) ||
        f.remark.toLowerCase().includes(q) ||
        String(f.user_id).includes(q),
    );
  }, [friendsQuery.data, query]);

  const filteredGroups = useMemo(() => {
    const q = query.trim().toLowerCase();
    const list = groupsQuery.data ?? [];
    if (!q) return list;
    return list.filter(
      (g) => g.group_name.toLowerCase().includes(q) || String(g.group_id).includes(q),
    );
  }, [groupsQuery.data, query]);

  const loading = tab === 'friends' ? friendsQuery.isLoading : groupsQuery.isLoading;
  const showEmpty = !selectedBotId;

  return (
    <div className="space-y-6 pt-2">
      <div>
        <div className="flex items-center gap-4 mb-2">
          <div className="w-1.5 h-8 bg-brand rounded-full shadow-sm" />
          <h1 className="text-2xl font-black text-text-main tracking-tight font-brand">
            好友/群组
          </h1>
        </div>
        <p className="text-sm font-bold text-text-main/60 pl-6">当前实例：{botName}</p>
      </div>

      <div className="flex flex-col lg:flex-row lg:items-center lg:justify-between gap-4">
        <div className="flex bg-brand-soft p-1.5 rounded-2xl border border-brand/10 shadow-inner w-fit">
          <button
            className={
              tab === 'friends'
                ? 'px-6 py-2 rounded-xl bg-white text-brand font-black shadow-sm transition-all'
                : 'px-6 py-2 rounded-xl text-brand/40 font-bold hover:text-brand transition-all'
            }
            onClick={() => setTab('friends')}
          >
            我的好友 <span className="ml-2 text-[10px] opacity-70">({filteredFriends.length})</span>
          </button>
          <button
            className={
              tab === 'groups'
                ? 'px-6 py-2 rounded-xl bg-white text-brand font-black shadow-sm transition-all'
                : 'px-6 py-2 rounded-xl text-brand/40 font-bold hover:text-brand transition-all'
            }
            onClick={() => setTab('groups')}
          >
            我的群组 <span className="ml-2 text-[10px] opacity-70">({filteredGroups.length})</span>
          </button>
        </div>

        <div className="relative w-full lg:w-96">
          <Search className="w-4 h-4 absolute left-4 top-1/2 -translate-y-1/2 text-brand/30" />
          <input
            className="w-full pl-11 pr-4 py-3 rounded-2xl border border-brand-soft bg-white/70 hover:bg-white focus:bg-white focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all text-sm font-bold text-text-main"
            placeholder="搜索昵称 / QQ / 群号..."
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>
      </div>

      {showEmpty ? (
        <div className="card-md text-center py-16">
          <div className="w-16 h-16 rounded-[28px] bg-brand-soft flex items-center justify-center text-brand mx-auto mb-4 shadow-inner">
            <Users className="w-7 h-7" />
          </div>
          <div className="text-xl font-black text-text-main mb-2">请先选择一个实例</div>
          <div className="text-sm font-bold text-text-main/60">
            在顶部下拉框中选择已连接的机器人，然后查看好友/群组列表。
          </div>
        </div>
      ) : (
        <div className="pb-10">
          {loading ? (
            <div className="flex items-center justify-center py-20">
              <div className="w-12 h-12 border-4 border-brand border-t-transparent rounded-full animate-spin" />
            </div>
          ) : tab === 'friends' ? (
            filteredFriends.length ? (
              <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-6">
                {filteredFriends.map((f) => (
                  <RelationCard
                    key={f.user_id}
                    title={f.remark || f.nickname || String(f.user_id)}
                    subtitle={`${f.nickname || ''} · ${f.user_id}`}
                    avatarUrl={`https://q1.qlogo.cn/g?b=qq&nk=${f.user_id}&s=100`}
                    onChat={() =>
                      setChatTarget({
                        kind: 'friend',
                        id: f.user_id,
                        name: f.remark || f.nickname || String(f.user_id),
                      })
                    }
                    actions={[
                      {
                        label: '聊天',
                        onClick: () =>
                          setChatTarget({
                            kind: 'friend',
                            id: f.user_id,
                            name: f.remark || f.nickname || String(f.user_id),
                          }),
                      },
                    ]}
                  />
                ))}
              </div>
            ) : (
              <EmptyBlock icon={<Users className="w-14 h-14 mx-auto mb-4 opacity-20" />} label="暂无好友数据" />
            )
          ) : filteredGroups.length ? (
            <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-6">
              {filteredGroups.map((g) => (
                <RelationCard
                  key={g.group_id}
                  title={g.group_name || String(g.group_id)}
                  subtitle={`群号 ${g.group_id} · ${g.member_count} 人`}
                  avatarUrl={`https://p.qlogo.cn/gh/${g.group_id}/${g.group_id}/100`}
                  onChat={() =>
                    setChatTarget({ kind: 'group', id: g.group_id, name: g.group_name || String(g.group_id) })
                  }
                  actions={[
                    {
                      label: '聊天',
                      onClick: () =>
                        setChatTarget({
                          kind: 'group',
                          id: g.group_id,
                          name: g.group_name || String(g.group_id),
                        }),
                    },
                    {
                      label: '成员',
                      onClick: () => setMembersTarget(g),
                    },
                  ]}
                />
              ))}
            </div>
          ) : (
            <EmptyBlock icon={<Users className="w-14 h-14 mx-auto mb-4 opacity-20" />} label="暂无群组数据" />
          )}
        </div>
      )}

      {chatTarget && selectedBotId ? (
        <ChatModal botId={selectedBotId} target={chatTarget} onClose={() => setChatTarget(null)} />
      ) : null}

      {membersTarget && selectedBotId ? (
        <GroupMembersModal
          botId={selectedBotId}
          group={membersTarget}
          onClose={() => setMembersTarget(null)}
        />
      ) : null}
    </div>
  );
}

function EmptyBlock({ icon, label }: { icon: ReactNode; label: string }) {
  return (
    <div className="text-center py-20 bg-brand-soft/50 rounded-[32px] border-2 border-dashed border-brand/10">
      {icon}
      <p className="font-black uppercase tracking-widest text-brand/40">{label}</p>
    </div>
  );
}

function RelationCard({
  title,
  subtitle,
  avatarUrl,
  onChat,
  actions,
}: {
  title: string;
  subtitle: string;
  avatarUrl: string;
  onChat: () => void;
  actions: Array<{ label: string; onClick: () => void }>;
}) {
  return (
    <div className="bg-white rounded-[32px] p-7 border border-brand-soft hover:shadow-xl transition-all duration-500">
      <div className="flex items-center gap-5">
        <div className="w-14 h-14 rounded-2xl bg-brand-soft border border-white shadow-inner overflow-hidden shrink-0">
          <img src={avatarUrl} alt={title} className="w-full h-full object-cover" loading="lazy" />
        </div>
        <div className="flex-1 min-w-0">
          <div
            className="font-black text-text-main text-lg truncate cursor-pointer hover:text-brand transition-colors"
            onClick={onChat}
            title="打开聊天"
          >
            {title}
          </div>
          <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest truncate">
            {subtitle}
          </div>
        </div>
      </div>

      <div className="mt-5 flex items-center gap-2">
        {actions.map((a) => (
          <button
            key={a.label}
            className="btn-secondary flex items-center gap-2"
            onClick={a.onClick}
          >
            {a.label === '聊天' ? <MessageSquare className="w-4 h-4" /> : <Users className="w-4 h-4" />}
            {a.label}
          </button>
        ))}
      </div>
    </div>
  );
}

function ChatModal({
  botId,
  target,
  onClose,
}: {
  botId: string;
  target: ChatTarget;
  onClose: () => void;
}) {
  const [input, setInput] = useState('');
  const [sending, setSending] = useState(false);
  const [messages, setMessages] = useState<ChatMessage[]>([]);

  const historyQuery = useQuery({
    queryKey: ['chat-history', botId, target.kind, target.id],
    queryFn: async () => {
      const params =
        target.kind === 'group'
          ? { bot_id: botId, group_id: target.id, count: 50 }
          : { bot_id: botId, user_id: target.id, count: 50 };
      const resp = await api.get('/chat/history', { params });
      if (resp.data?.status !== 'success') {
        throw new Error(resp.data?.message ?? '获取聊天记录失败');
      }
      return (resp.data?.messages ?? []) as ChatMessage[];
    },
    refetchInterval: 1500,
  });

  useEffect(() => {
    if (historyQuery.data) setMessages(historyQuery.data);
  }, [historyQuery.data]);

  useEffect(() => {
    if (historyQuery.error) toast.error((historyQuery.error as Error).message);
  }, [historyQuery.error]);

  const avatarUrl =
    target.kind === 'group'
      ? `https://p.qlogo.cn/gh/${target.id}/${target.id}/100`
      : `https://q1.qlogo.cn/g?b=qq&nk=${target.id}&s=100`;

  async function send() {
    const text = input.trim();
    if (!text || sending) return;
    setSending(true);
    try {
      const payload =
        target.kind === 'group'
          ? { bot_id: botId, group_id: target.id, message: text }
          : { bot_id: botId, user_id: target.id, message: text };
      const resp = await api.post('/chat/send', payload);
      if (resp.data?.status === 'success') {
        const now = Math.floor(Date.now() / 1000);
        setMessages((prev) => [
          ...prev,
          {
            message_id: 0,
            time: now,
            sender_id: 0,
            sender_name: '我',
            segments: [{ type: 'text', text }],
            is_self: true,
          },
        ]);
        setInput('');
      } else {
        toast.error(resp.data?.message ?? '发送失败');
      }
    } catch (e: unknown) {
      toast.error(getApiErrorMessage(e, '发送失败'));
    } finally {
      setSending(false);
    }
  }

  return (
    <div className="modal-backdrop" onClick={() => (!sending ? onClose() : null)}>
      <div className="modal-container max-w-4xl" onClick={(e) => e.stopPropagation()}>
        <div className="px-8 py-6 bg-brand-soft/50 border-b border-brand/10 flex items-center justify-between">
          <div className="flex items-center gap-4 min-w-0">
            <div className="w-12 h-12 rounded-2xl bg-brand-soft border-2 border-white shadow-inner overflow-hidden">
              <img src={avatarUrl} className="w-full h-full object-cover" alt={target.name} />
            </div>
            <div className="min-w-0">
              <div className="font-black text-xl text-text-main truncate">{target.name}</div>
              <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest truncate">
                {target.kind === 'group' ? '群聊' : '私聊'} · {target.id}
              </div>
            </div>
          </div>
          <button
            className="p-2 rounded-full hover:bg-brand/10 text-brand/40 hover:text-brand transition-all"
            onClick={onClose}
            disabled={sending}
            title="关闭"
          >
            <X className="w-6 h-6" />
          </button>
        </div>

        <div className="p-6">
          <div className="h-[60vh] bg-white rounded-[28px] border border-brand-soft overflow-hidden flex flex-col">
            <div className="flex-1 p-6 overflow-y-auto clean-scroll space-y-4 bg-brand-soft/10">
              {historyQuery.isLoading ? (
                <div className="flex items-center justify-center h-full">
                  <div className="w-10 h-10 border-4 border-brand border-t-transparent rounded-full animate-spin" />
                </div>
              ) : messages.length ? (
                messages.map((m, idx) => <ChatBubble key={idx} msg={m} />)
              ) : (
                <div className="flex flex-col items-center justify-center h-full text-brand/40">
                  <MessageSquare className="w-12 h-12 opacity-50" />
                  <div className="mt-3 font-black">暂无消息记录</div>
                </div>
              )}
            </div>

            <div className="p-4 border-t border-brand-soft bg-white">
              <div className="flex items-end gap-3">
                <textarea
                  className="flex-1 px-5 py-3 rounded-2xl border border-brand-soft bg-brand-soft/30 text-sm font-medium text-text-main focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all resize-none"
                  rows={2}
                  placeholder="输入消息..."
                  value={input}
                  onChange={(e) => setInput(e.target.value)}
                  disabled={sending}
                />
                <button
                  className="btn-primary"
                  onClick={send}
                  disabled={sending || !input.trim()}
                >
                  {sending ? '发送中...' : '发送'}
                </button>
              </div>
              <div className="text-[10px] text-text-main/40 font-bold mt-2 px-1">
                提示：聊天记录来自 OneBot 历史接口，可能受平台限制。
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

function ChatBubble({ msg }: { msg: ChatMessage }) {
  const timeStr = useMemo(() => {
    if (!msg.time) return '';
    const d = new Date(msg.time * 1000);
    return d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
  }, [msg.time]);

  const content = msg.segments
    .map((seg) => {
      if (seg.type === 'text') return seg.text;
      if (seg.type === 'at') return `@${seg.qq}`;
      if (seg.type === 'image') return `[图片] ${seg.url}`;
      if (seg.type === 'face') return '[表情]';
      if (seg.type === 'reply') return '[回复]';
      return '';
    })
    .filter(Boolean)
    .join('');

  if (msg.is_self) {
    return (
      <div className="flex justify-end gap-3">
        <div className="max-w-[75%] text-right">
          <div className="text-[10px] text-brand/40 font-black uppercase tracking-widest mb-1">
            {timeStr}
          </div>
          <div className="bg-brand text-white px-5 py-3 rounded-2xl rounded-tr-md shadow-sm whitespace-pre-wrap text-sm font-medium">
            {content}
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="flex justify-start gap-3">
      <div className="max-w-[75%]">
        <div className="text-[10px] text-brand/40 font-black uppercase tracking-widest mb-1">
          {msg.sender_name || msg.sender_id} · {timeStr}
        </div>
        <div className="bg-white px-5 py-3 rounded-2xl rounded-tl-md shadow-sm border border-brand-soft whitespace-pre-wrap text-sm font-medium">
          {content}
        </div>
      </div>
    </div>
  );
}

function GroupMembersModal({
  botId,
  group,
  onClose,
}: {
  botId: string;
  group: GroupInfo;
  onClose: () => void;
}) {
  const [query, setQuery] = useState('');

  const membersQuery = useQuery({
    queryKey: ['group-members', botId, group.group_id],
    queryFn: async () => {
      const resp = await api.get('/relations/group-members', {
        params: { bot_id: botId, group_id: group.group_id },
      });
      if (resp.data?.status !== 'success') {
        throw new Error(resp.data?.message ?? '获取群成员失败');
      }
      return (resp.data?.members ?? []) as GroupMemberInfo[];
    },
  });

  useEffect(() => {
    if (membersQuery.error) toast.error((membersQuery.error as Error).message);
  }, [membersQuery.error]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    const list = membersQuery.data ?? [];
    if (!q) return list;
    return list.filter(
      (m) =>
        m.nickname.toLowerCase().includes(q) ||
        m.card.toLowerCase().includes(q) ||
        String(m.user_id).includes(q) ||
        m.role.toLowerCase().includes(q),
    );
  }, [membersQuery.data, query]);

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal-container max-w-4xl" onClick={(e) => e.stopPropagation()}>
        <div className="px-8 py-6 bg-brand-soft/50 border-b border-brand/10 flex items-center justify-between">
          <div className="min-w-0">
            <div className="font-black text-xl text-text-main truncate">群成员</div>
            <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest truncate">
              {group.group_name} · {group.group_id}
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

        <div className="p-8 space-y-4">
          <div className="relative w-full">
            <Search className="w-4 h-4 absolute left-4 top-1/2 -translate-y-1/2 text-brand/30" />
            <input
              className="w-full pl-11 pr-4 py-3 rounded-2xl border border-brand-soft bg-white/70 hover:bg-white focus:bg-white focus:outline-none focus:ring-4 focus:ring-brand/10 transition-all text-sm font-bold text-text-main"
              placeholder="搜索昵称 / 群名片 / QQ / 角色..."
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </div>

          <div className="max-h-[60vh] overflow-auto clean-scroll space-y-2">
            {membersQuery.isLoading ? (
              <div className="flex items-center justify-center py-16">
                <div className="w-10 h-10 border-4 border-brand border-t-transparent rounded-full animate-spin" />
              </div>
            ) : filtered.length ? (
              filtered.map((m) => (
                <div
                  key={m.user_id}
                  className="bg-white rounded-2xl border border-brand-soft px-5 py-4 flex items-center justify-between gap-4"
                >
                  <div className="min-w-0">
                    <div className="font-black text-text-main truncate">
                      {m.card || m.nickname || String(m.user_id)}
                    </div>
                    <div className="text-[10px] font-black text-brand/40 uppercase tracking-widest truncate">
                      {m.nickname} · {m.user_id} · {m.role}
                    </div>
                  </div>
                  <a
                    className="btn-secondary"
                    href={`https://q1.qlogo.cn/g?b=qq&nk=${m.user_id}&s=100`}
                    target="_blank"
                    rel="noreferrer"
                  >
                    头像
                  </a>
                </div>
              ))
            ) : (
              <div className="text-center py-20 text-brand/40 font-black uppercase tracking-widest">
                暂无成员数据
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
