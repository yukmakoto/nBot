/**
 * nBot Smart Assistant Plugin v2.2.33
 * Auto-detects if user needs help, enters multi-turn conversation mode,
 * replies in a QQ-friendly style (short, low-noise)
 */

import { getConfig } from "./config.js";
import { handleDecisionResult } from "./decision/handlers.js";
import { flushDueDecisionBatches, scheduleDecisionFlush } from "./decision/batch.js";
import { getDecisionTrigger, handleGroupInfoResponse } from "./decision/decision_model.js";
import {
  extractImageUrlsFromCtx,
  extractRecordUrlsFromCtx,
  extractReplyMessageContext,
  extractVideoUrlsFromCtx,
  noteRecentGroupImages,
  noteRecentGroupRecords,
  noteRecentGroupVideos,
  noteRecentUserImages,
  noteRecentUserRecords,
  noteRecentUserVideos,
} from "./media.js";
import { sanitizeMessageForLlm, summarizeMentions } from "./message.js";
import { handleReplyResult } from "./reply/reply.js";
import { flushDueReplyBatches, scheduleReplyFlush } from "./reply/batch.js";
import { checkCooldown, cleanupExpiredSessions, addMessageToSession, createSession, endSession } from "./session.js";
import {
  resetAllState,
  decisionBatches,
  pendingGroupInfoRequests,
  pendingReplySessions,
  pendingRequests,
  sessions,
} from "./state.js";
import { cleanupStaleRequests } from "./timeouts.js";
import { containsKeyword, stripAllCqSegments } from "./utils/text.js";

function looksLikeShortClarifyAnswer(text) {
  const s = stripAllCqSegments(String(text || "")).trim();
  if (!s) return false;
  if (s.length > 10) return false;

  const t = s.toLowerCase();
  const direct = [
    "java",
    "java版",
    "bedrock",
    "基岩",
    "基岩版",
    "fabric",
    "forge",
    "paper",
    "spigot",
    "win",
    "windows",
    "mac",
    "linux",
    "ios",
    "android",
    "pc",
    "电脑版",
    "手机",
    "是",
    "不是",
    "不",
    "对",
    "不对",
    "可以",
    "不可以",
    "能",
    "不能",
  ];
  if (direct.includes(t)) return true;

  // Version-like answers: 1.20.1 / 25w41a
  if (/^\d+\.\d+(?:\.\d+)?(?:[-_][0-9a-z]+)?$/i.test(t)) return true;
  if (/^\d{2}w\d{2}[a-z]$/i.test(t)) return true;

  return false;
}

export default {
  onEnable() {
    nbot.log.info("Smart Assistant Plugin v2.2.33 enabled");
  },

  onDisable() {
    resetAllState();
    nbot.log.info("Smart Assistant Plugin disabled");
  },

  // Backend tick event: used to implement 5-second message merge without JS timers.
  async onMetaEvent(ctx) {
    try {
      if (!ctx || ctx.meta_event_type !== "tick") return true;
      const config = getConfig();
      flushDueDecisionBatches(config);
      flushDueReplyBatches(config);
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
      const selfId = ctx.self_id !== undefined && ctx.self_id !== null ? String(ctx.self_id) : "";
      const mentions = summarizeMentions(ctx);
      const imageUrls = extractImageUrlsFromCtx(ctx);
      const videoUrls = extractVideoUrlsFromCtx(ctx);
      const recordUrls = extractRecordUrlsFromCtx(ctx);
      const replyCtx = extractReplyMessageContext(ctx);
      const hasMedia = !!(imageUrls.length || videoUrls.length || recordUrls.length);
      const hasFile =
        String(llmMessage || message).includes("[文件]") || /\[CQ:file[,\]]/i.test(String(message || ""));
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
      if (videoUrls.length || recordUrls.length) {
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

        // Continue conversation (store context).
        addMessageToSession(session, "user", llmMessage || message, { mentioned: !!mentions.bot });

        // In an active session, default to replying every user turn (otherwise it feels "dead").
        if (config.alwaysReplyInSession && !session.passive) {
          if (pendingReplySessions.has(sessionKey)) {
            session.pendingUserInput = true;
            scheduleReplyFlush(sessionKey, config);
            return true;
          }
          scheduleReplyFlush(sessionKey, config);
          return true;
        }

        // Session follow-up: reply-thread is a strong signal the user is talking to the bot.
        const repliedToBot = !!replyCtx?.replyToBot;
        if (repliedToBot) {
          if (pendingReplySessions.has(sessionKey)) {
            session.pendingUserInput = true;
          }
          scheduleReplyFlush(sessionKey, config);
          return true;
        }

        // Fallback: let the decision model decide whether we should reply to this new message.
        // Important: do NOT route every single message in an active session (it makes the bot "chatty");
        // only route when there is a strong signal the user is still asking for help.
        const trigger = getDecisionTrigger(ctx, message, config);
        const clarifyFollowupWindowMs = 2 * 60 * 1000;
        const recentBotQuestion =
          !!session.lastAssistantAt && nbot.now() - Number(session.lastAssistantAt || 0) <= clarifyFollowupWindowMs;
        const urgentFollowup =
          (session.passive && looksLikeShortClarifyAnswer(message)) ||
          (recentBotQuestion && looksLikeShortClarifyAnswer(message));
        const shouldRoute = trigger.shouldCheck || hasMedia || hasFile || urgentFollowup;
        if (!shouldRoute) {
          return true;
        }

        let batch = decisionBatches.get(sessionKey);
        if (!batch) {
          batch = { userId: user_id, groupId: group_id, items: [] };
          decisionBatches.set(sessionKey, batch);
        }
        batch.userId = user_id;
        batch.groupId = group_id;
        batch.selfId = selfId || batch.selfId || "";
        batch.items.push({
          t: nbot.now(),
          text: sanitizeMessageForLlm(message, ctx),
          mentioned: !!trigger.mentioned,
          mentionedOther: !!mentions.other,
          mentionedAll: !!mentions.all,
          isReply: !!replyCtx,
          replyToBot: !!replyCtx?.replyToBot,
          imageUrls,
          replySnippet: replyCtx ? replyCtx.snippet : "",
        });
        // Media updates should be handled promptly, but still go through the router to avoid redundant follow-ups
        // when another plugin (e.g. log analyzer) is already processing the same case.
        scheduleDecisionFlush(sessionKey, !!(trigger.urgent || hasMedia || hasFile || urgentFollowup), config);
        return true;
      }

      // No active session, decide whether to run decision model.
      const trigger = getDecisionTrigger(ctx, message, config);
      // Avoid inserting into reply threads unless the bot is explicitly involved.
      if (replyCtx && !replyCtx.replyToBot && !trigger.mentioned) {
        return true;
      }

      // Note: do not auto-reply on mention; still run through decision model to avoid the bot joining chat.
      // Mentions bypass cooldown but are flushed urgently (no merge wait).
      const bypass =
        !!replyCtx?.replyToBot || (trigger.mentioned && (hasMedia || hasFile));
      const shouldCheck =
        bypass || ((trigger.mentioned || checkCooldown(sessionKey, config.cooldownMs)) && trigger.shouldCheck);
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
        batch.selfId = selfId || batch.selfId || "";
        batch.items.push({
          t: nbot.now(),
          text: sanitizeMessageForLlm(message, ctx),
          mentioned: !!trigger.mentioned,
          mentionedOther: !!mentions.other,
          mentionedAll: !!mentions.all,
          isReply: !!replyCtx,
          replyToBot: !!replyCtx?.replyToBot,
          imageUrls,
          replySnippet: replyCtx ? replyCtx.snippet : "",
        });
        scheduleDecisionFlush(sessionKey, !!(trigger.urgent || bypass), config);
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
