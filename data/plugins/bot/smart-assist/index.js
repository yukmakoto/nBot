/**
 * nBot Smart Assistant Plugin v2.1.3
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

// Last decision check time Map<sessionKey, ts>
const decisionLastCheck = new Map();

// Sessions with pending decision/context requests (avoid spamming)
const pendingDecisionSessions = new Set();
const pendingContextSessions = new Set();
const pendingReplySessions = new Set();
const pendingReportSessions = new Set();

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
    "你是群聊中的智能助手触发器。请判断用户这句话是否明确在向机器人求助/需要帮助。\n\n只允许输出：YES 或 NO。\n- YES：用户在寻求帮助/需要解释/需要方案/需要排查。\n- NO：闲聊、无意义信息、转发、表情、无明确需求。\n\n不要输出任何额外文字。";

  const replySystemPrompt =
    cfg.reply_system_prompt ||
    "你是群聊中的智能助手。目标：快速澄清需求、给出可执行建议。\n\n要求：\n- 用中文回答。\n- 先问 1-2 个关键澄清问题（如果信息不足）。\n- 如果可以直接解决，给出步骤化方案。\n- 不要泄露任何密钥/Token。";

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
    decisionCheckIntervalMs: (cfg.decision_check_interval_seconds || 20) * 1000,
    requestTimeoutMs: (cfg.request_timeout_seconds || 90) * 1000,
    contextTimeoutMs: (cfg.context_timeout_seconds || 15) * 1000,
    autoTrigger: cfg.auto_trigger !== false,
    autoTriggerMode: cfg.auto_trigger_mode || "mention",
    triggerMinLength: cfg.trigger_min_length || 12,
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
    contextMessageCount: cfg.context_message_count || 20,
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

function looksLikeHelpRequest(text, minLength) {
  const t = String(text || "").trim();
  if (!t) return false;
  if (t.startsWith("/")) return false;
  if (t.length < (minLength || 0)) return false;
  if (/[?？]/.test(t)) return true;
  return /(怎么|如何|为什么|为啥|是什么|怎么办|求助|帮忙|帮助|help)/i.test(t);
}

// Decide whether to run the decision model for a message.
// Goal: avoid keyword-based monitoring by default; only check when necessary (e.g. user @ bot).
// Modes (auto_trigger_mode):
// - mention: only when user @ bot (default)
// - mention_or_question: @ bot OR contains '?'/'？' and length >= trigger_min_length
// - all: any message with length >= trigger_min_length
// - legacy_keyword: old behavior (mention OR looksLikeHelpRequest)
function shouldCheckDecision(ctx, message, config) {
  if (!config.autoTrigger) return false;

  const t = String(message || "").trim();
  if (!t) return false;
  if (t.startsWith("/")) return false;

  const mode = String(config.autoTriggerMode || "mention").trim() || "mention";
  const mentioned = isMentioningBot(ctx);
  const minLen = config.triggerMinLength || 0;
  const longEnough = t.length >= minLen;
  const hasQuestionMark = /[?？]/.test(t);

  switch (mode) {
    case "mention":
      return mentioned;
    case "mention_or_question":
      return mentioned || (longEnough && hasQuestionMark);
    case "all":
      return longEnough;
    case "legacy_keyword":
      return mentioned || looksLikeHelpRequest(t, minLen);
    default:
      return mentioned;
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
function createSession(sessionKey, userId, groupId, initialMessage) {
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
function fetchGroupContext(sessionKey, userId, groupId, message, config) {
  const requestId = genRequestId("context");
  pendingGroupInfoRequests.set(requestId, {
    type: "context",
    sessionKey,
    userId,
    groupId,
    message,
    createdAt: nbot.now(),
    step: "notice", // Start with fetching notice
    notice: null,
    history: null,
  });

  // First fetch group announcements
  nbot.fetchGroupNotice(requestId, groupId);
}

// Call decision model
function callDecisionModel(sessionKey, userId, groupId, message, config, groupContext) {
  const requestId = genRequestId("decision");
  pendingDecisionSessions.add(sessionKey);
  pendingRequests.set(requestId, {
    type: "decision",
    sessionKey,
    userId,
    groupId,
    message,
    createdAt: nbot.now(),
  });

  // Build context-aware prompt
  let contextInfo = "";
  if (groupContext) {
    if (groupContext.notice && groupContext.notice.length > 0) {
      contextInfo += "\n\n【群公告】\n";
      groupContext.notice.slice(0, 3).forEach((n, i) => {
        const content = n.msg?.text || n.message?.text || "";
        if (content) {
          contextInfo += `${i + 1}. ${content.substring(0, 200)}\n`;
        }
      });
    }
    if (groupContext.history && groupContext.history.length > 0) {
      contextInfo += "\n\n【用户近期群消息】\n";
      const userMessages = groupContext.history
        .filter(m => m.sender?.user_id === userId)
        .slice(0, 5);
      userMessages.forEach((m, i) => {
        const content = m.raw_message || "";
        if (content) {
          contextInfo += `${i + 1}. ${content.substring(0, 100)}\n`;
        }
      });
    }
  }

  const messages = [
    { role: "system", content: config.decisionSystemPrompt },
    { role: "user", content: `用户消息：${message}${contextInfo}` },
  ];

  nbot.callLlmChat(requestId, messages, {
    modelName: config.decisionModel,
    maxTokens: 10,
  });
}

// Call reply model
function callReplyModel(session, sessionKey, config) {
  pendingReplySessions.add(sessionKey);
  const requestId = genRequestId("reply");
  pendingRequests.set(requestId, {
    type: "reply",
    sessionKey,
    createdAt: nbot.now(),
  });

  const messages = [
    { role: "system", content: config.replySystemPrompt },
    ...session.messages,
  ];

  nbot.callLlmChat(requestId, messages, {
    modelName: config.replyModel,
    maxTokens: 1024,
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

  nbot.sendReply(
    session.userId,
    session.groupId || 0,
    config.enableWebsearch
      ? "正在联网搜索并生成分析报告，请稍候..."
      : "正在生成分析报告，请稍候..."
  );
  callReportModel(session, sessionKey, config);
}

// Handle decision result
function handleDecisionResult(requestInfo, success, content) {
  const { sessionKey, userId, groupId, message } = requestInfo;
  const config = getConfig();
  pendingDecisionSessions.delete(sessionKey);

  if (!success) {
    nbot.log.warn(`Decision model call failed: ${content}`);
    return;
  }

  const existing = sessions.get(sessionKey);
  if (existing) {
    return;
  }

  const decision = content.trim().toUpperCase();
  const needsHelp = decision === "YES" || decision.startsWith("YES");

  if (!needsHelp) {
    return;
  }

  // Check cooldown (from last session cleanup)
  if (!checkCooldown(sessionKey, config.cooldownMs)) {
    nbot.log.info(`User ${userId} is in cooldown, skipping`);
    return;
  }

  // Create new session
  const session = createSession(sessionKey, userId, groupId, message);
  addMessageToSession(session, "user", message);

  nbot.log.info(`Created new session for user ${userId}: ${sessionKey}`);

  // Start assisting immediately
  nbot.sendReply(userId, groupId, "智能助手已启动，正在思考中...");
  callReplyModel(session, sessionKey, config);
}

// Handle reply result
function handleReplyResult(requestInfo, success, content) {
  const { sessionKey } = requestInfo;
  pendingReplySessions.delete(sessionKey);

  const session = sessions.get(sessionKey);
  const config = getConfig();

  if (!session) {
    nbot.log.warn(`Session not found: ${sessionKey}`);
    return;
  }

  if (!success) {
    nbot.sendReply(
      session.userId,
      session.groupId || 0,
      "抱歉，发生错误，请稍后再试。"
    );
    endSession(sessionKey);
    return;
  }

  // Add assistant reply to session
  addMessageToSession(session, "assistant", content);
  session.turnCount++;

  // Send reply with remaining turns
  const remaining = config.maxTurns - session.turnCount;
  const interruptHint = (config.interruptKeywords && config.interruptKeywords[0]) || "我明白了";
  const earlyHint = (config.earlyAnalysisKeywords && config.earlyAnalysisKeywords[0]) || "这就是我想说的";
  const controlHint =
    session.turnCount === 1
      ? `\n\n（可回复「${interruptHint}」结束，回复「${earlyHint}」生成报告）`
      : "";
  const replyWithRemaining = `${content}\n\n（剩余对话次数：${remaining}）${controlHint}`;
  nbot.sendReply(session.userId, session.groupId || 0, replyWithRemaining);

  // Check if max turns reached
  if (session.turnCount >= config.maxTurns) {
    nbot.sendReply(
      session.userId,
      session.groupId || 0,
      `已达到最大对话轮数（${config.maxTurns}），正在生成分析报告...`
    );
    endSessionWithReport(session, sessionKey, config);
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
    nbot.log.warn(`Session not found: ${sessionKey}`);
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
  const meta = `用户：${session.userId}  轮数：${session.turnCount}  时间：${now.toLocaleString()}`;
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
  const { sessionKey, userId, groupId, message, step } = requestInfo;
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
    callDecisionModel(sessionKey, userId, groupId, message, config, groupContext);
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
      callDecisionModel(sessionKey, info.userId, info.groupId, info.message, config, null);
    }

    nbot.log.warn(`Context timeout: ${requestId}`);
  }
}

// Generate help image
function sendHelpImage(userId, groupId) {
  const config = getConfig();
  const helpMarkdown = `# 智能助手使用指南

## 自动触发
默认仅在「@ 机器人」时才会触发判定并进入多轮对话（更像“人类助手”：你叫我我才来）。

可通过配置项 \`auto_trigger_mode\` 调整触发范围：
- \`mention\`：仅 @ 机器人（默认）
- \`mention_or_question\`：@ 或包含问号且长度达标
- \`all\`：所有群消息（长度达标）
- \`legacy_keyword\`：旧逻辑（@ 或关键词/疑问）

## 多轮对话
进入对话后，继续发送消息即可；每次回复会提示剩余对话次数。

## 中断对话（不生成报告）
发送以下任一关键词：
${config.interruptKeywords.map((k) => "- " + k).join("\n")}

## 提前生成报告
发送以下任一关键词：
${config.earlyAnalysisKeywords.map((k) => "- " + k).join("\n")}

## 当前配置
- 最大对话轮数：${config.maxTurns}
- 会话超时：${config.sessionTimeoutMs / 60000} 分钟
- 冷却时间：${config.cooldownMs / 1000} 秒
- 判定检测间隔：${config.decisionCheckIntervalMs / 1000} 秒
- 自动触发：${config.autoTrigger ? "开启" : "关闭"}
- 自动触发模式：${config.autoTriggerMode}
- 联网搜索：${config.enableWebsearch ? "开启" : "关闭"}
`;

  const title = "智能助手";
  const meta = "nBot Plugin v2.1.3";
  const imageBase64 = nbot.renderMarkdownImage(title, meta, helpMarkdown, 600);

  if (imageBase64) {
    nbot.sendReply(userId, groupId, `[CQ:image,file=base64://${imageBase64}]`);
  } else {
    nbot.sendReply(userId, groupId, helpMarkdown);
  }
}

// Plugin object
return {
  onEnable() {
    nbot.log.info("Smart Assistant Plugin v2.1.3 enabled");
  },

  onDisable() {
    sessions.clear();
    cooldowns.clear();
    pendingRequests.clear();
    pendingGroupInfoRequests.clear();
    nbot.log.info("Smart Assistant Plugin disabled");
  },

  // Monitor each message
  preMessage(ctx) {
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
      addMessageToSession(session, "user", message);
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
    const shouldCheck =
      checkCooldown(sessionKey, config.cooldownMs) &&
      shouldCheckDecision(ctx, message, config);

    const now = nbot.now();
    const lastCheck = decisionLastCheck.get(sessionKey) || 0;
    const checkIntervalOk = now - lastCheck >= config.decisionCheckIntervalMs;

    if (
      shouldCheck &&
      checkIntervalOk &&
      !pendingDecisionSessions.has(sessionKey) &&
      !pendingContextSessions.has(sessionKey)
    ) {
      decisionLastCheck.set(sessionKey, now);
      // Fetch group context first if enabled, then call decision model
      if (config.fetchGroupContext) {
        pendingContextSessions.add(sessionKey);
        fetchGroupContext(sessionKey, user_id, group_id, message, config);
      } else {
        callDecisionModel(sessionKey, user_id, group_id, message, config, null);
      }
    }

    return true;
  },

  // Handle commands
  onCommand(ctx) {
    const { command, user_id, group_id, args } = ctx;

    if (command !== "smart-assist" && command !== "智能助手") {
      return;
    }

    const gid = group_id || 0;
    const config = getConfig();

    // Sub-command handling
    const subCmd = args && args.length > 0 ? args[0] : "";

    if (subCmd === "start" || subCmd === "开始") {
      if (!gid) {
        nbot.sendReply(user_id, 0, "该命令仅限群聊使用。");
        return;
      }

      const sessionKey = `${gid}:${user_id}`;
      if (!checkCooldown(sessionKey, config.cooldownMs)) {
        nbot.sendReply(user_id, gid, "你刚结束过一轮会话，请稍后再试。");
        return;
      }

      const existing = sessions.get(sessionKey);
      if (existing && existing.state === "active") {
        nbot.sendReply(user_id, gid, "会话已在进行中。");
        return;
      }
      if (existing && existing.state === "generating_report") {
        nbot.sendReply(user_id, gid, "正在生成报告，请稍候...");
        return;
      }

      const initial = (args || []).slice(1).join(" ").trim();
      const session = createSession(sessionKey, user_id, gid, initial);
      if (initial) {
        addMessageToSession(session, "user", initial);
        callReplyModel(session, sessionKey, config);
      } else {
        const greeting = config.greetingTemplate.replace(
          "{remaining}",
          String(config.maxTurns)
        );
        nbot.sendReply(user_id, gid, greeting);
      }
      return;
    }

    if (subCmd === "status" || subCmd === "状态") {
      const sessionKey = `${gid}:${user_id}`;
      const session = sessions.get(sessionKey);
      if (session) {
        nbot.sendReply(
          user_id,
          gid,
          `当前会话状态：\n- 轮数：${session.turnCount}/${config.maxTurns}\n- 状态：${session.state}`
        );
      } else {
        nbot.sendReply(user_id, gid, "当前没有进行中的会话。");
      }
      return;
    }

    if (subCmd === "end" || subCmd === "结束") {
      const sessionKey = `${gid}:${user_id}`;
      const session = sessions.get(sessionKey);
      if (session) {
        if (session.state === "active") {
          endSessionWithReport(session, sessionKey, config);
        } else {
          nbot.sendReply(user_id, gid, "会话正在处理中，请稍候...");
        }
      } else {
        nbot.sendReply(user_id, gid, "当前没有进行中的会话。");
      }
      return;
    }

    if (subCmd === "help" || subCmd === "帮助" || !subCmd) {
      sendHelpImage(user_id, gid);
      return;
    }

    nbot.sendReply(user_id, gid, '未知子命令：使用 "/smart-assist help" 查看帮助');
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
