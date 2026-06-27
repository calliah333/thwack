use std::{collections::HashSet, io, process::Command};

use crate::fetch::{fetch_comments, fetch_posts};
use crate::model::{Comment, Mode, Post, Source, source_label};
use crate::text::{clean_comment_text, extract_first_url};
use crate::ui::{comment_descendant_count, visible_comment_indices};

pub(crate) struct App {
    pub(crate) client: reqwest::blocking::Client,
    pub(crate) source: Source,
    pub(crate) mode: Mode,
    pub(crate) posts: Vec<Post>,
    pub(crate) post_selected: usize,
    pub(crate) comments: Vec<Comment>,
    pub(crate) comment_selected: usize,
    pub(crate) comment_scroll: usize,
    pub(crate) comment_max_scroll: usize,
    pub(crate) collapsed_comments: HashSet<usize>,
    pub(crate) comment_keep_selection_visible: bool,
    pub(crate) status: String,
}

impl App {
    pub(crate) fn new(client: reqwest::blocking::Client) -> Self {
        Self {
            client,
            source: Source::HackerNews,
            mode: Mode::Posts,
            posts: Vec::new(),
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

    pub(crate) fn refresh(&mut self) {
        let source = self.source;
        let label = source_label(source);
        match fetch_posts(&self.client, source) {
            Ok(posts) => {
                let count = posts.len();
                self.set_posts(posts, format!("Loaded {count} {label} posts"));
            }
            Err(err) => self.status = format!("Error: {err}"),
        }
    }

    pub(crate) fn set_posts(&mut self, posts: Vec<Post>, status: String) {
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

    pub(crate) fn load_comments(&mut self) {
        let Some(post) = self.selected_post() else {
            self.status = "No post selected".to_string();
            return;
        };

        let result = fetch_comments(&self.client, post);
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

    pub(crate) fn selected_post(&self) -> Option<&Post> {
        self.posts.get(self.post_selected)
    }

    pub(crate) fn selected_comment(&self) -> Option<&Comment> {
        self.comments.get(self.comment_selected)
    }

    pub(crate) fn move_down(&mut self) {
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

    pub(crate) fn move_up(&mut self) {
        match self.mode {
            Mode::Posts => self.post_selected = self.post_selected.saturating_sub(1),
            Mode::Comments => {
                self.comment_scroll = self.comment_scroll.saturating_sub(1);
                self.comment_keep_selection_visible = false;
            }
        }
    }

    pub(crate) fn move_top(&mut self) {
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

    pub(crate) fn move_bottom(&mut self) {
        match self.mode {
            Mode::Posts if !self.posts.is_empty() => self.post_selected = self.posts.len() - 1,
            Mode::Comments if !self.comments.is_empty() => {
                self.comment_scroll = self.comment_max_scroll;
                self.comment_keep_selection_visible = false;
            }
            _ => {}
        }
    }

    pub(crate) fn select_next_comment(&mut self) {
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

    pub(crate) fn select_previous_comment(&mut self) {
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

    pub(crate) fn toggle_comment_collapse(&mut self) {
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

    pub(crate) fn switch_source(&mut self, source: Source) {
        if self.source == source {
            return;
        }

        self.source = source;
        self.set_posts(Vec::new(), format!("Loading {}...", source_label(source)));
        self.refresh();
    }

    pub(crate) fn open_selected_link(&mut self) {
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

    pub(crate) fn open_url(&mut self, url: Option<String>) {
        let Some(url) = url else {
            self.status = "No link selected".to_string();
            return;
        };

        match open_in_browser(&url) {
            Ok(()) => self.status = format!("Opened {url}"),
            Err(err) => self.status = format!("Open failed: {err}"),
        }
    }
}

fn open_in_browser(url: &str) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg(url);
        command
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", url]);
        command
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    };
    #[cfg(not(any(unix, windows)))]
    return Err(io::Error::other(
        "opening URLs is unsupported on this platform",
    ));

    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("opener exited with {status}")))
    }
}
