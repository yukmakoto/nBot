const STORAGE_KEY = "jrys_records";
const DEFAULT_API_URL = "https://uapis.cn/api/v1/random/image?category=acg&type=pc";
const CANVAS_WIDTH = 900;
const CANVAS_HEIGHT = 540;
const RENDER_QUALITY = 92;
const MAX_STAR = 7;
const STAR_LINE1_COUNT = 4;
const STAR_LINE2_COUNT = 3;
const MAX_STAR_DISPLAY = STAR_LINE1_COUNT + STAR_LINE2_COUNT;

const DEFAULT_OPTIONS = [
  { name: "大吉+财运", star: 7, weight: 1 },
  { name: "大吉+事业", star: 7, weight: 1 },
  { name: "大吉+桃花", star: 7, weight: 1 },
  { name: "大吉+健康", star: 7, weight: 1 },
  { name: "中吉+财运", star: 6, weight: 2 },
  { name: "中吉+事业", star: 6, weight: 2 },
  { name: "中吉+桃花", star: 6, weight: 2 },
  { name: "中吉+健康", star: 6, weight: 2 },
  { name: "吉+财运", star: 5, weight: 4 },
  { name: "吉+事业", star: 5, weight: 4 },
  { name: "吉+桃花", star: 5, weight: 4 },
  { name: "吉+健康", star: 5, weight: 4 },
  { name: "小吉+财运", star: 4, weight: 6 },
  { name: "小吉+事业", star: 4, weight: 6 },
  { name: "小吉+桃花", star: 4, weight: 6 },
  { name: "小吉+健康", star: 4, weight: 6 },
  { name: "平", star: 3, weight: 30 },
  { name: "小凶", star: 2, weight: 12 },
  { name: "大凶", star: 1, weight: 6 },
];

let cachedOptions = null;
let cachedConfigKey = "";

function parseOptionsInput(input) {
  if (Array.isArray(input)) return input;
  if (typeof input === "string") {
    try {
      const parsed = JSON.parse(input);
      return Array.isArray(parsed) ? parsed : null;
    } catch (e) {
      return null;
    }
  }
  return null;
}

function normalizeOptions(input) {
  if (!Array.isArray(input)) return null;
  const normalized = input
    .map((item) => {
      if (!item || typeof item !== "object") return null;
      const name = String(item.name || "").trim();
      const star = Number(item.star);
      const weight = Number(item.weight);
      if (!name || !Number.isFinite(star) || !Number.isFinite(weight)) return null;
      if (weight <= 0) return null;
      return { name, star, weight };
    })
    .filter(Boolean);
  return normalized.length ? normalized : null;
}

function loadOptions() {
  const config = typeof nbot?.getConfig === "function" ? nbot.getConfig() : null;
  let rawOptions = null;

  if (Array.isArray(config)) {
    rawOptions = config;
  } else if (config && Array.isArray(config.options)) {
    rawOptions = config.options;
  } else if (config && typeof config.options_json === "string") {
    rawOptions = parseOptionsInput(config.options_json);
  } else if (typeof config === "string") {
    rawOptions = parseOptionsInput(config);
  }

  const fallback = rawOptions || DEFAULT_OPTIONS;
  const configKey = JSON.stringify(fallback);
  if (cachedOptions && cachedConfigKey === configKey) {
    return cachedOptions;
  }

  const normalized = normalizeOptions(fallback);
  cachedOptions = normalized;
  cachedConfigKey = configKey;
  return normalized;
}

function getTodayKey() {
  const now = new Date();
  const yyyy = String(now.getFullYear());
  const mm = String(now.getMonth() + 1).padStart(2, "0");
  const dd = String(now.getDate()).padStart(2, "0");
  return `${yyyy}-${mm}-${dd}`;
}

function pickWeighted(options) {
  const total = options.reduce((sum, item) => sum + item.weight, 0);
  if (!total) return null;
  let r = Math.random() * total;
  for (const item of options) {
    r -= item.weight;
    if (r <= 0) return item;
  }
  return options[options.length - 1] || null;
}

function buildImageMessage(image) {
  if (!image) return "";
  if (typeof image === "string") {
    if (image.startsWith("[CQ:image")) return image;
    return `[CQ:image,url=${image}]`;
  }
  if (typeof image === "object") {
    if (image.url) return `[CQ:image,url=${image.url}]`;
    if (image.file) return `[CQ:image,file=${image.file}]`;
  }
  return "";
}

function escapeHtml(text) {
  return String(text || "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll("\"", "&quot;")
    .replaceAll("'", "&#39;");
}

function getAvatarUrl(userId) {
  const qq = String(userId || "").trim();
  if (!qq) return "";
  return `https://q1.qlogo.cn/g?b=qq&nk=${qq}&s=640`;
}

function clampStar(star) {
  const value = Number(star);
  if (!Number.isFinite(value)) return 0;
  return Math.floor(Math.max(0, Math.min(MAX_STAR, value)));
}

function buildStarLines(star) {
  const safeStar = clampStar(star);
  const filled = "★".repeat(safeStar);
  const empty = "☆".repeat(MAX_STAR - safeStar);
  const combined = filled + empty;
  return {
    line1: combined.slice(0, STAR_LINE1_COUNT),
    line2: combined.slice(STAR_LINE1_COUNT, STAR_LINE1_COUNT + STAR_LINE2_COUNT),
    value: safeStar,
  };
}

function splitSignature(signature) {
  const text = String(signature || "").trim();
  if (!text) {
    return { top: "", bottom: "" };
  }
  const parts = text.split("+");
  if (parts.length >= 2) {
    return {
      top: parts[0].trim(),
      bottom: parts.slice(1).join("+").trim(),
    };
  }
  return { top: text, bottom: "" };
}

function buildFortuneHtml(data) {
  const bg = escapeHtml(data.backgroundUrl);
  const avatar = escapeHtml(data.avatarUrl);
  const userName = escapeHtml(data.userName || "群友");
  const signature = splitSignature(data.signature);
  const sigTop = escapeHtml(signature.top);
  const sigBottom = escapeHtml(signature.bottom);
  const starLines = buildStarLines(data.star);
  const starLine1 = escapeHtml(starLines.line1);
  const starLine2 = escapeHtml(starLines.line2);
  const starValue = `${starLines.value}/${MAX_STAR}`;

  return `<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8" />
  <style>
    html, body { margin: 0; padding: 0; width: 100%; height: 100%; }
    body { font-family: "Microsoft YaHei", "PingFang SC", sans-serif; }
    .canvas {
      width: ${CANVAS_WIDTH}px;
      height: ${CANVAS_HEIGHT}px;
      position: relative;
      background-image: url('${bg}');
      background-size: cover;
      background-position: center;
    }
    .overlay {
      position: absolute;
      left: 4%;
      top: 6%;
      width: 20%;
      min-width: 260px;
      height: 88%;
      background: rgba(255, 255, 255, 0.72);
      border-radius: 18px;
      padding: 18px 16px;
      box-sizing: border-box;
      display: flex;
      flex-direction: column;
      gap: 12px;
      align-items: center;
      text-align: center;
    }
    .avatar {
      width: 118px;
      height: 118px;
      border-radius: 50%;
      object-fit: cover;
      border: 3px solid rgba(255, 255, 255, 0.9);
      align-self: center;
    }
    .name {
      font-size: 22px;
      font-weight: 600;
      color: #222;
    }
    .label {
      font-size: 12px;
      color: #666;
      margin-bottom: 4px;
    }
    .value {
      font-size: 15px;
      color: #222;
      word-break: break-word;
    }
    .signature {
      display: flex;
      flex-direction: column;
      gap: 6px;
    }
    .signature .sig-line {
      font-size: 30px;
      color: #222;
    }
    .signature .sig-plus {
      font-size: 27px;
      font-weight: 600;
      color: #444;
      line-height: 1;
    }
    .stars-block {
      height: 20%;
      width: 100%;
      display: flex;
      flex-direction: column;
      justify-content: center;
      align-items: center;
      gap: 6px;
      margin-top: 12px;
    }
    .stars {
      color: #f5a623;
      letter-spacing: 4px;
      line-height: 1;
      display: flex;
      flex-direction: column;
      gap: 4px;
      width: 100%;
      align-items: center;
    }
    .stars-line {
      font-size: 30px;
    }
    .star-value {
      font-size: 14px;
      color: #444;
    }
  </style>
</head>
<body>
  <div class="canvas">
    <div class="overlay">
      <img class="avatar" src="${avatar}" />
      <div class="name">${userName}</div>
      <div>
        <div class="label">签名</div>
        <div class="signature">
          <div class="sig-line">${sigTop}</div>
          <div class="sig-plus">+</div>
          <div class="sig-line">${sigBottom}</div>
        </div>
      </div>
      <div class="stars-block">
        <div class="label">星级</div>
        <div class="stars">
          <div class="stars-line">${starLine1}</div>
          <div class="stars-line">${starLine2}</div>
        </div>
        <div class="star-value">${starValue}</div>
      </div>
    </div>
  </div>
</body>
</html>`;
}

async function generateFortuneImage(payload) {
  const url = DEFAULT_API_URL;
  if (!url) return null;
  if (typeof nbot.renderHtmlImage !== "function") {
    return url;
  }

  const html = buildFortuneHtml({
    backgroundUrl: url,
    avatarUrl: getAvatarUrl(payload.userId),
    userName: payload.userName,
    signature: payload.signature,
    star: payload.star,
  });

  const base64 = await nbot.renderHtmlImage(html, CANVAS_WIDTH, RENDER_QUALITY);
  if (!base64) return null;
  return `[CQ:image,file=base64://${base64}]`;
}

return {
  onEnable() {
    nbot.log.info("今日运势插件已启用");
  },

  onDisable() {
    nbot.log.info("今日运势插件已禁用");
  },

  async onCommand(ctx) {
    const { command, user_id, group_id } = ctx;
    if (command !== "运势" && command !== "今日运势") return;

    const gid = group_id || 0;
    const options = loadOptions();
    if (!options || options.length === 0) {
      nbot.sendReply(user_id, gid, "运势配置读取失败，请检查 config.json");
      return;
    }

    const today = getTodayKey();
    const data = nbot.storage.get(STORAGE_KEY) || {};
    const record = data[user_id];

    if (record && record.date === today && record.image) {
      const msg = buildImageMessage(record.image);
      if (msg) {
        nbot.sendReply(user_id, gid, msg);
        return;
      }
    }

    const picked = pickWeighted(options);
    if (!picked) {
      nbot.sendReply(user_id, gid, "运势生成失败，请稍后再试");
      return;
    }

    let image = null;
    try {
      const sender = ctx.sender || {};
      const userName = String(sender.card || sender.nickname || sender.name || user_id || "群友")
        .replace(/\s+/g, " ")
        .trim();
      image = await generateFortuneImage({
        userId: user_id,
        userName,
        signature: picked.name,
        star: picked.star,
      });
    } catch (e) {
      nbot.log.warn(`运势图片生成失败: ${e}`);
    }

    const msg = buildImageMessage(image);
    if (!msg) {
      nbot.sendReply(user_id, gid, "图片生成失败，请稍后再试");
      return;
    }

    data[user_id] = {
      date: today,
      name: picked.name,
      star: picked.star,
      image,
    };
    nbot.storage.set(STORAGE_KEY, data);

    nbot.sendReply(user_id, gid, msg);
  },
};
