// nBot Plugin SDK Runtime
const core = Deno.core;

const toBigInt = (v) => {
  if (typeof v === "bigint") return v;
  if (v === null || v === undefined) return 0n;
  const s = String(v).trim();
  if (!s) return 0n;
  try {
    return BigInt(s);
  } catch {
    return 0n;
  }
};

globalThis.nbot = {
  // CQ helper: mention (at) a user
  at: (userId) => {
    const qq = String(userId ?? "").trim();
    return qq ? `[CQ:at,qq=${qq}]` : "";
  },

  // Send message to QQ group (legacy)
  sendMessage: (groupId, content) => {
    return core.ops.op_send_message(toBigInt(groupId), content);
  },

  // Send reply message
  sendReply: (userId, groupId, content) => {
    return core.ops.op_send_reply(toBigInt(userId), toBigInt(groupId || 0), content);
  },

  // Call QQ API
  callApi: (action, params = {}) => {
    return core.ops.op_call_api(action, JSON.stringify(params));
  },

  // Call LLM and send result as forward message
  callLlmForward: (userId, groupId, systemPrompt, prompt, content, title) => {
    return core.ops.op_call_llm_forward(
      toBigInt(userId),
      toBigInt(groupId || 0),
      systemPrompt,
      prompt,
      content,
      title
    );
  },

  // Call LLM by downloading content from URL (temp file, removed after processing)
  callLlmForwardFromUrl: (
    userId,
    groupId,
    systemPrompt,
    prompt,
    url,
    title,
    fileName = "",
    timeoutMs = 30000,
    maxBytes = 2_000_000,
    maxChars = 50_000,
    options = {}
  ) => {
    const payload = {
      model_name: options.modelName ? String(options.modelName) : null,
      url: String(url),
      title: String(title),
      file_name: fileName ? String(fileName) : null,
      timeout_ms: timeoutMs,
      max_bytes: maxBytes,
      max_chars: maxChars,
    };
    return core.ops.op_call_llm_forward_from_url(
      toBigInt(userId),
      toBigInt(groupId || 0),
      systemPrompt,
      prompt,
      JSON.stringify(payload)
    );
  },

  // Call LLM by downloading an archive URL and extracting a log/text file (temp file, removed after processing)
  // Supported: .zip / .tar / .tar.gz (.tgz) / .gz
  callLlmForwardArchiveFromUrl: (
    userId,
    groupId,
    systemPrompt,
    prompt,
    url,
    title,
    fileName = "",
    timeoutMs = 30000,
    maxDownloadBytes = 30_000_000,
    maxExtractBytes = 120_000_000,
    maxFileBytes = 15_000_000,
    maxFiles = 50,
    keywords = [],
    options = {}
  ) => {
    const payload = {
      model_name: options.modelName ? String(options.modelName) : null,
      url: String(url),
      title: String(title),
      file_name: fileName ? String(fileName) : null,
      timeout_ms: timeoutMs,
      max_download_bytes: maxDownloadBytes,
      max_extract_bytes: maxExtractBytes,
      max_file_bytes: maxFileBytes,
      max_files: maxFiles,
      keywords: Array.isArray(keywords) ? keywords.map((x) => String(x)) : [],
    };
    return core.ops.op_call_llm_forward_archive_from_url(
      toBigInt(userId),
      toBigInt(groupId || 0),
      systemPrompt,
      prompt,
      JSON.stringify(payload)
    );
  },

  // Call multimodal LLM with an image from URL (downloaded by core, temp file removed after processing)
  callLlmForwardImageFromUrl: (
    userId,
    groupId,
    systemPrompt,
    prompt,
    url,
    title,
    fileName = "",
    timeoutMs = 30000,
    maxBytes = 10_000_000,
    maxWidth = 1024,
    maxHeight = 1024,
    jpegQuality = 85,
    maxOutputBytes = 2_000_000,
    options = {}
  ) => {
    const payload = {
      model_name: options.modelName ? String(options.modelName) : null,
      url: String(url),
      title: String(title),
      file_name: fileName ? String(fileName) : null,
      timeout_ms: timeoutMs,
      max_bytes: maxBytes,
      max_width: maxWidth,
      max_height: maxHeight,
      jpeg_quality: jpegQuality,
      max_output_bytes: maxOutputBytes,
    };
    return core.ops.op_call_llm_forward_image_from_url(
      toBigInt(userId),
      toBigInt(groupId || 0),
      systemPrompt,
      prompt,
      JSON.stringify(payload)
    );
  },

  // Call multimodal LLM with a video from URL (downloaded by core, frames extracted, temp files removed after processing)
  callLlmForwardVideoFromUrl: (
    userId,
    groupId,
    systemPrompt,
    prompt,
    url,
    title,
    fileName = "",
    timeoutMs = 30000,
    maxBytes = 50_000_000,
    maxFrames = 12,
    frameMaxWidth = 1024,
    frameMaxHeight = 1024,
    frameJpegQuality = 80,
    frameMaxOutputBytes = 600_000,
    transcribeAudio = true,
    transcriptionModel = "whisper-1",
    maxAudioSeconds = 180,
    requireTranscript = false,
    mode = "direct",
    options = {}
  ) => {
    const payload = {
      model_name: options.modelName ? String(options.modelName) : null,
      url: String(url),
      title: String(title),
      file_name: fileName ? String(fileName) : null,
      mode: mode ? String(mode) : null,
      timeout_ms: timeoutMs,
      max_bytes: maxBytes,
      max_frames: maxFrames,
      frame_max_width: frameMaxWidth,
      frame_max_height: frameMaxHeight,
      frame_jpeg_quality: frameJpegQuality,
      frame_max_output_bytes: frameMaxOutputBytes,
      transcribe_audio: !!transcribeAudio,
      transcription_model: transcriptionModel ? String(transcriptionModel) : null,
      max_audio_seconds: maxAudioSeconds,
      require_transcript: !!requireTranscript,
    };
    return core.ops.op_call_llm_forward_video_from_url(
      toBigInt(userId),
      toBigInt(groupId || 0),
      systemPrompt,
      prompt,
      JSON.stringify(payload)
    );
  },

  // Call multimodal LLM with an audio from URL (downloaded by core, temp file removed after processing)
  callLlmForwardAudioFromUrl: (
    userId,
    groupId,
    systemPrompt,
    prompt,
    url,
    title,
    fileName = "",
    timeoutMs = 30000,
    maxBytes = 20_000_000,
    maxAudioSeconds = 180,
    requireTranscript = false,
    recordFile = "",
    options = {}
  ) => {
    const payload = {
      model_name: options.modelName ? String(options.modelName) : null,
      url: String(url),
      title: String(title),
      file_name: fileName ? String(fileName) : null,
      record_file: recordFile ? String(recordFile) : null,
      timeout_ms: timeoutMs,
      max_bytes: maxBytes,
      max_audio_seconds: maxAudioSeconds,
      require_transcript: !!requireTranscript,
    };
    return core.ops.op_call_llm_forward_audio_from_url(
      toBigInt(userId),
      toBigInt(groupId || 0),
      systemPrompt,
      prompt,
      JSON.stringify(payload)
    );
  },

  // Call multimodal LLM with a bundle: optional text + multiple media items (downloaded by core, temp files removed after processing)
  callLlmForwardMediaBundle: (
    userId,
    groupId,
    systemPrompt,
    prompt,
    title,
    text,
    items = [],
    options = {}
  ) => {
    const payload = {
      model_name: options.modelName ? String(options.modelName) : null,
      title: String(title || "Multimodal Analysis"),
      text: text ? String(text) : null,
      items: Array.isArray(items) ? items : [],
      timeout_ms: options.timeout_ms,
      image_max_bytes: options.image_max_bytes,
      image_max_width: options.image_max_width,
      image_max_height: options.image_max_height,
      image_jpeg_quality: options.image_jpeg_quality,
      image_max_output_bytes: options.image_max_output_bytes,
      video_max_bytes: options.video_max_bytes,
      audio_max_bytes: options.audio_max_bytes,
    };
    return core.ops.op_call_llm_forward_media_bundle(
      toBigInt(userId),
      toBigInt(groupId || 0),
      systemPrompt,
      prompt,
      JSON.stringify(payload)
    );
  },

  // Call LLM for multi-turn chat (async, result returned via onLlmResponse hook)
  // requestId: unique identifier for matching response
  // messages: array of {role: "system"|"user"|"assistant", content: "..."}
  // options: { modelName?: string, maxTokens?: number }
  // Returns immediately; result delivered via onLlmResponse({ requestId, success, content })
  callLlmChat: (requestId, messages, options = {}) => {
    const payload = {
      request_id: String(requestId),
      model_name: options.modelName ? String(options.modelName) : null,
      messages: Array.isArray(messages) ? messages : [],
      max_tokens: options.maxTokens || null,
    };
    return core.ops.op_call_llm_chat(JSON.stringify(payload));
  },

  // Call LLM with web search capability (async, result returned via onLlmResponse hook)
  // requestId: unique identifier for matching response
  // messages: array of {role: "system"|"user"|"assistant", content: "..."}
  // options: { modelName?: string, maxTokens?: number, enableSearch?: boolean }
  // Returns immediately; result delivered via onLlmResponse({ requestId, success, content })
  callLlmChatWithSearch: (requestId, messages, options = {}) => {
    const payload = {
      request_id: String(requestId),
      model_name: options.modelName ? String(options.modelName) : null,
      messages: Array.isArray(messages) ? messages : [],
      max_tokens: options.maxTokens || null,
      enable_search: options.enableSearch !== false, // default true
    };
    return core.ops.op_call_llm_chat_with_search(JSON.stringify(payload));
  },

  // Send forward message (merged forward message)
  // userId: target user ID
  // groupId: target group ID (0 for private message)
  // nodes: array of { name: string, content: string | onebotMessageSegments }
  sendForwardMessage: (userId, groupId, nodes) => {
    const normalizeContent = (c) => {
      if (c === undefined || c === null) return "";
      if (typeof c === "string") return c;
      if (Array.isArray(c)) return c;
      if (typeof c === "object") return c;
      return String(c);
    };
    const payload = {
      nodes: Array.isArray(nodes) ? nodes.map(n => ({
        name: String(n.name || ""),
        content: normalizeContent(n.content),
      })) : [],
    };
    return core.ops.op_send_forward_message(
      toBigInt(userId),
      toBigInt(groupId || 0),
      JSON.stringify(payload)
    );
  },

  // HTTP fetch (async) - download content from URL
  httpFetch: (url, timeoutMs = 30000) => {
    return core.ops.op_http_fetch(url, timeoutMs);
  },

  // Render Markdown into an image (base64) using core renderer
  renderMarkdownImage: (title, meta, markdown, width = 520) => {
    return core.ops.op_render_markdown_image(String(title), String(meta), String(markdown), width);
  },

  // Render raw HTML into an image (base64) using core renderer
  renderHtmlImage: (html, width = 520, quality = 92) => {
    return core.ops.op_render_html_image(String(html), width, quality);
  },

  // Log functions
  log: {
    info: (msg) => core.ops.op_log("info", String(msg)),
    warn: (msg) => core.ops.op_log("warn", String(msg)),
    error: (msg) => core.ops.op_log("error", String(msg)),
  },

  // Get current timestamp (milliseconds)
  now: () => core.ops.op_now(),

  // Get plugin ID
  getPluginId: () => core.ops.op_get_plugin_id(),

  // Get plugin config
  getConfig: () => {
    const configStr = core.ops.op_get_config();
    try {
      return JSON.parse(configStr);
    } catch {
      return {};
    }
  },

  // Set plugin config (persist + hot update)
  setConfig: (config) => {
    try {
      const str = typeof config === 'string' ? config : JSON.stringify(config ?? {});
      return core.ops.op_set_config(str);
    } catch {
      return false;
    }
  },

  // Storage API
  storage: {
    get: (key) => {
      const value = core.ops.op_storage_get(key);
      if (value === null || value === undefined) return null;
      try {
        return JSON.parse(value);
      } catch {
        return value;
      }
    },
    set: (key, value) => {
      const str = typeof value === 'string' ? value : JSON.stringify(value);
      return core.ops.op_storage_set(key, str);
    },
    delete: (key) => core.ops.op_storage_delete(key),
  },

  // Group info fetch APIs (async, result returned via onGroupInfoResponse hook)
  // All these functions return immediately; results are delivered via onGroupInfoResponse({ requestId, infoType, success, data })

  // Fetch group announcements
  // requestId: unique identifier for matching response
  // groupId: group ID
  fetchGroupNotice: (requestId, groupId) => {
    return core.ops.op_fetch_group_notice(String(requestId), toBigInt(groupId));
  },

  // Fetch group message history
  // requestId: unique identifier for matching response
  // groupId: group ID
  // options: { count?: number, messageSeq?: number }
  fetchGroupMsgHistory: (requestId, groupId, options = {}) => {
    return core.ops.op_fetch_group_msg_history(
      String(requestId),
      toBigInt(groupId),
      options.count || 0,
      toBigInt(options.messageSeq || 0)
    );
  },

  // Fetch group files
  // requestId: unique identifier for matching response
  // groupId: group ID
  // folderId: folder ID (empty string or omit for root directory)
  fetchGroupFiles: (requestId, groupId, folderId = "") => {
    return core.ops.op_fetch_group_files(
      String(requestId),
      toBigInt(groupId),
      String(folderId || "")
    );
  },

  // Fetch group file download URL
  // requestId: unique identifier for matching response
  // groupId: group ID
  // fileId: file ID
  // busid: optional busid
  fetchGroupFileUrl: (requestId, groupId, fileId, busid = 0) => {
    return core.ops.op_fetch_group_file_url(
      String(requestId),
      toBigInt(groupId),
      String(fileId),
      busid || 0
    );
  },

  // Fetch friend list
  // requestId: unique identifier for matching response
  fetchFriendList: (requestId) => {
    return core.ops.op_fetch_friend_list(String(requestId));
  },

  // Fetch group list
  // requestId: unique identifier for matching response
  fetchGroupList: (requestId) => {
    return core.ops.op_fetch_group_list(String(requestId));
  },

  // Fetch group member list
  // requestId: unique identifier for matching response
  // groupId: group ID
  fetchGroupMemberList: (requestId, groupId) => {
    return core.ops.op_fetch_group_member_list(String(requestId), toBigInt(groupId));
  },

  // Download file to cache directory
  // requestId: unique identifier for matching response
  // url: file URL
  // options: { threadCount?: number, headers?: string[] }
  downloadFile: (requestId, url, options = {}) => {
    const headersJson = options.headers ? JSON.stringify(options.headers) : "";
    return core.ops.op_download_file(
      String(requestId),
      String(url),
      options.threadCount || 0,
      headersJson
    );
  },
};

// Helper to define plugin
globalThis.definePlugin = (config) => {
  return { default: config };
};

// Export for ES modules
export const sendMessage = globalThis.nbot.sendMessage;
export const sendReply = globalThis.nbot.sendReply;
export const at = globalThis.nbot.at;
export const callApi = globalThis.nbot.callApi;
export const callLlmForward = globalThis.nbot.callLlmForward;
export const callLlmForwardFromUrl = globalThis.nbot.callLlmForwardFromUrl;
export const callLlmForwardArchiveFromUrl = globalThis.nbot.callLlmForwardArchiveFromUrl;
export const callLlmForwardImageFromUrl = globalThis.nbot.callLlmForwardImageFromUrl;
export const callLlmForwardVideoFromUrl = globalThis.nbot.callLlmForwardVideoFromUrl;
export const callLlmForwardAudioFromUrl = globalThis.nbot.callLlmForwardAudioFromUrl;
export const callLlmForwardMediaBundle = globalThis.nbot.callLlmForwardMediaBundle;
export const callLlmChat = globalThis.nbot.callLlmChat;
export const callLlmChatWithSearch = globalThis.nbot.callLlmChatWithSearch;
export const sendForwardMessage = globalThis.nbot.sendForwardMessage;
export const httpFetch = globalThis.nbot.httpFetch;
export const renderMarkdownImage = globalThis.nbot.renderMarkdownImage;
export const renderHtmlImage = globalThis.nbot.renderHtmlImage;
export const log = globalThis.nbot.log;
export const now = globalThis.nbot.now;
export const getConfig = globalThis.nbot.getConfig;
export const setConfig = globalThis.nbot.setConfig;
export const getPluginId = globalThis.nbot.getPluginId;
export const storage = globalThis.nbot.storage;
export const fetchGroupNotice = globalThis.nbot.fetchGroupNotice;
export const fetchGroupMsgHistory = globalThis.nbot.fetchGroupMsgHistory;
export const fetchGroupFiles = globalThis.nbot.fetchGroupFiles;
export const fetchGroupFileUrl = globalThis.nbot.fetchGroupFileUrl;
export const fetchFriendList = globalThis.nbot.fetchFriendList;
export const fetchGroupList = globalThis.nbot.fetchGroupList;
export const fetchGroupMemberList = globalThis.nbot.fetchGroupMemberList;
export const downloadFile = globalThis.nbot.downloadFile;
export const definePlugin = globalThis.definePlugin;
