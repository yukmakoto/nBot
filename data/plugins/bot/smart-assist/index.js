/**
 * nBot Smart Assistant Plugin v2.2.25
 * Auto-detects if user needs help, enters multi-turn conversation mode,
 * replies in a QQ-friendly style (short, low-noise)
 *
 * Features:
 * 1. Decision model: Monitors each message, strictly judges if user needs help
 * 2. Multi-turn conversation: After entering conversation mode, interacts with user
 * 3. Interrupt conversation: User can interrupt at any time
 * 4. Group context: Fetches group announcements and user history for better decisions
 * 7. Group context: Fetches group announcements and user history for better decisions
 * 8. Auto-timeout: Sessions auto-cleanup with notification
 * 9. Cooldown from cleanup: Cooldown starts from session cleanup, not trigger
 */

// Session state Map<sessionKey, Session>
const sessions = new Map();

// Cooldown records Map<sessionKey, lastCleanupTime>
const cooldowns = new Map();

// Pending LLM requests Map<requestId, RequestInfo>
const pendingRequests = new Map();

// Pending group info requests Map<requestId, RequestInfo>
const pendingGroupInfoRequests = new Map();

// Sessions with pending decision/context requests (avoid spamming)
const pendingDecisionSessions = new Set();
const pendingContextSessions = new Set();
const pendingReplySessions = new Set();

// Decision batching (reduce LLM calls while still judging every message)
const decisionBatches = new Map(); // Map<sessionKey, { userId:number, groupId:number, items: {t:number,text:string,mentioned:boolean}[] }>
const DECISION_BATCH_MAX_ITEMS = 8;

// LLM max token budgets (keep high to avoid "thinking" consuming output; length is still controlled by one-line formatting)
const DECISION_MAX_TOKENS = 256;
const REPLY_MAX_TOKENS = 1024;
const REPLY_RETRY_MAX_TOKENS = 256;

// Recent images (help the model resolve "the image above")
const recentGroupImages = new Map(); // Map<groupId, { t:number, urls:string[] }>
const recentUserImages = new Map(); // Map<sessionKey, { t:number, urls:string[] }>
const recentGroupVideos = new Map(); // Map<groupId, { t:number, urls:string[] }>
const recentUserVideos = new Map(); // Map<sessionKey, { t:number, urls:string[] }>
const recentGroupRecords = new Map(); // Map<groupId, { t:number, urls:string[] }>
const recentUserRecords = new Map(); // Map<sessionKey, { t:number, urls:string[] }>

// Request ID counter
let requestIdCounter = 0;

// Generate unique request ID
function genRequestId(type) {
  return `smart-assist-${type}-${++requestIdCounter}-${nbot.now()}`;
}

function maskSensitiveForLog(text) {
  return String(text || "")
    // mask long digit sequences (QQ/IDs/etc)
    .replace(/\d{5,}/g, "***")
    .replace(/\s+/g, " ")
    .trim();
}

function escapeForLog(text, maxLen = 600) {
  let s = "";
  try {
    s = JSON.stringify(String(text || ""));
  } catch {
    s = String(text || "");
  }
  s = maskSensitiveForLog(s);
  if (s.length > maxLen) s = s.slice(0, maxLen) + "...";
  return s;
}

// Get config
function getConfig() {
  const cfg = nbot.getConfig();
  const interruptKeywords =
    Array.isArray(cfg.interrupt_keywords) && cfg.interrupt_keywords.length
      ? cfg.interrupt_keywords
      : ["我明白了", "结束", "停止"];
  const decisionSystemPrompt =
    cfg.decision_system_prompt ||
    [
      "你是 QQ 群聊里的「路由器（Router）」：你不负责输出回复内容，只负责决定机器人要不要介入、以及需不需要联网搜索。",
      "",
      "重要：要非常保守，避免误触发。",
      "- 只要像玩笑/吐槽/阴阳怪气/反讽/自问自答/口头禅、或没有明确问题与需求，一律 action=IGNORE。",
      "- 被 @ 机器人只是“优先级更高”的信号，仍然可以 action=IGNORE。",
      "- 没有 @ 机器人时：除非用户明显是在向全群求助/提问（期待任何人回答），否则一律 action=IGNORE。不要抢别人的对话。",
      "- 如果【最近群聊片段】里已经有人给出明确答案/解决步骤/指路（例如“群文件/看公告/看置顶/去某某页面”），通常 action=IGNORE（机器人不要抢答/复读）。",
      "- 起哄/调戏/让机器人叫称呼/要机器人表白/刷屏/群聊闲聊，通常 action=IGNORE。",
      "- 只有媒体/占位符（如“[图片] / [视频] / [语音] / [卡片]”）且没有任何文字内容，一律 action=IGNORE（不要去‘说明无法判断’）。",
      "- 只有表情/颜文字/一个词/无意义应答（如“哈哈”“？”“。。。”）一律 action=IGNORE。",
      "- 用户在 @ 其他人（而不是 @ 机器人）时，通常是在找那个人说话：除非明确要求机器人回答，否则 action=IGNORE。",
      "",
      "你必须输出严格 JSON（不要 Markdown、不要解释文本），字段如下：",
      '{"action":"IGNORE|REPLY|REACT","confidence":0.0,"reason":"<=20字中文","use_search":true|false,"topic":"<=12字中文","need_clarify":true|false}',
      "输出必须为【单行 JSON】，且必须以 { 开头、以 } 结尾；除此之外禁止任何字符；confidence 取 0~1。",
      "action 说明：IGNORE=不介入；REPLY=需要机器人回一句；REACT=仅表情/已读式回应（如果不确定请用 IGNORE）。",
      "use_search 说明：只有当需要查询公开资料/最新信息/外部知识时才为 true；纯群内问题/本地报错排查/需要对方补充信息时为 false。",
      "",
      "action=REPLY 的条件（同时满足）：",
      "1) 明确在求助/提问/请求解释/要建议；且",
      "2) 用户期待机器人回答；且",
      "3) 群里还没人给出明确答案；且",
      "4) 你非常确定需要你插嘴：否则用 IGNORE。",
    ].join("\n");

  const replySystemPrompt =
    cfg.reply_system_prompt ||
    [
      "你是 QQ 群里的热心老群友式助手。目标：用一句话给出最有用的下一步，尽量少打扰。",
      "",
      "输出要求（硬性）：",
      "- 只输出【一行】中文短句；禁止换行；禁止 Markdown/列表/编号/加粗/代码块。",
      "- 每条消息不超过 20 字；如果必须分多条，用「||」分隔成 2~3 条（仍然同一行输出）。",
      "- 语气自然像群友：别写长段落、别客服腔、别“为了更好地帮助你…”。",
      "- 最多问 1 个关键追问；否则直接给一个最可能有效的下一步。",
      "- 禁止笼统套话（如“各有优缺点/取决于情况/看需求/因人而异”）。不确定就问 1 个能推进问题的关键点。",
      "- 禁止编造任何未在上文出现的事实（例如版本/整合包/服务器细节/群内信息）。不确定就问一句。",
      "- 不要复述/引用聊天记录内容（不要“某某: xxx”这种复读）；直接给结论或下一步。",
      "- 如果群里已经有人给出答案/指路，你最多补充一个更精确的关键字/入口；否则就别插嘴。",
      "- 允许提供公开/官方/开源的下载入口或检索关键字；不要输出盗版/破解/私服资源。遇到缩写歧义（例如 PCL 可能指点云库也可能指 MC 启动器）先问一句确认。",
      "- 群表情/颜文字一般不需要回应；不要说“无法理解表情”。",
      "- 你可以承认自己是本群机器人助手，但禁止自称“Google/OpenAI/某公司训练的模型”等；不要角色扮演、不要撒娇、不要陪聊式发散。",
      "- 不要输出任何 QQ 号/ID/Token/密钥；@ 由系统自动添加，你不要手写 @。",
    ].join("\n");

  return {
    decisionModel: cfg.decision_model || "default",
    replyModel: cfg.reply_model || "default",
    websearchModel: cfg.websearch_model || "default",
    enableWebsearch: cfg.enable_websearch !== false,
    maxTurns: cfg.max_turns || 10,
    sessionTimeoutMs: (cfg.session_timeout_minutes || 10) * 60 * 1000,
    cooldownMs: (cfg.cooldown_seconds || 60) * 1000,
    requestTimeoutMs: (cfg.request_timeout_seconds || 90) * 1000,
    contextTimeoutMs: (cfg.context_timeout_seconds || 15) * 1000,
    autoTrigger: cfg.auto_trigger !== false,
    decisionMergeIdleMs: (() => {
      const v = Number(cfg.decision_merge_seconds ?? 5);
      const secs = Number.isFinite(v) ? Math.max(1, Math.min(30, Math.floor(v))) : 5;
      return secs * 1000;
    })(),
    decisionSystemPrompt,
    replySystemPrompt,
    interruptKeywords,
    botName: cfg.bot_name || "智能助手",
    fetchGroupContext: cfg.fetch_group_context !== false,
    contextMessageCount: (() => {
      const v = Number(cfg.context_message_count ?? 20);
      if (!Number.isFinite(v)) return 20;
      return Math.max(5, Math.min(100, Math.floor(v)));
    })(),
    // Keep formatting limits internal; don't rely on config for behavior.
    replyMaxChars: 20,
    replyMaxParts: 3,
    replyPartsSeparator: "||",
  };
}

// Check cooldown (cooldown starts from session cleanup)
function checkCooldown(sessionKey, cooldownMs) {
  const now = nbot.now();
  const lastCleanupTime = cooldowns.get(sessionKey);
  if (lastCleanupTime && now - lastCleanupTime < cooldownMs) {
    return false;
  }
  return true;
}

// Update cooldown (called when session is cleaned up)
function updateCooldown(sessionKey) {
  cooldowns.set(sessionKey, nbot.now());
}

// Cleanup expired sessions (silent; don't spam in group)
function cleanupExpiredSessions(timeoutMs) {
  const now = nbot.now();
  for (const [key, session] of sessions.entries()) {
    if (now - session.lastActivity > timeoutMs) {
      nbot.log.info(`Session timeout, auto-ending: ${key}`);

      // Update cooldown from cleanup time
      updateCooldown(key);
      sessions.delete(key);
    }
  }
}

// Check if contains keyword
function containsKeyword(text, keywords) {
  if (!text || !keywords || keywords.length === 0) return false;
  const lowerText = text.toLowerCase();
  return keywords.some((kw) => lowerText.includes(kw.toLowerCase()));
}

function stripLeadingCqSegments(text) {
  let s = String(text || "").trim();
  while (s.startsWith("[CQ:")) {
    const end = s.indexOf("]");
    if (end < 0) break;
    s = s.slice(end + 1).trimStart();
  }
  return s.trim();
}

function stripAllCqSegments(text) {
  return String(text || "")
    .replace(/\[CQ:[^\]]+\]/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

function decodeHtmlEntities(s) {
  return String(s || "")
    .replace(/&amp;/g, "&")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'");
}

function extractImageUrlsFromCtx(ctx) {
  const urls = [];
  if (ctx && Array.isArray(ctx.message)) {
    for (const seg of ctx.message) {
      if (!seg || seg.type !== "image") continue;
      const u = seg.data && seg.data.url !== undefined ? String(seg.data.url).trim() : "";
      if (u) urls.push(decodeHtmlEntities(u));
    }
  }

  const raw = ctx ? String(ctx.raw_message || "") : "";
  if (raw && raw.includes("[CQ:image")) {
    const re = /\[CQ:image,[^\]]*?\burl=([^\],]+)[^\]]*\]/gi;
    let m;
    while ((m = re.exec(raw))) {
      const u = m[1] ? decodeHtmlEntities(String(m[1]).trim()) : "";
      if (u) urls.push(u);
    }
  }

  // de-dup, keep order
  return [...new Set(urls)].slice(0, 4);
}

function extractVideoUrlsFromCtx(ctx) {
  const urls = [];
  if (ctx && Array.isArray(ctx.message)) {
    for (const seg of ctx.message) {
      if (!seg || seg.type !== "video") continue;
      const u = seg.data && seg.data.url !== undefined ? String(seg.data.url).trim() : "";
      if (u) urls.push(decodeHtmlEntities(u));
    }
  }

  const raw = ctx ? String(ctx.raw_message || "") : "";
  if (raw && raw.includes("[CQ:video")) {
    const re = /\[CQ:video,[^\]]*?\burl=([^\],]+)[^\]]*\]/gi;
    let m;
    while ((m = re.exec(raw))) {
      const u = m[1] ? decodeHtmlEntities(String(m[1]).trim()) : "";
      if (u) urls.push(u);
    }
  }

  return [...new Set(urls)].filter((u) => /^https?:\/\//i.test(String(u || ""))).slice(0, 2);
}

function extractRecordUrlsFromCtx(ctx) {
  const urls = [];
  if (ctx && Array.isArray(ctx.message)) {
    for (const seg of ctx.message) {
      if (!seg || seg.type !== "record") continue;
      const u = seg.data && seg.data.url !== undefined ? String(seg.data.url).trim() : "";
      if (u) urls.push(decodeHtmlEntities(u));
    }
  }

  const raw = ctx ? String(ctx.raw_message || "") : "";
  if (raw && raw.includes("[CQ:record")) {
    const re = /\[CQ:record,[^\]]*?\burl=([^\],]+)[^\]]*\]/gi;
    let m;
    while ((m = re.exec(raw))) {
      const u = m[1] ? decodeHtmlEntities(String(m[1]).trim()) : "";
      if (u) urls.push(u);
    }
  }

  return [...new Set(urls)].filter((u) => /^https?:\/\//i.test(String(u || ""))).slice(0, 2);
}

function extractReplyMessageContext(ctx) {
  const rm = ctx && ctx.reply_message ? ctx.reply_message : null;
  if (!rm) return null;

  const raw = String(rm.raw_message || "");
  const text = sanitizeMessageForLlm(raw, null);
  const snippet = text.length > 240 ? `${text.slice(0, 240)}…` : text;

  const imageUrls = [];
  const videoUrls = [];
  const recordUrls = [];
  const addTo = (arr, u) => {
    const s = String(u || "").trim();
    if (!s) return;
    if (!/^https?:\/\//i.test(s)) return;
    const decoded = decodeHtmlEntities(s);
    if (!arr.includes(decoded)) arr.push(decoded);
  };

  // Prefer media attachments from the replied message.
  addTo(imageUrls, rm.image_url);
  addTo(videoUrls, rm.video_url);
  addTo(recordUrls, rm.record_url);

  // If the replied message is a forward, it may contain multiple media items.
  const fm = rm.forward_media;
  if (Array.isArray(fm)) {
    for (const item of fm) {
      if (!item) continue;
      const t = String(item.type || "").toLowerCase();
      if (t === "video") addTo(videoUrls, item.url);
      else if (t === "record" || t === "audio") addTo(recordUrls, item.url);
      else addTo(imageUrls, item.url);
      if (imageUrls.length + videoUrls.length + recordUrls.length >= 3) break;
    }
  }

  return {
    snippet,
    imageUrls: imageUrls.slice(0, 2),
    videoUrls: videoUrls.slice(0, 1),
    recordUrls: recordUrls.slice(0, 1),
  };
}

function noteRecentGroupImages(groupId, urls) {
  const gid = Number(groupId);
  if (!gid || !Array.isArray(urls) || urls.length === 0) return;
  recentGroupImages.set(gid, { t: nbot.now(), urls: urls.slice(0, 4) });
}

function noteRecentUserImages(sessionKey, urls) {
  if (!sessionKey || !Array.isArray(urls) || urls.length === 0) return;
  recentUserImages.set(String(sessionKey), { t: nbot.now(), urls: urls.slice(0, 4) });
}

function noteRecentGroupVideos(groupId, urls) {
  const gid = Number(groupId);
  if (!gid || !Array.isArray(urls) || urls.length === 0) return;
  recentGroupVideos.set(gid, { t: nbot.now(), urls: urls.slice(0, 2) });
}

function noteRecentUserVideos(sessionKey, urls) {
  if (!sessionKey || !Array.isArray(urls) || urls.length === 0) return;
  recentUserVideos.set(String(sessionKey), { t: nbot.now(), urls: urls.slice(0, 2) });
}

function noteRecentGroupRecords(groupId, urls) {
  const gid = Number(groupId);
  if (!gid || !Array.isArray(urls) || urls.length === 0) return;
  recentGroupRecords.set(gid, { t: nbot.now(), urls: urls.slice(0, 2) });
}

function noteRecentUserRecords(sessionKey, urls) {
  if (!sessionKey || !Array.isArray(urls) || urls.length === 0) return;
  recentUserRecords.set(String(sessionKey), { t: nbot.now(), urls: urls.slice(0, 2) });
}

function looksReferentialShortQuestion(text) {
  const t = stripAllCqSegments(String(text || "")).trim();
  if (!t) return false;
  if (t.length > 40) return false;
  return /(?:这个|那个|上面|刚才|这张|那张|啥|什么|哪个|哪款|哪套|什么意思|怎么弄|光影|这是啥|这是什么)/u.test(t);
}

function buildRecentGroupSnippet(groupContext, limit = 15) {
  if (!groupContext || !Array.isArray(groupContext.history) || groupContext.history.length === 0) return "";
  const maxLines = Number.isFinite(limit) ? Math.max(3, Math.min(100, Math.floor(limit))) : 15;

  const lines = [];
  const slice = groupContext.history.slice(0, maxLines).slice();
  const timed = slice.filter((m) => Number.isFinite(Number(m?.time))).length;
  if (timed >= Math.ceil(slice.length / 2)) {
    slice.sort((a, b) => Number(a?.time || 0) - Number(b?.time || 0));
  }
  const maxChars = 6000;
  for (const m of slice) {
    const sender = m?.sender || {};
    const name = String(sender.card || sender.nickname || "群友").replace(/\s+/g, " ").trim() || "群友";
    const content = sanitizeMessageForLlm(String(m?.raw_message || ""), null);
    if (!content) continue;
    const line = `${name}: ${content.slice(0, 120)}`;
    lines.push(line);
    if (lines.join("\n").length >= maxChars) break;
  }
  if (!lines.length) return "";
  return `【最近群聊片段】\n${lines.join("\n")}`.trim();
}

function buildMultimodalImageMessage(imageUrls) {
  const urls = Array.isArray(imageUrls) ? imageUrls.filter(Boolean) : [];
  if (!urls.length) return null;
  return {
    role: "user",
    content: [
      { type: "text", text: "参考附件（仅用于理解当前问题，不要回复这句话）：" },
      ...urls.slice(0, 2).map((url) => ({ type: "image_url", image_url: { url: String(url) } })),
    ],
  };
}

function getRelevantImageUrlsForSession(session, sessionKey) {
  const now = nbot.now();
  const fromSession =
    session &&
    Array.isArray(session.lastImageUrls) &&
    session.lastImageUrls.length > 0 &&
    now - Number(session.lastImageAt || 0) <= 2 * 60 * 1000
      ? session.lastImageUrls
      : [];
  if (fromSession.length) return fromSession;

  const fromUser = sessionKey ? recentUserImages.get(String(sessionKey)) : null;
  if (fromUser && Array.isArray(fromUser.urls) && fromUser.urls.length && now - Number(fromUser.t || 0) <= 2 * 60 * 1000) {
    return fromUser.urls;
  }

  const gid = session && session.groupId ? Number(session.groupId) : 0;
  const recent = gid ? recentGroupImages.get(gid) : null;
  if (recent && Array.isArray(recent.urls) && recent.urls.length && now - Number(recent.t || 0) <= 2 * 60 * 1000) {
    return recent.urls;
  }
  return [];
}

function getRelevantVideoUrlsForSession(session, sessionKey) {
  const now = nbot.now();
  const fromSession =
    session &&
    Array.isArray(session.lastVideoUrls) &&
    session.lastVideoUrls.length > 0 &&
    now - Number(session.lastMediaAt || 0) <= 2 * 60 * 1000
      ? session.lastVideoUrls
      : [];
  if (fromSession.length) return fromSession;

  const fromUser = sessionKey ? recentUserVideos.get(String(sessionKey)) : null;
  if (fromUser && Array.isArray(fromUser.urls) && fromUser.urls.length && now - Number(fromUser.t || 0) <= 2 * 60 * 1000) {
    return fromUser.urls;
  }

  const gid = session && session.groupId ? Number(session.groupId) : 0;
  const recent = gid ? recentGroupVideos.get(gid) : null;
  if (recent && Array.isArray(recent.urls) && recent.urls.length && now - Number(recent.t || 0) <= 2 * 60 * 1000) {
    return recent.urls;
  }
  return [];
}

function getRelevantRecordUrlsForSession(session, sessionKey) {
  const now = nbot.now();
  const fromSession =
    session &&
    Array.isArray(session.lastRecordUrls) &&
    session.lastRecordUrls.length > 0 &&
    now - Number(session.lastMediaAt || 0) <= 2 * 60 * 1000
      ? session.lastRecordUrls
      : [];
  if (fromSession.length) return fromSession;

  const fromUser = sessionKey ? recentUserRecords.get(String(sessionKey)) : null;
  if (fromUser && Array.isArray(fromUser.urls) && fromUser.urls.length && now - Number(fromUser.t || 0) <= 2 * 60 * 1000) {
    return fromUser.urls;
  }

  const gid = session && session.groupId ? Number(session.groupId) : 0;
  const recent = gid ? recentGroupRecords.get(gid) : null;
  if (recent && Array.isArray(recent.urls) && recent.urls.length && now - Number(recent.t || 0) <= 2 * 60 * 1000) {
    return recent.urls;
  }
  return [];
}

function summarizeMentions(ctx) {
  const out = { bot: false, other: false, all: false, any: false };
  if (!ctx) return out;
  if (ctx.at_bot === true) {
    out.bot = true;
    out.any = true;
  }

  const selfId = ctx.self_id !== undefined && ctx.self_id !== null ? String(ctx.self_id) : "";
  const segments = Array.isArray(ctx.message) ? ctx.message : null;
  if (!segments) return out;

  for (const seg of segments) {
    if (!seg || seg.type !== "at") continue;
    const qq = seg.data && seg.data.qq !== undefined ? String(seg.data.qq).trim() : "";
    if (!qq) continue;
    out.any = true;
    if (qq.toLowerCase() === "all") {
      out.all = true;
      continue;
    }
    if (selfId && qq === selfId) {
      out.bot = true;
      continue;
    }
    out.other = true;
  }
  return out;
}

function sanitizeMessageForLlm(text, ctx) {
  const s = String(text || "");
  if (!s) return "";

  const selfId = ctx && ctx.self_id !== undefined && ctx.self_id !== null ? String(ctx.self_id) : "";

  // Prefer structured segments from ctx.message (backend enriches face segments with `data.name`).
  if (ctx && Array.isArray(ctx.message) && ctx.message.length) {
    const parts = [];
    for (const seg of ctx.message) {
      if (!seg || typeof seg !== "object") continue;
      const type = String(seg.type || "").toLowerCase();
      const data = seg.data && typeof seg.data === "object" ? seg.data : {};
      if (type === "text") {
        const t = data.text !== undefined ? String(data.text) : "";
        if (t) parts.push(t);
        continue;
      }
      if (type === "at") {
        const qq = data.qq !== undefined ? String(data.qq).trim() : "";
        if (!qq) {
          parts.push("@他人");
        } else if (qq.toLowerCase() === "all") {
          parts.push("@全体");
        } else if (selfId && qq === selfId) {
          parts.push("@机器人");
        } else {
          parts.push("@他人");
        }
        continue;
      }
      if (type === "face") {
        const name = data.name !== undefined ? String(data.name).trim() : "";
        const id = data.id !== undefined ? String(data.id).trim() : "";
        if (name) parts.push(`[表情:${name}]`);
        else if (id) parts.push(`[表情:${id}]`);
        else parts.push("[表情]");
        continue;
      }
      if (type === "mface") {
        parts.push("[表情]");
        continue;
      }
      if (type === "image") {
        parts.push("[图片]");
        continue;
      }
      if (type === "video") {
        parts.push("[视频]");
        continue;
      }
      if (type === "record") {
        parts.push("[语音]");
        continue;
      }
      if (type === "file") {
        parts.push("[文件]");
        continue;
      }
      if (type === "reply") {
        continue;
      }
      if (type === "json" || type === "xml" || type === "markdown") {
        parts.push("[卡片]");
        continue;
      }
    }
    return parts.join(" ").replace(/\s+/g, " ").trim();
  }

  // Fallback: sanitize raw CQ string.
  return s
    .replace(/\[CQ:at,([^\]]+)\]/g, (_m, inner) => {
      const m = String(inner || "").match(/(?:^|,)qq=([^,]+)(?:,|$)/i);
      const qq = m && m[1] ? String(m[1]).trim() : "";
      if (!qq) return "@他人";
      if (qq.toLowerCase() === "all") return "@全体";
      if (selfId && qq === selfId) return "@机器人";
      return "@他人";
    })
    .replace(/\[CQ:reply,[^\]]*\]/g, " ")
    .replace(/\[CQ:face,[^\]]*\]/gi, "[表情]")
    .replace(/\[CQ:mface,[^\]]*\]/g, "[表情]")
    .replace(/\[CQ:image,[^\]]*\]/g, "[图片]")
    .replace(/\[CQ:video,[^\]]*\]/g, "[视频]")
    .replace(/\[CQ:record,[^\]]*\]/g, "[语音]")
    .replace(/\[CQ:file,[^\]]*\]/g, "[文件]")
    .replace(/\[CQ:(?:xml|json),[^\]]*\]/g, "[卡片]")
    .replace(/\[CQ:[^\]]+\]/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

function getDecisionTrigger(ctx, message, config) {
  const empty = { shouldCheck: false, mentioned: false, urgent: false };
  if (!config.autoTrigger) return empty;

  const t = stripLeadingCqSegments(String(message || "").trim());
  if (!t) return empty;
  if (t.startsWith("/")) return empty;
  // Treat "AI分析 ..." as a command (avoid hijacking command messages).
  const firstToken = t.split(/\s+/)[0]?.trim().toLowerCase();
  if (firstToken === "ai分析") return empty;

  const mentions = summarizeMentions(ctx);
  const mentioned = mentions.bot || isMentioningBot(ctx);
  // Delegate the trigger decision to the LLM: always check (merged in 5s window to reduce cost).
  const shouldCheck = true;
  return { shouldCheck, mentioned, urgent: mentioned };
}

function buildDecisionPayload(sessionKey) {
  const batch = decisionBatches.get(sessionKey);
  if (!batch || !batch.items.length) return null;

  const items = batch.items.splice(0, DECISION_BATCH_MAX_ITEMS);
  const mentionedAny = items.some((x) => !!x?.mentioned);

  const merged = items
    .map((x, idx) => {
      const base = String(x?.text || "").trim();
      const reply = x?.replySnippet ? `（回复内容：${String(x.replySnippet).trim()}） ` : "";
      return `${idx + 1}. ${reply}${base}`.trim();
    })
    .filter(Boolean)
    .join("\n");

  return {
    userId: batch.userId,
    groupId: batch.groupId,
    mentioned: mentionedAny,
    merged,
    items,
  };
}

function scheduleDecisionFlush(sessionKey, urgent, config) {
  if (urgent) {
    flushDecisionBatch(sessionKey, config);
    return;
  }
  // No JS timers in plugin runtime; real 5s merge is driven by backend tick -> onMetaEvent.
  // Fallback: if the batch grows too large, flush immediately to avoid unbounded memory.
  const batch = decisionBatches.get(sessionKey);
  if (batch && Array.isArray(batch.items) && batch.items.length >= DECISION_BATCH_MAX_ITEMS) {
    flushDecisionBatch(sessionKey, config);
  }
}

function restoreDecisionPayload(sessionKey, payload) {
  if (!payload) return;
  let batch = decisionBatches.get(sessionKey);
  if (!batch) {
    batch = {
      userId: payload.userId,
      groupId: payload.groupId,
      items: [],
    };
    decisionBatches.set(sessionKey, batch);
  }
  batch.userId = payload.userId;
  batch.groupId = payload.groupId;
  if (Array.isArray(payload.items) && payload.items.length) {
    batch.items = [...payload.items, ...batch.items];
  }
}

function flushDecisionBatch(sessionKey, config) {
  const payload = buildDecisionPayload(sessionKey);
  if (!payload) return;

  if (pendingDecisionSessions.has(sessionKey) || pendingContextSessions.has(sessionKey)) {
    restoreDecisionPayload(sessionKey, payload);
    return;
  }

  if (config.fetchGroupContext) {
    pendingContextSessions.add(sessionKey);
    fetchGroupContext(
      sessionKey,
      payload.userId,
      payload.groupId,
      payload.merged,
      payload.mentioned,
      payload.items,
      config
    );
  } else {
    callDecisionModel(
      sessionKey,
      payload.userId,
      payload.groupId,
      payload.merged,
      payload.mentioned,
      payload.items,
      config,
      null
    );
  }
}

// Check if mentioning bot
function isMentioningBot(ctx) {
  if (!ctx) return false;
  if (ctx.at_bot === true) return true;

  const selfId = ctx.self_id;
  if (!selfId) return false;

  const segments = ctx.message;
  if (Array.isArray(segments)) {
    for (const seg of segments) {
      if (!seg || seg.type !== "at") continue;
      const qq = seg.data && seg.data.qq !== undefined ? String(seg.data.qq) : "";
      if (qq && qq === String(selfId)) {
        return true;
      }
    }
  }

  const raw = String(ctx.raw_message || "");
  if (raw && raw.includes(`[CQ:at,qq=${selfId}]`)) {
    return true;
  }

  return false;
}

// Create new session
function createSession(sessionKey, userId, groupId, initialMessage, options = {}) {
  const config = getConfig();
  const session = {
    userId,
    groupId,
    messages: [],
    turnCount: 0,
    lastActivity: nbot.now(),
    state: "active",
    initialMessage,
    maxTurns: config.maxTurns,
    groupContext: null, // Will be populated with group announcements and history
    mentionUserOnFirstReply: !!options.mentionUserOnFirstReply,
    lastImageUrls: [],
    lastImageAt: 0,
    lastVideoUrls: [],
    lastRecordUrls: [],
    lastMediaAt: 0,
    lastReplySnippet: "",
    lastReplyAt: 0,
  };
  sessions.set(sessionKey, session);
  return session;
}

// Add message to session
function addMessageToSession(session, role, content) {
  session.messages.push({ role, content });
  session.lastActivity = nbot.now();
}

// End session and update cooldown
function endSession(sessionKey) {
  sessions.delete(sessionKey);

  // Best-effort cleanup for any in-flight async operations tied to this sessionKey
  pendingDecisionSessions.delete(sessionKey);
  pendingContextSessions.delete(sessionKey);
  pendingReplySessions.delete(sessionKey);
  decisionBatches.delete(sessionKey);

  for (const [rid, info] of pendingRequests.entries()) {
    if (info && info.sessionKey === sessionKey) {
      pendingRequests.delete(rid);
    }
  }
  for (const [rid, info] of pendingGroupInfoRequests.entries()) {
    if (info && info.sessionKey === sessionKey) {
      pendingGroupInfoRequests.delete(rid);
    }
  }

  updateCooldown(sessionKey);
}

// Fetch group context (announcements and recent messages)
function fetchGroupContext(sessionKey, userId, groupId, message, mentioned, items, config) {
  const requestId = genRequestId("context");
  pendingGroupInfoRequests.set(requestId, {
    type: "context",
    sessionKey,
    userId,
    groupId,
    message,
    mentioned: !!mentioned,
    items: Array.isArray(items) ? items : [],
    createdAt: nbot.now(),
    step: "notice", // Start with fetching notice
    notice: null,
    history: null,
  });

  // First fetch group announcements
  nbot.fetchGroupNotice(requestId, groupId);
}

// Call decision model
function callDecisionModel(sessionKey, userId, groupId, message, mentioned, items, config, groupContext, options = {}) {
  const requestId = genRequestId("decision");
  pendingDecisionSessions.add(sessionKey);
  pendingRequests.set(requestId, {
    requestId,
    type: "decision",
    sessionKey,
    userId,
    groupId,
    message,
    mentioned: !!mentioned,
    items: Array.isArray(items) ? items : [],
    groupContext: groupContext || null,
    formatRetry: !!options.formatRetry,
    createdAt: nbot.now(),
    modelName: config.decisionModel,
    maxTokens: DECISION_MAX_TOKENS,
  });

  // Build context-aware prompt
  let contextInfo = "";
  if (groupContext) {
    if (groupContext.notice && groupContext.notice.length > 0) {
      contextInfo += "\n\n【群公告】\n";
      groupContext.notice.slice(0, 3).forEach((n, i) => {
        const content = stripAllCqSegments(n.msg?.text || n.message?.text || "");
        if (content) {
          contextInfo += `${i + 1}. ${content.substring(0, 200)}\n`;
        }
      });
    }
    if (groupContext.history && groupContext.history.length > 0) {
      contextInfo += "\n\n【用户近期群消息】\n";
      const uidStr = String(userId);
      const userMessages = groupContext.history
        .filter(m => String(m?.sender?.user_id ?? "") === uidStr)
        .slice(0, 5);
      userMessages.forEach((m, i) => {
        const content = stripAllCqSegments(m.raw_message || "");
        if (content) {
          contextInfo += `${i + 1}. ${content.substring(0, 100)}\n`;
        }
      });
      if (!userMessages.length) {
        contextInfo += "(未匹配到该用户的历史发言)\n";
      }
    }
    const groupSnippet = buildRecentGroupSnippet(groupContext, Math.min(config.contextMessageCount, 30));
    if (groupSnippet) {
      contextInfo += `\n\n${groupSnippet}\n`;
    }
  }
  const recent = recentGroupImages.get(Number(groupId));
  if (recent && Array.isArray(recent.urls) && recent.urls.length && nbot.now() - Number(recent.t || 0) <= 2 * 60 * 1000) {
    contextInfo += "\n\n【最近图片URL】\n";
    recent.urls.slice(0, 2).forEach((u, i) => {
      contextInfo += `${i + 1}. ${String(u).slice(0, 200)}\n`;
    });
  }

  const messages = [
    { role: "system", content: options.decisionSystemPromptOverride || config.decisionSystemPrompt },
    {
      role: "user",
      content: [
        `是否 @ 机器人：${mentioned ? "是" : "否"}`,
        "",
        "候选消息（按时间）：",
        message,
        contextInfo ? `\n${contextInfo}` : "",
      ].join("\n"),
    },
  ];

  nbot.callLlmChat(requestId, messages, {
    modelName: config.decisionModel,
    maxTokens: DECISION_MAX_TOKENS,
  });
}

function buildReplyContextForPrompt(groupContext, userId) {
  if (!groupContext) return "";
  let contextInfo = "";
  if (groupContext.history && groupContext.history.length > 0) {
    const uidStr = String(userId);
    const userMessages = groupContext.history
      .filter(m => String(m?.sender?.user_id ?? "") === uidStr)
      .slice(0, 5);
    if (userMessages.length) {
      contextInfo += "【该用户近期群内发言】\n";
      userMessages.forEach((m, i) => {
        const content = stripAllCqSegments(m.raw_message || "");
        if (content) {
          contextInfo += `${i + 1}. ${content.substring(0, 80)}\n`;
        }
      });
      contextInfo += "\n";
    }
  }
  return contextInfo.trim();
}

function formatOneLinePlain(text) {
  let s = String(text || "");
  if (!s) return "";

  // Strip control characters that may cause downstream truncation (e.g. NUL).
  s = s.replace(/[\u0000-\u001F\u007F]/g, " ");

  // Remove common markdown formatting tokens.
  s = s
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/`+/g, "")
    .replace(/\*\*+/g, "")
    .replace(/__+/g, "")
    .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1") // markdown links
    .replace(/<\/?[^>]+>/g, " ") // html tags
    .replace(/#+\s*/g, "")
    .replace(/^\s*>+\s*/gm, "")
    .replace(/^\s*[-*+]\s+/gm, "")
    .replace(/^\s*\d+\s*[\.\)]\s+/gm, "")
    .replace(/\r\n/g, "\n");

  // Merge all lines into a single line.
  s = s
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean)
    .join(" ");

  // Final whitespace cleanup.
  s = s.replace(/\s+/g, " ").trim();

  // Drop leading greetings; @ is handled by the framework.
  s = s.replace(/^(?:你好|您好|哈喽|嗨|在吗|在不在)\s*[!！。]?\s*/u, "");
  // Drop leading "某某: " quoting style (avoid parroting chat logs).
  s = s.replace(/^[^\s]{1,12}\s*[:：]\s*/u, "");
  return s;
}

function splitQqReply(text, maxChars, maxParts, sep = "||") {
  const s = String(text || "").trim();
  if (!s) return { parts: [], overflow: false };

  const normalized = s.replace(/\s*\|\|\s*/g, "||");
  const rawParts =
    normalized.includes("||") ?
      normalized
        .split("||")
        .map((p) => p.trim())
        .filter(Boolean)
    : [normalized];

  const parts = [];
  let overflow = false;
  const hardMaxChars = Math.max(8, Number(maxChars) || 20);
  const hardMaxParts = Math.max(1, Number(maxParts) || 3);

  const findCutIndex = (chunk) => {
    const window = chunk.slice(0, hardMaxChars + 1);
    const minGood = Math.max(8, Math.floor(hardMaxChars * 0.6));
    const candidates = ["。", "！", "？", "!", "?", "；", ";", "，", ",", "、", " "];
    let best = -1;
    for (const p of candidates) {
      const idx = window.lastIndexOf(p);
      if (idx >= minGood) best = Math.max(best, idx + 1);
    }
    if (best > 0) return best;
    return hardMaxChars;
  };

  for (const part of rawParts) {
    let rest = String(part || "").replace(/\s+/g, " ").trim();
    while (rest.length > hardMaxChars) {
      if (parts.length >= hardMaxParts) {
        overflow = true;
        return { parts, overflow };
      }
      const cut = findCutIndex(rest);
      const head = rest.slice(0, cut).trim();
      if (head) parts.push(head);
      rest = rest.slice(cut).trim();
    }
    if (rest) {
      if (parts.length >= hardMaxParts) {
        overflow = true;
        return { parts, overflow };
      }
      parts.push(rest);
    }
  }

  return { parts, overflow };
}

// Call reply model
function buildReplyMessages(session, sessionKey, config, attachImages) {
  const messages = [{ role: "system", content: config.replySystemPrompt }];
  if (session && session.lastReplySnippet && nbot.now() - Number(session.lastReplyAt || 0) <= 2 * 60 * 1000) {
    messages.push({
      role: "system",
      content: `用户正在回复一条消息，被回复内容（截断）如下，仅用于理解上下文：${session.lastReplySnippet}`,
    });
  }
  const contextInfo = buildReplyContextForPrompt(session.groupContext, session.userId);
  const lastUserMsg = session.messages.slice().reverse().find((m) => m && m.role === "user")?.content || "";
  if (contextInfo && session.turnCount === 0) {
    messages.push({
      role: "system",
      content: `以下是该用户在本群最近发言的原文（截断），仅用于理解语境；禁止推断任何未出现的事实，也不要输出任何 QQ 号/ID：\n\n${contextInfo}`,
    });
  }

  if (attachImages) {
    const shouldAttachMedia =
      looksReferentialShortQuestion(lastUserMsg) ||
      String(lastUserMsg).includes("[图片]") ||
      String(lastUserMsg).includes("[视频]") ||
      String(lastUserMsg).includes("[语音]") ||
      String(lastUserMsg).includes("[文件]") ||
      (session.lastMediaAt && nbot.now() - Number(session.lastMediaAt || 0) <= 15 * 1000);

    if (shouldAttachMedia) {
      const imageUrls = getRelevantImageUrlsForSession(session, sessionKey)
        .filter((u) => /^https?:\/\//i.test(String(u || "")))
        .slice(0, 2);
      const videoUrls = getRelevantVideoUrlsForSession(session, sessionKey).slice(0, 1);
      const recordUrls = getRelevantRecordUrlsForSession(session, sessionKey).slice(0, 1);
      const urls = [...imageUrls, ...videoUrls, ...recordUrls].filter(Boolean).slice(0, 4);
      if (urls.length) {
        const mm = buildMultimodalImageMessage(urls);
        if (mm) messages.push(mm);
      }
    }
  }

  messages.push(...session.messages);
  return messages;
}

function callReplyModel(session, sessionKey, config, useSearch = false) {
  pendingReplySessions.add(sessionKey);
  const requestId = genRequestId("reply");
  const messages = buildReplyMessages(session, sessionKey, config, true);

  const usedImages = messages.some((m) => Array.isArray(m?.content) && m.content.some((p) => p && p.type === "image_url"));
  pendingRequests.set(requestId, {
    requestId,
    type: "reply",
    sessionKey,
    createdAt: nbot.now(),
    usedImages,
    noImageRetry: false,
    modelName: useSearch && config.enableWebsearch ? config.websearchModel : config.replyModel,
    maxTokens: REPLY_MAX_TOKENS,
  });

  if (useSearch && config.enableWebsearch) {
    nbot.callLlmChatWithSearch(requestId, messages, {
      modelName: config.websearchModel,
      maxTokens: REPLY_MAX_TOKENS,
      enableSearch: true,
    });
  } else {
    nbot.callLlmChat(requestId, messages, {
      modelName: config.replyModel,
      maxTokens: REPLY_MAX_TOKENS,
    });
  }
}

// Handle decision result
function handleDecisionResult(requestInfo, success, content) {
  const { sessionKey, userId, groupId, message, mentioned, items, groupContext } = requestInfo;
  const config = getConfig();
  pendingDecisionSessions.delete(sessionKey);

  function parseDecision(raw) {
    const text = String(raw || "").trim();
    if (!text) {
      return { action: "IGNORE", confidence: 0, reason: "", useSearch: false, topic: "", needClarify: false };
    }

    const direct = text.toUpperCase();
    if (direct === "YES" || direct === "NO") {
      return {
        action: direct === "YES" ? "REPLY" : "IGNORE",
        confidence: 1,
        reason: "direct",
        useSearch: false,
        topic: "",
        needClarify: false,
      };
    }

    const fenced = text.match(/```(?:json)?\s*([\s\S]*?)```/i);
    const candidate = (fenced ? fenced[1] : text).trim();

    const tryParseJson = (s) => {
      if (!s) return null;
      const t = String(s).trim();
      if (!(t.startsWith("{") && t.endsWith("}"))) return null;
      try {
        const obj = JSON.parse(t);
        const actionRaw = String(obj.action || obj.router_action || obj.mode || "").trim().toUpperCase();
        const decision = String(obj.decision || obj.answer || "").trim().toUpperCase();
        const confidence = Number(obj.confidence);
        const reason = String(obj.reason || "").trim();
        const useSearchRaw = obj.use_search ?? obj.useSearch ?? obj.search ?? obj.use_websearch;
        const useSearch = useSearchRaw === true || String(useSearchRaw || "").toLowerCase() === "true";
        const topic = String(obj.topic || "").trim();
        const needClarifyRaw = obj.need_clarify ?? obj.needClarify ?? obj.clarify;
        const needClarify =
          needClarifyRaw === true || String(needClarifyRaw || "").toLowerCase() === "true";

        const action =
          actionRaw === "REPLY" || actionRaw === "IGNORE" || actionRaw === "REACT"
            ? actionRaw
            : decision === "YES"
              ? "REPLY"
              : decision === "NO"
                ? "IGNORE"
                : "IGNORE";
        return {
          action,
          confidence: Number.isFinite(confidence) ? Math.max(0, Math.min(1, confidence)) : 0,
          reason,
          useSearch,
          topic,
          needClarify,
        };
      } catch {
        return null;
      }
    };

    // 1) strict JSON (or fenced JSON)
    const parsedDirect = tryParseJson(candidate);
    if (parsedDirect) return parsedDirect;

    // 2) tolerant extraction: find first {...} in the output
    const first = candidate.indexOf("{");
    const last = candidate.lastIndexOf("}");
    if (first !== -1 && last !== -1 && last > first) {
      const maybe = candidate.slice(first, last + 1);
      const parsed = tryParseJson(maybe);
      if (parsed) return parsed;
    }

    // 3) heuristic fallback: accept obvious YES/NO tokens when the model didn't follow format
    const m = candidate.match(/\b(YES|NO)\b/i);
    if (m && m[1]) {
      const token = String(m[1]).toUpperCase();
      return {
        action: token === "YES" ? "REPLY" : "IGNORE",
        confidence: 0.9,
        reason: "heuristic_token",
        useSearch: false,
        topic: "",
        needClarify: false,
      };
    }
    const m2 = candidate.match(/decision\s*[:=]\s*(yes|no)/i);
    if (m2 && m2[1]) {
      const token = String(m2[1]).toUpperCase();
      return {
        action: token === "YES" ? "REPLY" : "IGNORE",
        confidence: 0.9,
        reason: "heuristic_decision",
        useSearch: false,
        topic: "",
        needClarify: false,
      };
    }

    // Strict mode: any other non-JSON response is treated as NO (avoid false positives).
    nbot.log.warn(
      `[smart-assist] decision parse failed mentioned=${mentioned ? "Y" : "N"} raw=${maskSensitiveForLog(text).slice(0, 220)}`
    );
    return { action: "IGNORE", confidence: 0, reason: "non_json", useSearch: false, topic: "", needClarify: false };
  }

  if (!success) {
    nbot.log.warn(`Decision model call failed: ${content}`);
    const batch = decisionBatches.get(sessionKey);
    if (batch && batch.items.length) {
      const urgent = batch.items.some((x) => !!x?.mentioned);
      scheduleDecisionFlush(sessionKey, urgent, config);
    }
    return;
  }

  const existing = sessions.get(sessionKey);
  const parsed = parseDecision(content);

  const needsFormatRetry =
    (parsed.reason === "non_json" || String(parsed.reason || "").startsWith("heuristic_")) && !requestInfo.formatRetry;

  // If the model didn't follow the strict JSON format (including plain YES/NO), retry once with a stronger instruction.
  if (needsFormatRetry) {
    const stronger = [
      config.decisionSystemPrompt,
      "",
      "你上一条输出不符合格式。再次强调：只允许输出单行 JSON，且必须以 { 开头、以 } 结尾；除此之外禁止任何字符。",
      "禁止输出 YES/NO/OK/好的 等单词；如果你想表达“要/不要介入”，也必须写进 JSON 的 action 字段。",
      "示例：{\"action\":\"IGNORE\",\"confidence\":0.0,\"reason\":\"不确定\",\"use_search\":false,\"topic\":\"\",\"need_clarify\":false}",
    ].join("\n");

    nbot.log.info(
      `[smart-assist] decision format retry reason=${parsed.reason || "-"} rid=${String(requestInfo.requestId || "").slice(0, 48)}`
    );
    callDecisionModel(
      sessionKey,
      userId,
      groupId,
      message,
      mentioned,
      items,
      config,
      groupContext || null,
      { formatRetry: true, decisionSystemPromptOverride: stronger }
    );
    return;
  }

  const action = parsed.action || "IGNORE";
  const shouldReply = action === "REPLY";

  nbot.log.info(
    `[smart-assist] action=${action} conf=${parsed.confidence.toFixed(2)} reply=${shouldReply ? "Y" : "N"} mentioned=${mentioned ? "Y" : "N"} search=${parsed.useSearch ? "Y" : "N"} clarify=${parsed.needClarify ? "Y" : "N"} reason=${parsed.reason || "-"} rid=${String(requestInfo.requestId || "").slice(0, 48)} text=${maskSensitiveForLog(sanitizeMessageForLlm(String(message || ""), null)).slice(0, 80)}`
  );

  if (!shouldReply) {
    const batch = decisionBatches.get(sessionKey);
    if (batch && batch.items.length) {
      const urgent = batch.items.some((x) => !!x?.mentioned);
      scheduleDecisionFlush(sessionKey, urgent, config);
    }
    return;
  }

  // If a session already exists, only reply when the decision model says YES.
  // This makes the assistant feel more like a human in QQ group chats (not every turn must reply).
  if (existing && existing.state === "active") {
    if (!pendingReplySessions.has(sessionKey)) {
      callReplyModel(existing, sessionKey, config, parsed.useSearch);
    }
    return;
  }

  // Check cooldown (from last session cleanup)
  if (!checkCooldown(sessionKey, config.cooldownMs)) {
    nbot.log.info("[smart-assist] skipped: cooldown");
    return;
  }

  const seedItems =
    Array.isArray(items) && items.length
      ? items.map((x) => String(x?.text ?? ""))
      : message
        ? [sanitizeMessageForLlm(message, null)]
        : [];

  // Create new session
  const session = createSession(sessionKey, userId, groupId, seedItems[0] || message || "", {
    mentionUserOnFirstReply: !!mentioned,
  });
  session.groupContext = groupContext || null;

  const replySnippetFromBatch = Array.isArray(items)
    ? items.map((x) => String(x?.replySnippet || "")).find((s) => !!s.trim())
    : "";
  if (replySnippetFromBatch) {
    session.lastReplySnippet = replySnippetFromBatch;
    session.lastReplyAt = nbot.now();
  }

  for (const t of seedItems) {
    addMessageToSession(session, "user", sanitizeMessageForLlm(t, null) || t);
  }

  // If user sent more messages while we were deciding, include them before reply.
  const batch = decisionBatches.get(sessionKey);
  if (batch && batch.items.length) {
    const extra = batch.items.splice(0, batch.items.length);
    for (const x of extra) {
      addMessageToSession(session, "user", sanitizeMessageForLlm(String(x?.text ?? ""), null));
    }
  }

  nbot.log.info("[smart-assist] created new session");

  // Start assisting immediately
  callReplyModel(session, sessionKey, config, parsed.useSearch);
}

// Handle reply result
function handleReplyResult(requestInfo, success, content) {
  const { sessionKey } = requestInfo;
  pendingReplySessions.delete(sessionKey);

  const session = sessions.get(sessionKey);
  const config = getConfig();

  if (!session) {
    nbot.log.warn("Session not found");
    return;
  }

  const raw = String(content || "");
  const rawLen = raw.length;
  const hasControl = /[\u0000-\u001F\u007F]/.test(raw);
  if (hasControl || rawLen <= 12) {
    nbot.log.warn(
      `[smart-assist] reply_raw len=${rawLen} ctl=${hasControl ? "Y" : "N"} usedImages=${requestInfo.usedImages ? "Y" : "N"} model=${requestInfo.modelName || "-"} maxTok=${requestInfo.maxTokens || "-"} rid=${String(requestInfo.requestId || "").slice(0, 48)} raw=${escapeForLog(raw, 500)}`
    );
  }

  if (!success) {
    // If model/provider doesn't support image_url, retry once without images.
    if (requestInfo && requestInfo.usedImages && !requestInfo.noImageRetry) {
      pendingReplySessions.add(sessionKey);
      const requestId = genRequestId("reply");
      const retryMessages = buildReplyMessages(session, sessionKey, config, false);
      pendingRequests.set(requestId, {
        requestId,
        type: "reply",
        sessionKey,
        createdAt: nbot.now(),
        usedImages: false,
        noImageRetry: true,
        modelName: config.replyModel,
        maxTokens: REPLY_MAX_TOKENS,
      });
      nbot.callLlmChat(requestId, retryMessages, {
        modelName: config.replyModel,
        maxTokens: REPLY_MAX_TOKENS,
      });
      return;
    }

    nbot.sendReply(session.userId, session.groupId || 0, "出错了，稍后再试。");
    endSession(sessionKey);
    return;
  }

  // Add assistant reply to session
  let cleaned = formatOneLinePlain(
    raw
      .replace(/\s+@(?:群主|管理员|全体|all|everyone|here)\b/g, "")
      .replace(/^(?:@(?:群主|管理员|全体|all|everyone|here)\b\s*)+/g, "")
      .trim()
  );
  if (/[\u0000-\u001F\u007F]/.test(cleaned)) {
    cleaned = cleaned.replace(/[\u0000-\u001F\u007F]/g, " ").replace(/\s+/g, " ").trim();
  }
  if (!cleaned) {
    // If cleaning removed everything, ask the model again rather than hard-coding a reply.
    pendingReplySessions.add(sessionKey);
    const requestId = genRequestId("reply");
    pendingRequests.set(requestId, {
      requestId,
      type: "reply",
      sessionKey,
      createdAt: nbot.now(),
      modelName: config.replyModel,
      maxTokens: REPLY_RETRY_MAX_TOKENS,
    });

    const retryMessages = [
      {
        role: "system",
        content:
          config.replySystemPrompt +
          "\n\n补充要求：你的上一条输出在清洗后为空；请只输出一句中文短句（同一行），不要任何标点以外的格式。",
      },
      ...session.messages,
    ];

    nbot.callLlmChat(requestId, retryMessages, {
      modelName: config.replyModel,
      maxTokens: REPLY_RETRY_MAX_TOKENS,
    });
    return;
  }

  const splitResult = splitQqReply(
    cleaned,
    config.replyMaxChars,
    config.replyMaxParts,
    config.replyPartsSeparator
  );
  if (splitResult.overflow && !requestInfo.compactRetry) {
    // Ask the model to compress/split properly instead of hard-truncating.
    pendingReplySessions.add(sessionKey);
    const requestId = genRequestId("reply");
    pendingRequests.set(requestId, {
      requestId,
      type: "reply",
      sessionKey,
      createdAt: nbot.now(),
      modelName: config.replyModel,
      maxTokens: REPLY_RETRY_MAX_TOKENS,
      compactRetry: true,
    });

    const retryMessages = [
      {
        role: "system",
        content:
          config.replySystemPrompt +
          `\n\n补充要求：你的上一条输出过长；请用「||」分成不超过 ${config.replyMaxParts} 条，每条不超过 ${config.replyMaxChars} 字；不要编号、不要换行、不要 Markdown。`,
      },
      ...session.messages,
    ];

    nbot.callLlmChat(requestId, retryMessages, {
      modelName: config.replyModel,
      maxTokens: REPLY_RETRY_MAX_TOKENS,
    });
    return;
  }

  const parts = splitResult.parts.length ? splitResult.parts : [cleaned];
  if (cleaned.length <= 12 || parts.length > 1) {
    nbot.log.info(
      `[smart-assist] reply_cleaned len=${cleaned.length} parts=${parts.length}/${config.replyMaxParts} maxChars=${config.replyMaxChars} usedImages=${requestInfo.usedImages ? "Y" : "N"} model=${requestInfo.modelName || "-"} rawLen=${rawLen} rid=${String(requestInfo.requestId || "").slice(0, 48)} cleaned=${escapeForLog(cleaned, 180)} raw=${escapeForLog(raw, 500)}`
    );
  }
  addMessageToSession(session, "assistant", parts.join(" "));
  session.turnCount++;

  // Send reply (hide counters; keep session limits internal)
  let prefix = "";
  if (session.mentionUserOnFirstReply) {
    prefix = nbot.at(session.userId) ? `${nbot.at(session.userId)} ` : "";
    session.mentionUserOnFirstReply = false;
  }
  parts.forEach((p, idx) => {
    const msg = idx === 0 ? `${prefix}${p}` : p;
    if (msg) nbot.sendReply(session.userId, session.groupId || 0, msg);
  });

  // Check if max turns reached (silent end; avoid spamming in QQ group)
  if (session.turnCount >= config.maxTurns) {
    endSession(sessionKey);
    return;
  }
}

// Handle group info response
function handleGroupInfoResponse(requestInfo, infoType, success, data) {
  const { sessionKey, userId, groupId, message, step, mentioned, items } = requestInfo;
  const config = getConfig();

  if (step === "notice") {
    // Store notice data
    requestInfo.notice = success ? data : null;
    requestInfo.step = "history";

    // Now fetch message history
    const requestId = genRequestId("context-history");
    pendingGroupInfoRequests.delete(requestInfo.requestId);
    requestInfo.requestId = requestId;
    pendingGroupInfoRequests.set(requestId, requestInfo);

    nbot.fetchGroupMsgHistory(requestId, groupId, { count: config.contextMessageCount });
  } else if (step === "history") {
    // Store history data
    requestInfo.history = success ? data?.messages : null;

    // Clean up pending request
    pendingGroupInfoRequests.delete(requestInfo.requestId);

    // Mark context fetch as finished for this sessionKey
    pendingContextSessions.delete(sessionKey);

    // Build group context
    const groupContext = {
      notice: requestInfo.notice,
      history: requestInfo.history,
    };

    // Now call decision model with context
    callDecisionModel(sessionKey, userId, groupId, message, mentioned, items, config, groupContext);
  } else {
    // Unexpected state; avoid permanently blocking future checks.
    if (requestInfo.requestId) {
      pendingGroupInfoRequests.delete(requestInfo.requestId);
    }
    pendingContextSessions.delete(sessionKey);
    nbot.log.warn(`Unknown group context step: ${step}`);
  }
}

function cleanupStaleRequests(config) {
  const now = nbot.now();

  // LLM requests
  for (const [requestId, info] of pendingRequests.entries()) {
    const createdAt = info?.createdAt || 0;
    if (!createdAt || now - createdAt <= config.requestTimeoutMs) continue;

    pendingRequests.delete(requestId);

    const sessionKey = info?.sessionKey;
    if (info?.type === "decision") {
      pendingDecisionSessions.delete(sessionKey);
    } else if (info?.type === "reply") {
      pendingReplySessions.delete(sessionKey);
      const session = sessions.get(sessionKey);
      if (session && session.state === "active") {
        nbot.sendReply(session.userId, session.groupId || 0, "回复超时，请再说一次。");
      }
    }

    nbot.log.warn(`Request timeout: ${info?.type || "unknown"} ${requestId}`);
  }

  // Group context requests
  for (const [requestId, info] of pendingGroupInfoRequests.entries()) {
    const createdAt = info?.createdAt || 0;
    if (!createdAt || now - createdAt <= config.contextTimeoutMs) continue;

    pendingGroupInfoRequests.delete(requestId);

    const sessionKey = info?.sessionKey;
    pendingContextSessions.delete(sessionKey);

    // Fallback: proceed without context so user won't get stuck.
    if (info?.type === "context") {
      callDecisionModel(
        sessionKey,
        info.userId,
        info.groupId,
        info.message,
        info.mentioned,
        info.items,
        config,
        null
      );
    }

    nbot.log.warn(`Context timeout: ${requestId}`);
  }
}

// Plugin object
return {
  onEnable() {
  nbot.log.info("Smart Assistant Plugin v2.2.24 enabled");
  },

  onDisable() {
    sessions.clear();
    cooldowns.clear();
    pendingRequests.clear();
    pendingGroupInfoRequests.clear();
    decisionBatches.clear();
    nbot.log.info("Smart Assistant Plugin disabled");
  },

  // Backend tick event: used to implement 5-second message merge without JS timers.
  async onMetaEvent(ctx) {
    try {
      if (!ctx || ctx.meta_event_type !== "tick") return true;
      const config = getConfig();
      const now = nbot.now();
      for (const [sessionKey, batch] of decisionBatches.entries()) {
        if (!batch || !Array.isArray(batch.items) || batch.items.length === 0) continue;
        if (pendingDecisionSessions.has(sessionKey) || pendingContextSessions.has(sessionKey)) {
          continue;
        }
        const firstAt = Number(batch.items[0]?.t || 0);
        const lastAt = Number(batch.items[batch.items.length - 1]?.t || 0);
        if (!firstAt || !lastAt) continue;
        const windowMs = config.decisionMergeIdleMs;
        const dueByIdle = now - lastAt >= windowMs;
        const dueByWindow = now - firstAt >= windowMs;
        if (!dueByIdle && !dueByWindow) continue;
        flushDecisionBatch(sessionKey, config);
      }
    } catch (e) {
      nbot.log.warn(`[smart-assist] onMetaEvent error: ${e}`);
    }
    return true;
  },

  // Monitor each message
  preMessage(ctx) {
    try {
      const config = getConfig();

      // Cleanup expired sessions with notification
      cleanupExpiredSessions(config.sessionTimeoutMs);
      cleanupStaleRequests(config);

      const { user_id, group_id, raw_message, message_type } = ctx;

      // Only process group messages
      if (message_type !== "group" || !group_id) {
        return true;
      }

      const sessionKey = `${group_id}:${user_id}`;
      const session = sessions.get(sessionKey);
      const message = raw_message || "";
      const llmMessage = sanitizeMessageForLlm(message, ctx);
      const mentions = summarizeMentions(ctx);
      const imageUrls = extractImageUrlsFromCtx(ctx);
      const videoUrls = extractVideoUrlsFromCtx(ctx);
      const recordUrls = extractRecordUrlsFromCtx(ctx);
      const replyCtx = extractReplyMessageContext(ctx);
      if (imageUrls.length) {
        noteRecentGroupImages(group_id, imageUrls);
        noteRecentUserImages(sessionKey, imageUrls);
        if (session) {
          session.lastImageUrls = imageUrls;
          session.lastImageAt = nbot.now();
        }
      }
      if (videoUrls.length) {
        noteRecentGroupVideos(group_id, videoUrls);
        noteRecentUserVideos(sessionKey, videoUrls);
        if (session) {
          session.lastVideoUrls = videoUrls;
          session.lastMediaAt = nbot.now();
        }
      }
      if (recordUrls.length) {
        noteRecentGroupRecords(group_id, recordUrls);
        noteRecentUserRecords(sessionKey, recordUrls);
        if (session) {
          session.lastRecordUrls = recordUrls;
          session.lastMediaAt = nbot.now();
        }
      }
      if (
        replyCtx &&
        ((Array.isArray(replyCtx.imageUrls) && replyCtx.imageUrls.length) ||
          (Array.isArray(replyCtx.videoUrls) && replyCtx.videoUrls.length) ||
          (Array.isArray(replyCtx.recordUrls) && replyCtx.recordUrls.length))
      ) {
        // Treat replied media as recent media for context resolution.
        // (Use the typed buckets so we don't mix image/video/record in the wrong cache.)
        const rImages = Array.isArray(replyCtx.imageUrls) ? replyCtx.imageUrls : [];
        const rVideos = Array.isArray(replyCtx.videoUrls) ? replyCtx.videoUrls : [];
        const rRecords = Array.isArray(replyCtx.recordUrls) ? replyCtx.recordUrls : [];

        if (rImages.length) {
          noteRecentGroupImages(group_id, rImages);
          noteRecentUserImages(sessionKey, rImages);
          if (session) {
            session.lastImageUrls = rImages;
            session.lastImageAt = nbot.now();
          }
        }
        if (rVideos.length) {
          noteRecentGroupVideos(group_id, rVideos);
          noteRecentUserVideos(sessionKey, rVideos);
          if (session) {
            session.lastVideoUrls = rVideos;
            session.lastMediaAt = nbot.now();
          }
        }
        if (rRecords.length) {
          noteRecentGroupRecords(group_id, rRecords);
          noteRecentUserRecords(sessionKey, rRecords);
          if (session) {
            session.lastRecordUrls = rRecords;
            session.lastMediaAt = nbot.now();
          }
        }
        if (session) {
          session.lastMediaAt = nbot.now();
        }
      }
      if (replyCtx && replyCtx.snippet && session) {
        session.lastReplySnippet = replyCtx.snippet;
        session.lastReplyAt = nbot.now();
      }

      // If active session exists
      if (session && session.state === "active") {
        // Check interrupt keywords
        if (containsKeyword(message, config.interruptKeywords)) {
          endSession(sessionKey);
          return true;
        }

        // Continue conversation (store context), but do NOT force a reply every turn.
        addMessageToSession(session, "user", llmMessage || message);

        // Let the decision model decide whether we should reply to this new message.
        const trigger = getDecisionTrigger(ctx, message, config);
        let batch = decisionBatches.get(sessionKey);
        if (!batch) {
          batch = { userId: user_id, groupId: group_id, items: [] };
          decisionBatches.set(sessionKey, batch);
        }
        batch.userId = user_id;
        batch.groupId = group_id;
        batch.items.push({
          t: nbot.now(),
          text: sanitizeMessageForLlm(message, ctx),
          mentioned: !!trigger.mentioned,
          imageUrls,
          replySnippet: replyCtx ? replyCtx.snippet : "",
        });
        scheduleDecisionFlush(sessionKey, trigger.urgent, config);
        return true;
      }

      // No active session, decide whether to run decision model.
      const trigger = getDecisionTrigger(ctx, message, config);
      const shouldCheck = checkCooldown(sessionKey, config.cooldownMs) && trigger.shouldCheck;
      if (shouldCheck) {
        // Store a sanitized copy for LLM so CQ segments don't mislead the decision model.
        // Still keep the boolean mentioned flag from the real message segments.
        let batch = decisionBatches.get(sessionKey);
        if (!batch) {
          batch = { userId: user_id, groupId: group_id, items: [] };
          decisionBatches.set(sessionKey, batch);
        }
        batch.userId = user_id;
        batch.groupId = group_id;
        batch.items.push({
          t: nbot.now(),
          text: sanitizeMessageForLlm(message, ctx),
          mentioned: !!trigger.mentioned,
          imageUrls,
          replySnippet: replyCtx ? replyCtx.snippet : "",
        });
        scheduleDecisionFlush(sessionKey, trigger.urgent, config);
      }

      return true;
    } catch (e) {
      // Never block messages when the plugin crashes.
      nbot.log.warn(`[smart-assist] preMessage error (ignored): ${e}`);
      return true;
    }
  },

  // LLM response callback
  onLlmResponse(response) {
    const { requestId, success, content } = response;

    const requestInfo = pendingRequests.get(requestId);
    if (!requestInfo) {
      return; // Not our request
    }

    pendingRequests.delete(requestId);

    switch (requestInfo.type) {
      case "decision":
        handleDecisionResult(requestInfo, success, content);
        break;
      case "reply":
        handleReplyResult(requestInfo, success, content);
        break;
      default:
        nbot.log.warn(`Unknown request type: ${requestInfo.type}`);
    }
  },

  // Group info response callback
  onGroupInfoResponse(response) {
    const { requestId, infoType, success, data } = response;

    const requestInfo = pendingGroupInfoRequests.get(requestId);
    if (!requestInfo) {
      return; // Not our request
    }

    // Store the requestId for cleanup
    requestInfo.requestId = requestId;

    handleGroupInfoResponse(requestInfo, infoType, success, data);
  },
};
