/**
 * nBot Smart Assistant Plugin v2.2.27
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
import { callReplyModel, handleReplyResult } from "./reply/reply.js";
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
import { containsKeyword } from "./utils/text.js";

export default {
  onEnable() {
    nbot.log.info("Smart Assistant Plugin v2.2.27 enabled");
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
        addMessageToSession(session, "user", llmMessage || message);

        // In an active session, default to replying every user turn (otherwise it feels "dead").
        if (config.alwaysReplyInSession) {
          if (pendingReplySessions.has(sessionKey)) {
            session.pendingUserInput = true;
            return true;
          }
          callReplyModel(session, sessionKey, config, false);
          return true;
        }

        // Fallback: Let the decision model decide whether we should reply to this new message.
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
          mentionedOther: !!mentions.other,
          mentionedAll: !!mentions.all,
          isReply: !!replyCtx,
          replyToBot: !!replyCtx?.replyToBot,
          imageUrls,
          replySnippet: replyCtx ? replyCtx.snippet : "",
        });
        scheduleDecisionFlush(sessionKey, trigger.urgent, config);
        return true;
      }

      // No active session, decide whether to run decision model.
      const trigger = getDecisionTrigger(ctx, message, config);
      // Avoid inserting into reply threads unless the bot is explicitly involved.
      if (replyCtx && !replyCtx.replyToBot && !trigger.mentioned) {
        return true;
      }

      // If user explicitly @ the bot, reply immediately (avoid strict JSON/router failures causing silence).
      if (trigger.mentioned) {
        const seed = llmMessage || message || "";
        const s = createSession(sessionKey, user_id, group_id, seed, {
          mentionUserOnFirstReply: config.mentionUserOnFirstReply,
          mentionUserOnEveryReply: config.mentionUserOnEveryReply,
        });
        if (replyCtx && replyCtx.snippet) {
          s.lastReplySnippet = replyCtx.snippet;
          s.lastReplyAt = nbot.now();
        }
        addMessageToSession(s, "user", seed);
        nbot.log.info("[smart-assist] created new session (mentioned)");
        callReplyModel(s, sessionKey, config, false);
        return true;
      }

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
          mentionedOther: !!mentions.other,
          mentionedAll: !!mentions.all,
          isReply: !!replyCtx,
          replyToBot: !!replyCtx?.replyToBot,
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
