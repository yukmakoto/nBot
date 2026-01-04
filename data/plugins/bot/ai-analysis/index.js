/**
 * nBot AI分析插件
 * 使用AI分析被回复消息中的内容/附件
 *
 * 用法：回复消息 + /AI分析
 */

return {
  onEnable() {
    nbot.log.info("AI分析插件已启用");
  },

  onDisable() {
    nbot.log.info("AI分析插件已禁用");
  },

  async onCommand(ctx) {
    const { command, user_id, group_id, reply_message } = ctx;

    // 只处理 AI分析 命令
    if (command !== "AI分析" && command !== "ai分析") {
      return;
    }

    // 非回复模式：静默
    if (!reply_message) {
      return;
    }

    const gid = group_id || 0;

    const config = nbot.getConfig();
    const analysisModel = String(config.analysis_model || "").trim() || "default";
    const systemPrompt = config.system_prompt || "你是一个专业的分析助手，擅长分析各种文件和内容。请用中文回复，分析要详细、有条理。";
    const defaultPrompt = config.default_prompt || "请分析以下内容";
    const maxLength = config.max_content_length || 50000;
    const showProcessing = config.show_processing_msg !== false;

    // Image options
    const maxImageBytes = config.max_image_bytes || 10_000_000;
    const imageMaxWidth = config.image_max_width || 1024;
    const imageMaxHeight = config.image_max_height || 1024;
    const imageJpegQuality = config.image_jpeg_quality || 85;
    const imageMaxOutputBytes = config.image_max_output_bytes || 2_000_000;

    // Video options
    const maxVideoBytes = config.max_video_bytes || 50_000_000;
    const videoMaxFrames = config.video_max_frames || 12;
    const videoFrameMaxWidth = config.video_frame_max_width || 1024;
    const videoFrameMaxHeight = config.video_frame_max_height || 1024;
    const videoFrameJpegQuality = config.video_frame_jpeg_quality || 80;
    const videoFrameMaxOutputBytes = config.video_frame_max_output_bytes || 600_000;
    const videoTranscribeAudio = config.video_transcribe_audio !== false;
    const videoTranscriptionModel = config.video_transcription_model || "whisper-1";
    const videoMaxAudioSeconds = config.video_max_audio_seconds || 180;
    const videoRequireTranscript = config.video_require_transcript === true;
    const videoInputMode = config.video_input_mode || "direct";

    // Audio options (pure voice/record)
    const maxAudioBytes = config.max_audio_bytes || 20_000_000;
    const audioMaxAudioSeconds = config.audio_max_audio_seconds || 180;
    const audioRequireTranscript = config.audio_require_transcript === true;

    if (reply_message.sender_is_bot) {
      nbot.sendReply(user_id, gid, "为避免循环与滥用，无法分析机器人的消息");
      return;
    }

    const prompt = defaultPrompt;
    const llmOptions = {
      modelName: analysisModel,
      timeout_ms: 30000,
      image_max_bytes: maxImageBytes,
      image_max_width: imageMaxWidth,
      image_max_height: imageMaxHeight,
      image_jpeg_quality: imageJpegQuality,
      image_max_output_bytes: imageMaxOutputBytes,
      video_max_bytes: maxVideoBytes,
      audio_max_bytes: maxAudioBytes,
    };

      // 图片：多模态分析
      if (reply_message.image_url) {
        const fileName = reply_message.image_name || "图片";
        if (showProcessing) {
          nbot.sendReply(user_id, gid, "正在分析 图片，请稍候...");
        }

        nbot.callLlmForwardImageFromUrl(
          user_id,
          gid,
          systemPrompt,
          prompt,
          reply_message.image_url,
          "图片分析",
          fileName,
          30000,
          maxImageBytes,
          imageMaxWidth,
          imageMaxHeight,
          imageJpegQuality,
          imageMaxOutputBytes,
          { modelName: analysisModel }
        );
        return;
      }

      // 视频：抽帧 +（可选）音频转写 + 多模态分析
      if (reply_message.video_url) {
        const fileName = reply_message.video_name || "视频";
        if (showProcessing) {
          nbot.sendReply(user_id, gid, "正在分析 视频，请稍候...");
        }

        nbot.callLlmForwardVideoFromUrl(
          user_id,
          gid,
          systemPrompt,
          prompt,
          reply_message.video_url,
          "视频分析",
          fileName,
          30000,
          maxVideoBytes,
          videoMaxFrames,
          videoFrameMaxWidth,
          videoFrameMaxHeight,
          videoFrameJpegQuality,
          videoFrameMaxOutputBytes,
          videoTranscribeAudio,
          videoTranscriptionModel,
          videoMaxAudioSeconds,
          videoRequireTranscript,
          videoInputMode,
          { modelName: analysisModel }
        );
        return;
      }

      // 语音：多模态分析
      if (reply_message.record_url || reply_message.record_file) {
        const fileName = reply_message.record_name || "语音";
        if (showProcessing) {
          nbot.sendReply(user_id, gid, "正在分析 语音，请稍候...");
        }

        nbot.callLlmForwardAudioFromUrl(
          user_id,
          gid,
          systemPrompt,
          prompt,
          reply_message.record_url || "",
          "语音分析",
          fileName,
          30000,
          maxAudioBytes,
          audioMaxAudioSeconds,
          audioRequireTranscript,
          reply_message.record_file || "",
          { modelName: analysisModel }
        );
        return;
      }

      // 文件：当作文本下载后分析
      if (reply_message.file_url) {
        const fileName = reply_message.file_name || "未知文件";
        if (showProcessing) {
          nbot.sendReply(user_id, gid, "正在分析 文件，请稍候...");
        }

        const maxChars = maxLength;
        const maxBytes = Math.max(1024, Math.min(50_000_000, maxChars * 6));

        nbot.callLlmForwardFromUrl(
          user_id,
          gid,
          systemPrompt,
          prompt,
          reply_message.file_url,
          "文件分析",
          fileName,
          30000,
          maxBytes,
          maxChars,
          { modelName: analysisModel }
        );
        return;
      }

      // 合并转发：展开全文分析
      if (reply_message.forward_text) {
        const forwardMedia = Array.isArray(reply_message.forward_media)
          ? reply_message.forward_media
          : [];

        if (reply_message.forward_truncated) {
          nbot.sendReply(user_id, gid, "合并转发内容较长，已自动截断后再分析");
        }
        if (reply_message.forward_media_truncated) {
          nbot.sendReply(user_id, gid, "合并转发附件较多，已自动截断部分附件后再分析");
        }

        let content = String(reply_message.forward_text || "");
        if (content.length > maxLength) {
          content = content.substring(0, maxLength);
          nbot.sendReply(user_id, gid, `内容过长，已截取前 ${maxLength} 字符进行分析`);
        }

        if (showProcessing) {
          nbot.sendReply(
            user_id,
            gid,
            forwardMedia.length > 0
              ? `正在分析 合并转发消息（含 ${forwardMedia.length} 个附件），请稍候...`
              : "正在分析 合并转发消息，请稍候..."
          );
        }

        nbot.callLlmForwardMediaBundle(
          user_id,
          gid,
          systemPrompt,
          prompt,
          "合并转发消息",
          content,
          forwardMedia,
          llmOptions
        );
        return;
      }

      // 分析文本消息
      const textContent = reply_message.raw_message || "";
      if (textContent) {
        if (showProcessing) {
          nbot.sendReply(user_id, gid, "正在分析 消息内容，请稍候...");
        }
        let content = textContent;
        if (content.length > maxLength) {
          content = content.substring(0, maxLength);
          nbot.sendReply(user_id, gid, `内容过长，已截取前 ${maxLength} 字符进行分析`);
        }

        nbot.callLlmForwardMediaBundle(
          user_id,
          gid,
          systemPrompt,
          prompt,
          "消息内容",
          content,
          [],
          llmOptions
        );
        return;
      }

      nbot.sendReply(user_id, gid, "无法获取被回复消息的内容");
  }
};
