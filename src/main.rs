use std::{
    collections::{HashMap, HashSet},
    io,
    time::Duration,
};

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, List, ListItem, ListState, Paragraph, Wrap},
};

const POST_LIMIT: usize = 30;
const HN_COMMENT_WORKERS: usize = 16;
const USER_AGENT: &str = "thwack/0.1";
const HN_TOP_URL: &str = "https://hacker-news.firebaseio.com/v0/topstories.json";
const LOBSTERS_HOTTEST_URL: &str = "https://lobste.rs/hottest.json";

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum Source {
    HackerNews,
    Lobsters,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    Posts,
    Comments,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Capabilities {
    login: bool,
    submit_post: bool,
    submit_comment: bool,
}

impl Capabilities {
    const READ_ONLY: Self = Self {
        login: false,
        submit_post: false,
        submit_comment: false,
    };
}

trait Provider {
    fn label(&self) -> &'static str;
    fn title(&self) -> &'static str;
    fn capabilities(&self) -> Capabilities {
        Capabilities::READ_ONLY
    }
    fn fetch_posts(&self, client: &reqwest::blocking::Client) -> Result<Vec<Post>>;
    fn fetch_comments(
        &self,
        client: &reqwest::blocking::Client,
        post: &Post,
    ) -> Result<Vec<Comment>>;
}

struct HackerNewsProvider;
struct LobstersProvider;

static HACKER_NEWS_PROVIDER: HackerNewsProvider = HackerNewsProvider;
static LOBSTERS_PROVIDER: LobstersProvider = LobstersProvider;

#[derive(Debug)]
struct Post {
    source: Source,
    id: String,
    title: String,
    author: String,
    score: i64,
    comment_count: usize,
    url: Option<String>,
    discussion_url: String,
    text: Option<String>,
    tags: Vec<String>,
}

#[derive(Debug)]
struct Comment {
    author: String,
    depth: usize,
    text: String,
    url: Option<String>,
}

struct App {
    client: reqwest::blocking::Client,
    source: Source,
    mode: Mode,
    posts: Vec<Post>,
    post_cache: HashMap<Source, Vec<Post>>,
    post_selected: usize,
    comments: Vec<Comment>,
    comment_selected: usize,
    comment_scroll: usize,
    comment_max_scroll: usize,
    collapsed_comments: HashSet<usize>,
    comment_keep_selection_visible: bool,
    status: String,
}

#[derive(serde::Deserialize)]
struct HnItem {
    id: u64,
    #[serde(rename = "type")]
    kind: Option<String>,
    by: Option<String>,
    title: Option<String>,
    url: Option<String>,
    text: Option<String>,
    score: Option<i64>,
    descendants: Option<usize>,
    kids: Option<Vec<u64>>,
    deleted: Option<bool>,
    dead: Option<bool>,
}

#[derive(serde::Deserialize)]
struct LobstersStory {
    short_id: String,
    title: String,
    #[serde(default)]
    url: String,
    score: i64,
    comment_count: usize,
    #[serde(default)]
    description_plain: String,
    submitter_user: String,
    #[serde(default)]
    tags: Vec<String>,
    short_id_url: String,
    comments_url: String,
    #[serde(default)]
    comments: Vec<LobstersComment>,
}

#[derive(serde::Deserialize)]
struct LobstersComment {
    is_deleted: bool,
    is_moderated: bool,
    #[serde(default)]
    comment_plain: String,
    depth: usize,
    commenting_user: String,
    url: String,
}

impl Provider for HackerNewsProvider {
    fn label(&self) -> &'static str {
        "Hacker News"
    }

    fn title(&self) -> &'static str {
        "Hacker News top"
    }

    fn fetch_posts(&self, client: &reqwest::blocking::Client) -> Result<Vec<Post>> {
        fetch_hn_posts(client)
    }

    fn fetch_comments(
        &self,
        client: &reqwest::blocking::Client,
        post: &Post,
    ) -> Result<Vec<Comment>> {
        fetch_hn_comments(client, post)
    }
}

impl Provider for LobstersProvider {
    fn label(&self) -> &'static str {
        "Lobsters"
    }

    fn title(&self) -> &'static str {
        "Lobsters hottest"
    }

    fn fetch_posts(&self, client: &reqwest::blocking::Client) -> Result<Vec<Post>> {
        fetch_lobsters_posts(client)
    }

    fn fetch_comments(
        &self,
        client: &reqwest::blocking::Client,
        post: &Post,
    ) -> Result<Vec<Comment>> {
        fetch_lobsters_comments(client, post)
    }
}

impl App {
    fn new(client: reqwest::blocking::Client) -> Self {
        Self {
            client,
            source: Source::HackerNews,
            mode: Mode::Posts,
            posts: Vec::new(),
            post_cache: HashMap::new(),
            post_selected: 0,
            comments: Vec::new(),
            comment_selected: 0,
            comment_scroll: 0,
            comment_max_scroll: 0,
            collapsed_comments: HashSet::new(),
            comment_keep_selection_visible: false,
            status: "Loading Hacker News...".to_string(),
        }
    }

    fn current_provider(&self) -> &'static dyn Provider {
        provider(self.source)
    }

    fn refresh(&mut self) {
        let source = self.source;
        let label = self.current_provider().label();
        match fetch_posts(&self.client, source) {
            Ok(posts) => {
                let count = posts.len();
                self.set_posts(posts, format!("Loaded {count} {label} posts"));
            }
            Err(err) => self.status = format!("Error: {err}"),
        }
    }

    fn set_posts(&mut self, posts: Vec<Post>, status: String) {
        self.posts = posts;
        self.post_selected = 0;
        self.comments.clear();
        self.comment_selected = 0;
        self.comment_scroll = 0;
        self.comment_max_scroll = 0;
        self.collapsed_comments.clear();
        self.comment_keep_selection_visible = false;
        self.mode = Mode::Posts;
        self.status = status;
    }

    fn load_comments(&mut self) {
        let Some(post) = self.selected_post() else {
            self.status = "No post selected".to_string();
            return;
        };

        let result = provider(post.source).fetch_comments(&self.client, post);
        match result {
            Ok(comments) => {
                let count = comments.len();
                self.comments = comments;
                self.comment_selected = 0;
                self.comment_scroll = 0;
                self.comment_max_scroll = 0;
                self.collapsed_comments.clear();
                self.comment_keep_selection_visible = true;
                self.mode = Mode::Comments;
                self.status = format!("Loaded {count} comments");
            }
            Err(err) => self.status = format!("Error: {err}"),
        }
    }

    fn selected_post(&self) -> Option<&Post> {
        self.posts.get(self.post_selected)
    }

    fn selected_comment(&self) -> Option<&Comment> {
        self.comments.get(self.comment_selected)
    }

    fn move_down(&mut self) {
        match self.mode {
            Mode::Posts if !self.posts.is_empty() => {
                self.post_selected = (self.post_selected + 1).min(self.posts.len() - 1);
            }
            Mode::Comments if !self.comments.is_empty() => {
                self.comment_scroll = (self.comment_scroll + 1).min(self.comment_max_scroll);
                self.comment_keep_selection_visible = false;
            }
            _ => {}
        }
    }

    fn move_up(&mut self) {
        match self.mode {
            Mode::Posts => self.post_selected = self.post_selected.saturating_sub(1),
            Mode::Comments => {
                self.comment_scroll = self.comment_scroll.saturating_sub(1);
                self.comment_keep_selection_visible = false;
            }
        }
    }

    fn move_top(&mut self) {
        match self.mode {
            Mode::Posts if !self.posts.is_empty() => self.post_selected = 0,
            Mode::Comments if !self.comments.is_empty() => {
                self.comment_scroll = 0;
                self.comment_selected = 0;
                self.comment_keep_selection_visible = true;
            }
            _ => {}
        }
    }

    fn move_bottom(&mut self) {
        match self.mode {
            Mode::Posts if !self.posts.is_empty() => self.post_selected = self.posts.len() - 1,
            Mode::Comments if !self.comments.is_empty() => {
                self.comment_scroll = self.comment_max_scroll;
                self.comment_keep_selection_visible = false;
            }
            _ => {}
        }
    }

    fn select_next_comment(&mut self) {
        let visible = visible_comment_indices(&self.comments, &self.collapsed_comments);
        if visible.is_empty() {
            return;
        }

        let position = visible
            .iter()
            .position(|&index| index == self.comment_selected)
            .unwrap_or(0);
        self.comment_selected = visible[(position + 1).min(visible.len() - 1)];
        self.comment_keep_selection_visible = true;
    }

    fn select_previous_comment(&mut self) {
        let visible = visible_comment_indices(&self.comments, &self.collapsed_comments);
        if visible.is_empty() {
            return;
        }

        let position = visible
            .iter()
            .position(|&index| index == self.comment_selected)
            .unwrap_or(0);
        self.comment_selected = visible[position.saturating_sub(1)];
        self.comment_keep_selection_visible = true;
    }

    fn toggle_comment_collapse(&mut self) {
        if self.mode != Mode::Comments || self.comments.is_empty() {
            return;
        }

        let index = self.comment_selected.min(self.comments.len() - 1);
        let hidden = comment_descendant_count(&self.comments, index);
        if self.collapsed_comments.remove(&index) {
            if hidden == 0 {
                self.status = "Expanded comment".to_string();
            } else {
                self.status = format!("Expanded {hidden} replies");
            }
        } else {
            self.collapsed_comments.insert(index);
            if hidden == 0 {
                self.status = "Collapsed comment".to_string();
            } else {
                self.status = format!("Collapsed {hidden} replies");
            }
        }
        self.comment_keep_selection_visible = true;
    }

    fn switch_source(&mut self, source: Source) {
        if self.source == source {
            return;
        }

        let old_source = self.source;
        let old_posts = std::mem::take(&mut self.posts);
        if !old_posts.is_empty() {
            self.post_cache.insert(old_source, old_posts);
        }

        self.source = source;
        if let Some(posts) = self.post_cache.remove(&source) {
            let count = posts.len();
            let label = self.current_provider().label();
            self.set_posts(posts, format!("Loaded {count} {label} posts (cached)"));
        } else {
            self.set_posts(
                Vec::new(),
                format!("Loading {}...", self.current_provider().label()),
            );
            self.refresh();
        }
    }

    fn open_selected_link(&mut self) {
        let url = match self.mode {
            Mode::Posts => self
                .selected_post()
                .map(|post| post.url.as_ref().unwrap_or(&post.discussion_url).clone()),
            Mode::Comments => self.selected_comment().and_then(|comment| {
                comment
                    .url
                    .clone()
                    .or_else(|| extract_first_url(&clean_comment_text(&comment.text)))
                    .or_else(|| self.selected_post().map(|post| post.discussion_url.clone()))
            }),
        };

        self.open_url(url);
    }

    fn open_discussion(&mut self) {
        let url = self.selected_post().map(|post| post.discussion_url.clone());
        self.open_url(url);
    }

    fn back_to_posts(&mut self) {
        self.mode = Mode::Posts;
    }

    fn open_url(&mut self, url: Option<String>) {
        let Some(url) = url else {
            self.status = "No link selected".to_string();
            return;
        };

        match webbrowser::open(&url) {
            Ok(()) => self.status = format!("Opened {url}"),
            Err(err) => self.status = format!("Open failed: {err}"),
        }
    }
}

fn hn_item_url(id: u64) -> String {
    format!("https://hacker-news.firebaseio.com/v0/item/{id}.json")
}

fn hn_discussion_url(id: u64) -> String {
    format!("https://news.ycombinator.com/item?id={id}")
}

fn lobsters_story_api_url(short_id: &str) -> String {
    format!("https://lobste.rs/s/{short_id}.json")
}

fn provider(source: Source) -> &'static dyn Provider {
    match source {
        Source::HackerNews => &HACKER_NEWS_PROVIDER,
        Source::Lobsters => &LOBSTERS_PROVIDER,
    }
}

fn fetch_posts(client: &reqwest::blocking::Client, source: Source) -> Result<Vec<Post>> {
    provider(source).fetch_posts(client)
}

fn fetch_hn_posts(client: &reqwest::blocking::Client) -> Result<Vec<Post>> {
    let ids: Vec<u64> = client
        .get(HN_TOP_URL)
        .send()
        .with_context(|| format!("GET {HN_TOP_URL}"))?
        .error_for_status()
        .with_context(|| format!("HTTP status for {HN_TOP_URL}"))?
        .json()
        .with_context(|| format!("decode JSON from {HN_TOP_URL}"))?;

    let mut posts = Vec::new();
    for id in ids.into_iter().take(POST_LIMIT) {
        let Some(item) = fetch_hn_item(client, id)? else {
            continue;
        };
        if item.deleted.unwrap_or(false)
            || item.dead.unwrap_or(false)
            || item.kind.as_deref() != Some("story")
        {
            continue;
        }

        let text = item
            .text
            .as_deref()
            .map(html_to_text)
            .filter(|text| !text.is_empty());
        let url = item.url.filter(|url| !url.trim().is_empty());
        posts.push(Post {
            source: Source::HackerNews,
            id: item.id.to_string(),
            title: item.title.unwrap_or_else(|| "(untitled)".to_string()),
            author: item.by.unwrap_or_else(|| "unknown".to_string()),
            score: item.score.unwrap_or(0),
            comment_count: item.descendants.unwrap_or(0),
            url,
            discussion_url: hn_discussion_url(item.id),
            text,
            tags: Vec::new(),
        });
    }

    Ok(posts)
}

fn fetch_hn_item(client: &reqwest::blocking::Client, id: u64) -> Result<Option<HnItem>> {
    let url = hn_item_url(id);
    client
        .get(&url)
        .send()
        .with_context(|| format!("GET {url}"))?
        .error_for_status()
        .with_context(|| format!("HTTP status for {url}"))?
        .json()
        .with_context(|| format!("decode JSON from {url}"))
}

fn fetch_hn_comments(client: &reqwest::blocking::Client, post: &Post) -> Result<Vec<Comment>> {
    let id = post
        .id
        .parse::<u64>()
        .with_context(|| format!("parse Hacker News post id {}", post.id))?;
    let story = fetch_hn_item(client, id)?
        .with_context(|| format!("load Hacker News story item {}", post.id))?;
    let mut comments = Vec::new();
    if let Some(kids) = story.kids.as_deref() {
        collect_hn_comments_parallel(client, kids, 0, &mut comments)?;
    }
    Ok(comments)
}

fn collect_hn_comments_parallel(
    client: &reqwest::blocking::Client,
    ids: &[u64],
    depth: usize,
    out: &mut Vec<Comment>,
) -> Result<()> {
    if ids.len() <= 1 {
        return collect_hn_comments(client, ids, depth, out);
    }

    let worker_count = ids.len().min(HN_COMMENT_WORKERS);
    let chunk_size = ids.len().div_ceil(worker_count);
    let mut chunk_results = Vec::new();

    std::thread::scope(|scope| -> Result<()> {
        let handles = ids
            .chunks(chunk_size)
            .map(|chunk| {
                let client = client.clone();
                scope.spawn(move || {
                    let mut comments = Vec::new();
                    collect_hn_comments(&client, chunk, depth, &mut comments)?;
                    Ok::<Vec<Comment>, anyhow::Error>(comments)
                })
            })
            .collect::<Vec<_>>();

        for handle in handles {
            let comments = handle
                .join()
                .map_err(|_| anyhow::anyhow!("Hacker News comment worker panicked"))??;
            chunk_results.push(comments);
        }

        Ok(())
    })?;

    for mut comments in chunk_results {
        out.append(&mut comments);
    }

    Ok(())
}

fn collect_hn_comments(
    client: &reqwest::blocking::Client,
    ids: &[u64],
    depth: usize,
    out: &mut Vec<Comment>,
) -> Result<()> {
    for &id in ids {
        let Some(item) = fetch_hn_item(client, id)? else {
            continue;
        };
        let kids = item.kids.unwrap_or_default();
        if item.deleted.unwrap_or(false) || item.dead.unwrap_or(false) {
            out.push(Comment {
                author: "deleted".to_string(),
                depth,
                text: "[deleted]".to_string(),
                url: None,
            });
            collect_hn_comments(client, &kids, depth + 1, out)?;
            continue;
        }

        let text = item.text.as_deref().map(html_to_text).unwrap_or_default();
        let text = clean_comment_text(&text);
        let url = extract_first_url(&text);
        out.push(Comment {
            author: item.by.unwrap_or_else(|| "unknown".to_string()),
            depth,
            text,
            url,
        });
        collect_hn_comments(client, &kids, depth + 1, out)?;
    }

    Ok(())
}

fn fetch_lobsters_posts(client: &reqwest::blocking::Client) -> Result<Vec<Post>> {
    let stories: Vec<LobstersStory> = client
        .get(LOBSTERS_HOTTEST_URL)
        .send()
        .with_context(|| format!("GET {LOBSTERS_HOTTEST_URL}"))?
        .error_for_status()
        .with_context(|| format!("HTTP status for {LOBSTERS_HOTTEST_URL}"))?
        .json()
        .with_context(|| format!("decode JSON from {LOBSTERS_HOTTEST_URL}"))?;

    Ok(stories
        .into_iter()
        .take(POST_LIMIT)
        .map(|story| {
            let discussion_url = if story.comments_url.trim().is_empty() {
                story.short_id_url.clone()
            } else {
                story.comments_url.clone()
            };
            Post {
                source: Source::Lobsters,
                id: story.short_id,
                title: story.title,
                author: story.submitter_user,
                score: story.score,
                comment_count: story.comment_count,
                url: (!story.url.trim().is_empty()).then_some(story.url),
                discussion_url,
                text: (!story.description_plain.trim().is_empty())
                    .then_some(story.description_plain),
                tags: story.tags,
            }
        })
        .collect())
}

fn fetch_lobsters_comments(
    client: &reqwest::blocking::Client,
    post: &Post,
) -> Result<Vec<Comment>> {
    let url = lobsters_story_api_url(&post.id);
    let story: LobstersStory = client
        .get(&url)
        .send()
        .with_context(|| format!("GET {url}"))?
        .error_for_status()
        .with_context(|| format!("HTTP status for {url}"))?
        .json()
        .with_context(|| format!("decode JSON from {url}"))?;

    Ok(story
        .comments
        .into_iter()
        .map(|comment| {
            let text = if comment.is_deleted || comment.is_moderated {
                "[deleted]".to_string()
            } else {
                comment.comment_plain
            };
            let url = extract_first_url(&text)
                .or_else(|| (!comment.url.trim().is_empty()).then_some(comment.url));
            Comment {
                author: comment.commenting_user,
                depth: comment.depth,
                text,
                url,
            }
        })
        .collect())
}

fn html_to_text(html: &str) -> String {
    html2text::from_read(html.as_bytes(), 80)
        .map(|text| text.trim().to_string())
        .unwrap_or_else(|_| html.to_string())
}

fn extract_first_url(text: &str) -> Option<String> {
    text.split_whitespace().find_map(|token| {
        let token = token
            .trim_start_matches(|ch: char| "([{<\"'".contains(ch))
            .trim_end_matches(|ch: char| ".,);]}>\"'".contains(ch));
        (token.starts_with("http://") || token.starts_with("https://")).then(|| token.to_string())
    })
}

fn main() -> Result<()> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(15))
        .build()?;
    let mut app = App::new(client);
    app.refresh();
    ratatui::run(|mut terminal| run(&mut terminal, &mut app))?;
    Ok(())
}

fn run(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> io::Result<()> {
    loop {
        terminal.draw(|frame| render(frame, app))?;
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if handle_key(app, key) {
                    break Ok(());
                }
            }
            _ => {}
        }
    }
}

fn handle_key(app: &mut App, key: KeyEvent) -> bool {
    if key.kind != KeyEventKind::Press {
        return false;
    }

    if key.code == KeyCode::Char('q')
        || key.code == KeyCode::Char('c') && key.modifiers.contains(event::KeyModifiers::CONTROL)
        || key.code == KeyCode::Char('d') && key.modifiers.contains(event::KeyModifiers::CONTROL)
    {
        return true;
    }

    match app.mode {
        Mode::Posts => match key.code {
            KeyCode::Char('j') | KeyCode::Down => app.move_down(),
            KeyCode::Char('k') | KeyCode::Up => app.move_up(),
            KeyCode::Char('g') => app.move_top(),
            KeyCode::Char('G') => app.move_bottom(),
            KeyCode::Enter | KeyCode::Right => app.load_comments(),
            KeyCode::Char('o') => app.open_selected_link(),
            KeyCode::Char('c') => app.open_discussion(),
            KeyCode::Char('r') => app.refresh(),
            KeyCode::Tab => {
                let source = match app.source {
                    Source::HackerNews => Source::Lobsters,
                    Source::Lobsters => Source::HackerNews,
                };
                app.switch_source(source);
            }
            KeyCode::Char('1') => app.switch_source(Source::HackerNews),
            KeyCode::Char('2') => app.switch_source(Source::Lobsters),
            _ => {}
        },
        Mode::Comments => match key.code {
            KeyCode::Char('j') | KeyCode::Down => app.move_down(),
            KeyCode::Char('k') | KeyCode::Up => app.move_up(),
            KeyCode::Char('g') => app.move_top(),
            KeyCode::Char('G') => app.move_bottom(),
            KeyCode::Esc | KeyCode::Char('b') => app.back_to_posts(),
            KeyCode::Left | KeyCode::Char('h') => app.select_previous_comment(),
            KeyCode::Right | KeyCode::Char('l') => app.select_next_comment(),
            KeyCode::Char('o') => app.open_selected_link(),
            KeyCode::Char('c') => app.open_discussion(),
            KeyCode::Char('r') => app.load_comments(),
            KeyCode::Char(' ') | KeyCode::Enter => app.toggle_comment_collapse(),
            _ => {}
        },
    }

    false
}

fn render(frame: &mut Frame, app: &mut App) {
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
        Paragraph::new(format!("{} | {mode}", app.current_provider().title()))
            .style(Style::default().add_modifier(Modifier::BOLD)),
        title_area,
    );

    match app.mode {
        Mode::Posts => render_posts(frame, app, content_area),
        Mode::Comments => render_comments(frame, app, content_area),
    }

    let capability = if app.current_provider().capabilities() == Capabilities::READ_ONLY {
        "read-only"
    } else {
        "write-capable"
    };
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

fn post_row_bg(index: usize) -> Color {
    if index % 2 == 0 {
        Color::Rgb(18, 18, 24)
    } else {
        Color::Rgb(28, 28, 36)
    }
}

fn post_row_style(index: usize) -> Style {
    Style::default().fg(Color::Gray).bg(post_row_bg(index))
}

fn post_list_item(post: &Post, index: usize) -> ListItem<'static> {
    let style = post_row_style(index);
    let title_style = style.fg(Color::White).add_modifier(Modifier::BOLD);
    let label_style = style.fg(Color::DarkGray);
    let score_style = style.fg(Color::Green);
    let comments_style = style.fg(Color::Cyan);
    let author_style = style.fg(Color::Yellow);
    let link_style = style.fg(Color::LightBlue);
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

fn post_header_text(post: Option<&Post>) -> Text<'static> {
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
            Span::styled(link.to_string(), Style::default().fg(Color::LightBlue)),
        ]),
        Line::from(vec![
            Span::styled("discussion: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                post.discussion_url.clone(),
                Style::default().fg(Color::LightBlue),
            ),
        ]),
    ];

    if let Some(text) = &post.text {
        let text = clean_comment_text(text);
        if !text.is_empty() {
            lines.push(Line::from(""));
            lines.extend(text.lines().map(|line| {
                Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(Color::Gray),
                ))
            }));
        }
    }

    Text::from(lines)
}

fn render_posts(frame: &mut Frame, app: &App, area: Rect) {
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

fn comment_prefix_for(depth: usize, starts_branch: bool) -> (String, String) {
    if depth == 0 {
        return (String::new(), "  ".to_string());
    }

    let rail = "│  ".repeat(depth - 1);
    let joint = if starts_branch { "┌─ " } else { "├─ " };
    (format!("{rail}{joint}"), format!("{rail}│  "))
}

#[cfg(test)]
fn comment_prefix(depth: usize) -> (String, String) {
    comment_prefix_for(depth, false)
}

fn selected_comment_prefix(depth: usize) -> String {
    if depth == 0 {
        "▶ ".to_string()
    } else {
        format!("{}▶─ ", "│  ".repeat(depth - 1))
    }
}

fn comment_descendant_count(comments: &[Comment], index: usize) -> usize {
    let Some(comment) = comments.get(index) else {
        return 0;
    };
    comments[index + 1..]
        .iter()
        .take_while(|child| child.depth > comment.depth)
        .count()
}

fn visible_comment_indices(comments: &[Comment], collapsed: &HashSet<usize>) -> Vec<usize> {
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

fn markdown_reference(line: &str) -> Option<(&str, &str)> {
    let line = line.trim();
    let rest = line.strip_prefix('[')?;
    let (id, rest) = rest.split_once("]:")?;
    Some((id.trim(), rest.trim()))
}

fn rewrite_reference_links(line: &str, references: &HashMap<String, String>) -> String {
    let mut output = String::new();
    let mut rest = line;

    while let Some(start) = rest.find('[') {
        output.push_str(&rest[..start]);
        let Some((label, after_label)) = rest[start + 1..].split_once("][") else {
            output.push_str(&rest[start..]);
            return output;
        };
        let Some(end) = after_label.find(']') else {
            output.push_str(&rest[start..]);
            return output;
        };

        let id = &after_label[..end];
        if let Some(url) = references.get(id) {
            if label.starts_with("http://") || label.starts_with("https://") {
                output.push_str(label);
            } else {
                output.push_str(label);
                output.push_str(" (");
                output.push_str(url);
                output.push(')');
            }
            rest = &after_label[end + 1..];
        } else {
            output.push('[');
            rest = &rest[start + 1..];
        }
    }

    output.push_str(rest);
    output
}

fn clean_comment_text(text: &str) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let references = normalized
        .lines()
        .filter_map(markdown_reference)
        .map(|(id, url)| (id.to_string(), url.to_string()))
        .collect::<HashMap<_, _>>();
    let mut lines = normalized
        .lines()
        .filter(|line| !line.trim_start().starts_with("```"))
        .filter(|line| markdown_reference(line).is_none())
        .map(|line| rewrite_reference_links(&line.replace('`', ""), &references))
        .collect::<Vec<_>>();

    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }

    lines.join("\n")
}

fn comment_text_lines(
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
        if index > 0 && last_visible_depth.is_some_and(|depth| comment.depth <= depth) {
            lines.push(String::new());
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

fn push_wrapped_lines(
    lines: &mut Vec<String>,
    owners: &mut Vec<Option<usize>>,
    owner: Option<usize>,
    prefix: &str,
    text: &str,
    width: usize,
) {
    let available = width.saturating_sub(line_width(prefix)).max(1);

    for raw in text.lines() {
        let mut rest = raw.trim_end();
        if rest.is_empty() {
            lines.push(prefix.to_string());
            owners.push(owner);
            continue;
        }

        while line_width(rest) > available {
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

fn split_at_width(text: &str, width: usize) -> usize {
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

fn line_width(text: &str) -> usize {
    text.chars().count()
}

fn author_color(author: &str) -> Color {
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

fn comment_line_style(
    _comments: &[Comment],
    owner: Option<usize>,
    selected: Option<usize>,
    is_comment_header: bool,
) -> Style {
    match owner {
        Some(index) if selected == Some(index) && is_comment_header => Style::default()
            .fg(Color::Rgb(255, 255, 0))
            .add_modifier(Modifier::BOLD),
        Some(_) => Style::default().fg(Color::Gray),
        None => Style::default().fg(Color::DarkGray),
    }
}

fn split_at_char_count(text: &str, count: usize) -> (&str, &str) {
    if count == 0 {
        return ("", text);
    }

    match text.char_indices().nth(count) {
        Some((index, _)) => text.split_at(index),
        None => (text, ""),
    }
}

fn deselected_comment_header_line(line: &str, comment: &Comment) -> Line<'static> {
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

fn deselected_comment_body_line(line: &str, depth: usize) -> Line<'static> {
    let (prefix, body) = split_at_char_count(line, if depth == 0 { 2 } else { depth * 3 });
    if prefix.is_empty() {
        return Line::styled(body.to_string(), Style::default().fg(Color::Gray));
    }

    Line::from(vec![
        Span::styled(prefix.to_string(), Style::default().fg(Color::DarkGray)),
        Span::styled(body.to_string(), Style::default().fg(Color::Gray)),
    ])
}

fn selected_comment_line(line: &str, depth: usize, is_comment_header: bool) -> Line<'static> {
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
            spans.push(Span::styled(rest.to_string(), text_style));
            return Line::from(spans);
        }
    }

    Line::styled(line.to_string(), text_style)
}

fn comment_lines_text(
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
                    Line::styled(
                        line.clone(),
                        comment_line_style(comments, *owner, selected, is_comment_header),
                    )
                }
            })
            .collect::<Vec<_>>(),
    )
}

fn owner_line_range(owners: &[Option<usize>], owner: usize) -> Option<(usize, usize)> {
    let first = owners
        .iter()
        .position(|&line_owner| line_owner == Some(owner))?;
    let last = owners
        .iter()
        .rposition(|&line_owner| line_owner == Some(owner))?;
    Some((first, last))
}

fn scroll_to_show_comment(
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

fn render_comments(frame: &mut Frame, app: &mut App, area: Rect) {
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

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn post_row_backgrounds_alternate() {
        assert_eq!(post_row_bg(0), post_row_bg(2));
        assert_ne!(post_row_bg(0), post_row_bg(1));
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
    fn clean_comment_text_rewrites_reference_links() {
        let text = " [https://taoofmac.com/space/blog/2026/06/18/1845][1]\n\n  [https://github.com/rcarmo/pve-microvm][2]\n\n  [1]: https://taoofmac.com/space/blog/2026/06/18/1845\n  [2]: https://github.com/rcarmo/pve-microvm";

        let cleaned = clean_comment_text(text);

        assert!(cleaned.contains("https://taoofmac.com/space/blog/2026/06/18/1845"));
        assert!(cleaned.contains("https://github.com/rcarmo/pve-microvm"));
        assert!(!cleaned.contains("[1]:"));
        assert!(!cleaned.contains("[2]:"));
        assert!(!cleaned.contains("][1]"));
        assert_eq!(
            extract_first_url(&cleaned),
            Some("https://taoofmac.com/space/blog/2026/06/18/1845".to_string())
        );
    }

    #[test]
    fn comment_author_lines_are_dull_and_selected_is_bright_yellow() {
        let comments = vec![
            test_comment("alice", 0, "one"),
            test_comment("bob", 0, "two"),
            test_comment("alice", 0, "three"),
        ];

        let alice = deselected_comment_header_line("alice", &comments[0]);
        let second_alice = deselected_comment_header_line("alice", &comments[2]);
        assert_eq!(alice.spans[0].style.fg, Some(author_color("alice")));
        assert_eq!(second_alice.spans[0].style.fg, Some(author_color("alice")));
        assert_eq!(
            comment_line_style(&comments, Some(1), None, false).fg,
            Some(Color::Gray)
        );
        let selected = comment_line_style(&comments, Some(1), Some(1), true);
        assert_eq!(selected.fg, Some(Color::Rgb(255, 255, 0)));
        assert_eq!(selected.bg, None);
        assert!(selected.add_modifier.contains(Modifier::BOLD));
        assert_eq!(
            comment_line_style(&comments, Some(1), None, true).fg,
            Some(Color::Gray)
        );
    }

    #[test]
    fn post_header_text_includes_colored_metadata_lines() {
        let mut post = test_post(Source::HackerNews, "1");
        post.text = Some("summary".to_string());

        let text = post_header_text(Some(&post));

        assert!(text.lines.len() >= 5);
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
    fn switching_to_cached_source_skips_refetch() {
        let mut app = App::new(reqwest::blocking::Client::new());
        app.posts = vec![test_post(Source::HackerNews, "hn")];
        app.post_cache
            .insert(Source::Lobsters, vec![test_post(Source::Lobsters, "lob")]);

        app.switch_source(Source::Lobsters);

        assert_eq!(app.source, Source::Lobsters);
        assert_eq!(app.posts.len(), 1);
        assert_eq!(app.posts[0].id, "lob");
        assert!(app.post_cache.contains_key(&Source::HackerNews));
        assert!(!app.post_cache.contains_key(&Source::Lobsters));
        assert_eq!(app.mode, Mode::Posts);
        assert!(app.status.contains("(cached)"));
    }

    #[test]
    fn comment_prefix_makes_nested_comments_visible() {
        assert_eq!(comment_prefix(0), ("".to_string(), "  ".to_string()));
        assert_eq!(comment_prefix(1), ("├─ ".to_string(), "│  ".to_string()));
        assert_eq!(
            comment_prefix(2),
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
    fn selected_comment_colors_only_nearest_rail() {
        let line = selected_comment_line("│  │  body", 2, false);

        assert_eq!(line.spans.len(), 3);
        assert_eq!(line.spans[0].content.as_ref(), "│  ");
        assert_eq!(line.spans[0].style.fg, Some(Color::DarkGray));
        assert_eq!(line.spans[1].content.as_ref(), "│  ");
        assert_eq!(line.spans[1].style.fg, Some(Color::Rgb(255, 255, 0)));
        assert_eq!(line.spans[2].content.as_ref(), "body");
        assert_eq!(line.spans[2].style.fg, Some(Color::Gray));
    }

    #[test]
    fn deselected_comment_colors_author_not_rails() {
        let comment = test_comment("alice", 1, "hello");

        let line = deselected_comment_header_line("┌─ alice", &comment);

        assert_eq!(line.spans.len(), 2);
        assert_eq!(line.spans[0].content.as_ref(), "┌─ ");
        assert_eq!(line.spans[0].style.fg, Some(Color::DarkGray));
        assert_eq!(line.spans[1].content.as_ref(), "alice");
        assert_eq!(line.spans[1].style.fg, Some(author_color("alice")));
    }

    #[test]
    fn deselected_comment_body_colors_rails_not_text() {
        let line = deselected_comment_body_line("│  │  body", 2);

        assert_eq!(line.spans.len(), 2);
        assert_eq!(line.spans[0].content.as_ref(), "│  │  ");
        assert_eq!(line.spans[0].style.fg, Some(Color::DarkGray));
        assert_eq!(line.spans[1].content.as_ref(), "body");
        assert_eq!(line.spans[1].style.fg, Some(Color::Gray));
    }

    #[test]
    fn child_comment_starts_without_detached_separator() {
        let comments = vec![
            test_comment("parent", 0, "parent text"),
            test_comment("child", 1, "child text"),
        ];

        let (lines, owners) = comment_text_lines(&comments, &HashSet::new(), None, 80);

        assert_eq!(lines[1], "  parent text");
        assert_eq!(owners[1], Some(0));
        assert_eq!(lines[2], "┌─ child");
        assert_eq!(owners[2], Some(1));
    }

    #[test]
    fn comment_text_wraps_long_comments_with_rails() {
        let comments = vec![Comment {
            author: "alice".to_string(),
            depth: 1,
            text: "one two three four five".to_string(),
            url: None,
        }];

        let (lines, owners) = comment_text_lines(&comments, &HashSet::new(), None, 8);

        assert_eq!(
            owners,
            vec![Some(0), Some(0), Some(0), Some(0), Some(0), Some(0)]
        );
        assert_eq!(
            lines,
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

        let (lines, _) = comment_text_lines(&comments, &HashSet::new(), None, 80);

        assert!(lines.iter().any(|line| line.contains("let x = value;")));
        assert!(!lines.iter().any(|line| line.contains("```")));
        assert!(!lines.iter().any(|line| line.contains('`')));
    }

    #[test]
    fn selected_comment_is_marked() {
        let comments = vec![test_comment("alice", 0, "hello")];

        let (lines, _) = comment_text_lines(&comments, &HashSet::new(), Some(0), 80);

        assert!(lines[0].starts_with("▶ alice"));
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

        let (lines, owners) = comment_text_lines(&comments, &collapsed, None, 80);

        assert!(
            lines
                .iter()
                .any(|line| line.contains("2 replies collapsed"))
        );
        assert!(!lines.iter().any(|line| line.contains("root text")));
        assert!(lines.iter().any(|line| line.contains("sibling")));
        assert!(!lines.iter().any(|line| line.contains("child")));
        assert!(owners.contains(&Some(0)));
        assert!(owners.contains(&Some(3)));
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

        let (lines, _) = comment_text_lines(&app.comments, &app.collapsed_comments, None, 80);
        assert!(
            lines
                .iter()
                .any(|line| line.contains("sibling [collapsed]"))
        );
        assert!(!lines.iter().any(|line| line.contains("sibling text")));
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
        let owners = vec![
            Some(0),
            Some(0),
            None,
            Some(1),
            Some(1),
            Some(1),
            None,
            Some(2),
        ];

        assert_eq!(owner_line_range(&owners, 1), Some((3, 5)));
        assert_eq!(scroll_to_show_comment(&owners, 1, 0, 4, 4), 2);
        assert_eq!(scroll_to_show_comment(&owners, 1, 4, 4, 4), 3);
        assert_eq!(scroll_to_show_comment(&owners, 0, 3, 4, 4), 0);
        assert_eq!(scroll_to_show_comment(&owners, 1, 0, 2, 6), 3);
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

    #[test]
    fn providers_are_read_only_for_now() {
        assert_eq!(
            provider(Source::HackerNews).capabilities(),
            Capabilities::READ_ONLY
        );
        assert_eq!(
            provider(Source::Lobsters).capabilities(),
            Capabilities::READ_ONLY
        );
        assert_eq!(provider(Source::HackerNews).label(), "Hacker News");
        assert_eq!(provider(Source::Lobsters).label(), "Lobsters");
    }
}
