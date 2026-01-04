/**
 * nBot Like Plugin
 * QQ点赞功能，支持每日限额和排行榜
 */

// 解析 @ 格式：[CQ:at,qq=123456] 或纯数字
function parseTarget(arg, defaultId) {
  if (!arg) return defaultId;
  if (arg.includes("qq=")) {
    const match = arg.match(/qq=(\d+)/);
    return match ? parseInt(match[1]) : defaultId;
  }
  const num = parseInt(arg);
  return isNaN(num) ? defaultId : num;
}

// 获取今日日期字符串
function getToday() {
  return new Date().toISOString().split('T')[0];
}

return {
  onEnable() {
    const config = nbot.getConfig();
    nbot.log.info("Like plugin enabled, daily_limit: " + (config.daily_limit || 10));
  },

  onDisable() {
    nbot.log.info("Like plugin disabled");
  },

  // 处理命令
  onCommand(ctx) {
    const { command, command_used, user_id, group_id, args } = ctx;
    const config = nbot.getConfig();

    const used = String(command_used || command || "");

    // 兼容：排行榜相关词作为别名时，通过 command_used 区分功能
    const isRank = /榜|排行|rank/i.test(used);

    if (isRank) {
      this.handleRank(user_id, group_id, config);
      return;
    }

    // 点赞
    if (command === "点赞") {
      this.handleLike(user_id, group_id, args, config);
    }
  },

  handleLike(userId, groupId, args, config) {
    const dailyLimit = config.daily_limit || 10;
    const maxTimes = config.max_times_per_like || 10;
    const target = parseTarget(args[0], userId);
    const today = getToday();

    // 加载存储数据
    let data = nbot.storage.get("likes") || { records: {}, daily: {} };

    // 初始化每日记录
    if (!data.daily[today]) {
      data.daily[today] = {};
    }

    // 检查今日限额
    const dailyKey = `${userId}_${target}`;
    const usedToday = data.daily[today][dailyKey] || 0;
    const remaining = dailyLimit - usedToday;

    if (remaining <= 0) {
      const totalLikes = data.records[target] || 0;
      nbot.sendReply(userId, groupId,
        `今日已达点赞上限！\n${target} 累计被赞: ${totalLikes} 次\n每天每人限点同一目标 ${dailyLimit} 次`
      );
      return;
    }

    // 计算实际点赞次数
    const actualTimes = Math.min(maxTimes, remaining);

    // 更新记录
    data.daily[today][dailyKey] = usedToday + actualTimes;
    data.records[target] = (data.records[target] || 0) + actualTimes;

    // 清理旧的每日记录（保留最近7天）
    const dates = Object.keys(data.daily).sort();
    while (dates.length > 7) {
      delete data.daily[dates.shift()];
    }

    // 保存
    nbot.storage.set("likes", data);

    // 调用QQ点赞API
    nbot.callApi("send_like", { user_id: target, times: actualTimes });

    const newRemaining = dailyLimit - (usedToday + actualTimes);
    const totalLikes = data.records[target];

    nbot.sendReply(userId, groupId,
      `已为 ${target} 点赞 ${actualTimes} 次！\n累计被赞: ${totalLikes} 次 | 今日剩余: ${newRemaining} 次`
    );
  },

  handleRank(userId, groupId, config) {
    const rankLimit = config.rank_limit || 10;
    const showEmoji = config.show_emoji !== false;

    let data = nbot.storage.get("likes") || { records: {} };

    // 生成排行榜
    const entries = Object.entries(data.records)
      .map(([id, count]) => [parseInt(id), count])
      .sort((a, b) => b[1] - a[1])
      .slice(0, rankLimit);

    if (entries.length === 0) {
      nbot.sendReply(userId, groupId, "暂无点赞记录");
      return;
    }

    let msg = `点赞排行榜 TOP ${rankLimit}\n\n`;
    entries.forEach(([uid, count], index) => {
      let medal;
      if (showEmoji && index < 3) {
        medal = ["🥇", "🥈", "🥉"][index];
      } else {
        medal = `${index + 1}.`;
      }
      msg += `${medal} ${uid} - ${count} 次\n`;
    });

    nbot.sendReply(userId, groupId, msg);
  }
};
