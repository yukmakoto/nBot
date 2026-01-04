/**
 * nBot 灰条攻击检测插件
 *
 * 检测伪造灰条消息（如假戳一戳、假系统消息）的攻击者
 *
 * 原理：
 * - 正常的系统灰条消息 senderUin 为 '0' 或空
 * - 伪造的灰条消息 senderUin 是攻击者的QQ号
 * - 通过检查消息内容中的灰条特征 + 非零发送者来识别攻击
 *
 * 灰条特征关键词：
 * - subElementType=17 (JSON灰条)
 * - busiId=1061 (戳一戳)
 * - grayTipElement
 * - elementType=8 (灰条元素类型)
 *
 * 撤回说明：
 * - 如果消息有 message_id，可以尝试撤回
 * - 需要机器人是群管理员才能撤回他人消息
 */

// 灰条攻击特征模式
const GREYTIP_PATTERNS = [
  /subElementType[=:]\s*17/i,
  /busiId[=:]\s*["']?1061["']?/i,   // 戳一戳
  /busiId[=:]\s*["']?2050["']?/i,   // 群公告/广告类灰条
  /busiId[=:]\s*["']?2401["']?/i,   // 其他灰条类型
  /grayTipElement/i,
  /elementType[=:]\s*8/i,
  /jsonGrayTipElement/i,
  /xmlElement.*busiId/i,
  /\[CQ:json.*busiId/i,
  /\[CQ:xml.*subElementType.*17/i,
];

// 记录检测到的攻击
const attackLog = [];
const MAX_LOG_SIZE = 100;

function logAttack(groupId, userId, detail) {
  const record = {
    time: new Date().toISOString(),
    group_id: groupId,
    user_id: userId,
    detail: detail
  };

  attackLog.unshift(record);
  if (attackLog.length > MAX_LOG_SIZE) {
    attackLog.pop();
  }

  return record;
}

function checkGreyTipAttack(rawMessage, messageSegments) {
  // 检查原始消息字符串
  if (rawMessage) {
    for (const pattern of GREYTIP_PATTERNS) {
      if (pattern.test(rawMessage)) {
        return { detected: true, pattern: pattern.toString(), source: 'raw_message' };
      }
    }
  }

  // 检查消息段
  if (Array.isArray(messageSegments)) {
    for (const seg of messageSegments) {
      const segStr = JSON.stringify(seg);
      for (const pattern of GREYTIP_PATTERNS) {
        if (pattern.test(segStr)) {
          return { detected: true, pattern: pattern.toString(), source: 'message_segment' };
        }
      }

      // 检查 json 类型消息段
      if (seg.type === 'json' && seg.data) {
        const jsonData = typeof seg.data.data === 'string' ? seg.data.data : JSON.stringify(seg.data);
        for (const pattern of GREYTIP_PATTERNS) {
          if (pattern.test(jsonData)) {
            return { detected: true, pattern: pattern.toString(), source: 'json_segment' };
          }
        }
      }

      // 检查 xml 类型消息段
      if (seg.type === 'xml' && seg.data) {
        const xmlData = typeof seg.data.data === 'string' ? seg.data.data : JSON.stringify(seg.data);
        for (const pattern of GREYTIP_PATTERNS) {
          if (pattern.test(xmlData)) {
            return { detected: true, pattern: pattern.toString(), source: 'xml_segment' };
          }
        }
      }
    }
  }

  return { detected: false };
}

return {
  onEnable() {
    nbot.log.info("灰条攻击检测插件已启用");
    const config = nbot.getConfig();
    nbot.log.info("当前配置: 自动禁言=" + config.auto_mute + ", 禁言时长=" + config.mute_duration + "秒");
  },

  onDisable() {
    nbot.log.info("灰条攻击检测插件已禁用");
  },

  /**
   * preMessage 钩子 - 在消息处理前检测
   *
   * 关键逻辑：
   * 1. 检查消息内容是否包含灰条特征
   * 2. 如果包含，且 user_id 不为 0，则该 user_id 就是攻击者
   */
  preMessage(ctx) {
    const config = nbot.getConfig();

    const { user_id, group_id, raw_message, message, message_type, message_id } = ctx;

    // 只检测群消息
    if (message_type !== 'group' || !group_id) {
      return true;
    }

    // 检查是否在监控列表中
    const watchGroups = config.watch_groups || [];
    if (watchGroups.length > 0 && !watchGroups.includes(String(group_id))) {
      return true;
    }

    // user_id 为 0 说明是真正的系统消息，不是伪造的
    if (!user_id || user_id === 0 || user_id === '0') {
      return true;
    }

    // 检测灰条攻击特征
    const result = checkGreyTipAttack(raw_message, message);

    if (result.detected) {
      // 发现攻击！
      const attackRecord = logAttack(group_id, user_id, {
        pattern: result.pattern,
        source: result.source,
        raw_message: raw_message ? raw_message.substring(0, 500) : '',
        message_id: message_id
      });

      nbot.log.warn("========== 灰条攻击检测 ==========");
      nbot.log.warn("群号: " + group_id);
      nbot.log.warn("攻击者QQ: " + user_id);
      nbot.log.warn("消息ID: " + (message_id || "无"));
      nbot.log.warn("匹配模式: " + result.pattern);
      nbot.log.warn("来源: " + result.source);
      nbot.log.warn("时间: " + attackRecord.time);
      nbot.log.warn("===================================");

      // 尝试撤回消息
      if (config.auto_recall && message_id) {
        nbot.log.info("正在尝试撤回消息 ID: " + message_id);
        nbot.callApi("delete_msg", {
          message_id: Number(message_id),
        });
      }

      // 通知管理员
      if (config.notify_admin) {
        const warningMsg = [
          "[灰条攻击检测]",
          "检测到伪造灰条消息！",
          "攻击者QQ: " + user_id,
          "时间: " + new Date().toLocaleString('zh-CN'),
          message_id ? ("消息已尝试撤回") : ("消息无法撤回(无message_id)"),
          "",
          "该用户正在发送伪造的系统消息，可能用于欺骗或骚扰群成员。"
        ].join("\n");

        nbot.sendReply(user_id, group_id, warningMsg);
      }

      // 自动禁言
      if (config.auto_mute) {
        const duration = config.mute_duration || 86400;
        nbot.log.info("正在禁言攻击者 " + user_id + " " + duration + " 秒");

        nbot.callApi("set_group_ban", {
          group_id: Number(group_id),
          user_id: Number(user_id),
          duration,
        });
      }

      // 阻止消息继续处理
      return false;
    }

    return true;
  },

  /**
   * onNotice 钩子 - 处理通知事件（灰条消息）
   * 这是从 NapCat 上报的 gray_tip 事件
   */
  onNotice(ctx) {
    const config = nbot.getConfig();

    const { notice_type, sub_type, user_id, group_id, message_id, busi_id, content } = ctx;

    // 只处理灰条消息
    if (notice_type !== 'notify' || sub_type !== 'gray_tip') {
      return true;
    }

    // 检查是否在监控列表中
    const watchGroups = config.watch_groups || [];
    if (watchGroups.length > 0 && !watchGroups.includes(String(group_id))) {
      return true;
    }

    // 有真实发送者，说明是伪造的灰条
    if (user_id && user_id !== 0) {
      const attackRecord = logAttack(group_id, user_id, {
        type: 'notice_event',
        busi_id: busi_id,
        content: content ? content.substring(0, 500) : '',
        message_id: message_id
      });

      nbot.log.warn("========== 灰条攻击检测 (Notice) ==========");
      nbot.log.warn("群号: " + group_id);
      nbot.log.warn("攻击者QQ: " + user_id);
      nbot.log.warn("消息ID: " + (message_id || "无"));
      nbot.log.warn("业务ID: " + busi_id);
      nbot.log.warn("时间: " + attackRecord.time);
      nbot.log.warn("============================================");

      // 尝试撤回消息
      if (config.auto_recall && message_id) {
        nbot.log.info("正在尝试撤回消息 ID: " + message_id);
        nbot.callApi("delete_msg", {
          message_id: Number(message_id),
        });
      }

      // 通知管理员
      if (config.notify_admin) {
        const warningMsg = [
          "[灰条攻击检测]",
          "检测到伪造灰条消息！",
          "攻击者QQ: " + user_id,
          "业务ID: " + busi_id,
          "时间: " + new Date().toLocaleString('zh-CN'),
          message_id ? "消息已尝试撤回" : "消息无法撤回(无message_id)",
          "",
          "该用户正在发送伪造的系统消息。"
        ].join("\n");

        nbot.sendReply(user_id, group_id, warningMsg);
      }

      // 自动禁言
      if (config.auto_mute) {
        const duration = config.mute_duration || 86400;
        nbot.log.info("正在禁言攻击者 " + user_id + " " + duration + " 秒");

        nbot.callApi("set_group_ban", {
          group_id: Number(group_id),
          user_id: Number(user_id),
          duration,
        });
      }
    }

    return true;
  },

  /**
   * 指令处理 - 查看检测记录
   */
  onCommand(ctx) {
    const { command, user_id, group_id, args, is_admin, is_super_admin } = ctx;

    if (command !== "灰条检测") {
      return;
    }

    const gid = group_id || 0;

    if (!is_admin && !is_super_admin) {
      nbot.sendReply(user_id, gid, "权限不足，需要管理员权限");
      return;
    }

    const subCmd = args && args.length > 0 ? args[0] : "status";

    if (subCmd === "status" || subCmd === "状态") {
      const config = nbot.getConfig();
      const statusMsg = [
        "[灰条攻击检测状态]",
        "插件状态: 已启用（如需关闭请在 WebUI 插件中心禁用）",
        "自动禁言: " + (config.auto_mute ? "已开启" : "已关闭"),
        "禁言时长: " + config.mute_duration + " 秒",
        "通知管理: " + (config.notify_admin ? "已开启" : "已关闭"),
        "监控群数: " + (config.watch_groups?.length || 0) + " 个",
        "检测记录: " + attackLog.length + " 条"
      ].join("\n");

      nbot.sendReply(user_id, gid, statusMsg);
      return;
    }

    if (subCmd === "log" || subCmd === "记录") {
      if (attackLog.length === 0) {
        nbot.sendReply(user_id, gid, "暂无攻击检测记录");
        return;
      }

      const recentLogs = attackLog.slice(0, 10);
      const logLines = ["[最近10条攻击记录]"];

      for (const record of recentLogs) {
        logLines.push("");
        logLines.push("时间: " + record.time);
        logLines.push("群号: " + record.group_id);
        logLines.push("攻击者: " + record.user_id);
      }

      nbot.sendReply(user_id, gid, logLines.join("\n"));
      return;
    }

    if (subCmd === "on" || subCmd === "开启" || subCmd === "off" || subCmd === "关闭") {
      nbot.sendReply(
        user_id,
        gid,
        "该指令已废弃：请在 WebUI 插件中心启用/禁用「灰条攻击检测」插件以统一开关。"
      );
      return;
    }

    if (subCmd === "automute" || subCmd === "自动禁言") {
      const config = nbot.getConfig();
      config.auto_mute = !config.auto_mute;
      nbot.setConfig(config);
      nbot.sendReply(user_id, gid, "自动禁言已" + (config.auto_mute ? "开启" : "关闭"));
      return;
    }

    // 帮助信息
    const helpMsg = [
      "[灰条检测指令帮助]",
      "/灰条检测 status - 查看状态",
      "/灰条检测 log - 查看攻击记录",
      "/灰条检测 automute - 切换自动禁言"
    ].join("\n");

    nbot.sendReply(user_id, gid, helpMsg);
  }
};
