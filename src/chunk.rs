pub const MAX_CHUNK_CHARS: usize = 400;

pub fn chunk_text(text: &str) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    for sentence in split_sentences(text) {
        let sentence = sentence.trim();
        if sentence.is_empty() {
            continue;
        }
        if sentence.chars().count() <= MAX_CHUNK_CHARS {
            chunks.push(sentence.to_string());
        } else {
            chunks.extend(hard_cut(sentence, MAX_CHUNK_CHARS));
        }
    }
    chunks
}

fn split_sentences(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut chunks = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if matches!(b, b'.' | b'!' | b'?') {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j > i + 1 {
                chunks.push(&text[start..=i]);
                start = j;
                i = j;
                continue;
            }
        }
        i += 1;
    }
    if start < bytes.len() {
        chunks.push(&text[start..]);
    }
    chunks
}

fn hard_cut(s: &str, max_chars: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    for ch in s.chars() {
        buf.push(ch);
        if buf.chars().count() >= max_chars {
            out.push(std::mem::take(&mut buf));
        }
    }
    if !buf.is_empty() {
        out.push(buf);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_returns_empty() {
        assert!(chunk_text("").is_empty());
        assert!(chunk_text("   \n  ").is_empty());
    }

    #[test]
    fn single_sentence() {
        assert_eq!(chunk_text("Hello there."), vec!["Hello there."]);
    }

    #[test]
    fn multiple_sentences() {
        let out = chunk_text("First. Second! Third?");
        assert_eq!(out, vec!["First.", "Second!", "Third?"]);
    }

    #[test]
    fn sentence_without_trailing_punct_kept() {
        let out = chunk_text("First. Tail with no period");
        assert_eq!(out, vec!["First.", "Tail with no period"]);
    }

    #[test]
    fn run_on_over_limit_is_hard_cut() {
        let s = "a".repeat(900);
        let out = chunk_text(&s);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].chars().count(), 400);
        assert_eq!(out[1].chars().count(), 400);
        assert_eq!(out[2].chars().count(), 100);
    }

    #[test]
    fn decimals_dont_split() {
        let out = chunk_text("Pi is 3.14 roughly.");
        assert_eq!(out, vec!["Pi is 3.14 roughly."]);
    }

    #[test]
    fn unicode_counted_by_chars_not_bytes() {
        let s: String = std::iter::repeat('é').take(500).collect();
        let out = chunk_text(&s);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].chars().count(), 400);
        assert_eq!(out[1].chars().count(), 100);
    }
}
