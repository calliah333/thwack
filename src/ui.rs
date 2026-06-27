use std::collections::HashSet;

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::app::App;
use crate::model::{Comment, Mode, Post, source_title};
use crate::text::clean_comment_text;

pub(crate) fn link_style() -> Style {
    Style::default()
        .fg(Color::LightBlue)
        .add_modifier(Modifier::UNDERLINED)
}

pub(crate) fn link_spans(text: &str, plain_style: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut rest = text;

    while let Some(start) = find_url_start(rest) {
        if start > 0 {
            spans.push(Span::styled(rest[..start].to_string(), plain_style));
        }

        let tail = &rest[start..];
        let token_end = tail.find(char::is_whitespace).unwrap_or(tail.len());
        let token = &tail[..token_end];
        let url = token.trim_end_matches(|ch: char| ".,);]}>\"'".contains(ch));
        if !url.is_empty() {
            spans.push(Span::styled(url.to_string(), link_style()));
        }
        if url.len() < token.len() {
            spans.push(Span::styled(token[url.len()..].to_string(), plain_style));
        }

        rest = &tail[token_end..];
    }

    if !rest.is_empty() {
        spans.push(Span::styled(rest.to_string(), plain_style));
    }

    spans
}

pub(crate) fn find_url_start(text: &str) -> Option<usize> {
    match (text.find("http://"), text.find("https://")) {
        (Some(http), Some(https)) => Some(http.min(https)),
        (Some(http), None) => Some(http),
        (None, Some(https)) => Some(https),
        (None, None) => None,
    }
}

pub(crate) fn render(frame: &mut Frame, app: &mut App) {
    let [title_area, content_area, status_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    let mode = match app.mode {
        Mode::Posts => "posts",
        Mode::Comments => "comments",
    };
    frame.render_widget(
        Paragraph::new(format!("{} | {mode}", source_title(app.source)))
            .style(Style::default().add_modifier(Modifier::BOLD)),
        title_area,
    );

    match app.mode {
        Mode::Posts => render_posts(frame, app, content_area),
        Mode::Comments => render_comments(frame, app, content_area),
    }

    let capability = "read-only";
    let help = match app.mode {
        Mode::Posts => {
            "j/k/↑/↓ move · Enter comments · o open · c discussion · r refresh · Tab/1/2 source · q quit"
        }
        Mode::Comments => {
            "j/k/↑/↓ scroll · h/l/←/→ select · Space collapse · Esc/b posts · o open · c discussion · r reload · q quit"
        }
    };
    frame.render_widget(
        Paragraph::new(format!("{} | {capability} | {help}", app.status)),
        status_area,
    );
}

pub(crate) fn post_list_item(post: &Post, index: usize) -> ListItem<'static> {
    let bg = if index.is_multiple_of(2) {
        Color::Rgb(18, 18, 24)
    } else {
        Color::Rgb(28, 28, 36)
    };
    let style = Style::default().fg(Color::Gray).bg(bg);
    let title_style = style.fg(Color::White).add_modifier(Modifier::BOLD);
    let label_style = style.fg(Color::DarkGray);
    let score_style = style.fg(Color::Green);
    let comments_style = style.fg(Color::Cyan);
    let author_style = style.fg(Color::Yellow);
    let link_style = link_style().bg(bg);
    let tag_style = style.fg(Color::Magenta);
    let link = post.url.as_deref().unwrap_or(&post.discussion_url);

    let mut meta = vec![
        Span::styled(format!("  {} points", post.score), score_style),
        Span::styled(" | ", label_style),
        Span::styled(format!("{} comments", post.comment_count), comments_style),
        Span::styled(" | by ", label_style),
        Span::styled(post.author.clone(), author_style),
        Span::styled(" | ", label_style),
        Span::styled(link.to_string(), link_style),
    ];
    if !post.tags.is_empty() {
        meta.push(Span::styled(" | tags: ", label_style));
        meta.push(Span::styled(post.tags.join(", "), tag_style));
    }

    ListItem::new(Text::from(vec![
        Line::from(Span::styled(post.title.clone(), title_style)),
        Line::from(meta),
    ]))
    .style(style)
}

pub(crate) fn post_header_text(post: Option<&Post>) -> Text<'static> {
    let Some(post) = post else {
        return Text::from(Line::from(Span::styled(
            "No post selected",
            Style::default().fg(Color::DarkGray),
        )));
    };

    let link = post.url.as_deref().unwrap_or(&post.discussion_url);
    let mut lines = vec![
        Line::from(Span::styled(
            post.title.clone(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled("by ", Style::default().fg(Color::DarkGray)),
            Span::styled(post.author.clone(), Style::default().fg(Color::Yellow)),
            Span::styled(" | ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{} points", post.score),
                Style::default().fg(Color::Green),
            ),
            Span::styled(" | ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{} comments", post.comment_count),
                Style::default().fg(Color::Cyan),
            ),
        ]),
        Line::from(vec![
            Span::styled("url: ", Style::default().fg(Color::DarkGray)),
            Span::styled(link.to_string(), link_style()),
        ]),
        Line::from(vec![
            Span::styled("discussion: ", Style::default().fg(Color::DarkGray)),
            Span::styled(post.discussion_url.clone(), link_style()),
        ]),
    ];

    if let Some(text) = &post.text {
        let text = clean_comment_text(text);
        if !text.is_empty() {
            lines.push(Line::from(""));
            lines.extend(
                text.lines()
                    .map(|line| Line::from(link_spans(line, Style::default().fg(Color::Gray)))),
            );
        }
    }

    Text::from(lines)
}

pub(crate) fn render_posts(frame: &mut Frame, app: &App, area: Rect) {
    let items = if app.posts.is_empty() {
        vec![ListItem::new("No posts loaded. Press r to retry.")]
    } else {
        app.posts
            .iter()
            .enumerate()
            .map(|(index, post)| post_list_item(post, index))
            .collect()
    };

    let mut state = ListState::default();
    let selected = if app.posts.is_empty() {
        None
    } else {
        Some(app.post_selected.min(app.posts.len() - 1))
    };
    state.select(selected);
    let list = List::new(items)
        .block(Block::bordered().title("Posts"))
        .highlight_symbol(">> ")
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_stateful_widget(list, area, &mut state);
}

pub(crate) fn comment_prefix_for(depth: usize, starts_branch: bool) -> (String, String) {
    if depth == 0 {
        return (String::new(), "  ".to_string());
    }

    let rail = "│  ".repeat(depth - 1);
    let joint = if starts_branch { "┌─ " } else { "├─ " };
    (format!("{rail}{joint}"), format!("{rail}│  "))
}

pub(crate) fn selected_comment_prefix(depth: usize) -> String {
    if depth == 0 {
        "▶ ".to_string()
    } else {
        format!("{}▶─ ", "│  ".repeat(depth - 1))
    }
}

pub(crate) fn comment_separator_for(previous_depth: usize, depth: usize) -> Option<String> {
    let rails = previous_depth.min(depth);
    if rails > 0 {
        Some("│  ".repeat(rails))
    } else if depth <= previous_depth {
        Some(String::new())
    } else {
        None
    }
}

pub(crate) fn comment_descendant_count(comments: &[Comment], index: usize) -> usize {
    let Some(comment) = comments.get(index) else {
        return 0;
    };
    comments[index + 1..]
        .iter()
        .take_while(|child| child.depth > comment.depth)
        .count()
}

pub(crate) fn visible_comment_indices(
    comments: &[Comment],
    collapsed: &HashSet<usize>,
) -> Vec<usize> {
    let mut visible = Vec::new();
    let mut index = 0;

    while index < comments.len() {
        visible.push(index);
        let hidden = comment_descendant_count(comments, index);
        if hidden > 0 && collapsed.contains(&index) {
            index += hidden + 1;
        } else {
            index += 1;
        }
    }

    visible
}

pub(crate) fn comment_text_lines(
    comments: &[Comment],
    collapsed: &HashSet<usize>,
    selected: Option<usize>,
    width: usize,
) -> (Vec<String>, Vec<Option<usize>>) {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut owners = Vec::new();

    if comments.is_empty() {
        lines.push("No comments.".to_string());
        owners.push(None);
        return (lines, owners);
    }

    let mut index = 0;
    let mut last_visible_depth = None;
    while index < comments.len() {
        let comment = &comments[index];
        if let Some(last_depth) = last_visible_depth
            && let Some(separator) = comment_separator_for(last_depth, comment.depth)
        {
            lines.push(separator);
            owners.push(None);
        }

        let hidden = comment_descendant_count(comments, index);
        let is_collapsed = collapsed.contains(&index);
        let starts_branch = last_visible_depth.is_none_or(|depth| depth < comment.depth);
        let (mut author_prefix, text_prefix) = comment_prefix_for(comment.depth, starts_branch);
        if selected == Some(index) {
            author_prefix = selected_comment_prefix(comment.depth);
        }
        let author = if is_collapsed && hidden > 0 {
            format!("{} [+{}]", comment.author, hidden)
        } else if is_collapsed {
            format!("{} [collapsed]", comment.author)
        } else {
            comment.author.clone()
        };
        push_wrapped_lines(
            &mut lines,
            &mut owners,
            Some(index),
            &author_prefix,
            &author,
            width,
        );
        if !is_collapsed {
            let text = clean_comment_text(&comment.text);
            push_wrapped_lines(
                &mut lines,
                &mut owners,
                Some(index),
                &text_prefix,
                &text,
                width,
            );
        }
        if is_collapsed {
            let collapsed_text = if hidden == 0 {
                "… comment collapsed".to_string()
            } else {
                format!("… {hidden} replies collapsed")
            };
            push_wrapped_lines(
                &mut lines,
                &mut owners,
                Some(index),
                &text_prefix,
                &collapsed_text,
                width,
            );
            last_visible_depth = Some(comment.depth);
            index += hidden + 1;
        } else {
            last_visible_depth = Some(comment.depth);
            index += 1;
        }
    }

    (lines, owners)
}

pub(crate) fn push_wrapped_lines(
    lines: &mut Vec<String>,
    owners: &mut Vec<Option<usize>>,
    owner: Option<usize>,
    prefix: &str,
    text: &str,
    width: usize,
) {
    let available = width.saturating_sub(prefix.chars().count()).max(1);

    for raw in text.lines() {
        let mut rest = raw.trim_end();
        if rest.is_empty() {
            lines.push(prefix.to_string());
            owners.push(owner);
            continue;
        }

        while rest.chars().count() > available {
            let split = split_at_width(rest, available);
            let (head, tail) = rest.split_at(split);
            lines.push(format!("{prefix}{}", head.trim_end()));
            owners.push(owner);
            rest = tail.trim_start();
            if rest.is_empty() {
                break;
            }
        }

        if !rest.is_empty() {
            lines.push(format!("{prefix}{rest}"));
            owners.push(owner);
        }
    }
}

pub(crate) fn split_at_width(text: &str, width: usize) -> usize {
    let mut last_space = None;

    for (count, (index, ch)) in text.char_indices().enumerate() {
        if count == width {
            return last_space.unwrap_or(index);
        }
        if ch.is_whitespace() && index > 0 {
            last_space = Some(index);
        }
    }

    text.len()
}

pub(crate) fn author_color(author: &str) -> Color {
    const COLORS: [Color; 8] = [
        Color::Rgb(95, 135, 160),
        Color::Rgb(100, 145, 105),
        Color::Rgb(170, 145, 70),
        Color::Rgb(165, 115, 90),
        Color::Rgb(145, 105, 150),
        Color::Rgb(95, 150, 140),
        Color::Rgb(160, 95, 95),
        Color::Rgb(120, 140, 160),
    ];

    let hash = author.bytes().fold(0usize, |hash, byte| {
        hash.wrapping_mul(31).wrapping_add(byte as usize)
    });
    COLORS[hash % COLORS.len()]
}

pub(crate) fn split_at_char_count(text: &str, count: usize) -> (&str, &str) {
    if count == 0 {
        return ("", text);
    }

    match text.char_indices().nth(count) {
        Some((index, _)) => text.split_at(index),
        None => (text, ""),
    }
}

pub(crate) fn deselected_comment_header_line(line: &str, comment: &Comment) -> Line<'static> {
    let (prefix, author) = split_at_char_count(line, comment.depth * 3);
    let mut spans = Vec::new();
    if !prefix.is_empty() {
        spans.push(Span::styled(
            prefix.to_string(),
            Style::default().fg(Color::DarkGray),
        ));
    }
    spans.push(Span::styled(
        author.to_string(),
        Style::default().fg(author_color(&comment.author)),
    ));
    Line::from(spans)
}

pub(crate) fn deselected_comment_body_line(line: &str, depth: usize) -> Line<'static> {
    let (prefix, body) = split_at_char_count(line, if depth == 0 { 2 } else { depth * 3 });
    let mut spans = Vec::new();
    if !prefix.is_empty() {
        spans.push(Span::styled(
            prefix.to_string(),
            Style::default().fg(Color::DarkGray),
        ));
    }
    spans.extend(link_spans(body, Style::default().fg(Color::Gray)));
    Line::from(spans)
}

pub(crate) fn selected_comment_line(
    line: &str,
    depth: usize,
    is_comment_header: bool,
) -> Line<'static> {
    let selected_style = Style::default()
        .fg(Color::Rgb(255, 255, 0))
        .add_modifier(Modifier::BOLD);
    let ancestor_style = Style::default().fg(Color::DarkGray);
    let text_style = Style::default().fg(Color::Gray);

    if is_comment_header {
        let ancestor = "│  ".repeat(depth.saturating_sub(1));
        let closest = if depth == 0 { "▶ " } else { "▶─ " };
        let prefix = format!("{ancestor}{closest}");
        if let Some(rest) = line.strip_prefix(&prefix) {
            let mut spans = Vec::new();
            if !ancestor.is_empty() {
                spans.push(Span::styled(ancestor, ancestor_style));
            }
            spans.push(Span::styled(closest.to_string(), selected_style));
            spans.push(Span::styled(rest.to_string(), selected_style));
            return Line::from(spans);
        }

        return Line::styled(line.to_string(), selected_style);
    }

    if depth > 0 {
        let ancestor = "│  ".repeat(depth - 1);
        let closest = "│  ";
        let prefix = format!("{ancestor}{closest}");
        if let Some(rest) = line.strip_prefix(&prefix) {
            let mut spans = Vec::new();
            if !ancestor.is_empty() {
                spans.push(Span::styled(ancestor, ancestor_style));
            }
            spans.push(Span::styled(closest.to_string(), selected_style));
            spans.extend(link_spans(rest, text_style));
            return Line::from(spans);
        }
    }

    Line::from(link_spans(line, text_style))
}

pub(crate) fn comment_lines_text(
    lines: &[String],
    owners: &[Option<usize>],
    comments: &[Comment],
    selected: Option<usize>,
) -> Text<'static> {
    Text::from(
        lines
            .iter()
            .zip(owners.iter())
            .enumerate()
            .map(|(line_index, (line, owner))| {
                let is_comment_header = owner.is_some()
                    && line_index.checked_sub(1).and_then(|i| owners.get(i)) != Some(owner);
                if *owner == selected {
                    let depth = owner
                        .and_then(|index| comments.get(index))
                        .map_or(0, |comment| comment.depth);
                    selected_comment_line(line, depth, is_comment_header)
                } else if let (Some(index), true) = (*owner, is_comment_header) {
                    comments.get(index).map_or_else(
                        || Line::styled(line.clone(), Style::default().fg(Color::Gray)),
                        |comment| deselected_comment_header_line(line, comment),
                    )
                } else if let Some(index) = *owner {
                    comments.get(index).map_or_else(
                        || Line::styled(line.clone(), Style::default().fg(Color::Gray)),
                        |comment| deselected_comment_body_line(line, comment.depth),
                    )
                } else {
                    Line::styled(line.clone(), Style::default().fg(Color::DarkGray))
                }
            })
            .collect::<Vec<_>>(),
    )
}

pub(crate) fn owner_line_range(owners: &[Option<usize>], owner: usize) -> Option<(usize, usize)> {
    let first = owners
        .iter()
        .position(|&line_owner| line_owner == Some(owner))?;
    let last = owners
        .iter()
        .rposition(|&line_owner| line_owner == Some(owner))?;
    Some((first, last))
}

pub(crate) fn scroll_to_show_comment(
    owners: &[Option<usize>],
    selected: usize,
    current_scroll: usize,
    viewport_height: usize,
    max_scroll: usize,
) -> usize {
    let Some((first, last)) = owner_line_range(owners, selected) else {
        return current_scroll.min(max_scroll);
    };
    let viewport_height = viewport_height.max(1);
    let comment_height = last - first + 1;

    if comment_height > viewport_height || first < current_scroll {
        first.min(max_scroll)
    } else if last >= current_scroll + viewport_height {
        last.saturating_sub(viewport_height - 1).min(max_scroll)
    } else {
        current_scroll.min(max_scroll)
    }
}

pub(crate) fn render_comments(frame: &mut Frame, app: &mut App, area: Rect) {
    let [header_area, comments_area] =
        Layout::vertical([Constraint::Length(7), Constraint::Min(0)]).areas(area);

    frame.render_widget(
        Paragraph::new(post_header_text(app.selected_post()))
            .block(Block::bordered().title("Post"))
            .wrap(Wrap { trim: false }),
        header_area,
    );

    let inner_width = comments_area.width.saturating_sub(2) as usize;
    let inner_height = (comments_area.height.saturating_sub(2) as usize).max(1);
    let (lines, owners) = comment_text_lines(
        &app.comments,
        &app.collapsed_comments,
        Some(app.comment_selected),
        inner_width,
    );
    app.comment_max_scroll = lines.len().saturating_sub(inner_height);
    app.comment_scroll = app.comment_scroll.min(app.comment_max_scroll);
    if app.comment_keep_selection_visible {
        app.comment_scroll = scroll_to_show_comment(
            &owners,
            app.comment_selected,
            app.comment_scroll,
            inner_height,
            app.comment_max_scroll,
        );
        app.comment_keep_selection_visible = false;
    }

    let title = if app.comments.is_empty() {
        "Comments".to_string()
    } else {
        format!(
            "Comments · line {}/{}",
            app.comment_scroll + 1,
            lines.len().max(1)
        )
    };
    let scroll = app.comment_scroll.min(u16::MAX as usize) as u16;
    frame.render_widget(
        Paragraph::new(comment_lines_text(
            &lines,
            &owners,
            &app.comments,
            Some(app.comment_selected),
        ))
        .block(Block::bordered().title(title))
        .scroll((scroll, 0)),
        comments_area,
    );
}
