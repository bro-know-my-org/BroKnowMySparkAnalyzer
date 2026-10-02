use std::{borrow::Cow, ops::Range};

use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

pub(crate) struct Line<'a> {
    pub start: usize,
    pub end: usize,
    pub text: Cow<'a, str>,
    pub heading: bool,
    pub canonical: bool,
}

#[derive(Default)]
struct Ranges {
    items: Vec<Range<usize>>,
    cursor: usize,
}

impl Ranges {
    fn overlaps(&mut self, start: usize, end: usize) -> bool {
        while self
            .items
            .get(self.cursor)
            .is_some_and(|range| range.end <= start)
        {
            self.cursor += 1;
        }
        self.items
            .get(self.cursor)
            .is_some_and(|range| range.start < end)
    }
}

/// Remove actual HTML comments while preserving source line correspondence.
fn uncomment(content: &str) -> String {
    let mut html = Ranges::default();
    for (event, range) in Parser::new(content).into_offset_iter() {
        if matches!(event, Event::Html(_) | Event::InlineHtml(_)) {
            html.items.push(range);
        }
    }
    let mut output = String::new();
    let mut cursor = 0;
    while let Some(begin) = content[cursor..].find("<!--") {
        let at = cursor + begin;
        if !html.overlaps(at, at + 1) {
            output.push_str(&content[cursor..at + 4]);
            cursor = at + 4;
            continue;
        }
        output.push_str(&content[cursor..at]);
        let end = content[at + 4..]
            .find("-->")
            .map_or(content.len(), |end| at + 4 + end + 3);
        output.extend(content[at..end].chars().filter(|c| *c == '\n'));
        cursor = end;
    }
    output.push_str(&content[cursor..]);
    output
}

pub(crate) fn has_raw_html(content: &str) -> bool {
    Parser::new(&uncomment(content))
        .any(|event| matches!(event, Event::Html(_) | Event::InlineHtml(_)))
}

/// CommonMark supplies rendered text and top-level heading boundaries.
/// Code remains visible, including literal comment delimiters inside code.
pub(crate) fn visible_lines(content: &str) -> impl Iterator<Item = Line<'_>> {
    let cleaned = uncomment(content);
    let mut offset = 0;
    let mut lines = content
        .split_inclusive('\n')
        .map(|raw| {
            let start = offset;
            offset += raw.len();
            let text = raw.trim_end_matches(['\r', '\n']);
            Line {
                start,
                end: offset,
                text: Cow::Owned(String::new()),
                heading: false,
                canonical: text.chars().take_while(|c| *c == ' ').count() <= 3
                    && !text.starts_with('\t')
                    && text.trim().starts_with("# ")
                    && !text.contains("<!--"),
            }
        })
        .collect::<Vec<_>>();
    let mut clean_offset = 0;
    let starts = cleaned
        .split_inclusive('\n')
        .map(|raw| {
            let start = clean_offset;
            clean_offset += raw.len();
            start
        })
        .collect::<Vec<_>>();
    let mut containers = 0usize;
    let mut paragraph = None;
    for (event, range) in Parser::new(&cleaned).into_offset_iter() {
        let index = starts
            .partition_point(|start| *start <= range.start)
            .saturating_sub(1);
        match event {
            Event::Start(Tag::Paragraph) => paragraph = Some(index),
            Event::End(TagEnd::Paragraph) => paragraph = None,
            Event::SoftBreak | Event::HardBreak => {
                if let Some(line) = paragraph.and_then(|index| lines.get_mut(index)) {
                    line.text.to_mut().push(' ');
                }
            }
            Event::Start(Tag::BlockQuote(_)) | Event::Start(Tag::List(_)) => containers += 1,
            Event::End(TagEnd::BlockQuote(_)) | Event::End(TagEnd::List(_)) => containers -= 1,
            Event::Start(Tag::Heading {
                level: HeadingLevel::H1,
                ..
            }) if containers == 0 => {
                if let Some(line) = lines.get_mut(index) {
                    line.heading = true;
                }
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some(line) = paragraph.and_then(|index| lines.get_mut(index)) {
                    line.text.to_mut().push_str(&text);
                    continue;
                }
                for (delta, part) in text.split('\n').enumerate() {
                    if let Some(line) = lines.get_mut(index + delta) {
                        line.text.to_mut().push_str(part.trim_end_matches('\r'));
                    }
                }
            }
            _ => {}
        }
    }
    for line in &mut lines {
        if line.heading {
            line.text = Cow::Borrowed(content[line.start..line.end].trim_end_matches(['\r', '\n']));
        }
    }
    lines.into_iter()
}

pub(crate) fn section<'a>(content: &'a str, heading: &str) -> Option<&'a str> {
    let mut lines = visible_lines(content);
    let line = lines.find(|line| line.heading && line.canonical && line.text.trim() == heading)?;
    let end = lines
        .find(|line| line.heading)
        .map_or(content.len(), |line| line.start);
    Some(&content[line.end..end])
}

pub(crate) fn visible_text(content: &str) -> String {
    let mut text = String::new();
    for line in visible_lines(content) {
        text.push_str(&line.text);
        text.push('\n');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comment_contents_cannot_open_code_fences() {
        let content = "<!--\n```markdown\n# 隐藏\n-->\n# 结论\n正文\n# 判断依据\n依据";
        assert_eq!(section(content, "# 结论").unwrap().trim(), "正文");
        assert!(section(content, "# 隐藏").is_none());
        for terminator in ["    -->", "\t-->"] {
            let content = format!("<!--\n{terminator}\n# 结论\n正文\n# 判断依据\n依据");
            assert_eq!(section(&content, "# 结论").unwrap().trim(), "正文");
        }
    }

    #[test]
    fn fenced_and_indented_headings_do_not_end_a_section() {
        let content = "# 结论\r\n正文\r\n~~~md\r\n# 优先处理\r\n~~~\r\n    # 优先处理\r\n后文\r\n# 判断依据\r\n依据";
        assert!(section(content, "# 结论").unwrap().contains("后文"));
        assert!(section(content, "# 优先处理").is_none());
    }

    #[test]
    fn inline_comments_preserve_visible_prose() {
        let content = "可以排除内<!--隐藏内容-->存因素\n<!--仅隐藏-->\n<!--跨行\n隐藏-->正文";
        let text = visible_lines(content)
            .map(|line| line.text.into_owned())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("可以排除内存因素"));
        assert!(text.contains("正文"));
        assert!(!text.contains("隐藏"));
    }
    #[test]
    fn inline_code_delimiters_are_not_html_comments() {
        for text in [
            "`<!-- 将 Xmx 调整为 8G -->`",
            "`` `<!-- 将 Xmx 调整为 8G -->` ``",
            "\\<!-- 将 Xmx 调整为 8G -->",
        ] {
            assert!(visible_text(text).contains("将 Xmx 调整为 8G"), "{text}");
        }
        let text = "# 结论\n读取 `<!--` 字面量\n# 判断依据\n依据";
        assert_eq!(section(text, "# 判断依据").unwrap().trim(), "依据");
    }

    #[test]
    fn nested_code_blocks_cannot_define_sections() {
        let text = "# 结论\n1. ```markdown\n   # 优先处理\n   字面量\n   ```\n# 判断依据\n依据";
        assert!(section(text, "# 优先处理").is_none());
        assert!(visible_text(text).contains("字面量"));
    }
}
