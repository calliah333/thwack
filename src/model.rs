#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum Source {
    HackerNews,
    Lobsters,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Mode {
    Posts,
    Comments,
}

#[derive(Clone, Debug)]
pub(crate) struct Post {
    pub(crate) source: Source,
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) author: String,
    pub(crate) score: i64,
    pub(crate) comment_count: usize,
    pub(crate) url: Option<String>,
    pub(crate) discussion_url: String,
    pub(crate) text: Option<String>,
    pub(crate) tags: Vec<String>,
}

#[derive(Debug)]
pub(crate) struct Comment {
    pub(crate) author: String,
    pub(crate) depth: usize,
    pub(crate) text: String,
    pub(crate) url: Option<String>,
}

pub(crate) fn source_label(source: Source) -> &'static str {
    match source {
        Source::HackerNews => "Hacker News",
        Source::Lobsters => "Lobsters",
    }
}

pub(crate) fn source_title(source: Source) -> &'static str {
    match source {
        Source::HackerNews => "Hacker News top",
        Source::Lobsters => "Lobsters hottest",
    }
}
