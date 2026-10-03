use pulldown_cmark::{Event, Parser, Tag, TagEnd};

pub(crate) const POLICY: &str = "不输出具体 Spark 命令、子命令或参数，包括聊天栏、控制台、命名空间和别名写法；即便用户索要或历史回答中已有命令，也不要引用或补全。报告无法可靠确认安装构建及命令用途。改用自然语言说明采样目的、模式、时长、条件和观察指标；需要具体写法时让用户查其安装版本的官方帮助。健康/内存快照只用于内存池状态和聚合统计，不提供逐次 GC 暂停的时间轴；GC 与卡顿的时间对齐应采带时间戳的 JVM GC 日志及同期 tick 记录。";

#[derive(Clone, Copy, PartialEq, Eq)]
enum TextContext {
    Prose,
    InlineCode { has_prose: bool },
    CodeBlock,
}

/// Reject command-shaped text conservatively, rather than asserting that a
/// command's syntax or claimed capabilities are correct for an unknown build.
pub(crate) fn contains_spark_command(content: &str) -> bool {
    if contains_command_text(content, TextContext::Prose, "") {
        return true;
    }
    let mut rendered = String::new();
    let mut in_code = false;
    let events = Parser::new(content).collect::<Vec<_>>();
    for (index, event) in events.iter().enumerate() {
        match event {
            Event::Start(Tag::CodeBlock(_)) => in_code = true,
            Event::End(TagEnd::CodeBlock) => {
                in_code = false;
                rendered.push('\n');
            }
            Event::Code(text) => {
                let has_prose = !only_punctuation(rendered.rsplit('\n').next().unwrap_or_default())
                    || has_following_prose(&events[index + 1..]);
                if contains_command_text(text, TextContext::InlineCode { has_prose }, &rendered) {
                    return true;
                }
                rendered.push_str(&text);
            }
            Event::Text(text) => {
                if in_code && contains_command_text(&text, TextContext::CodeBlock, &rendered) {
                    return true;
                }
                rendered.push_str(&text);
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                if contains_command_text(&dest_url, TextContext::Prose, "") {
                    return true;
                }
            }
            Event::SoftBreak | Event::HardBreak => rendered.push(' '),
            Event::End(TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::Item) => {
                rendered.push('\n');
            }
            _ => {}
        }
    }
    contains_command_text(&rendered, TextContext::Prose, "")
}

fn contains_command_text(text: &str, context: TextContext, prefix: &str) -> bool {
    if spark_subcommand(prefix) && !only_punctuation(text) {
        return true;
    }
    let code = context != TextContext::Prose;
    let inherited_instruction = spark_instruction(prefix);
    let usage = is_usage_prefix(prefix);
    let prefixed_execution = !usage && is_execution_prefix(prefix);
    let inherited_execution = inherited_instruction && is_execution_prefix(prefix);
    let text = text
        .chars()
        .filter(|c| {
            !matches!(
                c,
                '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{2060}' | '\u{feff}'
            )
        })
        .map(|c| {
            if ('\u{ff01}'..='\u{ff5e}').contains(&c) {
                char::from_u32(c as u32 - 0xfee0).unwrap()
            } else {
                c
            }
        })
        .collect::<String>()
        .to_ascii_lowercase();
    let span = text.trim().strip_prefix("spark ").unwrap_or(text.trim());
    let sampling_span = span.starts_with("cpu profile") || span.starts_with("allocation profile");
    let metric_span = usage && word_in("gc tps ping activity", span);
    // The full raw/rendered sentence distinguishes metric/profiler prose
    // from a terminal invocation; inline formatting alone must not decide it.
    if matches!(context, TextContext::InlineCode { has_prose: true })
        && (metric_span || (span == "profiler" && !inherited_instruction))
    {
        return false;
    }
    let leading = text
        .trim_start()
        .split(|c: char| !is_command_word(c))
        .next()
        .unwrap_or_default()
        .rsplit(':')
        .next()
        .unwrap_or_default();
    // Explicitly named external tools retain their own syntax even when the
    // surrounding sentence discusses Spark.
    let executable = text
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_matches(['"', '\''])
        .rsplit('/')
        .next()
        .unwrap_or_default();
    let external_tool = is_external_tool(executable);
    let spark_fragment = leading.starts_with("--") || word_in("spark sparkc sparkb sparkv profiler sampler healthreport heapsummary heapdump tickmonitor gcmonitor", leading);
    if code
        && inherited_execution
        && !sampling_span
        && !external_tool
        && (!usage || spark_fragment || leading.contains('-'))
    {
        return true;
    }
    let mut tokens = text.char_indices().peekable();
    let mut external_arguments = false;
    let mut first_external_argument = false;
    while let Some((start, first)) = tokens.next() {
        if !is_command_word(first) {
            if matches!(first, '`' | '\n' | ';' | '&' | '|' | ',' | '。' | '；')
                || (first == '.' && tokens.peek().is_none_or(|(_, c)| c.is_whitespace()))
                || (!first.is_ascii() && first.is_alphabetic())
            {
                external_arguments = false;
            }
            continue;
        }
        let mut end = start + first.len_utf8();
        while let Some(&(index, c)) = tokens.peek() {
            if !is_command_word(c) {
                break;
            }
            end = index + c.len_utf8();
            tokens.next();
        }
        let token = text[start..end].trim_end_matches(':');
        let spark_namespace = token.rsplit_once(':').is_some_and(|(namespace, _)| {
            namespace
                .split(':')
                .any(|part| word_in("spark sparkc sparkb sparkv", part))
        });
        let root = token.rsplit(':').next().unwrap_or_default();
        // A terminal component of a qualified class name is evidence, even
        // when its spelling overlaps with a Spark command module.
        let nested_class = text[..start].strip_suffix('$').is_some_and(|prefix| {
            let qualifier = prefix
                .rsplit(|c: char| !c.is_ascii_alphanumeric() && !matches!(c, '_' | '.' | '$'))
                .next()
                .unwrap_or_default();
            qualifier.contains('.')
                && qualifier.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        });
        if text[..start].ends_with('.') || nested_class {
            continue;
        }
        if is_external_tool(root) {
            // Docker can execute a nested command; JVM diagnostic tools only
            // consume their own arguments and output paths. A mere prose
            // mention of a tool does not introduce an argument list.
            let argument = text[end..].trim_start_matches(['"', '\'']).trim_start();
            external_arguments = root != "docker"
                && ((code && external_tool)
                    || argument.starts_with(|c: char| c.is_ascii_digit() || c == '-')
                    || (root == "jcmd"
                        && argument.split_whitespace().nth(1).is_some_and(|command| {
                            command == "help"
                                || command.split_once('.').is_some_and(|(group, _)| {
                                    word_in("gc vm thread compiler managementagent jfr", group)
                                })
                        }))
                    || (root == "jfr"
                        && word_in(
                            "print summary configure metadata assemble disassemble view help version scrub",
                            argument.split_whitespace().next().unwrap_or_default(),
                        )));
            first_external_argument = external_arguments;
            continue;
        }
        if word_in("run execute then use using command console and or", root) {
            external_arguments = false;
        }
        if external_arguments {
            let rest = &text[end..];
            let nested_command = !first_external_argument
                && (spark_namespace || word_in("spark sparkc sparkb sparkv", root))
                && rest.starts_with(char::is_whitespace)
                && rest
                    .trim_start()
                    .starts_with(|c: char| c.is_ascii_alphanumeric() || c == '-');
            first_external_argument = false;
            if !nested_command {
                continue;
            }
            external_arguments = false;
        }
        let ordinary_root = text[start..end].trim_end_matches(':') == "spark";
        if matches!(&text[start..end], "http:" | "https:") && text[end..].starts_with("//") {
            while tokens.peek().is_some_and(|(_, c)| {
                !c.is_whitespace() && !matches!(c, '`' | '"' | '<' | '>' | ')' | ']')
            }) {
                tokens.next();
            }
            continue;
        }
        let spark_context = || {
            (inherited_instruction && (!code || spark_fragment))
                || spark_instruction(&text[..start])
        };
        // Also reject copied Spark options and command fragments that omit
        // the root, without treating unrelated JVM options as Spark syntax.
        if root.starts_with("--") && root.len() > 2 && spark_context() {
            return true;
        }
        let unambiguous_root = spark_namespace || word_in("spark sparkc sparkb sparkv profiler sampler healthreport heapsummary heapdump tickmonitor gcmonitor", root);
        let local_prefix = &text[..start];
        if spark_subcommand(local_prefix)
            && (code || unambiguous_root || root == "help" || local_prefix.contains("子命令"))
        {
            return true;
        }
        if spark_instruction(local_prefix)
            && is_execution_prefix(local_prefix)
            && (!is_usage_prefix(local_prefix)
                || root.contains('-')
                || (unambiguous_root && !word_in("spark profiler", root)))
            && !word_in("cpu allocation", root)
        {
            return true;
        }
        if !unambiguous_root
            && !(word_in("gc tps ping activity", root)
                && ((inherited_execution && (!usage || spark_fragment))
                    || spark_instruction(&text[..start])))
        {
            continue;
        }
        if text[..start].ends_with("://") {
            continue;
        }
        let slash_prefix = text[..start].strip_suffix('/');
        if let Some(prefix) = slash_prefix {
            if text[end..].starts_with('/') {
                continue;
            }
            if prefix.chars().next_back().is_none_or(|c| {
                !c.is_ascii()
                    || c.is_whitespace()
                    || matches!(c, '`' | '(' | '[' | ':' | '"' | '\'')
            }) {
                return true;
            }
            // A component within a path is an argument, not a command root.
            continue;
        }
        let rest = &text[end..];
        let spaced = rest.chars().next().is_some_and(char::is_whitespace);
        let next = rest.trim_start().chars().next();
        let tail = rest.trim_start();
        let argument_end = tail
            .find(|c: char| !is_command_word(c))
            .unwrap_or(tail.len());
        let next_word = &tail[..argument_end];
        let after = tail[argument_end..].trim_start();
        let further = after
            .split(|c: char| !is_command_word(c))
            .next()
            .unwrap_or_default();
        let arguments = "profiler sampler healthreport heapsummary heapdump tickmonitor gcmonitor tps ping gc activity start stop show upload cancel open info help";
        let terminal = only_punctuation(after);
        let known_argument = word_in(arguments, next_word)
            && (word_in(
                "sampler healthreport heapsummary heapdump tickmonitor gcmonitor help",
                next_word,
            ) || (!ordinary_root && word_in("gc tps ping activity", next_word))
                || (terminal && !word_in("gc tps ping activity", next_word))
                || word_in(arguments, further)
                || further.starts_with("--"));
        let sampling_prose = (word_in("cpu allocation", next_word) && further == "profile")
            || (ordinary_root
                && next_word == "profiler"
                && !terminal
                && !known_argument
                && is_usage_prefix(&text[..start]));
        let metric_prose = word_in("gc tps ping activity", next_word)
            && !matches!(
                context,
                TextContext::CodeBlock | TextContext::InlineCode { has_prose: false }
            )
            && ordinary_root
            && !prefixed_execution
            && (!is_execution_prefix(&text[..start])
                || (!terminal && is_usage_prefix(&text[..start])));
        // Console commands omit the slash. Require command context or a
        // recognized argument so “Spark version 2” remains ordinary prose.
        let command_context = (code && !sampling_prose && !metric_prose)
            || (known_argument && !sampling_prose)
            || (!sampling_prose && !metric_prose && is_execution_prefix(&text[..start]))
            || (!ordinary_root && word_in("spark sparkc sparkb sparkv", root))
            || next_word.starts_with("--")
            || after.starts_with("--");
        let metric_reference = word_in("gc tps ping activity", root)
            && !matches!(
                context,
                TextContext::CodeBlock | TextContext::InlineCode { has_prose: false }
            )
            && !inherited_execution
            && !is_execution_prefix(local_prefix);
        if (spaced
            && command_context
            && next.is_some_and(|c| c.is_ascii_alphanumeric() || c == '-'))
            || (code
                && rest.trim().is_empty()
                && !(ordinary_root
                    && matches!(context, TextContext::InlineCode { has_prose: true })
                    && !inherited_instruction
                    && (!is_execution_prefix(prefix) || is_usage_prefix(prefix))))
            || (code && rest.trim_start().starts_with([';', '&', '|']))
            || (spark_context() && only_punctuation(rest) && !metric_reference)
            || spark_namespace
            || (token.contains(':')
                && word_in("spark sparkc sparkb sparkv", root)
                && only_punctuation(rest)
                && only_punctuation(text[..start].rsplit('\n').next().unwrap_or_default()))
            || (only_punctuation(rest)
                && is_execution_prefix(&text[..start])
                && !is_usage_prefix(&text[..start]))
        {
            return true;
        }
    }
    false
}

fn only_punctuation(text: &str) -> bool {
    text.chars()
        .all(|c| c.is_whitespace() || c.is_ascii_punctuation() || "。，；：！？".contains(c))
}

fn has_following_prose(events: &[Event<'_>]) -> bool {
    events
        .iter()
        .find_map(|event| match event {
            Event::Text(text) if !only_punctuation(text) => Some(true),
            Event::Text(_) | Event::SoftBreak | Event::HardBreak => None,
            Event::Start(Tag::Emphasis | Tag::Strong | Tag::Strikethrough | Tag::Link { .. })
            | Event::End(
                TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link,
            ) => None,
            _ => Some(false),
        })
        .unwrap_or(false)
}

fn is_usage_prefix(prefix: &str) -> bool {
    let prefix = prefix.trim_end_matches(|c: char| {
        c.is_whitespace() || matches!(c, ':' | '：' | '`' | '"' | '\'')
    });
    let word = prefix
        .rsplit(|c: char| !is_command_word(c))
        .next()
        .unwrap_or_default();
    prefix.ends_with("使用")
        || prefix.ends_with("可以用")
        || word.eq_ignore_ascii_case("use")
        || word.eq_ignore_ascii_case("using")
}

fn is_external_tool(root: &str) -> bool {
    word_in("jcmd java jstat jmap jstack jfr docker", root)
}

fn is_execution_prefix(prefix: &str) -> bool {
    let prefix = prefix.trim_end_matches(|c: char| {
        c.is_whitespace() || matches!(c, ':' | '：' | '`' | '"' | '\'')
    });
    [
        "执行",
        "运行",
        "输入",
        "使用",
        "可以用",
        "控制台",
        "命令",
        "run",
        "execute",
        "console",
        "command",
        "use",
    ]
    .iter()
    .any(|word| {
        let Some(at) = prefix.len().checked_sub(word.len()) else {
            return false;
        };
        prefix
            .get(at..)
            .is_some_and(|suffix| suffix.eq_ignore_ascii_case(word))
            && (!word.is_ascii()
                || prefix[..at]
                    .chars()
                    .next_back()
                    .is_none_or(|c| !is_command_word(c)))
    })
}

fn spark_instruction(prefix: &str) -> bool {
    let prefix = prefix.trim_end();
    let start = prefix.char_indices().rev().nth(160).map_or(0, |(at, _)| at);
    let context = prefix[start..]
        .rsplit(['\n', '。', '；', ';'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let Some((_, tail)) = context.rsplit_once("spark") else {
        return false;
    };
    let attributed = tail.split(|c: char| !c.is_ascii_alphabetic()).all(|word| word.is_empty()
        || word_in("in use using option options parameter parameters argument arguments flag flags run execute command commands subcommand subcommands is are console", word));
    let local = tail.chars().all(|c| {
        !c.is_alphabetic()
            || c.is_ascii()
            || "的中里内在服务器端控制台游戏聊天栏执行运行输入使用可以用子命令参数选项标志是为"
                .contains(c)
    });
    attributed
        && local
        && (is_execution_prefix(&context)
            || tail.trim().trim_end_matches([':', '：']).trim() == "的"
            || "子命令 subcommand 参数 选项 标志 option argument parameter flag"
                .split_whitespace()
                .any(|s| tail.contains(s)))
}

fn word_in(words: &str, word: &str) -> bool {
    words.split_whitespace().any(|candidate| candidate == word)
}

fn spark_subcommand(prefix: &str) -> bool {
    spark_instruction(prefix)
        && prefix
            .to_ascii_lowercase()
            .rsplit_once("spark")
            .is_some_and(|(_, tail)| tail.contains("子命令") || tail.contains("subcommand"))
}

fn is_command_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ':')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_known_and_invented_commands_across_markdown_and_aliases() {
        for text in [
            "运行 `/spark healthreport --memory` 取得 GC 时间戳。",
            "spark help",
        ] {
            assert!(contains_spark_command(text), "{text}");
        }
    }

    #[test]
    fn preserves_sampling_instructions_and_report_links() {
        for text in [
            "使用 Spark 采普通 CPU profile，持续 120 秒，再比较 MSPT。",
            "使用 `Spark` 采集普通 CPU profile，持续 120 秒。",
            "使用 Spark CPU profile，持续 120 秒。",
            "Use Spark CPU profile for 120 seconds.",
            "Use Spark profiler to collect a CPU profile for 120 seconds.",
            "使用 Spark profiler 采集 120 秒的 CPU 样本。",
            "使用 `Spark profiler` 采集 120 秒的 CPU 样本。",
        ] {
            assert!(!contains_spark_command(text), "{text}");
        }
    }
}
