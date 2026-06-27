use std::collections::HashSet;

use ratatui::crossterm::event::{self, KeyCode, KeyEvent};
use ratatui::style::{Color, Modifier, Style};

use crate::app::App;
use crate::fetch::parse_hn_comments_html;
use crate::input::handle_key;
use crate::model::{Comment, Mode, Post, Source};
use crate::text::{extract_first_url, html_to_text};
use crate::ui::*;

fn test_post(source: Source, id: &str) -> Post {
    Post {
        source,
        id: id.to_string(),
        title: "title".to_string(),
        author: "author".to_string(),
        score: 1,
        comment_count: 1,
        url: Some("https://example.com".to_string()),
        discussion_url: "https://example.com/discussion".to_string(),
        text: None,
        tags: Vec::new(),
    }
}

fn test_comment(author: &str, depth: usize, text: &str) -> Comment {
    Comment {
        author: author.to_string(),
        depth,
        text: text.to_string(),
        url: None,
    }
}

fn line_texts(lines: &[CommentLine]) -> Vec<&str> {
    lines.iter().map(|(line, _)| line.as_str()).collect()
}

fn line_owners(lines: &[CommentLine]) -> Vec<Option<usize>> {
    lines.iter().map(|(_, owner)| *owner).collect()
}

#[test]
fn extract_first_url_trims_common_trailing_punctuation() {
    assert_eq!(
        extract_first_url("see (https://example.com/a)."),
        Some("https://example.com/a".to_string())
    );
    assert_eq!(extract_first_url("none"), None);
}

#[test]
fn link_spans_underlines_urls() {
    let spans = link_spans(
        "see (https://example.com/a).",
        Style::default().fg(Color::Gray),
    );

    assert_eq!(spans[0].content, "see (");
    assert_eq!(spans[1].content, "https://example.com/a");
    assert!(spans[1].style.add_modifier.contains(Modifier::UNDERLINED));
    assert_eq!(spans[2].content, ").");
    assert!(!spans[2].style.add_modifier.contains(Modifier::UNDERLINED));
}

#[test]
fn html_to_text_preserves_basic_breaks_and_entities() {
    assert_eq!(
        html_to_text("AT&T <p>one&nbsp;&amp;&#x27;</p><p>two</p>"),
        "AT&T\n\none &'\n\ntwo"
    );
}

#[test]
fn parse_hn_comments_html_reads_depth_author_text_and_url() {
    let html = r#"
      <table class="comment-tree">
        <tr class="athing comtr" id="1"><td><table><tr>
          <td class="ind" indent="0"><img width="0"></td>
          <td class="default"><span class="comhead"><a class="hnuser">alice</a></span>
          <div class="comment"><div class="commtext c00">hello <a href="https://example.com/full/path?x=1&amp;y=2" rel="nofollow">https://example.com/full/...</a><p>second paragraph</div></div></td>
        </tr></table></td></tr>
        <tr class='athing comtr' id="2"><td><table><tr>
          <td class="ind" indent='2'><img width="80"></td>
          <td class="default"><span class="comhead"><a class='hnuser'>bob</a></span>
          <div class="comment"><div class='commtext c00'>reply &amp; more</div></div></td>
        </tr></table></td></tr>
      </table>
    "#;

    let comments = parse_hn_comments_html(html);

    assert_eq!(comments.len(), 2);
    assert_eq!(comments[0].author, "alice");
    assert_eq!(comments[0].depth, 0);
    assert!(comments[0].text.contains("hello"));
    assert_eq!(
        comments[0].url.as_deref(),
        Some("https://example.com/full/path?x=1&y=2")
    );
    assert!(!comments[0].text.contains("..."));
    assert!(
        comments[0]
            .text
            .contains("hello https://example.com/full/path?x=1&y=2\n\nsecond paragraph")
    );
    assert_eq!(comments[1].author, "bob");
    assert_eq!(comments[1].depth, 2);
    assert!(comments[1].text.contains("reply & more"));
}

#[test]
fn parse_hn_comments_html_returns_empty_for_no_comments() {
    assert!(parse_hn_comments_html("<html></html>").is_empty());
}

#[test]
fn post_header_text_includes_colored_metadata_lines() {
    let mut post = test_post(Source::HackerNews, "1");
    post.text = Some("summary".to_string());

    let text = post_header_text(Some(&post));

    assert!(text.lines.len() >= 5);
    assert!(
        text.lines[2].spans[1]
            .style
            .add_modifier
            .contains(Modifier::UNDERLINED)
    );
}

#[test]
fn movement_on_empty_lists_is_a_noop() {
    let mut app = App::new(reqwest::blocking::Client::new());
    app.move_down();
    app.move_up();
    app.move_bottom();
    app.mode = Mode::Comments;
    app.move_down();
    app.move_up();
    app.move_bottom();
    assert_eq!(app.post_selected, 0);
    assert_eq!(app.comment_selected, 0);
    assert_eq!(app.comment_scroll, 0);
    assert_eq!(app.comment_max_scroll, 0);
}

#[test]
fn switching_sources_resets_selection_and_comments() {
    let mut app = App::new(reqwest::blocking::Client::new());
    app.posts = vec![
        test_post(Source::HackerNews, "1"),
        test_post(Source::HackerNews, "2"),
    ];
    app.comments = vec![
        Comment {
            author: "a".to_string(),
            depth: 0,
            text: "x".to_string(),
            url: None,
        },
        Comment {
            author: "b".to_string(),
            depth: 1,
            text: "y".to_string(),
            url: None,
        },
    ];
    app.post_selected = 1;
    app.comment_selected = 1;
    app.mode = Mode::Comments;
    app.comment_scroll = 2;
    app.comment_max_scroll = 3;
    app.source = Source::Lobsters;
    app.collapsed_comments.insert(0);

    app.set_posts(Vec::new(), "Loaded 0 Lobsters posts".to_string());

    assert_eq!(app.source, Source::Lobsters);
    assert!(app.posts.is_empty());
    assert!(app.comments.is_empty());
    assert_eq!(app.post_selected, 0);
    assert_eq!(app.comment_selected, 0);
    assert_eq!(app.comment_scroll, 0);
    assert_eq!(app.comment_max_scroll, 0);
    assert!(app.collapsed_comments.is_empty());
    assert_eq!(app.mode, Mode::Posts);
}

#[test]
fn comment_prefix_makes_nested_comments_visible() {
    assert_eq!(
        comment_prefix_for(0, false),
        ("".to_string(), "  ".to_string())
    );
    assert_eq!(
        comment_prefix_for(1, false),
        ("├─ ".to_string(), "│  ".to_string())
    );
    assert_eq!(
        comment_prefix_for(2, false),
        ("│  ├─ ".to_string(), "│  │  ".to_string())
    );
}

#[test]
fn selected_comment_prefix_keeps_nested_rails_aligned() {
    assert_eq!(selected_comment_prefix(0), "▶ ");
    assert_eq!(selected_comment_prefix(1), "▶─ ");
    assert_eq!(selected_comment_prefix(2), "│  ▶─ ");
}

#[test]
fn child_comment_starts_without_detached_separator() {
    let comments = vec![
        test_comment("parent", 0, "parent text"),
        test_comment("child", 1, "child text"),
    ];

    let lines = comment_text_lines(&comments, &HashSet::new(), None, 80);

    assert_eq!(lines[1].0.as_str(), "  parent text");
    assert_eq!(lines[1].1, Some(0));
    assert_eq!(lines[2].0.as_str(), "┌─ child");
    assert_eq!(lines[2].1, Some(1));
}

#[test]
fn comment_text_keeps_blank_line_between_paragraphs() {
    let comments = vec![test_comment("alice", 0, "first\n\nsecond")];
    let lines = comment_text_lines(&comments, &HashSet::new(), None, 80);

    assert_eq!(
        line_texts(&lines),
        vec!["alice", "  first", "  ", "  second"]
    );
}

#[test]
fn nested_comment_separators_keep_rails() {
    let comments = vec![
        test_comment("root", 0, "root text"),
        test_comment("first", 1, "first text"),
        test_comment("child", 2, "child text"),
        test_comment("second", 1, "second text"),
    ];

    let lines = comment_text_lines(&comments, &HashSet::new(), None, 80);

    assert_eq!(
        line_texts(&lines),
        vec![
            "root",
            "  root text",
            "┌─ first",
            "│  first text",
            "│  ",
            "│  ┌─ child",
            "│  │  child text",
            "│  ",
            "├─ second",
            "│  second text",
        ]
    );
    assert_eq!(lines[4].1, None);
    assert_eq!(lines[7].1, None);
}

#[test]
fn comment_lines_text_underlines_urls() {
    let comments = vec![test_comment("alice", 0, "see https://example.com/a.")];
    let lines = comment_text_lines(&comments, &HashSet::new(), None, 80);

    let text = comment_lines_text(&lines, &comments, None);
    let url_span = text.lines[1]
        .spans
        .iter()
        .find(|span| span.content == "https://example.com/a")
        .expect("url span");

    assert!(url_span.style.add_modifier.contains(Modifier::UNDERLINED));
}

#[test]
fn comment_text_wraps_long_comments_with_rails() {
    let comments = vec![Comment {
        author: "alice".to_string(),
        depth: 1,
        text: "one two three four five".to_string(),
        url: None,
    }];

    let lines = comment_text_lines(&comments, &HashSet::new(), None, 8);

    assert_eq!(
        line_owners(&lines),
        vec![Some(0), Some(0), Some(0), Some(0), Some(0), Some(0)]
    );
    assert_eq!(
        line_texts(&lines),
        vec![
            "┌─ alice",
            "│  one",
            "│  two",
            "│  three",
            "│  four",
            "│  five"
        ]
    );
}

#[test]
fn comment_text_strips_code_fence_backticks() {
    let comments = vec![test_comment(
        "alice",
        0,
        "before\n```rust\nlet x = `value`;\n```\nafter",
    )];

    let lines = comment_text_lines(&comments, &HashSet::new(), None, 80);

    assert!(
        lines
            .iter()
            .any(|(line, _)| line.contains("let x = value;"))
    );
    assert!(!lines.iter().any(|(line, _)| line.contains("```")));
    assert!(!lines.iter().any(|(line, _)| line.contains('`')));
}

#[test]
fn selected_comment_is_marked() {
    let comments = vec![test_comment("alice", 0, "hello")];

    let lines = comment_text_lines(&comments, &HashSet::new(), Some(0), 80);

    assert!(lines[0].0.starts_with("▶ alice"));
}

#[test]
fn collapsed_comment_hides_descendants() {
    let comments = vec![
        test_comment("root", 0, "root text"),
        test_comment("child", 1, "child text"),
        test_comment("grandchild", 2, "grandchild text"),
        test_comment("sibling", 0, "sibling text"),
    ];
    let collapsed = HashSet::from([0]);

    let lines = comment_text_lines(&comments, &collapsed, None, 80);

    assert!(
        lines
            .iter()
            .any(|(line, _)| line.contains("2 replies collapsed"))
    );
    assert!(!lines.iter().any(|(line, _)| line.contains("root text")));
    assert!(lines.iter().any(|(line, _)| line.contains("sibling")));
    assert!(!lines.iter().any(|(line, _)| line.contains("child")));
    assert!(lines.iter().any(|(_, owner)| *owner == Some(0)));
    assert!(lines.iter().any(|(_, owner)| *owner == Some(3)));
}

#[test]
fn toggle_comment_collapse_tracks_selected_tree() {
    let mut app = App::new(reqwest::blocking::Client::new());
    app.mode = Mode::Comments;
    app.comments = vec![
        test_comment("root", 0, "root text"),
        test_comment("child", 1, "child text"),
        test_comment("sibling", 0, "sibling text"),
    ];

    app.toggle_comment_collapse();
    assert!(app.collapsed_comments.contains(&0));
    assert!(app.status.contains("Collapsed 1 replies"));

    app.toggle_comment_collapse();
    assert!(!app.collapsed_comments.contains(&0));
    assert!(app.status.contains("Expanded 1 replies"));

    app.comment_selected = 2;
    app.toggle_comment_collapse();
    assert!(app.collapsed_comments.contains(&2));
    assert_eq!(app.status, "Collapsed comment");

    let lines = comment_text_lines(&app.comments, &app.collapsed_comments, None, 80);
    assert!(
        lines
            .iter()
            .any(|(line, _)| line.contains("sibling [collapsed]"))
    );
    assert!(!lines.iter().any(|(line, _)| line.contains("sibling text")));
}

#[test]
fn left_right_select_visible_comments() {
    let mut app = App::new(reqwest::blocking::Client::new());
    app.mode = Mode::Comments;
    app.comments = vec![
        test_comment("root", 0, "root text"),
        test_comment("child", 1, "child text"),
        test_comment("sibling", 0, "sibling text"),
    ];
    app.collapsed_comments.insert(0);

    app.select_next_comment();
    assert_eq!(app.comment_selected, 2);
    assert!(app.comment_keep_selection_visible);

    app.select_previous_comment();
    assert_eq!(app.comment_selected, 0);
}

#[test]
fn selecting_comment_scrolls_until_whole_comment_is_visible() {
    let lines = [
        ("", Some(0)),
        ("", Some(0)),
        ("", None),
        ("", Some(1)),
        ("", Some(1)),
        ("", Some(1)),
        ("", None),
        ("", Some(2)),
    ]
    .map(|(line, owner)| (line.to_string(), owner));

    assert_eq!(owner_line_range(&lines, 1), Some((3, 5)));
    assert_eq!(scroll_to_show_comment(&lines, 1, 0, 4, 4), 2);
    assert_eq!(scroll_to_show_comment(&lines, 1, 4, 4, 4), 3);
    assert_eq!(scroll_to_show_comment(&lines, 0, 3, 4, 4), 0);
    assert_eq!(scroll_to_show_comment(&lines, 1, 0, 2, 6), 3);
}

#[test]
fn control_c_and_control_d_quit() {
    let mut app = App::new(reqwest::blocking::Client::new());

    assert!(handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('c'), event::KeyModifiers::CONTROL),
    ));
    assert!(handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('d'), event::KeyModifiers::CONTROL),
    ));
}

#[test]
fn post_index_right_does_not_open_comments() {
    let mut app = App::new(reqwest::blocking::Client::new());

    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Right, event::KeyModifiers::NONE),
    );
    assert_eq!(app.mode, Mode::Posts);
    assert_eq!(app.status, "Loading Hacker News...");

    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Enter, event::KeyModifiers::NONE),
    );
    assert_eq!(app.status, "No post selected");
}

#[test]
fn comment_mode_scrolls_by_line() {
    let mut app = App::new(reqwest::blocking::Client::new());
    app.mode = Mode::Comments;
    app.comments = vec![test_comment("alice", 0, "x"), test_comment("bob", 0, "y")];
    app.comment_selected = 1;
    app.comment_max_scroll = 2;

    app.move_down();
    app.move_down();
    app.move_down();
    assert_eq!(app.comment_scroll, 2);
    assert_eq!(app.comment_selected, 1);
    assert!(!app.comment_keep_selection_visible);

    app.move_up();
    assert_eq!(app.comment_scroll, 1);
    assert_eq!(app.comment_selected, 1);

    app.move_top();
    assert_eq!(app.comment_scroll, 0);
    assert_eq!(app.comment_selected, 0);

    app.move_bottom();
    assert_eq!(app.comment_scroll, 2);
}

#[test]
fn rendering_empty_comments_does_not_panic() {
    let backend = ratatui::backend::TestBackend::new(80, 24);
    let mut terminal = ratatui::Terminal::new(backend).expect("test terminal");
    let mut app = App::new(reqwest::blocking::Client::new());
    app.mode = Mode::Comments;

    terminal
        .draw(|frame| render(frame, &mut app))
        .expect("draw empty comments");
}
