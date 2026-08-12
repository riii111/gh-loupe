#[derive(Clone, Copy, PartialEq, Eq)]
enum TagKind {
    DetailsOpen,
    DetailsClose,
    SummaryOpen,
    SummaryClose,
}

#[derive(Clone, Copy)]
struct Tag {
    kind: TagKind,
    start: usize,
    end: usize,
}

#[derive(Clone, Copy)]
struct DetailsBlock {
    start: usize,
    end: usize,
}

pub fn omit_details(body: &str) -> (String, bool) {
    let protected = code_mask(body);
    let tags = tags(body, &protected);
    let blocks = complete_details_blocks(&tags);
    if blocks.is_empty() {
        return (body.to_owned(), false);
    }

    let mut result = String::with_capacity(body.len());
    let mut cursor = 0;
    for block in blocks {
        result.push_str(&body[cursor..block.start]);
        if let Some((summary_start, summary_end)) = summary_content(&tags, block) {
            let result_ends_whitespace = result.chars().last().is_some_and(char::is_whitespace);
            if summary_start != summary_end && !result.is_empty() && !result_ends_whitespace {
                result.push('\n');
            }
            result.push_str(&body[summary_start..summary_end]);
            let next_starts_whitespace = body[block.end..]
                .chars()
                .next()
                .is_some_and(char::is_whitespace);
            if !body[block.end..].is_empty() && !next_starts_whitespace && !result.ends_with('\n') {
                result.push('\n');
            }
        }
        cursor = block.end;
    }
    result.push_str(&body[cursor..]);
    (result, true)
}

fn complete_details_blocks(tags: &[Tag]) -> Vec<DetailsBlock> {
    let mut stack = Vec::new();
    let mut blocks = Vec::new();
    for tag in tags {
        match tag.kind {
            TagKind::DetailsOpen => stack.push(tag.start),
            TagKind::DetailsClose => {
                if stack.len() == 1 {
                    let start = stack.pop().expect("stack length is one");
                    blocks.push(DetailsBlock {
                        start,
                        end: tag.end,
                    });
                } else if !stack.is_empty() {
                    stack.pop();
                }
            }
            TagKind::SummaryOpen | TagKind::SummaryClose => {}
        }
    }
    blocks.sort_unstable_by_key(|block| block.start);
    blocks
}

fn summary_content(tags: &[Tag], block: DetailsBlock) -> Option<(usize, usize)> {
    let mut details_depth = 1;
    let mut summary_start = None;
    for tag in tags {
        if tag.start <= block.start || tag.end >= block.end {
            continue;
        }
        match tag.kind {
            TagKind::DetailsOpen => details_depth += 1,
            TagKind::DetailsClose => details_depth -= 1,
            TagKind::SummaryOpen if details_depth == 1 && summary_start.is_none() => {
                summary_start = Some(tag.end);
            }
            TagKind::SummaryClose if details_depth == 1 => {
                if let Some(start) = summary_start {
                    return Some((start, tag.start));
                }
            }
            TagKind::SummaryOpen | TagKind::SummaryClose => {}
        }
    }
    None
}

fn tags(body: &str, protected: &[bool]) -> Vec<Tag> {
    let bytes = body.as_bytes();
    let mut result = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] == b'<'
            && !protected[cursor]
            && !is_escaped(bytes, cursor)
            && let Some(tag) = parse_tag(bytes, cursor)
            && (cursor..tag.end).all(|index| !protected[index])
        {
            cursor = tag.end;
            result.push(tag);
            continue;
        }
        cursor += 1;
    }
    result
}

fn parse_tag(bytes: &[u8], start: usize) -> Option<Tag> {
    let (kind, mut cursor) = if bytes.get(start + 1) == Some(&b'/') {
        let (kind, cursor) = tag_name(bytes, start + 2)?;
        (kind.close(), cursor)
    } else {
        let (kind, cursor) = tag_name(bytes, start + 1)?;
        (kind, cursor)
    };

    if !matches!(
        bytes.get(cursor),
        Some(b'>' | b'/' | b' ' | b'\t' | b'\r' | b'\n')
    ) {
        return None;
    }
    if kind.is_close() {
        while matches!(bytes.get(cursor), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            cursor += 1;
        }
        return (bytes.get(cursor) == Some(&b'>')).then_some(Tag {
            kind,
            start,
            end: cursor + 1,
        });
    }
    if bytes.get(cursor) == Some(&b'/') {
        return None;
    }

    let mut quote = None;
    while let Some(byte) = bytes.get(cursor) {
        match (quote, byte) {
            (Some(expected), byte) if *byte == expected => quote = None,
            (None, b'"' | b'\'') => quote = Some(*byte),
            (None, b'<') => return None,
            (None, b'>') => {
                return Some(Tag {
                    kind,
                    start,
                    end: cursor + 1,
                });
            }
            _ => {}
        }
        cursor += 1;
    }
    None
}

fn tag_name(bytes: &[u8], start: usize) -> Option<(TagKind, usize)> {
    for (name, kind) in [
        (b"details".as_slice(), TagKind::DetailsOpen),
        (b"summary".as_slice(), TagKind::SummaryOpen),
    ] {
        let end = start + name.len();
        if bytes
            .get(start..end)
            .is_some_and(|value| value.eq_ignore_ascii_case(name))
        {
            return Some((kind, end));
        }
    }
    None
}

impl TagKind {
    fn close(self) -> Self {
        match self {
            Self::DetailsOpen => Self::DetailsClose,
            Self::SummaryOpen => Self::SummaryClose,
            Self::DetailsClose | Self::SummaryClose => self,
        }
    }

    fn is_close(self) -> bool {
        matches!(self, Self::DetailsClose | Self::SummaryClose)
    }
}

fn code_mask(body: &str) -> Vec<bool> {
    let bytes = body.as_bytes();
    let mut protected = vec![false; bytes.len()];
    let mut cursor = 0;
    while cursor < bytes.len() {
        let first_line_end = line_end(bytes, cursor);
        if let Some((fence, length)) = fence_start(bytes, cursor, first_line_end) {
            mark(&mut protected, cursor, first_line_end);
            cursor = first_line_end;
            while cursor < bytes.len() {
                let next_end = line_end(bytes, cursor);
                mark(&mut protected, cursor, next_end);
                let is_closing = fence_end(bytes, cursor, next_end, fence, length);
                cursor = next_end;
                if is_closing {
                    break;
                }
            }
        } else if indented_code(bytes, cursor, first_line_end) {
            mark(&mut protected, cursor, first_line_end);
            cursor = first_line_end;
        } else {
            cursor = first_line_end;
        }
    }

    let mut cursor = 0;
    while cursor < bytes.len() {
        if protected[cursor] {
            cursor += 1;
            continue;
        }
        if bytes[cursor] != b'`' {
            cursor += 1;
            continue;
        }
        let length = run_length(bytes, cursor, b'`');
        let Some(end) = inline_code_end(bytes, &protected, cursor + length, length) else {
            cursor += length;
            continue;
        };
        mark(&mut protected, cursor, end);
        cursor = end;
    }
    protected
}

fn line_end(bytes: &[u8], start: usize) -> usize {
    bytes[start..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(bytes.len(), |offset| start + offset + 1)
}

fn fence_start(bytes: &[u8], start: usize, end: usize) -> Option<(u8, usize)> {
    let cursor = container_prefix_end(bytes, start, end);
    if cursor >= end || !matches!(bytes[cursor], b'`' | b'~') {
        return None;
    }
    let fence = bytes[cursor];
    let length = run_length(bytes, cursor, fence);
    (length >= 3).then_some((fence, length))
}

fn fence_end(bytes: &[u8], start: usize, end: usize, fence: u8, length: usize) -> bool {
    let mut cursor = container_prefix_end(bytes, start, end);
    if bytes.get(cursor) != Some(&fence) {
        return false;
    }
    let run = run_length(bytes, cursor, fence);
    if run < length {
        return false;
    }
    cursor += run;
    bytes[cursor..end]
        .iter()
        .all(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
}

fn indented_code(bytes: &[u8], start: usize, end: usize) -> bool {
    let mut spaces = 0;
    while start + spaces < end && bytes[start + spaces] == b' ' {
        spaces += 1;
    }
    spaces >= 4 || bytes.get(start) == Some(&b'\t')
}

fn container_prefix_end(bytes: &[u8], start: usize, end: usize) -> usize {
    let mut cursor = start;
    loop {
        let before_marker = cursor;
        let mut spaces = 0;
        while cursor < end && spaces < 4 && matches!(bytes[cursor], b' ' | b'\t') {
            cursor += 1;
            spaces += 1;
        }
        if cursor >= end {
            return cursor;
        }
        if spaces == 4 {
            return before_marker;
        }
        if matches!(bytes[cursor], b'`' | b'~') {
            return cursor;
        }
        if bytes[cursor] == b'>' {
            cursor += 1;
            if cursor < end && matches!(bytes[cursor], b' ' | b'\t') {
                cursor += 1;
            }
            continue;
        }
        if matches!(bytes[cursor], b'-' | b'+' | b'*')
            && cursor + 1 < end
            && matches!(bytes[cursor + 1], b' ' | b'\t')
        {
            cursor += 1;
            while cursor < end && matches!(bytes[cursor], b' ' | b'\t') {
                cursor += 1;
            }
            continue;
        }
        let digits_start = cursor;
        while cursor < end && cursor - digits_start < 9 && bytes[cursor].is_ascii_digit() {
            cursor += 1;
        }
        if cursor > digits_start
            && cursor < end
            && matches!(bytes[cursor], b'.' | b')')
            && cursor + 1 < end
            && matches!(bytes[cursor + 1], b' ' | b'\t')
        {
            cursor += 1;
            while cursor < end && matches!(bytes[cursor], b' ' | b'\t') {
                cursor += 1;
            }
            continue;
        }
        return before_marker;
    }
}

fn is_escaped(bytes: &[u8], position: usize) -> bool {
    let mut backslashes = 0;
    let mut cursor = position;
    while cursor > 0 && bytes[cursor - 1] == b'\\' {
        backslashes += 1;
        cursor -= 1;
    }
    backslashes % 2 == 1
}

fn run_length(bytes: &[u8], start: usize, byte: u8) -> usize {
    bytes[start..]
        .iter()
        .take_while(|value| **value == byte)
        .count()
}

fn inline_code_end(bytes: &[u8], protected: &[bool], start: usize, length: usize) -> Option<usize> {
    let mut cursor = start;
    while cursor < bytes.len() {
        if protected[cursor] || bytes[cursor] != b'`' {
            cursor += 1;
            continue;
        }
        let run = run_length(bytes, cursor, b'`');
        if run == length && (cursor..cursor + run).all(|index| !protected[index]) {
            return Some(cursor + run);
        }
        cursor += run;
    }
    None
}

fn mark(protected: &mut [bool], start: usize, end: usize) {
    protected[start..end].fill(true);
}

#[cfg(test)]
mod tests {
    use super::omit_details;

    fn assert_omitted(input: &str, expected: &str) {
        assert_eq!(omit_details(input), (expected.to_owned(), true));
    }

    #[test]
    fn keeps_outer_text_and_summary() {
        assert_omitted(
            "before\n<details class=\"evidence\">\n<summary>判断</summary>\nsecret\n</details>\nafter",
            "before\n判断\nafter",
        );
    }

    #[test]
    fn handles_multiple_nested_attribute_case_and_japanese_blocks() {
        assert_omitted(
            "<DETAILS open>\n<SUMMARY>外側</SUMMARY>\nouter\n<details data-x='1'>\n<summary>内側</summary>\ninner\n</details>\n</DETAILS>\n<details>\n<summary>二つ目</summary>\n本文\n</details>",
            "外側\n二つ目",
        );
    }

    #[test]
    fn leaves_code_and_malformed_details_untouched() {
        let body = "`<details>inline</details>`\n\n```markdown\n<details>fenced</details>\n```\n\n    <details>indented</details>\n\n<details>\n<summary>open</summary>\nnot closed";
        assert_eq!(omit_details(body), (body.to_owned(), false));
    }

    #[test]
    fn leaves_container_fences_and_escaped_tags_literal() {
        assert_eq!(
            omit_details(
                "- ```markdown\n  <details>literal</details>\n  ```\n<details><summary>valid</summary>hidden</details>"
            ),
            (
                "- ```markdown\n  <details>literal</details>\n  ```\nvalid".to_owned(),
                true
            )
        );
        assert_eq!(
            omit_details(
                "\\<details>literal\\</details>\n<details><summary>valid</summary>hidden</details>"
            ),
            ("\\<details>literal\\</details>\nvalid".to_owned(), true)
        );
    }

    #[test]
    fn keeps_block_boundary_when_details_are_inline() {
        assert_omitted(
            "before<details><summary>heading</summary>hidden</details>after",
            "before\nheading\nafter",
        );
    }

    #[test]
    fn leaves_unmatched_closing_tag_and_removes_valid_block() {
        assert_omitted(
            "</details>\n<details><summary>kept</summary>hidden</details>",
            "</details>\nkept",
        );
    }

    #[test]
    fn keeps_details_text_inside_inline_code_with_longer_backtick_runs() {
        let body = "`` `<details>literal</details>` ``";
        assert_eq!(omit_details(body), (body.to_owned(), false));
    }
}
