use unic_emoji_char::{is_emoji, is_emoji_component};

/// Check if a character is a "visual" emoji that should be converted to Twemoji
/// Excludes ASCII digits, #, *, and other characters that are technically emoji
/// but shouldn't be rendered as images in normal text
fn is_visual_emoji(c: char) -> bool {
    // Exclude ASCII range (0-127) - digits, #, * etc are technically emoji bases
    // but we don't want to convert them
    if (c as u32) < 0x80 {
        return false;
    }
    // Exclude variation selectors and ZWJ (handled separately)
    if c == '\u{FE0F}' || c == '\u{FE0E}' || c == '\u{200D}' {
        return false;
    }
    is_emoji(c) || is_emoji_component(c)
}

/// 将 emoji 字符转换为 Twemoji 图片标签
pub fn emoji_to_twemoji(text: &str) -> String {
    let base = std::env::var("NBOT_TWEMOJI_BASE_URL")
        .ok()
        .unwrap_or_default();
    let base = base.trim();
    if base.is_empty() || base.eq_ignore_ascii_case("off") {
        return text.to_string();
    }
    let base = base.trim_end_matches('/');

    let mut result = String::new();
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if is_visual_emoji(c) {
            let mut codepoints = vec![c as u32];
            while let Some(&next) = chars.peek() {
                if is_visual_emoji(next)
                    || next == '\u{200D}'
                    || next == '\u{FE0F}'
                    || next == '\u{FE0E}'
                {
                    if let Some(c) = chars.next() {
                        codepoints.push(c as u32);
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            let code = codepoints
                .iter()
                .filter(|&&cp| cp != 0xFE0F && cp != 0xFE0E)
                .map(|cp| format!("{:x}", cp))
                .collect::<Vec<_>>()
                .join("-");
            result.push_str(&format!(
                r#"<img class="emoji" draggable="false" alt="" src="{}/{}.svg">"#,
                base, code
            ));
        } else {
            result.push(c);
        }
    }
    result
}
