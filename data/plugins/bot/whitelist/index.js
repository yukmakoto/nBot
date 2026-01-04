/**
 * nBot Whitelist Plugin
 * Controls bot response based on whitelist/blacklist mode
 *
 * Config:
 * - mode: "whitelist" or "blacklist"
 * - groups: Array of group IDs (strings)
 * - users: Array of user IDs (strings)
 */

function normalizeList(v) {
  if (!Array.isArray(v)) return [];
  return v.map((x) => String(x)).filter((x) => x.trim());
}

function checkAccess(ctx, scope) {
  if (ctx && ctx.is_super_admin) {
    return { allow: true, mode: "bypass", userStr: "", groupStr: null, messageType: "" };
  }

  const config = nbot.getConfig() || {};
  const mode = config.mode || "whitelist";
  const groups = normalizeList(config.groups);
  const users = normalizeList(config.users);
  const allowUserInGroups = config.allow_user_in_groups === true;

  const userStr = ctx && ctx.user_id !== undefined ? String(ctx.user_id) : "";
  const groupStr = ctx && ctx.group_id ? String(ctx.group_id) : null;
  const messageType = ctx && ctx.message_type ? String(ctx.message_type) : "";

  const userInList = userStr ? users.includes(userStr) : false;
  const groupInList = groupStr ? groups.includes(groupStr) : false;

  const isGroupContext =
    scope === "notice" || messageType === "group" || !!groupStr;

  const inList =
    scope === "notice"
      ? groupInList
      : isGroupContext
        ? groupInList || (allowUserInGroups && userInList)
        : userInList;

  if (mode === "whitelist") {
    return { allow: !!inList, mode, userStr, groupStr, messageType };
  }
  if (mode === "blacklist") {
    return { allow: !inList, mode, userStr, groupStr, messageType };
  }

  return { allow: true, mode: "unknown", userStr, groupStr, messageType };
}

// Return plugin object
return {
  onEnable() {
    const config = nbot.getConfig();
    nbot.log.info("Whitelist plugin enabled, mode: " + config.mode);
  },

  onDisable() {
    nbot.log.info("Whitelist plugin disabled");
  },

  preMessage(ctx) {
    const result = checkAccess(ctx, "message");
    if (!result.allow) {
      nbot.log.info(
        `Blocked by ${result.mode}: user=${result.userStr}, group=${result.groupStr}, type=${result.messageType}`
      );
    }
    return result.allow;
  },

  preCommand(ctx) {
    const result = checkAccess(ctx, "command");
    if (!result.allow) {
      nbot.log.info(
        `Blocked by ${result.mode} (command): user=${result.userStr}, group=${result.groupStr}`
      );
    }
    return result.allow;
  },

  onNotice(ctx) {
    const result = checkAccess(ctx, "notice");
    if (!result.allow) {
      nbot.log.info(
        `Blocked by ${result.mode} (notice): group=${result.groupStr}`
      );
    }
    return result.allow;
  },

  onCommand(ctx) {
    const { command, user_id, group_id, args, is_admin, is_super_admin } = ctx;

    if (command !== "加白") {
      return;
    }

    if (!group_id) {
      nbot.sendReply(user_id, 0, "此命令仅限群聊使用");
      return;
    }

    if (!is_super_admin && !is_admin) {
      nbot.sendReply(user_id, group_id, "权限不足，需要管理员权限");
      return;
    }

    const config = nbot.getConfig();
    const groups = Array.isArray(config.groups) ? config.groups.map(String) : [];
    const groupArg = Array.isArray(args) && args.length > 0 ? String(args[0]).trim() : "";
    const targetGroup = groupArg || String(group_id);

    if (!/^[0-9]{5,15}$/.test(targetGroup)) {
      nbot.sendReply(user_id, group_id, "群号格式不正确");
      return;
    }

    if (!groups.includes(targetGroup)) {
      groups.push(targetGroup);
    } else {
      nbot.sendReply(user_id, group_id, "该群已在白名单中");
      return;
    }

    const ok = nbot.setConfig({ ...config, groups });
    if (!ok) {
      nbot.sendReply(user_id, group_id, "更新白名单配置失败");
      return;
    }

    nbot.sendReply(user_id, group_id, `已将群 ${targetGroup} 添加到白名单`);
  }
};
