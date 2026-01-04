pub(super) fn redact_qq_ids(input: &str) -> String {
    fn is_digit(b: u8) -> bool {
        b.is_ascii_digit()
    }

    fn digit_span(bytes: &[u8], start: usize, max_len: usize) -> usize {
        let mut i = start;
        while i < bytes.len() && is_digit(bytes[i]) && (i - start) < max_len {
            i += 1;
        }
        i
    }

    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut last = 0usize;
    let mut i = 0usize;

    while i < bytes.len() {
        // @123456
        if bytes[i] == b'@' {
            let j = digit_span(bytes, i + 1, 12);
            let len = j.saturating_sub(i + 1);
            if (5..=12).contains(&len) {
                out.push_str(&input[last..i]);
                out.push_str("@用户");
                last = j;
                i = j;
                continue;
            }
        }

        // (123456789)
        if bytes[i] == b'(' {
            let j = digit_span(bytes, i + 1, 12);
            let len = j.saturating_sub(i + 1);
            if (5..=12).contains(&len) && j < bytes.len() && bytes[j] == b')' {
                out.push_str(&input[last..i]);
                out.push_str("(已隐藏)");
                last = j + 1;
                i = j + 1;
                continue;
            }
        }

        // qq=123456 or uin=123456 (case-insensitive)
        if i + 3 < bytes.len() {
            let b0 = bytes[i].to_ascii_lowercase();
            let b1 = bytes[i + 1].to_ascii_lowercase();
            let b2 = bytes[i + 2].to_ascii_lowercase();
            let b3 = bytes[i + 3];

            let is_qq = b0 == b'q' && b1 == b'q' && b2 == b'=' && is_digit(b3);
            let is_uin = b0 == b'u'
                && b1 == b'i'
                && b2 == b'n'
                && bytes.get(i + 3) == Some(&b'=')
                && bytes.get(i + 4).copied().is_some_and(is_digit);

            if is_qq {
                let j = digit_span(bytes, i + 3, 12);
                let len = j.saturating_sub(i + 3);
                if (5..=12).contains(&len) {
                    out.push_str(&input[last..(i + 3)]);
                    out.push_str("已隐藏");
                    last = j;
                    i = j;
                    continue;
                }
            } else if is_uin {
                // uin=... needs at least 4+1 chars, so guard i+4 above
                let j = digit_span(bytes, i + 4, 12);
                let len = j.saturating_sub(i + 4);
                if (5..=12).contains(&len) {
                    out.push_str(&input[last..(i + 4)]);
                    out.push_str("已隐藏");
                    last = j;
                    i = j;
                    continue;
                }
            }
        }

        i += 1;
    }

    out.push_str(&input[last..]);
    out
}
