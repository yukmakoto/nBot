import { getConfig } from "../config.js";
import { callReplyModel } from "../reply/reply.js";
import { sanitizeMessageForLlm } from "../message.js";
import { decisionBatches, pendingDecisionSessions, pendingReplySessions, sessions } from "../state.js";
import { checkCooldown, createSession, addMessageToSession } from "../session.js";
import { maskSensitiveForLog } from "../utils/log.js";
import { scheduleDecisionFlush } from "./batch.js";
import { callDecisionModel } from "./decision_model.js";
import { stripAllCqSegments } from "../utils/text.js";

function looksLikeDirectHelpRequest(text) {
  const s = stripAllCqSegments(String(text || "")).trim();
  if (!s) return false;
  if (s.length < 4) return false;

  // Strong troubleshooting / help-seeking patterns.
  if (
    /(?:报错|错误|崩溃|闪退|卡死|无响应|打不开|开不了|进不去|连不上|掉线|延迟|日志|crash|exception|stack|error|fail)/iu.test(
      s
    )
  ) {
    return true;
  }

  // Common "asking for help" wording (avoid treating generic clarifying questions as help requests).
  if (/(?:怎么办|怎么解决|怎么弄|怎么搞|如何|为何|为什么|咋办|求助|求救|帮忙|救命|请问)/u.test(s)) {
    return true;
  }

  // General question marks, but require first-person / problem framing.
  if (/[?？]/.test(s) && /(?:我|我的|为啥|怎么|出问题|有问题)/u.test(s)) {
    return true;
  }

  return false;
}

// Handle decision result
export function handleDecisionResult(requestInfo, success, content) {
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

    // 4) partial JSON salvage: some upstreams return truncated JSON (e.g. finish_reason=length)
    // like `{"action":"IGNORE","confidence":0.9` (missing closing brace/fields).
    const actionMatch = candidate.match(/"action"\s*:\s*"([^"]+)"/i);
    if (actionMatch && actionMatch[1]) {
      const actionRaw = String(actionMatch[1]).trim().toUpperCase();
      const confMatch = candidate.match(/"confidence"\s*:\s*([0-9]+(?:\.[0-9]+)?)/i);
      const confidence = confMatch ? Number(confMatch[1]) : 0;
      const action =
        actionRaw === "REPLY" || actionRaw === "IGNORE" || actionRaw === "REACT" ? actionRaw : "IGNORE";
      return {
        action,
        confidence: Number.isFinite(confidence) ? Math.max(0, Math.min(1, confidence)) : 0,
        reason: "partial_json",
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
    // If the user seems to be asking for help but the router decided to IGNORE (often because others are already helping),
    // keep a passive session so we can follow up after they answer clarifying questions.
    if (!mentioned && !existing && looksLikeDirectHelpRequest(message)) {
      const seedItems =
        Array.isArray(items) && items.length
          ? items.map((x) => String(x?.text ?? ""))
          : message
            ? [sanitizeMessageForLlm(message, null)]
            : [];

      if (seedItems.length) {
        const primed = createSession(sessionKey, userId, groupId, seedItems[0] || "", {
          mentionUserOnFirstReply: config.mentionUserOnFirstReply,
          mentionUserOnEveryReply: config.mentionUserOnEveryReply,
          passive: true,
        });
        primed.groupContext = groupContext || null;
        for (const t of seedItems) {
          addMessageToSession(primed, "user", sanitizeMessageForLlm(t, null) || t);
        }
        nbot.log.info("[smart-assist] primed passive session");
      }
    }

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
    if (existing.passive) {
      existing.passive = false;
      nbot.log.info("[smart-assist] passive session activated");
    }
    // Refresh group context for the reply model so it can see what happened in the group
    // (e.g. other plugins already analyzed a file) and avoid redundant follow-ups.
    if (groupContext) {
      existing.groupContext = groupContext;
    }
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
    mentionUserOnFirstReply: config.mentionUserOnFirstReply || !!mentioned,
    mentionUserOnEveryReply: config.mentionUserOnEveryReply,
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
