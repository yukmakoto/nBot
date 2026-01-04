// 新成员验证插件
// 当新成员进群时，要求在指定时间内发言，否则自动踢出

const { log, storage, callApi, sendMessage, getConfig, now } = globalThis.nbot;

// 待验证成员存储 key
const PENDING_KEY = "pending_members";

let lastCleanupAt = 0;
let warnedAdminCheck = false;

// 获取待验证成员列表
function getPendingMembers() {
  return storage.get(PENDING_KEY) || {};
}

// 保存待验证成员列表
function savePendingMembers(pending) {
  storage.set(PENDING_KEY, pending);
}

// 生成随机超时时间（秒）
function randomTimeout(min, max) {
  return Math.floor(Math.random() * (max - min + 1)) + min;
}

// 格式化消息模板
function formatMessage(template, replacements) {
  let result = template;
  for (const [key, value] of Object.entries(replacements)) {
    result = result.replace(new RegExp(`\\{${key}\\}`, 'g'), value);
  }
  return result;
}

function cleanupIntervalMs(config) {
  const secs = Number(config.cleanup_interval_seconds ?? 5);
  const safeSecs = Number.isFinite(secs) ? Math.max(1, Math.min(60, Math.floor(secs))) : 5;
  return safeSecs * 1000;
}

// 检查并踢出超时的成员（仅处理指定群；由 interval 节流触发）
function checkAndKickExpiredGroup(groupId, config) {
  const pending = getPendingMembers();
  const groupKey = String(groupId);
  const users = pending[groupKey];
  if (!users || typeof users !== "object") return;

  const currentTime = now();
  const expired = [];
  let changed = false;

  const remainingUsers = {};
  for (const [userId, data] of Object.entries(users || {})) {
    if (currentTime >= data.expireTime) {
      expired.push({ groupId: groupKey, userId, nickname: data.nickname });
      changed = true;
    } else {
      remainingUsers[userId] = data;
    }
  }

  if (Object.keys(remainingUsers).length > 0) {
    pending[groupKey] = remainingUsers;
  } else {
    delete pending[groupKey];
    changed = true;
  }

  if (changed) {
    savePendingMembers(pending);
  }

  for (const { groupId, userId, nickname } of expired) {
    try {
      log.info(`[member-verify] 踢出超时成员: ${userId} (${nickname}) from group ${groupId}`);

      callApi("set_group_kick", {
        group_id: Number(groupId),
        user_id: Number(userId),
        reject_add_request: config.kick_reject_reapply || false,
      });

      const kickMsg = formatMessage(
        config.kick_message || "用户 {user} 未在规定时间内完成验证，已被移出群聊。",
        {
          user: nickname || userId,
        },
      );
      sendMessage(groupId, kickMsg);
    } catch (e) {
      log.error(`[member-verify] 踢出成员失败: ${e}`);
    }
  }
}

// 检查机器人是否是管理员
async function isBotAdmin(groupId) {
  // 当前插件运行时不支持同步读取群权限信息，无法准确判断机器人是否为管理员。
  // 如踢人失败，请检查机器人是否具备群管理权限。
  return true;
}

return {
  onEnable() {
    log.info("[member-verify] 新成员验证插件已启用");
    // 清理可能残留的过期数据
    const pending = getPendingMembers();
    const currentTime = now();
    let changed = false;

    for (const groupId of Object.keys(pending)) {
      for (const userId of Object.keys(pending[groupId])) {
        if (currentTime >= pending[groupId][userId].expireTime) {
          delete pending[groupId][userId];
          changed = true;
        }
      }
      if (Object.keys(pending[groupId]).length === 0) {
        delete pending[groupId];
        changed = true;
      }
    }

    if (changed) {
      savePendingMembers(pending);
    }
  },

  onDisable() {
    log.info("[member-verify] 新成员验证插件已禁用");
  },

  // 处理通知事件（新成员进群）
  async onNotice(ctx) {
    const config = getConfig();

    if (config.require_admin && !warnedAdminCheck) {
      warnedAdminCheck = true;
      log.warn("[member-verify] 提示：当前运行时无法自动检测机器人是否为群管理员；如踢人失败请检查机器人权限");
    }

    // Best-effort cleanup for this group to improve timeout handling (interval throttled)
    const intervalMs = cleanupIntervalMs(config);
    const currentTime = now();
    if (currentTime - lastCleanupAt >= intervalMs) {
      lastCleanupAt = currentTime;
      if (ctx.group_id) {
        checkAndKickExpiredGroup(ctx.group_id, config);
      }
    }

    // 只处理群成员增加事件
    if (ctx.notice_type !== "group_increase") {
      return true;
    }

    const { user_id: userId, group_id: groupId, sub_type: subType } = ctx;

    log.info(`[member-verify] 新成员进群: ${userId} -> ${groupId} (${subType})`);

    // 检查是否需要管理员权限
    if (config.require_admin) {
      const isAdmin = await isBotAdmin(groupId);
      if (!isAdmin) {
        log.warn(`[member-verify] 机器人不是群 ${groupId} 的管理员，跳过验证`);
        return true;
      }
    }

    // 生成随机超时时间
    const minTimeout = config.min_timeout || 30;
    const maxTimeout = config.max_timeout || 90;
    const timeout = randomTimeout(minTimeout, maxTimeout);
    const expireTime = now() + timeout * 1000;

    // 记录待验证成员
    const pending = getPendingMembers();
    const groupKey = String(groupId);

    if (!pending[groupKey]) {
      pending[groupKey] = {};
    }

    pending[groupKey][String(userId)] = {
      joinTime: now(),
      expireTime: expireTime,
      timeout: timeout,
      nickname: String(userId) // 暂时用 QQ 号作为昵称
    };

    savePendingMembers(pending);

    // 发送欢迎消息
    const welcomeMsg = formatMessage(config.welcome_message || "{user} 欢迎加入本群！请在 {timeout} 秒内发送任意消息完成验证，否则将被移出群聊。", {
      user: `[CQ:at,qq=${userId}]`,
      timeout: String(timeout)
    });

    sendMessage(groupId, welcomeMsg);

    log.info(`[member-verify] 已记录待验证成员: ${userId}, 超时时间: ${timeout}秒`);

    return true;
  },

  // 处理消息事件（检查是否是待验证成员发言）
  async preMessage(ctx) {
    const config = getConfig();
    const { user_id: userId, group_id: groupId } = ctx;

    // Best-effort cleanup for this group (interval throttled)
    const intervalMs = cleanupIntervalMs(config);
    const currentTime = now();
    if (currentTime - lastCleanupAt >= intervalMs) {
      lastCleanupAt = currentTime;
      if (groupId) {
        checkAndKickExpiredGroup(groupId, config);
      }
    }

    // 只处理群消息
    if (!groupId) {
      return true;
    }

    // 检查发言者是否是待验证成员
    const pending = getPendingMembers();
    const groupKey = String(groupId);
    const userKey = String(userId);

    if (pending[groupKey] && pending[groupKey][userKey]) {
      // 验证成功，移除待验证状态
      delete pending[groupKey][userKey];

      if (Object.keys(pending[groupKey]).length === 0) {
        delete pending[groupKey];
      }

      savePendingMembers(pending);

      log.info(`[member-verify] 成员验证成功: ${userId} in group ${groupId}`);

      // 发送验证成功消息
      const successMsg = config.verify_success_message;
      if (successMsg) {
        const formattedMsg = formatMessage(successMsg, {
          user: `[CQ:at,qq=${userId}]`
        });
        sendMessage(groupId, formattedMsg);
      }
    }

    return true;
  }
};
