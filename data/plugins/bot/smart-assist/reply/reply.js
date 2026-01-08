import { getConfig } from "../config.js";
import {
  buildMultimodalImageMessage,
  getRelevantImageUrlsForSession,
  getRelevantRecordUrlsForSession,
  getRelevantVideoUrlsForSession,
  looksReferentialShortQuestion,
} from "../media.js";
import { genRequestId, pendingReplySessions, pendingRequests, sessions } from "../state.js";
import { addMessageToSession, endSession } from "../session.js";
import { escapeForLog } from "../utils/log.js";
import { stripAllCqSegments } from "../utils/text.js";

function buildReplyContextForPrompt(groupContext, userId) {
  if (!groupContext) return "";
  let contextInfo = "";
  if (groupContext.history && groupContext.history.length > 0) {
    const uidStr = String(userId);
    const userMessages = groupContext.history
      .filter((m) => String(m?.sender?.user_id ?? "") === uidStr)
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
    normalized.includes("||")
      ? normalized
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

export function callReplyModel(session, sessionKey, config, useSearch = false) {
  pendingReplySessions.add(sessionKey);
  const requestId = genRequestId("reply");
  if (session) {
    session.pendingUserInput = false;
  }
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
    maxTokens: config.replyMaxTokens ?? null,
  });

  if (useSearch && config.enableWebsearch) {
    const callOptions = { modelName: config.websearchModel, enableSearch: true };
    if (config.replyMaxTokens) callOptions.maxTokens = config.replyMaxTokens;
    nbot.callLlmChatWithSearch(requestId, messages, callOptions);
  } else {
    const callOptions = { modelName: config.replyModel };
    if (config.replyMaxTokens) callOptions.maxTokens = config.replyMaxTokens;
    nbot.callLlmChat(requestId, messages, callOptions);
  }
}

// Handle reply result
export function handleReplyResult(requestInfo, success, content) {
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
      `[smart-assist] reply_raw len=${rawLen} ctl=${hasControl ? "Y" : "N"} usedImages=${requestInfo.usedImages ? "Y" : "N"} model=${requestInfo.modelName || "-"} rid=${String(requestInfo.requestId || "").slice(0, 48)} raw=${escapeForLog(raw, 500)}`
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
        maxTokens: config.replyMaxTokens ?? null,
      });
      const callOptions = { modelName: config.replyModel };
      if (config.replyMaxTokens) callOptions.maxTokens = config.replyMaxTokens;
      nbot.callLlmChat(requestId, retryMessages, callOptions);
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
      maxTokens: config.replyRetryMaxTokens ?? null,
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

    const callOptions = { modelName: config.replyModel };
    if (config.replyRetryMaxTokens) callOptions.maxTokens = config.replyRetryMaxTokens;
    nbot.callLlmChat(requestId, retryMessages, callOptions);
    return;
  }

  const splitResult = splitQqReply(cleaned, config.replyMaxChars, config.replyMaxParts, config.replyPartsSeparator);
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
      compactRetry: true,
      maxTokens: config.replyRetryMaxTokens ?? null,
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

    const callOptions = { modelName: config.replyModel };
    if (config.replyRetryMaxTokens) callOptions.maxTokens = config.replyRetryMaxTokens;
    nbot.callLlmChat(requestId, retryMessages, callOptions);
    return;
  }

  const parts = splitResult.parts.length ? splitResult.parts : [cleaned];
  // Drop tiny fragments like "能/建议先" instead of hard-sending them.
  const stableParts = parts.length > 1 ? parts.filter((p) => String(p || "").trim().length >= 4) : parts;
  const finalParts = stableParts.length ? stableParts : parts;
  const allTiny = finalParts.length > 0 && finalParts.every((p) => String(p || "").trim().length < 4);
  if (allTiny) {
    if (!requestInfo.fragmentRetry) {
      pendingReplySessions.add(sessionKey);
      const requestId = genRequestId("reply");
      pendingRequests.set(requestId, {
        requestId,
        type: "reply",
        sessionKey,
        createdAt: nbot.now(),
        modelName: config.replyModel,
        fragmentRetry: true,
        maxTokens: config.replyRetryMaxTokens ?? null,
      });

      const retryMessages = [
        {
          role: "system",
          content:
            config.replySystemPrompt +
            "\n\n补充要求：你上一条输出无效；请只输出 1 条完整中文短句（同一行），长度 8~20 字；不要换行、不要 Markdown、不要「||」。",
        },
        ...session.messages,
      ];

      const callOptions = { modelName: config.replyModel };
      if (config.replyRetryMaxTokens) callOptions.maxTokens = config.replyRetryMaxTokens;
      nbot.callLlmChat(requestId, retryMessages, callOptions);
      return;
    }

    nbot.sendReply(session.userId, session.groupId || 0, "出错了，稍后再试。");
    endSession(sessionKey);
    return;
  }

  if (cleaned.length <= 12 || finalParts.length > 1 || finalParts.length !== parts.length) {
    nbot.log.info(
      `[smart-assist] reply_cleaned len=${cleaned.length} parts=${finalParts.length}/${config.replyMaxParts} maxChars=${config.replyMaxChars} usedImages=${requestInfo.usedImages ? "Y" : "N"} model=${requestInfo.modelName || "-"} rawLen=${rawLen} rid=${String(requestInfo.requestId || "").slice(0, 48)} cleaned=${escapeForLog(cleaned, 180)} raw=${escapeForLog(raw, 500)}`
    );
  }
  addMessageToSession(session, "assistant", finalParts.join(" "));
  session.turnCount++;

  // Send reply (hide counters; keep session limits internal)
  let prefix = "";
  const shouldMention = !!session.groupId && (session.mentionUserOnEveryReply || session.mentionUserOnFirstReply);
  if (shouldMention) {
    prefix = nbot.at(session.userId) ? `${nbot.at(session.userId)} ` : "";
    if (session.mentionUserOnFirstReply) {
      session.mentionUserOnFirstReply = false;
    }
  }
  finalParts.forEach((p, idx) => {
    const msg = idx === 0 ? `${prefix}${p}` : p;
    if (msg) nbot.sendReply(session.userId, session.groupId || 0, msg);
  });

  // Check if max turns reached (silent end; avoid spamming in QQ group)
  if (session.turnCount >= config.maxTurns) {
    endSession(sessionKey);
    return;
  }

  // If user sent more messages while we were replying, immediately continue.
  if (session.pendingUserInput && !pendingReplySessions.has(sessionKey)) {
    callReplyModel(session, sessionKey, config, false);
  }
}
