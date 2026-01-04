use deno_core::JsRuntime;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MediaBundleItem {
    /// image | video | record | file
    #[serde(rename = "type")]
    pub kind: String,
    /// URL for the media (if available)
    #[serde(default)]
    pub url: Option<String>,
    /// Display name (preferred)
    #[serde(default)]
    pub name: Option<String>,
    /// Raw file identifier (e.g. OneBot record `file` field)
    #[serde(default)]
    pub file: Option<String>,
}

/// 插件输出动作
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum PluginOutput {
    /// 更新插件配置（写入 manifest.json 并热更新到运行时）
    UpdateConfig {
        plugin_id: String,
        config: serde_json::Value,
    },
    /// 发送回复消息
    SendReply {
        user_id: u64,
        group_id: Option<u64>,
        content: String,
    },
    /// 调用 QQ API
    CallApi {
        action: String,
        params: serde_json::Value,
    },
    /// 调用 LLM 并发送结果（合并转发）
    CallLlmAndForward {
        user_id: u64,
        group_id: u64,
        system_prompt: String,
        prompt: String,
        content: String,
        title: String,
    },
    /// 从 URL 下载内容后调用 LLM（临时文件，处理完即删除）并发送结果（合并转发）
    CallLlmAndForwardFromUrl {
        user_id: u64,
        group_id: u64,
        system_prompt: String,
        prompt: String,
        url: String,
        title: String,
        #[serde(default)]
        file_name: Option<String>,
        #[serde(default)]
        timeout_ms: u64,
        #[serde(default)]
        max_bytes: u64,
        #[serde(default)]
        max_chars: u64,
    },
    /// 从 URL 下载图片后调用多模态 LLM，并发送结果（合并转发）
    CallLlmAndForwardImageFromUrl {
        user_id: u64,
        group_id: u64,
        system_prompt: String,
        prompt: String,
        url: String,
        title: String,
        #[serde(default)]
        file_name: Option<String>,
        #[serde(default)]
        timeout_ms: u64,
        #[serde(default)]
        max_bytes: u64,
        #[serde(default)]
        max_width: u32,
        #[serde(default)]
        max_height: u32,
        #[serde(default)]
        jpeg_quality: u8,
        #[serde(default)]
        max_output_bytes: u64,
    },
    /// 从 URL 下载视频后抽帧（可选转写音频）并调用多模态 LLM，发送结果（合并转发）
    CallLlmAndForwardVideoFromUrl {
        user_id: u64,
        group_id: u64,
        system_prompt: String,
        prompt: String,
        url: String,
        title: String,
        #[serde(default)]
        file_name: Option<String>,
        /// 视频输入模式：direct（直接传 video_url 给模型）或 frames（本地抽帧后传 image_url）
        #[serde(default)]
        mode: String,
        #[serde(default)]
        timeout_ms: u64,
        #[serde(default)]
        max_bytes: u64,
        #[serde(default)]
        max_frames: u32,
        #[serde(default)]
        frame_max_width: u32,
        #[serde(default)]
        frame_max_height: u32,
        #[serde(default)]
        frame_jpeg_quality: u8,
        #[serde(default)]
        frame_max_output_bytes: u64,
        #[serde(default)]
        transcribe_audio: bool,
        #[serde(default)]
        transcription_model: Option<String>,
        #[serde(default)]
        max_audio_seconds: u32,
        #[serde(default)]
        require_transcript: bool,
    },
    /// 从 URL 下载音频后调用多模态 LLM，发送结果（合并转发）
    CallLlmAndForwardAudioFromUrl {
        user_id: u64,
        group_id: u64,
        system_prompt: String,
        prompt: String,
        url: String,
        title: String,
        #[serde(default)]
        file_name: Option<String>,
        /// OneBot 语音（record）段的 file 字段，用于在 URL 不可用时回退调用 get_record
        #[serde(default)]
        record_file: Option<String>,
        #[serde(default)]
        timeout_ms: u64,
        #[serde(default)]
        max_bytes: u64,
        #[serde(default)]
        max_audio_seconds: u32,
        #[serde(default)]
        require_transcript: bool,
    },
    /// 多媒体 bundle：可包含文本 + 多个媒体附件（图片/视频/语音/文件），并调用多模态 LLM 后发送结果（合并转发）
    CallLlmAndForwardMediaBundle {
        user_id: u64,
        group_id: u64,
        system_prompt: String,
        prompt: String,
        title: String,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        items: Vec<MediaBundleItem>,
        #[serde(default)]
        timeout_ms: u64,
        // Image options
        #[serde(default)]
        image_max_bytes: u64,
        #[serde(default)]
        image_max_width: u32,
        #[serde(default)]
        image_max_height: u32,
        #[serde(default)]
        image_jpeg_quality: u8,
        #[serde(default)]
        image_max_output_bytes: u64,
        // Video / Audio options
        #[serde(default)]
        video_max_bytes: u64,
        #[serde(default)]
        audio_max_bytes: u64,
    },
    /// 调用 LLM 进行多轮对话（异步返回结果，不直接发送）
    CallLlmChat {
        /// 请求 ID，用于匹配响应
        request_id: String,
        /// 指定模型映射名称（如 "decision"、"reply"），为空则使用默认模型
        #[serde(default)]
        model_name: Option<String>,
        /// 消息列表 [{"role": "system"|"user"|"assistant", "content": "..."}]
        messages: Vec<serde_json::Value>,
        /// 最大 token 数
        #[serde(default)]
        max_tokens: Option<u32>,
    },
    /// 调用支持联网搜索的 LLM（异步返回结果）
    CallLlmChatWithSearch {
        /// 请求 ID，用于匹配响应
        request_id: String,
        /// 指定模型映射名称，为空则使用 websearch 默认模型
        #[serde(default)]
        model_name: Option<String>,
        /// 消息列表
        messages: Vec<serde_json::Value>,
        /// 最大 token 数
        #[serde(default)]
        max_tokens: Option<u32>,
        /// 是否启用联网搜索（默认 true）
        #[serde(default)]
        enable_search: Option<bool>,
    },
    /// 发送合并转发消息
    SendForwardMessage {
        user_id: u64,
        group_id: u64,
        /// 转发消息节点列表 [{ name, content }]
        nodes: Vec<ForwardNode>,
    },
    /// 获取群公告（异步返回结果）
    FetchGroupNotice {
        /// 请求 ID，用于匹配响应
        request_id: String,
        /// 群号
        group_id: u64,
    },
    /// 获取群历史消息（异步返回结果）
    FetchGroupMsgHistory {
        /// 请求 ID，用于匹配响应
        request_id: String,
        /// 群号
        group_id: u64,
        /// 获取消息数量
        #[serde(default)]
        count: Option<u32>,
        /// 起始消息序号（用于分页）
        #[serde(default)]
        message_seq: Option<u64>,
    },
    /// 获取群文件列表（异步返回结果）
    FetchGroupFiles {
        /// 请求 ID，用于匹配响应
        request_id: String,
        /// 群号
        group_id: u64,
        /// 文件夹 ID（空字符串或不传表示根目录）
        #[serde(default)]
        folder_id: Option<String>,
    },
    /// 获取群文件下载链接（异步返回结果）
    FetchGroupFileUrl {
        /// 请求 ID，用于匹配响应
        request_id: String,
        /// 群号
        group_id: u64,
        /// 文件 ID
        file_id: String,
        /// busid
        #[serde(default)]
        busid: Option<u32>,
    },
    /// 获取好友列表（异步返回结果）
    FetchFriendList {
        /// 请求 ID，用于匹配响应
        request_id: String,
    },
    /// 获取群列表（异步返回结果）
    FetchGroupList {
        /// 请求 ID，用于匹配响应
        request_id: String,
    },
    /// 获取群成员列表（异步返回结果）
    FetchGroupMemberList {
        /// 请求 ID，用于匹配响应
        request_id: String,
        /// 群号
        group_id: u64,
    },
    /// 下载文件到缓存目录（异步返回结果）
    DownloadFile {
        /// 请求 ID，用于匹配响应
        request_id: String,
        /// 文件 URL
        url: String,
        /// 线程数（默认 3）
        #[serde(default)]
        thread_count: Option<u32>,
        /// 自定义请求头
        #[serde(default)]
        headers: Option<Vec<String>>,
    },
}

/// 合并转发消息节点
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ForwardNode {
    /// 发送者名称
    pub name: String,
    /// 消息内容
    pub content: String,
}

#[derive(Default)]
pub(super) struct PluginOpState {
    pub(super) plugin_id: String,
    pub(super) config: serde_json::Value,
    pub(super) data_dir: String,
    pub(super) hook_result: Option<bool>,
    pub(super) outputs: Vec<PluginOutput>,
}

pub(super) fn take_outputs(runtime: &mut JsRuntime) -> Vec<PluginOutput> {
    let op_state = runtime.op_state();
    let mut op_state = op_state.borrow_mut();
    let state = op_state.borrow_mut::<PluginOpState>();
    std::mem::take(&mut state.outputs)
}

pub(super) fn reset_hook_state(runtime: &mut JsRuntime) {
    let op_state = runtime.op_state();
    let mut op_state = op_state.borrow_mut();
    let state = op_state.borrow_mut::<PluginOpState>();
    state.hook_result = None;
    state.outputs.clear();
}

pub(super) fn get_hook_result(runtime: &mut JsRuntime) -> bool {
    let op_state = runtime.op_state();
    let op_state = op_state.borrow();
    let state = op_state.borrow::<PluginOpState>();
    state.hook_result.unwrap_or(true)
}
