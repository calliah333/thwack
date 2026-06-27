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

pub(crate) fn extract_first_url(text: &str) -> Option<String> {
    text.split_whitespace().find_map(|token| {
        let token = token
            .trim_start_matches(|ch: char| "([{<\"'".contains(ch))
            .trim_end_matches(|ch: char| ".,);]}>\"'".contains(ch));
        (token.starts_with("http://") || token.starts_with("https://")).then(|| token.to_string())
    })
}

pub(crate) fn clean_comment_text(text: &str) -> String {
    let mut lines = text
        .lines()
        .filter(|line| !line.trim_start().starts_with("```"))
        .map(|line| line.replace('`', ""))
        .collect::<Vec<_>>();

    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }

    lines.join("\n")
}
