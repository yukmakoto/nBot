import { getConfig } from "../config.js";
import { buildRecentGroupSnippet } from "../media.js";
import { isMentioningBot, summarizeMentions } from "../message.js";
import {
  genRequestId,
  pendingContextSessions,
  pendingGroupInfoRequests,
  pendingDecisionSessions,
  pendingRequests,
  recentGroupImages,
  sessions,
} from "../state.js";
import { stripAllCqSegments, stripLeadingCqSegments } from "../utils/text.js";

function looksLikeInScopeHelpRequest(text) {
  const raw = String(text || "");
  const t = stripAllCqSegments(raw).toLowerCase();
  if (!t) return false;

  // Fast allow-list for the assistant's scope (avoid running the LLM router on generic chat).
  const keywordHits = [
    "minecraft",
    "我的世界",
    "mc",
    "pcl",
    "java",
    "bedrock",
    "基岩",
    "键位",
    "按键",
    "快捷键",
    "输入法",
    "forge",
    "fabric",
    "mod",
    "optifine",
    "sodium",
    "rubidium",
    "lwjgl",
    "crash",
    "exception",
    "stack",
    "error",
    "log.txt",
    "crash-reports",
    "村民",
    "繁殖",
    "繁衍",
    "末影珍珠",
    "红石",
    "附魔",
    "光影",
    "全英文",
    "语言",
  ].some((k) => t.includes(k));
  if (keywordHits) return true;

  // Common Chinese troubleshooting patterns.
  if (
    /(?:报错|错误|崩溃|闪退|卡死|无响应|打不开|开不了|启动不了|启动器|进不去|连不上|日志|存档|模组|整合包|服务端|服务器|村民|繁殖|繁衍|语言|全英文)/u.test(
      raw
    )
  ) {
    return true;
  }

  return false;
}

export function getDecisionTrigger(ctx, message, config) {
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

  // Only run the router LLM on likely in-scope help requests.
  // Note: being @mentioned is NOT a valid reason to join casual chat; it only upgrades urgency
  // when the message itself is within scope (or when media/reply-to-bot is present, handled by caller).
  const inScope = looksLikeInScopeHelpRequest(t);
  return { shouldCheck: inScope, mentioned, urgent: mentioned && inScope };
}

// Fetch group context (announcements and recent messages)
export function fetchGroupContext(sessionKey, userId, groupId, message, mentioned, items, config, selfId = "") {
  const requestId = genRequestId("context");
  pendingGroupInfoRequests.set(requestId, {
    type: "context",
    sessionKey,
    userId,
    groupId,
    message,
    mentioned: !!mentioned,
    items: Array.isArray(items) ? items : [],
    selfId: selfId !== undefined && selfId !== null ? String(selfId) : "",
    createdAt: nbot.now(),
    step: "notice", // Start with fetching notice
    notice: null,
    history: null,
  });

  // First fetch group announcements
  nbot.fetchGroupNotice(requestId, groupId);
}

// Call decision model
export function callDecisionModel(
  sessionKey,
  userId,
  groupId,
  message,
  mentioned,
  items,
  config,
  groupContext,
  options = {}
) {
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
    maxTokens: config.decisionMaxTokens ?? null,
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
        .filter((m) => String(m?.sender?.user_id ?? "") === uidStr)
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
        `是否回复他人：${
          Array.isArray(items) && items.some((x) => x && x.isReply && !x.replyToBot) ? "是" : "否"
        }`,
        `是否回复机器人：${
          Array.isArray(items) && items.some((x) => x && x.isReply && x.replyToBot) ? "是" : "否"
        }`,
        `是否 @ 他人：${
          Array.isArray(items) && items.some((x) => x && x.mentionedOther) ? "是" : "否"
        }`,
        `是否 @ 全体：${
          Array.isArray(items) && items.some((x) => x && x.mentionedAll) ? "是" : "否"
        }`,
        `是否处于会话中：${sessions.get(sessionKey)?.state === "active" ? "是" : "否"}`,
        "",
        "候选消息（按时间）：",
        message,
        contextInfo ? `\n${contextInfo}` : "",
      ].join("\n"),
    },
  ];

  const callOptions = { modelName: config.decisionModel };
  if (config.decisionMaxTokens) callOptions.maxTokens = config.decisionMaxTokens;
  nbot.callLlmChat(requestId, messages, callOptions);
}

export function handleGroupInfoResponse(requestInfo, infoType, success, data) {
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
      selfId: requestInfo.selfId || "",
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
