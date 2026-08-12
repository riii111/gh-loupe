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
        }
        let next_starts_whitespace = body[block.end..]
            .chars()
            .next()
            .is_some_and(char::is_whitespace);
        let result_ends_whitespace = result.chars().last().is_some_and(char::is_whitespace);
        if !body[block.end..].is_empty()
            && !result.is_empty()
            && !next_starts_whitespace
            && !result_ends_whitespace
        {
            result.push('\n');
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
    for (event, range) in pulldown_cmark::Parser::new(body).into_offset_iter() {
        if matches!(
            event,
            pulldown_cmark::Event::Code(_)
                | pulldown_cmark::Event::Start(pulldown_cmark::Tag::CodeBlock(_))
                | pulldown_cmark::Event::End(pulldown_cmark::TagEnd::CodeBlock)
        ) {
            mark(&mut protected, range.start, range.end);
        }
    }
    mask_container_indented_code(bytes, &mut protected);

    html_attribute_mask(bytes, &mut protected);
    html_raw_text_mask(bytes, &mut protected);
    html_comment_mask(bytes, &mut protected);
    protected
}

#[derive(Clone, Copy)]
struct ListPrefix {
    content_column: usize,
}

fn mask_container_indented_code(bytes: &[u8], protected: &mut [bool]) {
    let mut cursor = 0;
    let mut list_content_column = None;
    while cursor < bytes.len() {
        let end = line_end(bytes, cursor);
        let (container_cursor, blockquote_depth, current_list) =
            container_prefix_end(bytes, cursor, end);
        if let Some(prefix) = current_list {
            list_content_column = Some(prefix.content_column);
        }
        let line_content_column = visual_column(bytes, cursor, container_cursor)
            + leading_column(bytes, container_cursor, end);
        let list_code = list_content_column.is_some_and(|base| line_content_column >= base + 4);
        let blockquote_code = current_list.is_none()
            && blockquote_depth > 0
            && leading_column(bytes, container_cursor, end) >= 4;
        if !protected[cursor] && (list_code || blockquote_code) {
            mark(protected, cursor, end);
        }
        if current_list.is_none()
            && !is_blank_line(bytes, cursor, end)
            && list_content_column.is_none_or(|base| line_content_column < base)
        {
            list_content_column = None;
        }
        cursor = end;
    }
}

fn line_end(bytes: &[u8], start: usize) -> usize {
    bytes[start..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(bytes.len(), |offset| start + offset + 1)
}

fn leading_column(bytes: &[u8], start: usize, end: usize) -> usize {
    let mut column = 0;
    for byte in &bytes[start..end] {
        match byte {
            b' ' => column += 1,
            b'\t' => column = (column / 4 + 1) * 4,
            _ => break,
        }
    }
    column
}

fn visual_column(bytes: &[u8], start: usize, end: usize) -> usize {
    let mut column = 0;
    for byte in &bytes[start..end] {
        match byte {
            b'\t' => column = (column / 4 + 1) * 4,
            b'\r' | b'\n' => break,
            _ => column += 1,
        }
    }
    column
}

fn is_blank_line(bytes: &[u8], start: usize, end: usize) -> bool {
    bytes[start..end]
        .iter()
        .all(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
}

fn container_prefix_end(
    bytes: &[u8],
    start: usize,
    end: usize,
) -> (usize, usize, Option<ListPrefix>) {
    let mut cursor = start;
    let mut blockquote_depth = 0;
    let mut list_prefix = None;
    loop {
        let before_marker = cursor;
        let mut spaces = 0;
        while cursor < end && spaces < 4 && matches!(bytes[cursor], b' ' | b'\t') {
            cursor += 1;
            spaces += 1;
        }
        if cursor >= end {
            return (cursor, blockquote_depth, list_prefix);
        }
        if spaces == 4 {
            return (before_marker, blockquote_depth, list_prefix);
        }
        if bytes[cursor] == b'>' {
            blockquote_depth += 1;
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
            let marker_end = cursor + 1;
            cursor = marker_end;
            while cursor < end && matches!(bytes[cursor], b' ' | b'\t') {
                cursor += 1;
            }
            list_prefix = Some(ListPrefix {
                content_column: list_content_column(bytes, start, marker_end, cursor),
            });
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
            let marker_end = cursor + 1;
            cursor = marker_end;
            while cursor < end && matches!(bytes[cursor], b' ' | b'\t') {
                cursor += 1;
            }
            list_prefix = Some(ListPrefix {
                content_column: list_content_column(bytes, start, marker_end, cursor),
            });
            continue;
        }
        return (before_marker, blockquote_depth, list_prefix);
    }
}

fn list_content_column(bytes: &[u8], start: usize, marker_end: usize, padding_end: usize) -> usize {
    let marker_column = visual_column(bytes, start, marker_end);
    let padding_column = leading_column(bytes, marker_end, padding_end);
    if padding_column <= 4 {
        visual_column(bytes, start, padding_end)
    } else {
        marker_column
    }
}

fn html_attribute_mask(bytes: &[u8], protected: &mut [bool]) {
    let mut cursor = 0;
    while cursor + 1 < bytes.len() {
        if bytes[cursor] != b'<'
            || protected[cursor]
            || is_escaped(bytes, cursor)
            || !matches!(bytes[cursor + 1], b'!' | b'/' | b'?' | b'A'..=b'Z' | b'a'..=b'z')
        {
            cursor += 1;
            continue;
        }

        let mut scan = cursor + 1;
        let mut quote = None;
        let mut quoted_ranges: Vec<(usize, usize)> = Vec::new();
        while scan < bytes.len() {
            match quote {
                Some(expected) if bytes[scan] == expected => {
                    quoted_ranges.last_mut().expect("quote range starts").1 = scan + 1;
                    quote = None;
                }
                None if matches!(bytes[scan], b'"' | b'\'') => {
                    quoted_ranges.push((scan, scan + 1));
                    quote = Some(bytes[scan]);
                }
                None if bytes[scan] == b'>' => {
                    for (start, end) in quoted_ranges {
                        mark(protected, start, end);
                    }
                    break;
                }
                None if bytes[scan] == b'<' => break,
                Some(_) | None => {}
            }
            scan += 1;
        }
        if bytes.get(scan) == Some(&b'>') && quote.is_none() {
            cursor = scan + 1;
        } else {
            cursor += 1;
        }
    }
}

fn html_raw_text_mask(bytes: &[u8], protected: &mut [bool]) {
    for name in [
        b"script".as_slice(),
        b"style",
        b"textarea",
        b"title",
        b"xmp",
    ] {
        let mut cursor = 0;
        while cursor + name.len() + 2 <= bytes.len() {
            if bytes[cursor] != b'<'
                || protected[cursor]
                || is_escaped(bytes, cursor)
                || !bytes
                    .get(cursor + 1..cursor + name.len() + 1)
                    .is_some_and(|value| value.eq_ignore_ascii_case(name))
                || !matches!(
                    bytes.get(cursor + name.len() + 1),
                    Some(b'>' | b' ' | b'\t' | b'\r' | b'\n')
                )
            {
                cursor += 1;
                continue;
            }
            let Some(open_end) = html_tag_end(bytes, cursor) else {
                cursor += 1;
                continue;
            };
            let close_start = find_closing_tag(bytes, open_end, name).unwrap_or(bytes.len());
            let close_end = if close_start < bytes.len() {
                html_tag_end(bytes, close_start).unwrap_or(bytes.len())
            } else {
                bytes.len()
            };
            mark(protected, cursor, close_end);
            cursor = close_end;
        }
    }

    let mut cursor = 0;
    while cursor + 9 <= bytes.len() {
        if bytes[cursor..].starts_with(b"<![CDATA[")
            && !protected[cursor]
            && !is_escaped(bytes, cursor)
        {
            let end = bytes[cursor + 9..]
                .windows(3)
                .position(|window| window == b"]]>")
                .map_or(bytes.len(), |offset| cursor + 9 + offset + 3);
            mark(protected, cursor, end);
            cursor = end;
        } else {
            cursor += 1;
        }
    }
}

fn html_tag_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut quote = None;
    let mut cursor = start + 1;
    while cursor < bytes.len() {
        match quote {
            Some(expected) if bytes[cursor] == expected => quote = None,
            None if matches!(bytes[cursor], b'"' | b'\'') => quote = Some(bytes[cursor]),
            None if bytes[cursor] == b'>' => return Some(cursor + 1),
            None if bytes[cursor] == b'<' => return None,
            Some(_) | None => {}
        }
        cursor += 1;
    }
    None
}

fn find_closing_tag(bytes: &[u8], start: usize, name: &[u8]) -> Option<usize> {
    let mut cursor = start;
    while cursor + name.len() + 3 <= bytes.len() {
        if bytes[cursor] == b'<'
            && bytes.get(cursor + 1) == Some(&b'/')
            && bytes[cursor + 2..]
                .get(..name.len())
                .is_some_and(|value| value.eq_ignore_ascii_case(name))
            && matches!(
                bytes.get(cursor + name.len() + 2),
                Some(b'>' | b' ' | b'\t' | b'\r' | b'\n')
            )
        {
            return Some(cursor);
        }
        cursor += 1;
    }
    None
}

fn html_comment_mask(bytes: &[u8], protected: &mut [bool]) {
    let mut cursor = 0;
    while cursor + 4 <= bytes.len() {
        if !protected[cursor]
            && bytes[cursor..].starts_with(b"<!--")
            && !is_escaped(bytes, cursor)
            && (cursor..cursor + 4).all(|index| !protected[index])
        {
            let end = bytes[cursor + 4..]
                .windows(3)
                .position(|window| window == b"-->")
                .map_or(bytes.len(), |offset| cursor + 4 + offset + 3);
            mark(protected, cursor, end);
            cursor = end;
        } else {
            cursor += 1;
        }
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
        assert_eq!(
            omit_details("```foo`bar\n<details><summary>valid</summary>hidden</details>"),
            ("```foo`bar\nvalid".to_owned(), true)
        );
    }

    #[test]
    fn leaves_details_text_in_html_attributes_untouched() {
        let body =
            r#"<div title="<details><summary>literal</summary>hidden</details>">outside</div>"#;
        assert_eq!(omit_details(body), (body.to_owned(), false));
        for (body, expected) in [
            (
                "<div title=\"broken\n<details><summary>valid</summary>hidden</details>",
                "<div title=\"broken\nvalid",
            ),
            (
                "text <x a='broken\n<details><summary>valid</summary>hidden</details>",
                "text <x a='broken\nvalid",
            ),
        ] {
            assert_eq!(omit_details(body), (expected.to_owned(), true));
        }
    }

    #[test]
    fn leaves_raw_text_and_cdata_html_literal() {
        for body in [
            "<script>\n<details><summary>literal</summary>hidden</details>\n</script>",
            "<![CDATA[\n<details><summary>literal</summary>hidden</details>\n]]>",
        ] {
            assert_eq!(omit_details(body), (body.to_owned(), false));
        }
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
        assert_eq!(
            omit_details(
                "```markdown\n- ```\n<details>literal</details>\n```\n<details><summary>valid</summary>hidden</details>"
            ),
            (
                "```markdown\n- ```\n<details>literal</details>\n```\nvalid".to_owned(),
                true
            )
        );
    }

    #[test]
    fn leaves_html_comments_literal() {
        assert_eq!(
            omit_details(
                "<!-- <details><summary>literal</summary>hidden</details> -->\n<details><summary>valid</summary>hidden</details>"
            ),
            (
                "<!-- <details><summary>literal</summary>hidden</details> -->\nvalid".to_owned(),
                true
            )
        );
        assert_eq!(
            omit_details("\\<!--\n<details><summary>valid</summary>hidden</details>\n-->"),
            ("\\<!--\nvalid\n-->".to_owned(), true)
        );
    }

    #[test]
    fn respects_ordered_list_content_indent_and_escaped_backticks() {
        assert_eq!(
            omit_details("10. item\n    <details><summary>valid</summary>hidden</details>"),
            ("10. item\n    valid".to_owned(), true)
        );
        assert_eq!(
            omit_details(
                "10. ```markdown\n    literal\n    ```\n<details><summary>valid</summary>hidden</details>"
            ),
            (
                "10. ```markdown\n    literal\n    ```\nvalid".to_owned(),
                true
            )
        );
        assert_eq!(
            omit_details("10. item\n\n    <details><summary>valid</summary>hidden</details>"),
            ("10. item\n\n    valid".to_owned(), true)
        );
        for body in [
            "- item\n      <details><summary>literal</summary>hidden</details>",
            ">     <details><summary>literal</summary>hidden</details>",
            " \t<details><summary>literal</summary>hidden</details>",
            "-     <details><summary>literal</summary>hidden</details>",
            "-\titem\n    ~~~\n    <details><summary>literal</summary>hidden</details>\n    ~~~",
        ] {
            assert_eq!(omit_details(body), (body.to_owned(), false));
        }
        let body = r"\`
<details>
<summary>valid</summary>
hidden
</details>
\`";
        assert_eq!(
            omit_details(body),
            (
                r"\`
valid
\`"
                .to_owned(),
                true
            )
        );
    }

    #[test]
    fn keeps_block_boundary_when_details_are_inline() {
        assert_omitted(
            "before<details><summary>heading</summary>hidden</details>after",
            "before\nheading\nafter",
        );
        assert_omitted("before<details>hidden</details>after", "before\nafter");
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
