//! Text as chat platforms want it: split to their length limits, and
//! translated between Markdown and Slack's own markup.

/// Split `text` into pieces of at most `max` characters, breaking between
/// paragraphs or lines where it can. A code fence cut in two is closed in one
/// piece and reopened in the next.
pub fn chunks(text: &str, max: usize) -> Vec<String> {
    let text = text.trim();
    // Room to close a fence at the end of a piece and reopen it in the next.
    let budget = max.saturating_sub(16).max(1);
    let mut out = Vec::new();
    let mut rest = text;
    let mut fenced = false;
    while !rest.is_empty() {
        let (piece, tail) = if rest.chars().count() <= budget { (rest, "") } else { split_at_break(rest, budget) };
        let mut chunk = String::new();
        if fenced {
            chunk.push_str("```\n");
        }
        chunk.push_str(piece.trim_end());
        if piece.lines().filter(|line| line.trim_start().starts_with("```")).count() % 2 == 1 {
            fenced = !fenced;
        }
        if fenced {
            chunk.push_str("\n```");
        }
        if !chunk.trim().is_empty() {
            out.push(chunk);
        }
        rest = tail.trim_start_matches('\n');
    }
    out
}

/// Cut `text` at the best break at or before `max` characters.
fn split_at_break(text: &str, max: usize) -> (&str, &str) {
    let limit = text.char_indices().nth(max).map(|(i, _)| i).unwrap_or(text.len());
    let head = &text[..limit];
    // Prefer a paragraph, then a line, then a word; never cut in the first
    // third just to find one.
    let floor = limit / 3;
    let at = ["\n\n", "\n", " "]
        .iter()
        .find_map(|sep| head.rfind(sep).filter(|i| *i >= floor))
        .unwrap_or(limit);
    text.split_at(at)
}

/// Slack escapes three characters in message text.
fn unescape(text: &str) -> String {
    text.replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

/// The ids of the people a Slack message mentions.
pub fn slack_mentions(text: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("<@") {
        rest = &rest[start + 2..];
        let Some(end) = rest.find('>') else { break };
        let id = rest[..end].split('|').next().unwrap_or_default().to_string();
        if !id.is_empty() && !ids.contains(&id) {
            ids.push(id);
        }
        rest = &rest[end + 1..];
    }
    ids
}

/// Turn Slack's message markup into plain text: `<@U1>` becomes a name (or
/// nothing, for the bot itself), `<https://x|label>` its URL, and so on.
pub fn slack_decode(text: &str, bot_id: &str, name_of: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let Some(len) = rest[start..].find('>') else {
            out.push_str(&rest[start..]);
            rest = "";
            break;
        };
        let inner = &rest[start + 1..start + len];
        let (target, label) = match inner.split_once('|') {
            Some((target, label)) => (target, Some(label)),
            None => (inner, None),
        };
        if let Some(id) = target.strip_prefix('@') {
            if id != bot_id {
                out.push('@');
                out.push_str(&name_of(id).or(label.map(str::to_string)).unwrap_or_else(|| id.to_string()));
            }
        } else if let Some(id) = target.strip_prefix('#') {
            out.push('#');
            out.push_str(label.unwrap_or(id));
        } else if let Some(special) = target.strip_prefix('!') {
            out.push('@');
            out.push_str(label.unwrap_or(special));
        } else {
            // A link. The label is how it was shown; the URL is what matters.
            out.push_str(target);
        }
        rest = &rest[start + len + 1..];
    }
    out.push_str(rest);
    unescape(&out).trim().to_string()
}

/// Keep what an agent wrote from pinging a whole channel.
pub fn slack_defang(text: &str) -> String {
    text.replace("<!", "<\u{200b}!")
}

/// Markdown to Slack's `mrkdwn`, for where Slack will not take Markdown.
/// Covers what agents write most: bold, links and headings. Code is left alone.
pub fn to_mrkdwn(markdown: &str) -> String {
    let mut out = Vec::new();
    let mut fenced = false;
    for line in markdown.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            out.push(line.to_string());
            continue;
        }
        if fenced {
            out.push(line.to_string());
            continue;
        }
        let trimmed = line.trim_start();
        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ') {
            out.push(format!("*{}*", inline_mrkdwn(trimmed[hashes..].trim()).replace('*', "")));
        } else {
            out.push(inline_mrkdwn(line));
        }
    }
    out.join("\n")
}

fn inline_mrkdwn(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    let mut code = false;
    while let Some(c) = rest.chars().next() {
        if c == '`' {
            code = !code;
        } else if !code {
            if let Some(tail) = rest.strip_prefix("**") {
                out.push('*');
                rest = tail;
                continue;
            }
            if c == '['
                && let Some((label, tail)) = rest[1..].split_once("](")
                && let Some((url, tail)) = tail.split_once(')')
                && !label.contains('[')
                && !url.contains(char::is_whitespace)
            {
                out.push_str(&format!("<{url}|{label}>"));
                rest = tail;
                continue;
            }
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// Does `text` name the repository `full_name`, as a word of its own?
pub fn names_repo(text: &str, full_name: &str) -> bool {
    let (text, name) = (text.to_lowercase(), full_name.to_lowercase());
    let part = |c: char| c.is_alphanumeric() || matches!(c, '_' | '-' | '/');
    let mut from = 0;
    while let Some(found) = text[from..].find(&name) {
        let (start, end) = (from + found, from + found + name.len());
        let before = text[..start].chars().next_back();
        let mut after = text[end..].chars();
        let next = after.next();
        // A full stop ends a sentence unless more of a name follows it.
        let runs_on = match next {
            Some('.') => after.next().is_some_and(char::is_alphanumeric),
            Some(c) => part(c),
            None => false,
        };
        if !before.is_some_and(|c| part(c) || c == '.') && !runs_on {
            return true;
        }
        from = end;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_is_one_chunk() {
        assert_eq!(chunks("  hello  ", 100), vec!["hello"]);
        assert!(chunks("   ", 100).is_empty());
    }

    #[test]
    fn long_text_breaks_between_paragraphs() {
        let text = format!("{}\n\n{}", "a".repeat(60), "b".repeat(60));
        let parts = chunks(&text, 100);
        assert_eq!(parts, vec!["a".repeat(60), "b".repeat(60)]);
        // Nothing to break on: cut where the limit falls.
        let parts = chunks(&"x".repeat(250), 100);
        assert!(parts.iter().all(|p| p.chars().count() <= 100));
        assert_eq!(parts.concat().len(), 250);
    }

    #[test]
    fn a_split_code_fence_is_closed_and_reopened() {
        let code = (0..30).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let parts = chunks(&format!("Here:\n```\n{code}\n```\nDone."), 120);
        assert!(parts.len() > 1);
        for part in &parts {
            assert!(part.chars().count() <= 120, "{part}");
            assert_eq!(part.matches("```").count() % 2, 0, "unbalanced fence in {part:?}");
        }
        assert!(parts.last().unwrap().ends_with("Done."));
    }

    #[test]
    fn slack_markup_is_decoded() {
        let names = |id: &str| (id == "U2").then(|| "Mia".to_string());
        assert_eq!(slack_decode("<@UBOT> fix the build", "UBOT", names), "fix the build");
        assert_eq!(slack_decode("ask <@U2> or <@U3> in <#C1|dev>", "UBOT", names), "ask @Mia or @U3 in #dev");
        assert_eq!(slack_decode("see <https://example.com/a?b=1&amp;c=2|the docs> &lt;now&gt;", "UBOT", names), "see https://example.com/a?b=1&c=2 <now>");
        assert_eq!(slack_decode("a < b", "UBOT", names), "a < b");
        assert_eq!(slack_mentions("<@U1> and <@U2|mia> and <@U1>"), vec!["U1", "U2"]);
    }

    #[test]
    fn markdown_becomes_mrkdwn() {
        assert_eq!(to_mrkdwn("## Done\n**Fixed** the [test](https://x.dev/t)."), "*Done*\n*Fixed* the <https://x.dev/t|test>.");
        assert_eq!(to_mrkdwn("```\n**not bold**\n```\nuse `a**b` here"), "```\n**not bold**\n```\nuse `a**b` here");
        assert_eq!(slack_defang("hi <!channel>"), "hi <\u{200b}!channel>");
    }

    #[test]
    fn repositories_are_named_as_whole_words() {
        assert!(names_repo("fix the flaky test in acme/web.", "acme/web"));
        assert!(names_repo("Acme/Web: bump deps", "acme/web"));
        assert!(names_repo("(acme/web)", "acme/web"));
        assert!(!names_repo("fix acme/website", "acme/web"));
        assert!(!names_repo("fix acme/web.site", "acme/web"));
        assert!(!names_repo("fix org/acme/web", "acme/web"));
        assert!(!names_repo("https://github.com/acme/web/pull/3", "acme/web"));
    }
}
