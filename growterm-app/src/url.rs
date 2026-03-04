/// Find a URL at the given column (character index) position in the text.
/// Returns the URL string if `col` falls within a URL range.
pub fn find_url_at(text: &str, col: usize) -> Option<&str> {
    let byte_col = char_to_byte(text, col)?;
    let (start, end) = find_url_byte_range_at(text, byte_col)?;
    Some(&text[start..end])
}

/// Find all URLs in the text.
pub fn find_all_urls(text: &str) -> Vec<&str> {
    let mut urls = Vec::new();
    let mut search_start = 0;
    loop {
        let rest = &text[search_start..];
        let https_pos = rest.find("https://");
        let http_pos = rest.find("http://");
        let offset = match (https_pos, http_pos) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => break,
        };
        let url_start = search_start + offset;
        let url_end = find_url_end(text, url_start);
        urls.push(&text[url_start..url_end]);
        search_start = if url_end > url_start { url_end } else { url_start + 1 };
        if search_start >= text.len() {
            break;
        }
    }
    urls
}

/// Find the column (character index) range of a URL at the given column position.
/// Returns (start_col, end_col) character indices if `col` falls within a URL range.
pub fn find_url_range_at(text: &str, col: usize) -> Option<(usize, usize)> {
    let byte_col = char_to_byte(text, col)?;
    let (byte_start, byte_end) = find_url_byte_range_at(text, byte_col)?;
    let col_start = byte_to_char(text, byte_start);
    let col_end = byte_to_char(text, byte_end);
    Some((col_start, col_end))
}

/// Convert character index to byte offset. Returns None if out of bounds.
fn char_to_byte(text: &str, char_idx: usize) -> Option<usize> {
    if char_idx == 0 {
        return Some(0);
    }
    text.char_indices()
        .nth(char_idx)
        .map(|(byte_offset, _)| byte_offset)
        .or_else(|| {
            // char_idx == text.chars().count() means "one past end"
            if char_idx <= text.chars().count() {
                Some(text.len())
            } else {
                None
            }
        })
}

/// Convert byte offset to character index.
fn byte_to_char(text: &str, byte_offset: usize) -> usize {
    text[..byte_offset].chars().count()
}

/// Find the byte range of a URL at the given byte offset.
fn find_url_byte_range_at(text: &str, col: usize) -> Option<(usize, usize)> {
    let mut search_start = 0;
    loop {
        let rest = &text[search_start..];
        let https_pos = rest.find("https://");
        let http_pos = rest.find("http://");
        let offset = match (https_pos, http_pos) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => return None,
        };

        let url_start = search_start + offset;
        let url_end = find_url_end(text, url_start);

        if col >= url_start && col < url_end {
            return Some((url_start, url_end));
        }

        search_start = if url_end > url_start { url_end } else { url_start + 1 };

        if search_start >= text.len() {
            return None;
        }
    }
}

fn find_url_end(text: &str, start: usize) -> usize {
    let bytes = text.as_bytes();
    let mut i = start;
    let mut paren_depth: i32 = 0;

    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b' ' | b'\t' | b'\n' | b'\r' | b'>' | b'"' | b'\'' | b'<' | b']' => break,
            b'(' => {
                paren_depth += 1;
                i += 1;
            }
            b')' => {
                if paren_depth > 0 {
                    paren_depth -= 1;
                    i += 1;
                } else {
                    break;
                }
            }
            _ => {
                i += 1;
            }
        }
    }

    // Trim trailing punctuation that's unlikely part of URL
    while i > start {
        match bytes[i - 1] {
            b'.' | b',' | b';' | b':' | b'!' | b'?' => i -= 1,
            _ => break,
        }
    }

    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_https_url() {
        let text = "Visit https://google.com for more";
        assert_eq!(find_url_at(text, 6), Some("https://google.com"));
        assert_eq!(find_url_at(text, 23), Some("https://google.com"));
        // Just past the URL
        assert_eq!(find_url_at(text, 24), None);
    }

    #[test]
    fn simple_http_url() {
        let text = "Go to http://example.com now";
        assert_eq!(find_url_at(text, 6), Some("http://example.com"));
    }

    #[test]
    fn url_with_path() {
        let text = "See https://example.com/path/to/page?q=1&b=2#anchor end";
        assert_eq!(
            find_url_at(text, 4),
            Some("https://example.com/path/to/page?q=1&b=2#anchor")
        );
        assert_eq!(
            find_url_at(text, 30),
            Some("https://example.com/path/to/page?q=1&b=2#anchor")
        );
    }

    #[test]
    fn url_at_start_of_line() {
        let text = "https://start.com rest";
        assert_eq!(find_url_at(text, 0), Some("https://start.com"));
    }

    #[test]
    fn url_at_end_of_line() {
        let text = "end https://end.com";
        assert_eq!(find_url_at(text, 18), Some("https://end.com"));
    }

    #[test]
    fn click_outside_url() {
        let text = "no url here";
        assert_eq!(find_url_at(text, 3), None);
    }

    #[test]
    fn multiple_urls() {
        let text = "a https://first.com b https://second.com c";
        assert_eq!(find_url_at(text, 2), Some("https://first.com"));
        assert_eq!(find_url_at(text, 22), Some("https://second.com"));
    }

    #[test]
    fn url_in_angle_brackets() {
        let text = "Link: <https://example.com> done";
        assert_eq!(find_url_at(text, 7), Some("https://example.com"));
    }

    #[test]
    fn url_in_quotes() {
        let text = r#"url="https://example.com" done"#;
        assert_eq!(find_url_at(text, 5), Some("https://example.com"));
    }

    #[test]
    fn url_with_parentheses_wikipedia() {
        let text = "See https://en.wikipedia.org/wiki/Rust_(programming_language) for info";
        assert_eq!(
            find_url_at(text, 4),
            Some("https://en.wikipedia.org/wiki/Rust_(programming_language)")
        );
    }

    #[test]
    fn url_followed_by_paren_no_open() {
        let text = "check (https://example.com) now";
        assert_eq!(find_url_at(text, 7), Some("https://example.com"));
    }

    #[test]
    fn url_with_trailing_period() {
        let text = "Visit https://example.com.";
        assert_eq!(find_url_at(text, 6), Some("https://example.com"));
    }

    #[test]
    fn url_with_trailing_comma() {
        let text = "See https://example.com, and more";
        assert_eq!(find_url_at(text, 4), Some("https://example.com"));
    }

    #[test]
    fn click_between_urls() {
        let text = "https://a.com gap https://b.com";
        assert_eq!(find_url_at(text, 14), None);
    }

    #[test]
    fn empty_text() {
        assert_eq!(find_url_at("", 0), None);
    }

    #[test]
    fn col_past_text_length() {
        let text = "https://example.com";
        assert_eq!(find_url_at(text, 100), None);
    }

    #[test]
    fn url_range_simple() {
        let text = "Visit https://google.com for more";
        assert_eq!(find_url_range_at(text, 6), Some((6, 24)));
    }

    #[test]
    fn url_range_at_start() {
        let text = "https://start.com rest";
        assert_eq!(find_url_range_at(text, 0), Some((0, 17)));
    }

    #[test]
    fn url_range_outside() {
        let text = "no url here";
        assert_eq!(find_url_range_at(text, 3), None);
    }

    #[test]
    fn url_range_multiple() {
        let text = "a https://first.com b https://second.com c";
        assert_eq!(find_url_range_at(text, 2), Some((2, 19)));
        assert_eq!(find_url_range_at(text, 22), Some((22, 40)));
    }

    #[test]
    fn url_after_multibyte_chars() {
        // "한글 " = 3 characters, but 7 bytes (3+3+1)
        let text = "한글 https://example.com end";
        // col 3 = 'h' of https (character index 3)
        assert_eq!(find_url_at(text, 3), Some("https://example.com"));
        assert_eq!(find_url_range_at(text, 3), Some((3, 22)));
        // col 0 = '한', not a URL
        assert_eq!(find_url_at(text, 0), None);
    }

    #[test]
    fn url_range_after_multibyte_returns_char_indices() {
        // "가 " = 2 chars (but 4 bytes: 3+1)
        let text = "가 https://x.com done";
        // URL starts at char index 2, "https://x.com" = 13 chars, ends at char 15
        assert_eq!(find_url_range_at(text, 2), Some((2, 15)));
    }

    #[test]
    fn find_all_urls_in_text() {
        assert_eq!(find_all_urls("Visit https://google.com for more"), vec!["https://google.com"]);
        assert_eq!(find_all_urls("a https://first.com b https://second.com"), vec!["https://first.com", "https://second.com"]);
        assert!(find_all_urls("no url here").is_empty());
        assert!(find_all_urls("").is_empty());
        assert_eq!(find_all_urls("http://a.com and https://b.com"), vec!["http://a.com", "https://b.com"]);
    }

    #[test]
    fn col_past_text_length_multibyte() {
        let text = "한글";
        assert_eq!(find_url_at(text, 100), None);
    }
}
