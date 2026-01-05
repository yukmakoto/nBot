/**
 * nBot Smart Assistant Plugin v2.2.18
 * Auto-detects if user needs help, enters multi-turn conversation mode,
 * supports web search, generates analysis report via forward message
 *
 * Features:
 * 1. Decision model: Monitors each message, strictly judges if user needs help
 * 2. Multi-turn conversation: After entering conversation mode, interacts with user
 * 3. Interrupt conversation: User can interrupt at any time (no report generated)
 * 4. Early analysis: User can request early report generation
 * 5. Web search: Can enable web search when generating report
 * 6. Forward message: Final report sent via merged forward message
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
const pendingReportSessions = new Set();

// Decision batching (reduce LLM calls while still judging every message)
const decisionBatches = new Map(); // Map<sessionKey, { userId:number, groupId:number, items: {t:number,text:string,mentioned:boolean}[] }>
const DECISION_BATCH_MAX_ITEMS = 8;

// Recent images (help the model resolve "the image above")
const recentGroupImages = new Map(); // Map<groupId, { t:number, urls:string[] }>
const recentUserImages = new Map(); // Map<sessionKey, { t:number, urls:string[] }>

// Request ID counter
let requestIdCounter = 0;

// Generate unique request ID
function genRequestId(type) {
  return `smart-assist-${type}-${++requestIdCounter}-${nbot.now()}`;
}

// Get config
function getConfig() {
  const cfg = nbot.getConfig();
  const interruptKeywords =
    Array.isArray(cfg.interrupt_keywords) && cfg.interrupt_keywords.length
      ? cfg.interrupt_keywords
      : ["我明白了", "结束", "停止"];
  const earlyAnalysisKeywords =
    Array.isArray(cfg.early_analysis_keywords) && cfg.early_analysis_keywords.length
      ? cfg.early_analysis_keywords
      : ["这就是我想说的", "生成报告", "总结"];

  const decisionSystemPrompt =
    cfg.decision_system_prompt ||
    [
      "你是群聊中的智能助手触发器。下面给出的是同一用户在短时间内的多条连续消息，请判断【是否真的需要机器人介入回复】。",
      "",
      "重要：要非常保守，避免误触发。",
      "- 只要像玩笑/吐槽/阴阳怪气/反讽/自问自答/口头禅、或没有明确问题与需求，一律判定 NO。",
      "- 被 @ 机器人只是“优先级更高”的信号，仍然可以判定 NO。",
      "- 没有 @ 机器人时：除非用户明显是在向全群求助/提问（期待任何人回答），否则一律判定 NO。不要抢别人的对话。",
      "- 只有媒体/占位符（如“[图片] / [视频] / [语音] / [卡片]”）且没有任何文字内容，一律判定 NO（不要去‘说明无法判断’）。",
      "- 只有表情/颜文字/一个词/无意义应答（如“哈哈”“？”“。。。”）一律判定 NO。",
      "- 用户在 @ 其他人（而不是 @ 机器人）时，通常是在找那个人说话：除非明确要求机器人回答，否则判定 NO。",
      "",
      "请输出严格 JSON（不要 Markdown、不要解释文本）：",
      '{"decision":"YES|NO","confidence":0.0,"reason":"<=20字中文"}',
      "输出必须为【单行 JSON】，且必须以 { 开头、以 } 结尾；除此之外禁止任何字符。",
      "",
      "decision=YES 的条件（同时满足）：",
      "1) 明确在求助/提问/请求排查/要方案/要解释；且",
      "2) 用户期待机器人回答（不是玩笑、不是随口一句）；且",
      "3) 你对判断非常有把握：只有在你非常确定时才允许输出 YES，否则输出 NO。",
    ].join("\n");

  const replySystemPrompt =
    cfg.reply_system_prompt ||
    [
      "你是 QQ 群里的热心老群友式助手。目标：用一句话给出最有用的下一步，尽量少打扰。",
      "",
      "输出要求（硬性）：",
      "- 只输出【一行】中文短句；禁止换行；禁止 Markdown/列表/编号/加粗/代码块。",
      "- 语气自然像群友：别写长段落、别客服腔、别“为了更好地帮助你…”。",
      "- 最多问 1 个关键追问；否则直接给一个最可能有效的下一步。",
      "- 禁止编造任何未在上文出现的事实（例如版本/整合包/服务器细节/群内信息）。不确定就问一句。",
      "- 不要输出任何 QQ 号/ID/Token/密钥；@ 由系统自动添加，你不要手写 @。",
    ].join("\n");

  const reportPrompt =
    cfg.report_prompt ||
    [
      "请基于以上对话生成一份「分析报告」，并严格按以下格式输出两部分：",
      "",
      "===MARKDOWN===",
      "（这部分用 Markdown 写，适合渲染成图片；结构清晰，包含：问题概述、关键信息、分析、排查步骤、解决方案、后续建议）",
      "",
      "===COPY===",
      "（这部分给用户“方便复制”的纯文本内容：只保留最终可执行的步骤/命令/配置片段/关键链接；不要写长篇解释）",
      "",
      "要求：中文；不要输出除以上分隔符与内容外的任何额外文字。",
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
    reportPrompt,
    interruptKeywords,
    earlyAnalysisKeywords,
    greetingTemplate:
      cfg.greeting_template ||
      "你好，我注意到你可能需要帮助。\n\n剩余对话次数：{remaining}\n\n请在对话次数内向我描述清楚你的问题。\n\n如果你已经明白了，可以回复「我明白了」来结束对话。\n如果你已经说完了，可以回复「这就是我想说的」来提前生成分析报告。",
    botName: cfg.bot_name || "智能助手",
    fetchGroupContext: cfg.fetch_group_context !== false,
    contextMessageCount: (() => {
      const v = Number(cfg.context_message_count ?? 20);
      if (!Number.isFinite(v)) return 20;
      return Math.max(5, Math.min(100, Math.floor(v)));
    })(),
    // Keep formatting limits internal; don't rely on config for behavior.
    replyMaxChars: 80,
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

// Cleanup expired sessions with notification
function cleanupExpiredSessions(timeoutMs) {
  const now = nbot.now();
  for (const [key, session] of sessions.entries()) {
    if (now - session.lastActivity > timeoutMs) {
      nbot.log.info(`Session timeout, auto-ending: ${key}`);

      // Notify user about timeout
      nbot.sendReply(
        session.userId,
        session.groupId || 0,
        "会话长时间无操作，已自动结束。"
      );

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

function noteRecentGroupImages(groupId, urls) {
  const gid = Number(groupId);
  if (!gid || !Array.isArray(urls) || urls.length === 0) return;
  recentGroupImages.set(gid, { t: nbot.now(), urls: urls.slice(0, 4) });
}

function noteRecentUserImages(sessionKey, urls) {
  if (!sessionKey || !Array.isArray(urls) || urls.length === 0) return;
  recentUserImages.set(String(sessionKey), { t: nbot.now(), urls: urls.slice(0, 4) });
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
      { type: "text", text: "参考图片（来自群聊最近一张/几张图，仅用于理解当前问题，不要回复这句话）：" },
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
    .replace(/\[CQ:image,[^\]]*\]/g, "[图片]")
    .replace(/\[CQ:video,[^\]]*\]/g, "[视频]")
    .replace(/\[CQ:record,[^\]]*\]/g, "[语音]")
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
    .map((x, idx) => `${idx + 1}. ${String(x?.text || "").trim()}`)
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
    needsReply: false,
    mentionUserOnFirstReply: !!options.mentionUserOnFirstReply,
    lastImageUrls: [],
    lastImageAt: 0,
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
  pendingReportSessions.delete(sessionKey);
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
    const groupSnippet = buildRecentGroupSnippet(groupContext, 12);
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
    maxTokens: 96,
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

function formatOneLinePlain(text, maxChars = 160) {
  let s = String(text || "");
  if (!s) return "";

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

  // Prefer the first helpful sentence when output is overly long.
  if (s.length > maxChars) {
    // Avoid returning just a greeting.
    const cutCandidates = ["。", "！", "？", "!", "?", "；", ";"];
    let cutAt = -1;
    for (const p of cutCandidates) {
      const idx = s.indexOf(p);
      if (idx !== -1) {
        cutAt = idx + 1;
        // If the first sentence is too short, keep searching.
        if (cutAt >= 10) break;
      }
    }
    if (cutAt !== -1 && cutAt <= maxChars) {
      s = s.slice(0, cutAt).trim();
    }
  }

  if (s.length > maxChars) {
    s = s.slice(0, maxChars).trimEnd();
  }
  return s;
}

// Call reply model
function buildReplyMessages(session, sessionKey, config, attachImages) {
  const messages = [{ role: "system", content: config.replySystemPrompt }];
  const contextInfo = buildReplyContextForPrompt(session.groupContext, session.userId);
  const lastUserMsg = session.messages.slice().reverse().find((m) => m && m.role === "user")?.content || "";
  const groupSnippet =
    session.groupContext && looksReferentialShortQuestion(lastUserMsg)
      ? buildRecentGroupSnippet(session.groupContext, config.contextMessageCount)
      : "";
  if (contextInfo && session.turnCount === 0) {
    messages.push({
      role: "system",
      content: `以下是该用户在本群最近发言的原文（截断），仅用于理解语境；禁止推断任何未出现的事实，也不要输出任何 QQ 号/ID：\n\n${contextInfo}`,
    });
  }
  if (groupSnippet && session.turnCount === 0) {
    messages.push({
      role: "system",
      content:
        `以下是最近群聊片段（可能包含你需要指代的“上一条图片/上文”），仅用于定位上下文；` +
        `只回答当前用户的问题，不要替别人答话；不要推断任何未出现的事实：\n\n${groupSnippet}`,
    });
  }

  if (attachImages) {
    const shouldAttachImage =
      looksReferentialShortQuestion(lastUserMsg) ||
      String(lastUserMsg).includes("[图片]") ||
      (session.lastImageAt && nbot.now() - Number(session.lastImageAt || 0) <= 15 * 1000);
    const imageUrls = shouldAttachImage ? getRelevantImageUrlsForSession(session, sessionKey) : [];
    const httpImageUrls = imageUrls.filter((u) => /^https?:\/\//i.test(String(u || ""))).slice(0, 2);
    if (httpImageUrls.length) {
      const mm = buildMultimodalImageMessage(httpImageUrls);
      if (mm) messages.push(mm);
    }
  }

  messages.push(...session.messages);
  return messages;
}

function callReplyModel(session, sessionKey, config) {
  pendingReplySessions.add(sessionKey);
  const requestId = genRequestId("reply");
  const messages = buildReplyMessages(session, sessionKey, config, true);

  const usedImages = messages.some((m) => Array.isArray(m?.content) && m.content.some((p) => p && p.type === "image_url"));
  pendingRequests.set(requestId, {
    type: "reply",
    sessionKey,
    createdAt: nbot.now(),
    usedImages,
    noImageRetry: false,
  });

  nbot.callLlmChat(requestId, messages, {
    modelName: config.replyModel,
    maxTokens: 96,
  });
}

// Call report model (supports web search)
function callReportModel(session, sessionKey, config) {
  pendingReportSessions.add(sessionKey);
  const requestId = genRequestId("report");
  pendingRequests.set(requestId, {
    type: "report",
    sessionKey,
    createdAt: nbot.now(),
  });

  // Treat report generation as activity to avoid accidental timeout cleanup.
  session.lastActivity = nbot.now();

  // Build conversation history text
  let conversationText = "对话记录：\n\n";
  for (const msg of session.messages) {
    const roleLabel = msg.role === "user" ? "用户" : "助手";
    conversationText += `${roleLabel}: ${msg.content}\n\n`;
  }

  const messages = [
    { role: "system", content: config.replySystemPrompt },
    { role: "user", content: conversationText + "\n\n" + config.reportPrompt },
  ];

  session.state = "generating_report";

  // Use web search if enabled
  if (config.enableWebsearch) {
    nbot.callLlmChatWithSearch(requestId, messages, {
      modelName: config.websearchModel,
      maxTokens: 4096,
      enableSearch: true,
    });
  } else {
    nbot.callLlmChat(requestId, messages, {
      modelName: config.replyModel,
      maxTokens: 4096,
    });
  }
}

// End session and generate report
function endSessionWithReport(session, sessionKey, config) {
  // Treat control action as activity to avoid accidental timeout cleanup.
  session.lastActivity = nbot.now();

  if (session.messages.length < 2) {
    nbot.sendReply(session.userId, session.groupId || 0, "已结束本次对话。");
    endSession(sessionKey);
    return;
  }

  callReportModel(session, sessionKey, config);
}

// Handle decision result
function handleDecisionResult(requestInfo, success, content) {
  const { sessionKey, userId, groupId, message, mentioned, items, groupContext } = requestInfo;
  const config = getConfig();
  pendingDecisionSessions.delete(sessionKey);

  function maskSensitive(text) {
    return String(text || "")
      // mask long digit sequences (QQ/IDs/etc)
      .replace(/\d{5,}/g, "***")
      .replace(/\s+/g, " ")
      .trim();
  }

  function parseDecision(raw) {
    const text = String(raw || "").trim();
    if (!text) return { decision: "NO", confidence: 0, reason: "" };

    const direct = text.toUpperCase();
    if (direct === "YES" || direct === "NO") {
      return { decision: direct, confidence: 1, reason: "direct" };
    }

    const fenced = text.match(/```(?:json)?\s*([\s\S]*?)```/i);
    const candidate = (fenced ? fenced[1] : text).trim();

    const tryParseJson = (s) => {
      if (!s) return null;
      const t = String(s).trim();
      if (!(t.startsWith("{") && t.endsWith("}"))) return null;
      try {
        const obj = JSON.parse(t);
        const decision = String(obj.decision || obj.answer || "").trim().toUpperCase();
        const confidence = Number(obj.confidence);
        const reason = String(obj.reason || "").trim();
        return {
          decision: decision === "YES" ? "YES" : "NO",
          confidence: Number.isFinite(confidence) ? Math.max(0, Math.min(1, confidence)) : 0,
          reason,
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
      return { decision: token === "YES" ? "YES" : "NO", confidence: 0.9, reason: "heuristic_token" };
    }
    const m2 = candidate.match(/decision\s*[:=]\s*(yes|no)/i);
    if (m2 && m2[1]) {
      const token = String(m2[1]).toUpperCase();
      return { decision: token === "YES" ? "YES" : "NO", confidence: 0.9, reason: "heuristic_decision" };
    }

    // Strict mode: any other non-JSON response is treated as NO (avoid false positives).
    nbot.log.warn(
      `[smart-assist] decision parse failed mentioned=${mentioned ? "Y" : "N"} raw=${maskSensitive(text).slice(0, 220)}`
    );
    return { decision: "NO", confidence: 0, reason: "non_json" };
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
  if (existing) {
    return;
  }

  const parsed = parseDecision(content);

  // If the model didn't follow the strict JSON format, retry once with a stronger instruction.
  if (parsed.reason === "non_json" && !requestInfo.formatRetry) {
    const stronger = [
      config.decisionSystemPrompt,
      "",
      "你上一条输出不符合格式。再次强调：只允许输出单行 JSON，且必须以 { 开头、以 } 结尾；除此之外禁止任何字符。",
      "示例：{\"decision\":\"NO\",\"confidence\":0.0,\"reason\":\"不确定\"}",
    ].join("\n");

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

  // Final fallback: if mentioned but still non-JSON, treat as YES so the bot doesn't look broken.
  const needsHelp = parsed.decision === "YES" || (mentioned && parsed.reason === "non_json");

  nbot.log.info(
    `[smart-assist] decision=${parsed.decision} conf=${parsed.confidence.toFixed(2)} triggered=${needsHelp ? "Y" : "N"} mentioned=${mentioned ? "Y" : "N"} reason=${parsed.reason || "-"} text=${maskSensitive(sanitizeMessageForLlm(String(message || ""), null)).slice(0, 80)}`
  );

  if (!needsHelp) {
    const batch = decisionBatches.get(sessionKey);
    if (batch && batch.items.length) {
      const urgent = batch.items.some((x) => !!x?.mentioned);
      scheduleDecisionFlush(sessionKey, urgent, config);
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
    mentionUserOnFirstReply: true,
  });
  session.groupContext = groupContext || null;
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
  callReplyModel(session, sessionKey, config);
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

  if (!success) {
    // If model/provider doesn't support image_url, retry once without images.
    if (requestInfo && requestInfo.usedImages && !requestInfo.noImageRetry) {
      pendingReplySessions.add(sessionKey);
      const requestId = genRequestId("reply");
      const retryMessages = buildReplyMessages(session, sessionKey, config, false);
      pendingRequests.set(requestId, {
        type: "reply",
        sessionKey,
        createdAt: nbot.now(),
        usedImages: false,
        noImageRetry: true,
      });
      nbot.callLlmChat(requestId, retryMessages, {
        modelName: config.replyModel,
        maxTokens: 160,
      });
      return;
    }

    nbot.sendReply(session.userId, session.groupId || 0, "出错了，稍后再试。");
    endSession(sessionKey);
    return;
  }

  // Add assistant reply to session
  let cleaned = formatOneLinePlain(
    String(content || "")
    .replace(/\s+@(?:群主|管理员|全体|all|everyone|here)\b/g, "")
    .replace(/^(?:@(?:群主|管理员|全体|all|everyone|here)\b\s*)+/g, "")
    .trim(),
    config.replyMaxChars
  );
  if (!cleaned) {
    // If cleaning removed everything, ask the model again rather than hard-coding a reply.
    pendingReplySessions.add(sessionKey);
    const requestId = genRequestId("reply");
    pendingRequests.set(requestId, {
      type: "reply",
      sessionKey,
      createdAt: nbot.now(),
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
      maxTokens: 80,
    });
    return;
  }
  addMessageToSession(session, "assistant", cleaned);
  session.turnCount++;

  // Send reply (hide counters; keep session limits internal)
  let prefix = "";
  if (session.mentionUserOnFirstReply) {
    prefix = nbot.at(session.userId) ? `${nbot.at(session.userId)} ` : "";
    session.mentionUserOnFirstReply = false;
  }
  nbot.sendReply(session.userId, session.groupId || 0, `${prefix}${cleaned}`);

  // Check if max turns reached (silent end; avoid spamming in QQ group)
  if (session.turnCount >= config.maxTurns) {
    endSession(sessionKey);
    return;
  }

  // If user sent more messages while we were waiting for this reply, respond once more with latest context.
  if (session.needsReply && session.state === "active") {
    session.needsReply = false;
    callReplyModel(session, sessionKey, config);
  }
}

// Handle report result
function handleReportResult(requestInfo, success, content) {
  const { sessionKey } = requestInfo;
  pendingReportSessions.delete(sessionKey);

  const session = sessions.get(sessionKey);
  const config = getConfig();

  if (!session) {
    nbot.log.warn("Session not found");
    return;
  }

  if (!success) {
    nbot.sendReply(
      session.userId,
      session.groupId || 0,
      "分析报告生成失败，请稍后再试。"
    );
    endSession(sessionKey);
    return;
  }

  function splitReportParts(raw) {
    const text = String(raw || "");
    const mdSep = "===MARKDOWN===";
    const copySep = "===COPY===";

    const mdIdx = text.indexOf(mdSep);
    const copyIdx = text.indexOf(copySep);

    if (mdIdx !== -1 && copyIdx !== -1 && copyIdx > mdIdx) {
      const markdown = text.slice(mdIdx + mdSep.length, copyIdx).trim();
      const copy = text.slice(copyIdx + copySep.length).trim();
      return { markdown, copy };
    }

    return { markdown: text.trim(), copy: "" };
  }

  const parts = splitReportParts(content);
  const markdownReport = parts.markdown || "";
  let copyText = parts.copy || "";
  if (!copyText.trim() && markdownReport) {
    copyText = markdownReport;
  }

  const now = new Date();
  const meta = `轮数：${session.turnCount}  时间：${now.toLocaleString()}`;
  const title = `${config.botName} 分析报告`;
  const reportImageBase64 = markdownReport
    ? nbot.renderMarkdownImage(title, meta, markdownReport, 720)
    : "";

  const nodes = [
    {
      name: config.botName,
      content: `【${config.botName} 分析报告】\n${meta}`,
    },
  ];

  // Add conversation history summary
  let historyContent = "【对话摘要】\n\n";
  for (const msg of session.messages) {
    const roleLabel = msg.role === "user" ? "用户" : "助手";
    const shortContent =
      msg.content.length > 200
        ? msg.content.substring(0, 200) + "..."
        : msg.content;
    historyContent += `${roleLabel}: ${shortContent}\n\n`;
  }
  nodes.push({ name: config.botName, content: historyContent });

  if (reportImageBase64) {
    nodes.push({
      name: config.botName,
      content: `【图文版】\n\n[CQ:image,file=base64://${reportImageBase64}]`,
    });
  } else if (markdownReport) {
    const maxNodeLength = 1800;
    const full = `【图文版（文本回退）】\n\n${markdownReport}`;
    const chunks = [];
    let remaining = full;
    while (remaining.length > 0) {
      chunks.push(remaining.substring(0, maxNodeLength));
      remaining = remaining.substring(maxNodeLength);
    }
    chunks.forEach((chunk, idx) => {
      nodes.push({
        name: config.botName,
        content:
          chunks.length === 1
            ? chunk
            : `【图文版 ${idx + 1}/${chunks.length}】\n\n${chunk}`,
      });
    });
  }

  if (copyText.trim()) {
    const maxNodeLength = 1800;
    const full = `【可复制版】\n\n${copyText.trim()}`;
    const chunks = [];
    let remaining = full;
    while (remaining.length > 0) {
      chunks.push(remaining.substring(0, maxNodeLength));
      remaining = remaining.substring(maxNodeLength);
    }
    chunks.forEach((chunk, idx) => {
      nodes.push({
        name: config.botName,
        content:
          chunks.length === 1
            ? chunk
            : `【可复制版 ${idx + 1}/${chunks.length}】\n\n${chunk}`,
      });
    });
  }

  // Send forward message
  nbot.sendForwardMessage(session.userId, session.groupId || 0, nodes);

  // Cleanup session and update cooldown
  endSession(sessionKey);
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
    } else if (info?.type === "report") {
      pendingReportSessions.delete(sessionKey);
      const session = sessions.get(sessionKey);
      if (session) {
        nbot.sendReply(session.userId, session.groupId || 0, "分析报告生成超时，请稍后再试。");
        endSession(sessionKey);
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
    nbot.log.info("Smart Assistant Plugin v2.2.17 enabled");
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
      if (imageUrls.length) {
        noteRecentGroupImages(group_id, imageUrls);
        noteRecentUserImages(sessionKey, imageUrls);
        if (session) {
          session.lastImageUrls = imageUrls;
          session.lastImageAt = nbot.now();
        }
      }

      // If active session exists
      if (session && session.state === "active") {
        // Check interrupt keywords
        if (containsKeyword(message, config.interruptKeywords)) {
          nbot.sendReply(user_id, group_id, "好的，已结束本次对话。");
          endSession(sessionKey);
          return true;
        }

        // Check early analysis keywords
        if (containsKeyword(message, config.earlyAnalysisKeywords)) {
          endSessionWithReport(session, sessionKey, config);
          return true;
        }

        // Continue conversation
        addMessageToSession(session, "user", llmMessage || message);
        if (pendingReplySessions.has(sessionKey)) {
          session.needsReply = true;
        } else {
          callReplyModel(session, sessionKey, config);
        }
        return true;
      }

      // If session is generating report, ignore message
      if (session && session.state === "generating_report") {
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
      case "report":
        handleReportResult(requestInfo, success, content);
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
