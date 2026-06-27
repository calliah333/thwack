use std::{collections::HashSet, num::NonZeroU16};

use ratatui::{
    Frame,
    buffer::{Buffer, Cell, CellDiffOption},
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::app::App;
use crate::model::{Comment, Mode, Post, source_title};
use crate::text::{TextLink, clean_comment_lines_with_links, clean_comment_text};

pub(crate) fn link_style() -> Style {
    Style::default()
        .fg(Color::LightBlue)
        .add_modifier(Modifier::UNDERLINED)
}

fn is_link_cell(cell: &Cell) -> bool {
    cell.modifier.contains(Modifier::UNDERLINED)
}

fn is_osc8_uri(text: &str) -> bool {
    (text.starts_with("http://") || text.starts_with("https://"))
        && text.bytes().all(|byte| (32..=126).contains(&byte))
}

fn osc8_link(url: &str, text: &str) -> String {
    format!("\x1b]8;;{url}\x1b\\{text}\x1b]8;;\x1b\\")
}

pub(crate) fn apply_hyperlinks(buffer: &mut Buffer) {
    for y in buffer.area.top()..buffer.area.bottom() {
        let mut x = buffer.area.left();
        while x < buffer.area.right() {
            if !buffer.cell((x, y)).is_some_and(is_link_cell) {
                x += 1;
                continue;
            }

            let start = x;
            let mut text = String::new();
            while x < buffer.area.right() && buffer.cell((x, y)).is_some_and(is_link_cell) {
                text.push_str(buffer[(x, y)].symbol());
                x += 1;
            }

            if is_osc8_uri(&text) {
                let width = NonZeroU16::new(x - start).expect("non-empty link span");
                buffer
                    .cell_mut((start, y))
                    .expect("link start is inside buffer")
                    .set_symbol(&osc8_link(&text, &text))
                    .set_diff_option(CellDiffOption::ForcedWidth(width));
            }
        }
    }
}

pub(crate) fn link_spans(text: &str, plain_style: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut rest = text;

    while let Some(start) = [rest.find("http://"), rest.find("https://")]
        .into_iter()
        .flatten()
        .min()
    {
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

fn byte_at_char(text: &str, char_index: usize) -> usize {
    text.char_indices()
        .nth(char_index)
        .map(|(index, _)| index)
        .unwrap_or(text.len())
}

fn links_in_range(line: &CommentLine, start: usize, len: usize) -> Vec<CommentLink> {
    let end = start + len;
    line.links
        .iter()
        .filter_map(|link| {
            let link_start = link.start.max(start);
            let link_end = link.end.min(end);
            (link_start < link_end).then(|| CommentLink {
                start: link_start - start,
                end: link_end - start,
                url: link.url.clone(),
            })
        })
        .collect()
}

fn link_spans_with_ranges(
    text: &str,
    plain_style: Style,
    links: &[CommentLink],
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut cursor = 0usize;

    for link in links {
        if cursor < link.start {
            spans.extend(link_spans(
                &text[byte_at_char(text, cursor)..byte_at_char(text, link.start)],
                plain_style,
            ));
        }
        spans.push(Span::styled(
            text[byte_at_char(text, link.start)..byte_at_char(text, link.end)].to_string(),
            link_style(),
        ));
        cursor = link.end;
    }

    if cursor < text.chars().count() {
        spans.extend(link_spans(&text[byte_at_char(text, cursor)..], plain_style));
    }

    spans
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
            "j/k/↑/↓ scroll · h/l/←/→ select · Space collapse · Esc posts · o open · c discussion · r reload · q quit"
        }
    };
    frame.render_widget(
        Paragraph::new(format!("{} | {capability} | {help}", app.status)),
        status_area,
    );
    apply_hyperlinks(frame.buffer_mut());
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
            Span::styled(" | id ", Style::default().fg(Color::DarkGray)),
            Span::styled(post.id.clone(), Style::default().fg(Color::Magenta)),
        ]),
        Line::from(vec![
            Span::styled("url: ", Style::default().fg(Color::DarkGray)),
            Span::styled(link.to_string(), link_style()),
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

pub(crate) fn has_later_visible_sibling(
    comments: &[Comment],
    visible: &[usize],
    position: usize,
) -> bool {
    let depth = comments[visible[position]].depth;
    for &index in &visible[position + 1..] {
        let next_depth = comments[index].depth;
        if next_depth < depth {
            return false;
        }
        if next_depth == depth {
            return true;
        }
    }
    false
}

pub(crate) fn ancestor_has_later_visible_sibling(
    comments: &[Comment],
    visible: &[usize],
    position: usize,
    ancestor_depth: usize,
) -> bool {
    for previous in (0..position).rev() {
        let depth = comments[visible[previous]].depth;
        if depth == ancestor_depth {
            return has_later_visible_sibling(comments, visible, previous);
        }
        if depth < ancestor_depth {
            return false;
        }
    }
    false
}

pub(crate) fn comment_prefix_for(
    comments: &[Comment],
    visible: &[usize],
    position: usize,
) -> (String, String) {
    let depth = comments[visible[position]].depth;
    if depth == 0 {
        return (String::new(), "  ".to_string());
    }

    let mut prefix = String::new();
    for ancestor_depth in 1..depth {
        if ancestor_has_later_visible_sibling(comments, visible, position, ancestor_depth) {
            prefix.push_str("│  ");
        } else {
            prefix.push_str("   ");
        }
    }

    let has_later_sibling = has_later_visible_sibling(comments, visible, position);
    let mut text_prefix = prefix.clone();
    text_prefix.push_str(if has_later_sibling { "│  " } else { "   " });
    prefix.push_str(if has_later_sibling {
        "├─ "
    } else {
        "└─ "
    });
    (prefix, text_prefix)
}

pub(crate) fn selected_comment_prefix(author_prefix: &str, depth: usize) -> String {
    if depth == 0 {
        return "▶ ".to_string();
    }

    let mut prefix = author_prefix.to_string();
    if prefix.ends_with("├─ ") || prefix.ends_with("└─ ") {
        prefix.truncate(prefix.len() - "├─ ".len());
    }
    prefix.push_str("▶─ ");
    prefix
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CommentLink {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) url: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CommentLine {
    pub(crate) text: String,
    pub(crate) owner: Option<usize>,
    pub(crate) links: Vec<CommentLink>,
}

impl CommentLine {
    fn new(text: String, owner: Option<usize>) -> Self {
        Self {
            text,
            owner,
            links: Vec::new(),
        }
    }
}

pub(crate) fn comment_descendant_count(comments: &[Comment], index: usize) -> usize {
    let Some(comment) = comments.get(index) else {
        return 0;
    };
    comments[index + 1..]
        .iter()
        .position(|child| child.depth <= comment.depth)
        .unwrap_or(comments.len() - index - 1)
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
) -> Vec<CommentLine> {
    let width = width.max(1);
    let mut lines = Vec::new();

    if comments.is_empty() {
        lines.push(CommentLine::new("No comments.".to_string(), None));
        return lines;
    }

    let visible = visible_comment_indices(comments, collapsed);
    for (position, &index) in visible.iter().enumerate() {
        let comment = &comments[index];

        let hidden = comment_descendant_count(comments, index);
        let is_collapsed = collapsed.contains(&index);
        let (mut author_prefix, text_prefix) = comment_prefix_for(comments, &visible, position);
        if selected == Some(index) {
            author_prefix = selected_comment_prefix(&author_prefix, comment.depth);
        }
        let author = if is_collapsed && hidden > 0 {
            format!("{} [+{}]", comment.author, hidden)
        } else if is_collapsed {
            format!("{} [collapsed]", comment.author)
        } else {
            comment.author.clone()
        };

        if is_collapsed {
            let collapsed_text = if hidden == 0 {
                "… comment collapsed".to_string()
            } else {
                format!("… {hidden} replies collapsed")
            };
            let author_prefix = format!("{author_prefix}{author}: ");
            push_wrapped_lines_with_continuation(
                &mut lines,
                Some(index),
                &author_prefix,
                &text_prefix,
                &collapsed_text,
                width,
            );
        } else {
            let text = clean_comment_lines_with_links(&comment.text);
            if text.iter().all(|(line, _)| line.is_empty()) {
                push_wrapped_lines(
                    &mut lines,
                    Some(index),
                    &author_prefix,
                    &format!("{author}:"),
                    width,
                );
            } else {
                let author_prefix = format!("{author_prefix}{author}: ");
                push_wrapped_link_lines_with_continuation(
                    &mut lines,
                    Some(index),
                    &author_prefix,
                    &text_prefix,
                    &text,
                    width,
                );
            }
        }
        let spacer_prefix = if visible
            .get(position + 1)
            .is_some_and(|&next| comments[next].depth > comment.depth)
        {
            let (mut prefix, _) = comment_prefix_for(comments, &visible, position + 1);
            if prefix.ends_with("├─ ") || prefix.ends_with("└─ ") {
                prefix.truncate(prefix.len() - "├─ ".len());
            }
            prefix.push_str("│  ");
            prefix
        } else {
            text_prefix
        };
        lines.push(CommentLine::new(spacer_prefix, None));
    }

    lines
}

pub(crate) fn push_wrapped_lines(
    lines: &mut Vec<CommentLine>,
    owner: Option<usize>,
    prefix: &str,
    text: &str,
    width: usize,
) {
    push_wrapped_lines_with_continuation(lines, owner, prefix, prefix, text, width);
}

pub(crate) fn push_wrapped_lines_with_continuation(
    lines: &mut Vec<CommentLine>,
    owner: Option<usize>,
    first_prefix: &str,
    continuation_prefix: &str,
    text: &str,
    width: usize,
) {
    let clean_lines = text
        .lines()
        .map(|line| (line.to_string(), Vec::new()))
        .collect::<Vec<_>>();
    push_wrapped_link_lines_with_continuation(
        lines,
        owner,
        first_prefix,
        continuation_prefix,
        &clean_lines,
        width,
    );
}

fn push_wrapped_link_lines_with_continuation(
    lines: &mut Vec<CommentLine>,
    owner: Option<usize>,
    first_prefix: &str,
    continuation_prefix: &str,
    text: &[(String, Vec<TextLink>)],
    width: usize,
) {
    let mut prefix = first_prefix;

    for (raw, links) in text {
        let mut rest = raw.trim_end();
        let mut rest_start = 0usize;
        if rest.is_empty() {
            lines.push(CommentLine::new(prefix.to_string(), owner));
            prefix = continuation_prefix;
            continue;
        }

        while !rest.is_empty() {
            let available = width.saturating_sub(prefix.chars().count()).max(1);
            if rest.chars().count() > available {
                let split = split_at_width(rest, available);
                let (head, tail) = rest.split_at(split);
                let head = head.trim_end();
                push_wrapped_segment(lines, owner, prefix, raw, links, rest_start, head);
                let trim = tail.len() - tail.trim_start().len();
                rest = tail.trim_start();
                rest_start += split + trim;
                prefix = continuation_prefix;
            } else {
                push_wrapped_segment(lines, owner, prefix, raw, links, rest_start, rest);
                prefix = continuation_prefix;
                break;
            }
        }
    }
}

fn push_wrapped_segment(
    lines: &mut Vec<CommentLine>,
    owner: Option<usize>,
    prefix: &str,
    raw: &str,
    links: &[TextLink],
    start: usize,
    text: &str,
) {
    let prefix_chars = prefix.chars().count();
    let end = start + text.len();
    let mut line = CommentLine::new(format!("{prefix}{text}"), owner);
    for link in links {
        let link_start = link.start.max(start);
        let link_end = link.end.min(end);
        if link_start < link_end {
            let before = raw[start..link_start].chars().count();
            let len = raw[link_start..link_end].chars().count();
            line.links.push(CommentLink {
                start: prefix_chars + before,
                end: prefix_chars + before + len,
                url: link.url.clone(),
            });
        }
    }
    lines.push(line);
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

pub(crate) fn comment_line(
    line: &CommentLine,
    comments: &[Comment],
    selected: Option<usize>,
    is_comment_header: bool,
) -> Line<'static> {
    let Some(index) = line.owner else {
        return Line::styled(line.text.clone(), Style::default().fg(Color::DarkGray));
    };
    let Some(comment) = comments.get(index) else {
        return Line::styled(line.text.clone(), Style::default().fg(Color::Gray));
    };
    let rail_style = Style::default().fg(Color::DarkGray);
    let text_style = Style::default().fg(Color::Gray);

    let is_selected = selected == Some(index);

    let split = if is_comment_header {
        if is_selected && comment.depth == 0 {
            2
        } else if comment.depth == 0 {
            0
        } else {
            comment.depth * 3
        }
    } else if comment.depth > 0 {
        comment.depth * 3
    } else {
        2
    };
    let (prefix, body) = split_at_char_count(&line.text, split);
    let body_start = prefix.chars().count();
    let mut spans = Vec::new();
    if !prefix.is_empty() {
        spans.push(Span::styled(prefix.to_string(), rail_style));
    }
    if is_comment_header {
        if is_selected && !comment.author.is_empty() {
            if let Some(rest) = body.strip_prefix(&comment.author) {
                spans.push(Span::styled(
                    comment.author.clone(),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ));
                let start = body_start + comment.author.chars().count();
                spans.extend(link_spans_with_ranges(
                    rest,
                    text_style,
                    &links_in_range(line, start, rest.chars().count()),
                ));
            } else {
                spans.extend(link_spans_with_ranges(
                    body,
                    text_style,
                    &links_in_range(line, body_start, body.chars().count()),
                ));
            }
        } else if let Some((author, rest)) = body.split_once(": ") {
            spans.push(Span::styled(
                format!("{author}: "),
                Style::default().fg(author_color(&comment.author)),
            ));
            let start = body_start + author.chars().count() + ": ".chars().count();
            spans.extend(link_spans_with_ranges(
                rest,
                text_style,
                &links_in_range(line, start, rest.chars().count()),
            ));
        } else {
            spans.push(Span::styled(
                body.to_string(),
                Style::default().fg(author_color(&comment.author)),
            ));
        }
    } else {
        spans.extend(link_spans_with_ranges(
            body,
            text_style,
            &links_in_range(line, body_start, body.chars().count()),
        ));
    }
    Line::from(spans)
}

pub(crate) fn comment_lines_text(
    lines: &[CommentLine],
    comments: &[Comment],
    selected: Option<usize>,
) -> Text<'static> {
    Text::from(
        lines
            .iter()
            .enumerate()
            .map(|(line_index, line)| {
                let owner = line.owner;
                let previous_owner = line_index
                    .checked_sub(1)
                    .and_then(|i| lines.get(i))
                    .and_then(|line| line.owner);
                let is_comment_header = owner.is_some() && previous_owner != owner;
                comment_line(line, comments, selected, is_comment_header)
            })
            .collect::<Vec<_>>(),
    )
}

pub(crate) fn owner_line_range(lines: &[CommentLine], owner: usize) -> Option<(usize, usize)> {
    let first = lines.iter().position(|line| line.owner == Some(owner))?;
    let last = lines.iter().rposition(|line| line.owner == Some(owner))?;
    Some((first, last))
}

pub(crate) fn apply_comment_hyperlinks(
    buffer: &mut Buffer,
    area: Rect,
    lines: &[CommentLine],
    scroll: usize,
) {
    let x0 = area.x.saturating_add(1);
    let y0 = area.y.saturating_add(1);
    let width = area.width.saturating_sub(2);
    let height = area.height.saturating_sub(2);

    for row in 0..height {
        let Some(line) = lines.get(scroll + row as usize) else {
            break;
        };
        for link in &line.links {
            let start = link.start.min(width as usize) as u16;
            let end = link.end.min(width as usize) as u16;
            if start >= end || !is_osc8_uri(&link.url) {
                continue;
            }

            let y = y0 + row;
            let mut text = String::new();
            for x in x0 + start..x0 + end {
                text.push_str(buffer[(x, y)].symbol());
            }
            let Some(width) = NonZeroU16::new(end - start) else {
                continue;
            };
            buffer
                .cell_mut((x0 + start, y))
                .expect("comment link start is inside buffer")
                .set_symbol(&osc8_link(&link.url, &text))
                .set_diff_option(CellDiffOption::ForcedWidth(width));
        }
    }
}

pub(crate) fn scroll_to_show_comment(
    lines: &[CommentLine],
    selected: usize,
    current_scroll: usize,
    viewport_height: usize,
    max_scroll: usize,
) -> usize {
    let Some((first, last)) = owner_line_range(lines, selected) else {
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

fn wrapped_header_height(header: &Text<'_>, width: u16) -> u16 {
    let width = width.saturating_sub(2).max(1) as usize;
    let height = header
        .lines
        .iter()
        .map(|line| wrapped_line_count(line, width))
        .sum::<usize>();

    height.saturating_add(2).min(u16::MAX as usize) as u16
}

fn wrapped_line_count(line: &Line<'_>, width: usize) -> usize {
    let mut text = String::new();
    for span in &line.spans {
        text.push_str(&span.content);
    }

    let mut lines = 1;
    let mut used = 0;
    let mut saw_word = false;
    for word in text.split_whitespace() {
        saw_word = true;
        let word_width = word.chars().count();
        if used == 0 {
            lines += word_width.saturating_sub(1) / width;
            used = (word_width.saturating_sub(1) % width) + 1;
        } else if used + 1 + word_width <= width {
            used += 1 + word_width;
        } else {
            lines += 1 + word_width.saturating_sub(1) / width;
            used = (word_width.saturating_sub(1) % width) + 1;
        }
    }

    if saw_word { lines } else { 1 }
}

pub(crate) fn render_comments(frame: &mut Frame, app: &mut App, area: Rect) {
    let header_text = post_header_text(app.selected_post());
    let header_height = wrapped_header_height(&header_text, area.width);
    let header = Paragraph::new(header_text)
        .block(Block::bordered().title("Post"))
        .wrap(Wrap { trim: false });
    let [header_area, comments_area] =
        Layout::vertical([Constraint::Length(header_height), Constraint::Min(0)]).areas(area);

    frame.render_widget(header, header_area);

    let inner_width = comments_area.width.saturating_sub(2) as usize;
    let inner_height = (comments_area.height.saturating_sub(2) as usize).max(1);
    let lines = comment_text_lines(
        &app.comments,
        &app.collapsed_comments,
        Some(app.comment_selected),
        inner_width,
    );
    app.comment_max_scroll = lines.len().saturating_sub(inner_height);
    app.comment_scroll = app.comment_scroll.min(app.comment_max_scroll);
    if app.comment_keep_selection_visible {
        app.comment_scroll = scroll_to_show_comment(
            &lines,
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
            &app.comments,
            Some(app.comment_selected),
        ))
        .block(Block::bordered().title(title))
        .scroll((scroll, 0)),
        comments_area,
    );
    apply_comment_hyperlinks(
        frame.buffer_mut(),
        comments_area,
        &lines,
        app.comment_scroll,
    );
}
