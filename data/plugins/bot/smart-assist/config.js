export function getConfig() {
  const cfg = nbot.getConfig();
  const interruptKeywords =
    Array.isArray(cfg.interrupt_keywords) && cfg.interrupt_keywords.length
      ? cfg.interrupt_keywords
      : ["我明白了", "结束", "停止"];

  // Some upstreams default to a very small max_tokens (e.g. 256). For models that spend tokens on hidden reasoning,
  // this can truncate even tiny JSON outputs. Provide a safe default while allowing users to disable it by setting
  // the value to null/0 in config.
  const normalizeMaxTokens = (v, fallback) => {
    if (v === null) return null;
    const n = Number(v);
    if (!Number.isFinite(n) || n <= 0) return null;
    return Math.max(64, Math.min(8192, Math.floor(n || fallback)));
  };
  const decisionMaxTokens = normalizeMaxTokens(cfg.decision_max_tokens ?? 2048, 2048);
  const replyMaxTokens = normalizeMaxTokens(cfg.reply_max_tokens ?? 2048, 2048);
  const replyRetryMaxTokens = normalizeMaxTokens(cfg.reply_retry_max_tokens ?? 512, 512);

  const mentionUserOnFirstReply = cfg.mention_user_on_first_reply !== false;
  const mentionUserOnEveryReply = cfg.mention_user_on_every_reply !== false;
  const alwaysReplyInSession = cfg.always_reply_in_session !== false;
  const decisionSystemPrompt =
    cfg.decision_system_prompt ||
    [
      "你是 QQ 群聊里的「路由器（Router）」：你不负责输出回复内容，只负责决定机器人要不要介入、以及需不需要联网搜索。",
      "",
      "重要：要非常保守，避免误触发。",
      "- 你的帮助范围仅限：Minecraft/PCL 启动器/Java/模组/服务器/常见软件报错排查；硬件选购/配电脑/主板内存/价格闲聊等一律 action=IGNORE。",
      "- 只要像玩笑/吐槽/阴阳怪气/反讽/自问自答/口头禅、或没有明确问题与需求，一律 action=IGNORE。",
      "- 被 @ 机器人只是“优先级更高”的信号，仍然可以 action=IGNORE。",
      "- 没有 @ 机器人时：除非用户明显是在向全群求助/提问（期待任何人回答），否则一律 action=IGNORE。不要抢别人的对话。",
      "- 如果候选消息是【回复别人】的跟帖（例如带有“（回复内容：...）”），且没有 @ 机器人：通常是在接别人话，一律 action=IGNORE（机器人不要插嘴）。",
      "- 如果【最近群聊片段】里已经有人给出明确答案/解决步骤/指路（例如“群文件/看公告/看置顶/去某某页面”），通常 action=IGNORE（机器人不要抢答/复读）。",
      "- 起哄/调戏/让机器人叫称呼/要机器人表白/刷屏/群聊闲聊，通常 action=IGNORE。",
      "- 只有媒体/占位符（如“[图片] / [视频] / [语音] / [卡片]”）且没有任何文字内容，一律 action=IGNORE（不要去‘说明无法判断’）。",
      "- 只有表情/颜文字/一个词/无意义应答（如“哈哈”“？”“。。。”）一律 action=IGNORE。",
      "- 用户在 @ 其他人（而不是 @ 机器人）时，通常是在找那个人说话：除非明确要求机器人回答，否则 action=IGNORE。",
      "",
      "你必须输出严格 JSON（不要 Markdown、不要解释文本），字段如下：",
      '{"action":"IGNORE|REPLY|REACT","confidence":0.0,"reason":"<=20字中文","use_search":true|false,"topic":"<=12字中文","need_clarify":true|false}',
      "输出必须为【单行 JSON】，且必须以 { 开头、以 } 结尾；除此之外禁止任何字符；confidence 取 0~1。",
      "尽量输出最短 JSON：reason/topic 允许为空字符串；不要添加额外字段。",
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
      "- 每条消息不超过 20 字；通常输出 2 条，用「||」分隔（仍然同一行）。第 1 条可追问 1 个关键点，第 2 条给可执行步骤；如无需追问，可只输出 1 条步骤。",
      "- 如果只输出 1 条，必须是「可执行步骤」，不要只问问题。",
      "- 禁止半句碎片（如“建议先/能/然后”）。",
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
    decisionMaxTokens,
    replyMaxTokens,
    replyRetryMaxTokens,
    mentionUserOnFirstReply,
    mentionUserOnEveryReply,
    alwaysReplyInSession,
  };
}
