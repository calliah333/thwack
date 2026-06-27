pub(crate) fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut entity = None::<String>;

    for (index, ch) in html.char_indices() {
        match ch {
            '<' => {
                if let Some(entity) = entity.take() {
                    out.push('&');
                    out.push_str(&entity);
                }
                in_tag = true;
                if starts_html_break_tag(&html[index..]) {
                    out.push('\n');
                    out.push('\n');
                } else {
                    out.push(' ');
                }
            }
            '>' => in_tag = false,
            '&' if !in_tag => {
                if let Some(entity) = entity.take() {
                    out.push('&');
                    out.push_str(&entity);
                }
                entity = Some(String::new());
            }
            ';' if !in_tag && entity.is_some() => {
                push_html_entity(&mut out, entity.as_deref().unwrap_or_default());
                entity = None;
            }
            _ if !in_tag && entity.is_some() => {
                if let Some(entity) = &mut entity {
                    entity.push(ch);
                }
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }

    if let Some(entity) = entity {
        out.push('&');
        out.push_str(&entity);
    }

    collapse_spaces(&out)
}

fn push_html_entity(out: &mut String, entity: &str) {
    let decoded = match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" | "nbsp" => Some(' '),
        _ => entity
            .strip_prefix("#x")
            .or_else(|| entity.strip_prefix("#X"))
            .and_then(|hex| u32::from_str_radix(hex, 16).ok())
            .or_else(|| entity.strip_prefix('#').and_then(|n| n.parse().ok()))
            .and_then(char::from_u32),
    };

    if let Some(ch) = decoded {
        out.push(ch);
    } else {
        out.push('&');
        out.push_str(entity);
        out.push(';');
    }
}

fn starts_html_break_tag(tag: &str) -> bool {
    let Some(tag) = tag.strip_prefix('<') else {
        return false;
    };
    let tag = tag.strip_prefix('/').unwrap_or(tag);

    ["p", "br", "div", "li", "pre"].iter().any(|name| {
        tag.strip_prefix(name).is_some_and(|rest| {
            rest.starts_with('>')
                || rest.starts_with('/')
                || rest
                    .chars()
                    .next()
                    .is_some_and(|ch| ch.is_ascii_whitespace())
        })
    })
}

fn collapse_spaces(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    let mut pending_newlines = 0usize;

    for ch in text.chars() {
        if ch == '\n' {
            if !out.is_empty() {
                pending_newlines = (pending_newlines + 1).min(2);
            }
            pending_space = false;
        } else if ch.is_whitespace() {
            pending_space = !out.is_empty() && pending_newlines == 0;
        } else {
            for _ in 0..pending_newlines {
                out.push('\n');
            }
            if pending_newlines == 0 && pending_space {
                out.push(' ');
            }
            out.push(ch);
            pending_space = false;
            pending_newlines = 0;
        }
    }

    out
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TextLink {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) url: String,
}

fn is_http_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

fn find_url_start(text: &str) -> Option<usize> {
    [text.find("http://"), text.find("https://")]
        .into_iter()
        .flatten()
        .min()
}

pub(crate) fn clean_markdown_line_with_links(line: &str) -> (String, Vec<TextLink>) {
    let line = line.replace('`', "");
    let mut out = String::with_capacity(line.len());
    let mut links = Vec::new();
    let mut rest = line.as_str();

    while let Some(open) = rest.find('[') {
        let (before, after_open) = rest.split_at(open);
        let Some(close_label) = after_open.find("](") else {
            break;
        };
        let label = &after_open[1..close_label];
        let after_label = &after_open[close_label + "](".len()..];
        let Some(close_url) = after_label.find(')') else {
            break;
        };
        let url = &after_label[..close_url];

        if !is_http_url(url) {
            out.push_str(before);
            out.push('[');
            rest = &after_open[1..];
            continue;
        }

        out.push_str(before);
        let label = if label.is_empty() || label == url {
            url
        } else {
            label
        };
        let start = out.len();
        out.push_str(label);
        links.push(TextLink {
            start,
            end: out.len(),
            url: url.to_string(),
        });
        rest = &after_label[close_url + 1..];
    }

    out.push_str(rest);
    (out, links)
}

pub(crate) fn extract_first_url(text: &str) -> Option<String> {
    text.split_whitespace().find_map(|token| {
        let token =
            &token[find_url_start(token)?..].trim_end_matches(|ch: char| ".,);]}>\"'".contains(ch));
        is_http_url(token).then(|| token.to_string())
    })
}

pub(crate) fn clean_comment_lines_with_links(text: &str) -> Vec<(String, Vec<TextLink>)> {
    let mut lines = text
        .lines()
        .filter(|line| !line.trim_start().starts_with("```"))
        .map(clean_markdown_line_with_links)
        .collect::<Vec<_>>();

    while lines.last().is_some_and(|(line, _)| line.trim().is_empty()) {
        lines.pop();
    }

    lines
}

pub(crate) fn clean_comment_text(text: &str) -> String {
    clean_comment_lines_with_links(text)
        .into_iter()
        .map(|(line, _)| line)
        .collect::<Vec<_>>()
        .join("\n")
}
