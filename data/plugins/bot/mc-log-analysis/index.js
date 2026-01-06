const runtime = (() => {
  const tryRequire = (name) => {
    try {
      return require(name);
    } catch (e) {
      return null;
    }
  };
  if (typeof require !== "function") {
    return {
      fs: null,
      path: null,
      os: null,
      http: null,
      https: null,
      zlib: null,
      childProcess: null,
      util: null,
    };
  }
  return {
    fs: tryRequire("fs"),
    path: tryRequire("path"),
    os: tryRequire("os"),
    http: tryRequire("http"),
    https: tryRequire("https"),
    zlib: tryRequire("zlib"),
    childProcess: tryRequire("child_process"),
    util: tryRequire("util"),
  };
})();

const fs = runtime.fs;
const path = runtime.path;
const http = runtime.http;
const https = runtime.https;
const zlib = runtime.zlib;
const childProcess = runtime.childProcess;
const TextDecoderCtor =
  typeof TextDecoder === "function"
    ? TextDecoder
    : runtime.util && runtime.util.TextDecoder
      ? runtime.util.TextDecoder
      : null;

const TEXT_EXTENSIONS = [".txt", ".log"];
const ARCHIVE_EXTENSIONS = [".zip", ".tar", ".tar.gz", ".tgz", ".rar", ".7z", ".gz"];

const sessions = new Map();
const pendingFileUrlRequests = new Map();
const recentTriggerKeys = new Map();
let lastCleanupAt = 0;
let requestIdCounter = 0;

function clampNumber(value, fallback, min, max) {
  const n = Number(value);
  if (!Number.isFinite(n)) return fallback;
  return Math.max(min, Math.min(max, n));
}

function normalizeList(value) {
  if (!Array.isArray(value)) return [];
  return value.map((v) => String(v).trim()).filter((v) => v);
}

function getConfig() {
  const cfg = nbot.getConfig() || {};
  return {
    analysis_model: String(cfg.analysis_model || "").trim() || "default",
    prompt:
      typeof cfg.prompt === "string" && cfg.prompt.trim()
        ? cfg.prompt
        : "分析这个我的世界java版错误日志(只给出原因,解决方法，要求内容精简易懂)：",
    user_prompt:
      typeof cfg.user_prompt === "string" && cfg.user_prompt.trim()
        ? cfg.user_prompt
        : "请分析以下内容：",
    text1:
      typeof cfg.text1 === "string" && cfg.text1.trim()
        ? cfg.text1
        : "发送您要分析的内容或文件开始分析，发送退出结束分析(注:文件仅限txt,log如zip请解压)",
    text2:
      typeof cfg.text2 === "string" && cfg.text2.trim()
        ? cfg.text2
        : "已结束，感谢使用！",
    text3:
      typeof cfg.text3 === "string" && cfg.text3.trim()
        ? cfg.text3
        : "已开始解析，请耐心等待并注意群消息。",
    text4:
      typeof cfg.text4 === "string" && cfg.text4.trim()
        ? cfg.text4
        : "已超时，自动结束。",
    exit_keywords: normalizeList(cfg.exit_keywords).length
      ? normalizeList(cfg.exit_keywords)
      : ["退出", "结束", "取消"],
    text_file_keywords: normalizeList(cfg.text_file_keywords).length
      ? normalizeList(cfg.text_file_keywords)
      : ["crash", "fcl", "游戏崩溃", "log", "txt"],
    archive_keywords: normalizeList(cfg.archive_keywords).length
      ? normalizeList(cfg.archive_keywords)
      : ["错误报告"],
    max_content_length: clampNumber(cfg.max_content_length, 50000, 1000, 200000),
    wait_timeout_seconds: clampNumber(cfg.wait_timeout_seconds, 120, 10, 600),
    file_url_timeout_seconds: clampNumber(cfg.file_url_timeout_seconds, 15, 5, 120),
    max_download_bytes: clampNumber(cfg.max_download_bytes, 30000000, 1048576, 500000000),
    max_extract_bytes: clampNumber(cfg.max_extract_bytes, 120000000, 10485760, 1000000000),
    max_file_bytes: clampNumber(cfg.max_file_bytes, 15000000, 1048576, 200000000),
    allow_archive: cfg.allow_archive !== false,
    show_processing_msg: cfg.show_processing_msg !== false,
    auto_trigger: cfg.auto_trigger === true,
  };
}

function buildSessionKey(userId, groupId) {
  return `${groupId || 0}:${userId}`;
}

function buildTriggerKey(userId, groupId, target) {
  const gid = groupId || 0;
  const fid = target && target.fileId ? String(target.fileId) : "";
  const busid = target && target.busid !== undefined && target.busid !== null ? String(target.busid) : "";
  const url = target && target.url ? String(target.url) : "";
  const name = target && target.name ? String(target.name) : "";
  const identity = fid
    ? `fid:${fid}|busid:${busid}`
    : url
      ? `url:${url}`
      : `name:${name}`;
  return `${gid}:${userId}:${identity}`;
}

function cleanupRecentTriggerKeys(now) {
  for (const [k, expiresAt] of recentTriggerKeys.entries()) {
    if (now >= expiresAt) {
      recentTriggerKeys.delete(k);
    }
  }
}

function shouldDedupeTrigger(userId, groupId, target) {
  const now = nbot.now();
  cleanupRecentTriggerKeys(now);
  const key = buildTriggerKey(userId, groupId, target);
  const expiresAt = recentTriggerKeys.get(key);
  if (expiresAt && now < expiresAt) {
    return true;
  }
  // Dedupe window: avoid duplicate triggers when NapCat reports both message + group_upload notice.
  recentTriggerKeys.set(key, now + 5000);
  return false;
}

function genRequestId(type) {
  requestIdCounter += 1;
  return `mc-log-${type}-${requestIdCounter}-${nbot.now()}`;
}

function looksLikeUrl(value) {
  if (typeof value !== "string") return false;
  const v = value.trim().toLowerCase();
  return v.startsWith("http://") || v.startsWith("https://");
}

function hasExtension(name) {
  return /\.[a-z0-9]+$/i.test(String(name || ""));
}

function isExitMessage(text, keywords) {
  const t = String(text || "").trim();
  if (!t) return false;
  return keywords.some((kw) => t === String(kw).trim());
}

function isTextFile(name) {
  const lower = String(name || "").toLowerCase();
  return TEXT_EXTENSIONS.some((ext) => lower.endsWith(ext));
}

function isArchiveFile(name) {
  const lower = String(name || "").toLowerCase();
  return ARCHIVE_EXTENSIONS.some((ext) => lower.endsWith(ext));
}

function matchKeywordsOrdered(name, keywords) {
  const lower = String(name || "").toLowerCase();
  if (!lower) return { matched: false, keyword: "" };
  const list = Array.isArray(keywords)
    ? keywords.map((kw) => String(kw || "").toLowerCase()).filter((kw) => kw)
    : [];

  for (const keyword of list) {
    if (lower === keyword) {
      return { matched: true, keyword, mode: "exact" };
    }
  }

  for (const keyword of list) {
    if (lower.includes(keyword)) {
      return { matched: true, keyword, mode: "partial" };
    }
  }

  return { matched: false, keyword: "" };
}

function canUseLocalFiles() {
  return !!fs && !!path;
}

function getTempRoot() {
  if (!canUseLocalFiles()) return null;
  const baseDir = getPluginDir();
  if (baseDir) {
    return path.join(baseDir, "tmp");
  }
  return null;
}

function ensureDir(dirPath) {
  if (!fs || !dirPath) return false;
  try {
    fs.mkdirSync(dirPath, { recursive: true });
    return true;
  } catch (e) {
    return false;
  }
}

function removePath(targetPath) {
  if (!fs || !targetPath) return;
  try {
    if (fs.rmSync) {
      fs.rmSync(targetPath, { recursive: true, force: true });
      return;
    }
    if (fs.rmdirSync) {
      fs.rmdirSync(targetPath, { recursive: true });
    }
  } catch (e) {
    // ignore cleanup errors
  }
}

function sanitizeFileName(name) {
  const raw = String(name || "").trim();
  if (!raw) return "file";
  return raw.replace(/[\\/:*?"<>|]/g, "_");
}

function guessFileNameFromUrl(url) {
  if (typeof url !== "string") return "";
  const match = url.match(/\/([^/?#]+)(?:[?#]|$)/);
  if (!match) return "";
  try {
    return decodeURIComponent(match[1]);
  } catch (e) {
    return match[1];
  }
}

function getPluginDir() {
  if (typeof __dirname === "string") return __dirname;
  if (typeof process !== "undefined" && typeof process.cwd === "function") {
    return process.cwd();
  }
  return null;
}

function readPromptFile() {
  if (!fs || !path) return null;
  const baseDir = getPluginDir();
  if (!baseDir) return null;
  const promptPath = path.join(baseDir, "prompt.txt");
  try {
    if (!fs.existsSync(promptPath)) return null;
    const data = fs.readFileSync(promptPath);
    const text = decodeTextBuffer(data).trim();
    return text || null;
  } catch (e) {
    return null;
  }
}

function resolveSystemPrompt(config) {
  return readPromptFile() || config.prompt;
}

function listFilesRecursively(rootDir, maxFiles) {
  if (!fs || !path) return [];
  const results = [];
  const stack = [rootDir];

  while (stack.length) {
    const current = stack.pop();
    let entries = [];
    try {
      entries = fs.readdirSync(current, { withFileTypes: true });
    } catch (e) {
      continue;
    }

    for (const entry of entries) {
      const fullPath = path.join(current, entry.name);
      if (entry.isDirectory()) {
        stack.push(fullPath);
      } else if (entry.isFile()) {
        results.push(fullPath);
        if (maxFiles && results.length >= maxFiles) {
          return results;
        }
      }
    }
  }

  return results;
}

function chooseLogFile(files, keywords) {
  if (!Array.isArray(files) || files.length === 0 || !path) return null;
  const normalizedKeywords = Array.isArray(keywords)
    ? keywords.map((k) => String(k || "").toLowerCase())
    : [];

  const byName = (filePath) => path.basename(filePath || "");
  const filesByName = files.map((filePath) => ({
    path: filePath,
    name: byName(filePath),
    lower: byName(filePath).toLowerCase(),
  }));

  for (const keyword of normalizedKeywords) {
    if (!keyword) continue;
    const exact = filesByName.find((f) => f.lower === keyword);
    if (exact) return exact.path;
  }

  for (const keyword of normalizedKeywords) {
    if (!keyword) continue;
    const partial = filesByName.find((f) => f.lower.includes(keyword));
    if (partial) return partial.path;
  }

  const logFile = filesByName.find((f) => f.lower.endsWith(".log"));
  if (logFile) return logFile.path;

  const textFile = filesByName.find((f) => f.lower.endsWith(".txt"));
  if (textFile) return textFile.path;

  return filesByName[0].path;
}

function getDirectorySize(rootDir, maxBytes) {
  if (!fs || !path) return 0;
  let total = 0;
  const stack = [rootDir];

  while (stack.length) {
    const current = stack.pop();
    let entries = [];
    try {
      entries = fs.readdirSync(current, { withFileTypes: true });
    } catch (e) {
      continue;
    }

    for (const entry of entries) {
      const fullPath = path.join(current, entry.name);
      if (entry.isDirectory()) {
        stack.push(fullPath);
      } else if (entry.isFile()) {
        try {
          const stat = fs.statSync(fullPath);
          total += stat.size || 0;
          if (maxBytes && total > maxBytes) {
            return total;
          }
        } catch (e) {
          // ignore
        }
      }
    }
  }

  return total;
}

function decodeTextBuffer(buffer) {
  if (!buffer) return "";
  if (!TextDecoderCtor) {
    return buffer.toString ? buffer.toString("utf8") : String(buffer);
  }

  const tryDecode = (encoding, fatal) => {
    try {
      const decoder = new TextDecoderCtor(encoding, { fatal: !!fatal });
      return decoder.decode(buffer);
    } catch (e) {
      return null;
    }
  };

  return (
    tryDecode("utf-8", true) ||
    tryDecode("utf-8", false) ||
    tryDecode("latin1", false) ||
    ""
  );
}

function readTextFileLimited(filePath, maxBytes) {
  if (!fs) return "";
  try {
    const stat = fs.statSync(filePath);
    const size = stat && stat.size ? stat.size : 0;
    const readBytes = maxBytes && size > maxBytes ? maxBytes : size;
    if (!readBytes) return "";

    const fd = fs.openSync(filePath, "r");
    try {
      const buffer = Buffer.alloc(readBytes);
      const bytesRead = fs.readSync(fd, buffer, 0, readBytes, 0);
      const sliced = bytesRead < buffer.length ? buffer.slice(0, bytesRead) : buffer;
      return decodeTextBuffer(sliced);
    } finally {
      fs.closeSync(fd);
    }
  } catch (e) {
    return "";
  }
}

function isCommandAvailable(command) {
  if (!childProcess) return false;
  try {
    const result = childProcess.spawnSync(command, ["--help"], { stdio: "ignore" });
    if (result && result.error) return false;
    return result && typeof result.status === "number";
  } catch (e) {
    return false;
  }
}

function runCommand(command, args) {
  if (!childProcess) return { ok: false, error: "缺少子进程模块" };
  try {
    const result = childProcess.spawnSync(command, args, { stdio: "ignore" });
    if (result.error) {
      return { ok: false, error: result.error.message || "执行失败" };
    }
    if (result.status !== 0) {
      return { ok: false, error: `解压失败(${result.status})` };
    }
    return { ok: true };
  } catch (e) {
    return { ok: false, error: e && e.message ? e.message : "执行失败" };
  }
}

function extractArchive(archivePath, destDir) {
  if (!fs || !path) return { ok: false, error: "缺少文件模块" };
  const lower = String(archivePath || "").toLowerCase();
  if (lower.endsWith(".tar.gz") || lower.endsWith(".tgz")) {
    if (isCommandAvailable("tar")) {
      return runCommand("tar", ["-xf", archivePath, "-C", destDir]);
    }
    return { ok: false, error: "缺少 tar 命令" };
  }

  if (lower.endsWith(".tar")) {
    if (isCommandAvailable("tar")) {
      return runCommand("tar", ["-xf", archivePath, "-C", destDir]);
    }
    return { ok: false, error: "缺少 tar 命令" };
  }

  if (lower.endsWith(".zip")) {
    if (isCommandAvailable("unzip")) {
      return runCommand("unzip", ["-o", archivePath, "-d", destDir]);
    }
    if (isCommandAvailable("7z")) {
      return runCommand("7z", ["x", "-y", `-o${destDir}`, archivePath]);
    }
    if (isCommandAvailable("7za")) {
      return runCommand("7za", ["x", "-y", `-o${destDir}`, archivePath]);
    }
    return { ok: false, error: "缺少解压工具(unzip/7z)" };
  }

  if (lower.endsWith(".7z") || lower.endsWith(".rar")) {
    if (isCommandAvailable("7z")) {
      return runCommand("7z", ["x", "-y", `-o${destDir}`, archivePath]);
    }
    if (isCommandAvailable("7za")) {
      return runCommand("7za", ["x", "-y", `-o${destDir}`, archivePath]);
    }
    return { ok: false, error: "缺少解压工具(7z)" };
  }

  if (lower.endsWith(".gz")) {
    if (!zlib || !fs) return { ok: false, error: "缺少解压模块" };
    try {
      const outputName = path.basename(archivePath, ".gz");
      const outputPath = path.join(destDir, outputName);
      const input = fs.readFileSync(archivePath);
      const output = zlib.gunzipSync(input);
      fs.writeFileSync(outputPath, output);
      return { ok: true };
    } catch (e) {
      return { ok: false, error: "GZ 解压失败" };
    }
  }

  return { ok: false, error: "不支持的压缩格式" };
}

function downloadFileToPath(url, destPath, maxBytes, redirectsLeft = 3) {
  if (!fs || (!http && !https)) {
    return Promise.resolve({ ok: false, error: "缺少下载模块" });
  }
  const client = String(url || "").toLowerCase().startsWith("https://") ? https : http;
  if (!client) {
    return Promise.resolve({ ok: false, error: "不支持的协议" });
  }

  return new Promise((resolve) => {
    let done = false;
    const finish = (result) => {
      if (done) return;
      done = true;
      resolve(result);
    };

    const request = client.get(url, (response) => {
      if (
        response.statusCode >= 300 &&
        response.statusCode < 400 &&
        response.headers.location &&
        redirectsLeft > 0
      ) {
        response.resume();
        downloadFileToPath(
          response.headers.location,
          destPath,
          maxBytes,
          redirectsLeft - 1
        ).then(finish);
        return;
      }

      if (response.statusCode !== 200) {
        response.resume();
        return finish({ ok: false, error: `下载失败(${response.statusCode})` });
      }

      const contentLength = Number(response.headers["content-length"] || 0);
      if (maxBytes && contentLength && contentLength > maxBytes) {
        response.resume();
        return finish({ ok: false, error: "文件过大，已拒绝下载" });
      }

      const fileStream = fs.createWriteStream(destPath);
      let downloaded = 0;
      let aborted = false;

      response.on("data", (chunk) => {
        downloaded += chunk.length;
        if (maxBytes && downloaded > maxBytes && !aborted) {
          aborted = true;
          response.destroy();
          fileStream.destroy();
          removePath(destPath);
          finish({ ok: false, error: "文件过大，已中止下载" });
        }
      });

      fileStream.on("finish", () => {
        if (!aborted) {
          finish({ ok: true, bytes: downloaded });
        }
      });

      fileStream.on("error", () => {
        if (!aborted) {
          removePath(destPath);
          finish({ ok: false, error: "写入失败" });
        }
      });

      response.on("error", () => {
        if (!aborted) {
          removePath(destPath);
          finish({ ok: false, error: "下载失败" });
        }
      });

      response.pipe(fileStream);
    });

    request.on("error", () => {
      finish({ ok: false, error: "下载失败" });
    });
  });
}
function extractFileFromSegments(message) {
  if (!Array.isArray(message)) return null;
  for (const seg of message) {
    if (!seg || typeof seg !== "object") continue;
    if (String(seg.type || "") !== "file") continue;
    const data = seg.data || {};
    const fileField = typeof data.file === "string" ? data.file : "";
    const guessedName =
      (typeof data.name === "string" && data.name.trim()) ||
      (typeof data.file_name === "string" && data.file_name.trim()) ||
      (typeof data.filename === "string" && data.filename.trim()) ||
      (fileField && !looksLikeUrl(fileField) && hasExtension(fileField) ? fileField : "");
    const name = guessedName || null;
    const urlCandidate =
      data.url || data.file_url || data.download_url || data.down_url || data.path;
    const url = looksLikeUrl(urlCandidate)
      ? urlCandidate
      : looksLikeUrl(data.file)
        ? data.file
        : null;
    const fileId =
      data.file_id || data.fileId || data.fid || (!url && data.file ? data.file : null);
    const busid = data.busid || data.busi_id || data.busId || data.busiId;
    if (url || fileId) {
      return { url, fileId, busid, name };
    }
  }
  return null;
}

function extractTargetFromReply(reply) {
  if (!reply || typeof reply !== "object") return null;

  const fileName =
    reply.file_name || reply.fileName || reply.filename || reply.name || "日志文件";
  let fileUrl = reply.file_url || reply.fileUrl;
  if (!fileUrl && looksLikeUrl(reply.file)) {
    fileUrl = reply.file;
  }
  const fileId =
    (!fileUrl && (reply.file_id || reply.fileId)) || (!fileUrl ? reply.file : null);
  const busid = reply.busid || reply.busi_id || reply.busId || reply.busiId;

  if (fileUrl || fileId) {
    const fileField = typeof reply.file === "string" ? reply.file : "";
    const name =
      (typeof fileName === "string" && fileName.trim() && fileName !== "日志文件"
        ? fileName
        : fileField && !looksLikeUrl(fileField) && hasExtension(fileField)
          ? fileField
          : fileName) || "日志文件";
    return {
      type: "file",
      url: fileUrl || null,
      fileId: fileUrl ? null : fileId,
      busid,
      name,
    };
  }

  const text = reply.forward_text || reply.raw_message || reply.text || "";
  if (text) {
    return { type: "text", text };
  }

  return null;
}

function extractTargetFromCtx(ctx) {
  if (!ctx) return null;

  const fileName =
    ctx.file_name || ctx.fileName || ctx.filename || ctx.name || "日志文件";
  let fileUrl = ctx.file_url || ctx.fileUrl;
  if (!fileUrl && looksLikeUrl(ctx.file)) {
    fileUrl = ctx.file;
  }
  const fileId = (!fileUrl && (ctx.file_id || ctx.fileId)) || (!fileUrl ? ctx.file : null);
  const busid = ctx.busid || ctx.busi_id || ctx.busId || ctx.busiId;

  if (fileUrl || fileId) {
    const fileField = typeof ctx.file === "string" ? ctx.file : "";
    const name =
      (typeof fileName === "string" && fileName.trim() && fileName !== "日志文件"
        ? fileName
        : fileField && !looksLikeUrl(fileField) && hasExtension(fileField)
          ? fileField
          : fileName) || "日志文件";
    return {
      type: "file",
      url: fileUrl || null,
      fileId: fileUrl ? null : fileId,
      busid,
      name,
    };
  }

  if (ctx.file && typeof ctx.file === "object") {
    const url = ctx.file.url || ctx.file.file_url || ctx.file.file;
    const name = ctx.file.name || ctx.file.file_name || ctx.file.filename;
    if (url) {
      return { type: "file", url, name: name || "日志文件" };
    }
    const fileIdAlt = ctx.file.file_id || ctx.file.fileId || ctx.file.fid;
    const busidAlt = ctx.file.busid || ctx.file.busi_id || ctx.file.busId || ctx.file.busiId;
    if (fileIdAlt) {
      return {
        type: "file",
        url: null,
        fileId: fileIdAlt,
        busid: busidAlt,
        name: name || "日志文件",
      };
    }
  }

  const segFile = extractFileFromSegments(ctx.message);
  if (segFile) {
    return {
      type: "file",
      url: segFile.url || null,
      fileId: segFile.url ? null : segFile.fileId,
      busid: segFile.busid,
      name: segFile.name || "日志文件",
    };
  }

  const noticeFile =
    ctx.raw_event &&
    typeof ctx.raw_event === "object" &&
    ctx.raw_event.file &&
    typeof ctx.raw_event.file === "object"
      ? ctx.raw_event.file
      : null;

  if (noticeFile) {
    const url = noticeFile.url || noticeFile.file_url || noticeFile.file;
    const name = noticeFile.name || noticeFile.file_name || noticeFile.filename;
    const fileIdAlt = noticeFile.id || noticeFile.file_id || noticeFile.fileId || noticeFile.fid;
    const busidAlt =
      noticeFile.busid || noticeFile.busi_id || noticeFile.busId || noticeFile.busiId;

    if (url) {
      return { type: "file", url, name: name || "日志文件" };
    }

    if (fileIdAlt) {
      return {
        type: "file",
        url: null,
        fileId: fileIdAlt,
        busid: busidAlt,
        name: name || "日志文件",
      };
    }
  }

  const text = ctx.raw_message || "";
  if (text) {
    return { type: "text", text };
  }

  return null;
}

function sendProcessing(userId, groupId, config) {
  if (config.show_processing_msg && config.text3) {
    nbot.sendReply(userId, groupId || 0, config.text3);
  }
}

function handleFileTarget(userId, groupId, target, config) {
  if (shouldDedupeTrigger(userId, groupId, target)) {
    return;
  }

  if (target.url) {
    analyzeFileFromUrl(userId, groupId, target.url, target.name, config);
    return;
  }

  if (target.fileId) {
    requestGroupFileUrl(userId, groupId, target.fileId, target.busid, target.name, config);
    return;
  }

  nbot.sendReply(userId, groupId || 0, "无法获取文件链接");
}

function requestGroupFileUrl(userId, groupId, fileId, busid, name, config) {
  if (!groupId) {
    nbot.sendReply(userId, 0, "仅支持群文件");
    return;
  }
  if (typeof nbot.fetchGroupFileUrl !== "function") {
    nbot.sendReply(userId, groupId, "当前后端不支持拉取群文件链接");
    return;
  }

  const requestId = genRequestId("file-url");
  pendingFileUrlRequests.set(requestId, {
    userId,
    groupId,
    fileId,
    busid,
    name,
    createdAt: nbot.now(),
  });

  sendProcessing(userId, groupId, config);

  if (busid === undefined || busid === null || busid === "") {
    nbot.fetchGroupFileUrl(requestId, Number(groupId), String(fileId));
  } else {
    nbot.fetchGroupFileUrl(requestId, Number(groupId), String(fileId), busid);
  }
}

function extractFileUrlFromResponse(data) {
  if (!data) return null;
  if (typeof data === "string") return { url: data };
  if (typeof data !== "object") return null;
  const url = data.url || data.file_url || data.download_url || data.down_url || data.link;
  if (!url) return null;
  const name = data.name || data.file_name || data.filename;
  return { url, name };
}

async function analyzeArchiveFromUrlLegacy(userId, groupId, url, name, config) {
  if (!canUseLocalFiles()) {
    nbot.sendReply(userId, groupId || 0, "当前环境不支持解压，请解压后上传日志文件");
    return;
  }

  const tempRoot = getTempRoot();
  if (!tempRoot || !ensureDir(tempRoot)) {
    nbot.sendReply(userId, groupId || 0, "无法创建临时目录，已取消分析");
    return;
  }

  const taskId = `${nbot.now()}-${Math.random().toString(16).slice(2, 10)}`;
  const workDir = path.join(tempRoot, taskId);
  const extractDir = path.join(workDir, "extract");
  const safeName = sanitizeFileName(name || "archive");
  const archivePath = path.join(workDir, safeName);

  if (!ensureDir(workDir) || !ensureDir(extractDir)) {
    nbot.sendReply(userId, groupId || 0, "无法创建临时目录，已取消分析");
    return;
  }

  try {
    sendProcessing(userId, groupId, config);

    const downloadResult = await downloadFileToPath(
      url,
      archivePath,
      config.max_download_bytes
    );
    if (!downloadResult.ok) {
      nbot.sendReply(userId, groupId || 0, downloadResult.error || "下载失败");
      return;
    }

    const extractResult = extractArchive(archivePath, extractDir);
    if (!extractResult.ok) {
      nbot.sendReply(
        userId,
        groupId || 0,
        `解压失败：${extractResult.error || "未知错误"}`
      );
      return;
    }

    const totalSize = getDirectorySize(extractDir, config.max_extract_bytes);
    if (config.max_extract_bytes && totalSize > config.max_extract_bytes) {
      nbot.sendReply(userId, groupId || 0, "解压内容过大，已取消分析");
      return;
    }

    const allFiles = listFilesRecursively(extractDir, 2000);
    const textFiles = allFiles.filter((filePath) => isTextFile(filePath));
    const candidates = textFiles.length ? textFiles : allFiles;
    const selectedFile = chooseLogFile(candidates, config.text_file_keywords);

    if (!selectedFile) {
      nbot.sendReply(userId, groupId || 0, "压缩包中未找到可分析的日志文件");
      return;
    }

    const content = readTextFileLimited(selectedFile, config.max_file_bytes);
    if (!content) {
      nbot.sendReply(userId, groupId || 0, "读取日志文件失败或内容为空");
      return;
    }

    analyzeText(userId, groupId, content, config, { skipProcessing: true });
  } finally {
    removePath(workDir);
  }
}

function analyzeText(userId, groupId, text, config, options) {
  const maxLen = config.max_content_length;
  let content = String(text || "");
  if (content.length > maxLen) {
    content = content.substring(0, maxLen);
    nbot.sendReply(userId, groupId || 0, `内容过长，已截取前 ${maxLen} 字符进行分析`);
  }

  if (!options || options.skipProcessing !== true) {
    sendProcessing(userId, groupId, config);
  }

  const systemPrompt = resolveSystemPrompt(config);
  nbot.callLlmForwardMediaBundle(
    userId,
    groupId || 0,
    systemPrompt,
    config.user_prompt,
    "日志分析",
    content,
    [],
    { modelName: config.analysis_model }
  );
}

function analyzeFileFromUrl(userId, groupId, url, name, config) {
  const guessed = guessFileNameFromUrl(url);
  const fileName = name || guessed || "日志文件";
  const lower = String(fileName || "").toLowerCase();
  const isText = isTextFile(lower);
  const isArchive = isArchiveFile(lower);
  const hasExt = hasExtension(fileName);

  if (hasExt && !isText && !isArchive) {
    nbot.sendReply(userId, groupId || 0, "仅支持 txt/log 文本或压缩包文件");
    return;
  }

  if (isArchive && !config.allow_archive) {
    nbot.sendReply(userId, groupId || 0, "当前配置不允许分析压缩包，请解压后上传 txt/log 文件");
    return;
  }

  if (isArchive) {
    analyzeArchiveFromUrl(userId, groupId, url, fileName, config);
    return;
  }

  sendProcessing(userId, groupId, config);

  const maxChars = config.max_content_length;
  const maxBytes = Math.max(1024, Math.min(50_000_000, maxChars * 6));

  const systemPrompt = resolveSystemPrompt(config);
  nbot.callLlmForwardFromUrl(
    userId,
    groupId || 0,
    systemPrompt,
    config.user_prompt,
    url,
    "日志分析",
    fileName,
    30000,
    maxBytes,
    maxChars,
    { modelName: config.analysis_model }
  );
}

function analyzeArchiveFromUrl(userId, groupId, url, fileName, config) {
  if (typeof nbot.callLlmForwardArchiveFromUrl !== "function") {
    nbot.sendReply(userId, groupId || 0, "当前后端不支持分析压缩包，请解压后上传 txt/log 文件");
    return;
  }

  sendProcessing(userId, groupId, config);

  const systemPrompt = resolveSystemPrompt(config);
  const keywords = [
    ...normalizeList(config.text_file_keywords),
    ...normalizeList(config.archive_keywords),
  ];

  nbot.callLlmForwardArchiveFromUrl(
    userId,
    groupId || 0,
    systemPrompt,
    config.user_prompt,
    url,
    "日志分析",
    fileName,
    30000,
    config.max_download_bytes,
    config.max_extract_bytes,
    config.max_file_bytes,
    100,
    keywords,
    { modelName: config.analysis_model }
  );
}

function cleanupPendingFileUrlRequests(config) {
  const now = nbot.now();
  const timeoutMs = config.file_url_timeout_seconds * 1000;
  for (const [requestId, req] of pendingFileUrlRequests.entries()) {
    if (now - req.createdAt <= timeoutMs) continue;
    pendingFileUrlRequests.delete(requestId);
    nbot.sendReply(req.userId, req.groupId || 0, "获取文件链接超时");
  }
}

function cleanupExpiredSessions(config) {
  const now = nbot.now();
  if (now - lastCleanupAt < 1000) return;
  lastCleanupAt = now;

  for (const [key, session] of sessions.entries()) {
    if (now <= session.expiresAt) continue;
    sessions.delete(key);
    if (config.text4) {
      nbot.sendReply(session.userId, session.groupId || 0, config.text4);
    }
  }

  cleanupRecentTriggerKeys(now);
}

return {
  onEnable() {
    nbot.log.info("我的世界日志分析插件已启用");
  },

  onDisable() {
    sessions.clear();
    pendingFileUrlRequests.clear();
    nbot.log.info("我的世界日志分析插件已禁用");
  },

  async onCommand(ctx) {
    const { user_id, group_id, reply_message } = ctx;
    const config = getConfig();
    const gid = group_id || 0;

    if (reply_message) {
      if (reply_message.sender_is_bot) {
        nbot.sendReply(user_id, gid, "为避免循环，无法分析机器人的消息");
        return;
      }

      const target = extractTargetFromReply(reply_message);
      if (!target) {
        nbot.sendReply(user_id, gid, "无法获取被回复消息的内容");
        return;
      }

      if (target.type === "file") {
        handleFileTarget(user_id, gid, target, config);
        return;
      }

      if (target.type === "text") {
        analyzeText(user_id, gid, target.text, config);
        return;
      }

      nbot.sendReply(user_id, gid, "不支持的消息类型");
      return;
    }

    const key = buildSessionKey(user_id, gid);
    sessions.set(key, {
      userId: user_id,
      groupId: gid,
      expiresAt: nbot.now() + config.wait_timeout_seconds * 1000,
    });

    if (config.text1) {
      nbot.sendReply(user_id, gid, config.text1);
    }
  },

  preMessage(ctx) {
    const config = getConfig();
    cleanupExpiredSessions(config);
    cleanupPendingFileUrlRequests(config);

    const { user_id, group_id, raw_message } = ctx;
    if (!user_id) return true;

    const gid = group_id || 0;
    const key = buildSessionKey(user_id, gid);

  if (config.auto_trigger) {
      const autoTarget = extractTargetFromCtx(ctx);
      if (autoTarget && autoTarget.type === "file") {
        const fileName = autoTarget.name || "";
        const lower = String(fileName || "").toLowerCase();
        const isText = isTextFile(lower);
        const isArchive = isArchiveFile(lower);
        const matchResult = isText
          ? matchKeywordsOrdered(fileName, config.text_file_keywords)
          : isArchive
            ? matchKeywordsOrdered(fileName, config.archive_keywords).matched
              ? matchKeywordsOrdered(fileName, config.archive_keywords)
              : matchKeywordsOrdered(fileName, config.text_file_keywords)
            : { matched: false };

        if (matchResult.matched) {
          handleFileTarget(user_id, gid, autoTarget, config);
          return true;
        }
      }
    }

    const session = sessions.get(key);
    if (!session) return true;

    if (nbot.now() > session.expiresAt) {
      sessions.delete(key);
      if (config.text4) {
        nbot.sendReply(user_id, gid, config.text4);
      }
      return true;
    }

    const text = String(raw_message || "").trim();
    if (text && isExitMessage(text, config.exit_keywords)) {
      sessions.delete(key);
      if (config.text2) {
        nbot.sendReply(user_id, gid, config.text2);
      }
      return true;
    }

    const target = extractTargetFromCtx(ctx);
    if (!target) return true;

    if (target.type === "text") {
      if (text.startsWith("/")) {
        return true;
      }
      sessions.delete(key);
      analyzeText(user_id, gid, target.text, config);
      return true;
    }

    if (target.type === "file") {
      sessions.delete(key);
      handleFileTarget(user_id, gid, target, config);
      return true;
    }

    return true;
  },

  onNotice(ctx) {
    const config = getConfig();
    cleanupExpiredSessions(config);
    cleanupPendingFileUrlRequests(config);

    if (!ctx || ctx.notice_type !== "group_upload") {
      return true;
    }

    const groupId = Number(ctx.group_id || 0);
    const userId = Number(ctx.user_id || 0);
    const selfId = Number(ctx.self_id || 0);

    if (!groupId || !userId) return true;
    if (selfId && userId === selfId) return true;

    if (!config.auto_trigger) {
      return true;
    }

    const target = extractTargetFromCtx(ctx);
    if (!target || target.type !== "file") {
      return true;
    }

    const fileName = target.name || "";
    const lower = String(fileName || "").toLowerCase();
    const isText = isTextFile(lower);
    const isArchive = isArchiveFile(lower);
    const matchResult = isText
      ? matchKeywordsOrdered(fileName, config.text_file_keywords)
      : isArchive
        ? matchKeywordsOrdered(fileName, config.archive_keywords).matched
          ? matchKeywordsOrdered(fileName, config.archive_keywords)
          : matchKeywordsOrdered(fileName, config.text_file_keywords)
        : { matched: false };

    if (!matchResult.matched) {
      return true;
    }

    handleFileTarget(userId, groupId, target, config);
    return true;
  },

  onGroupInfoResponse(response) {
    if (!response || response.infoType !== "file_url") {
      return true;
    }

    const requestId = response.requestId;
    const req = pendingFileUrlRequests.get(requestId);
    if (!req) {
      return true;
    }

    pendingFileUrlRequests.delete(requestId);

    if (!response.success) {
      nbot.sendReply(req.userId, req.groupId || 0, "获取文件链接失败");
      return true;
    }

    const info = extractFileUrlFromResponse(response.data);
    if (!info || !info.url) {
      nbot.sendReply(req.userId, req.groupId || 0, "获取文件链接失败");
      return true;
    }

    const config = getConfig();
    analyzeFileFromUrl(req.userId, req.groupId, info.url, info.name || req.name, config);
    return true;
  },

  onMetaEvent(ctx) {
    if (!ctx) return true;
    if (ctx.meta_event_type !== "tick" && ctx.meta_event_type !== "heartbeat") {
      return true;
    }
    const config = getConfig();
    cleanupExpiredSessions(config);
    cleanupPendingFileUrlRequests(config);
    return true;
  }
};
