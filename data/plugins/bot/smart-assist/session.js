import { getConfig } from "./config.js";
import {
  cooldowns,
  decisionBatches,
  pendingContextSessions,
  pendingDecisionSessions,
  pendingGroupInfoRequests,
  pendingReplySessions,
  pendingRequests,
  sessions,
} from "./state.js";

// Check cooldown (cooldown starts from session cleanup)
export function checkCooldown(sessionKey, cooldownMs) {
  const now = nbot.now();
  const lastCleanupTime = cooldowns.get(sessionKey);
  if (lastCleanupTime && now - lastCleanupTime < cooldownMs) {
    return false;
  }
  return true;
}

// Update cooldown (called when session is cleaned up)
export function updateCooldown(sessionKey) {
  cooldowns.set(sessionKey, nbot.now());
}

// Cleanup expired sessions (silent; don't spam in group)
export function cleanupExpiredSessions(timeoutMs) {
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

// Create new session
export function createSession(sessionKey, userId, groupId, initialMessage, options = {}) {
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
    mentionUserOnEveryReply: !!options.mentionUserOnEveryReply,
    pendingUserInput: false, // User sent new messages while a reply was in-flight
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
export function addMessageToSession(session, role, content) {
  session.messages.push({ role, content });
  session.lastActivity = nbot.now();
}

// End session and update cooldown
export function endSession(sessionKey) {
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
