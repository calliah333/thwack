use anyhow::{Context, Result};

use crate::model::{Comment, Post, Source};
use crate::text::{clean_comment_text, extract_first_url, html_to_text};

const POST_LIMIT: usize = 30;
const HN_TOP_URL: &str = "https://hacker-news.firebaseio.com/v0/topstories.json";
const LOBSTERS_HOTTEST_URL: &str = "https://lobste.rs/hottest.json";

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

pub(crate) fn fetch_posts(client: &reqwest::blocking::Client, source: Source) -> Result<Vec<Post>> {
    match source {
        Source::HackerNews => fetch_hn_posts(client),
        Source::Lobsters => fetch_lobsters_posts(client),
    }
}

pub(crate) fn fetch_comments(
    client: &reqwest::blocking::Client,
    post: &Post,
) -> Result<Vec<Comment>> {
    match post.source {
        Source::HackerNews => fetch_hn_comments(client, post),
        Source::Lobsters => fetch_lobsters_comments(client, post),
    }
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
            discussion_url: format!("https://news.ycombinator.com/item?id={}", item.id),
            text,
            tags: Vec::new(),
        });
    }

    Ok(posts)
}

fn fetch_hn_item(client: &reqwest::blocking::Client, id: u64) -> Result<Option<HnItem>> {
    let url = format!("https://hacker-news.firebaseio.com/v0/item/{id}.json");
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
    match fetch_hn_comments_html(client, post) {
        Ok(comments) if !comments.is_empty() || post.comment_count == 0 => Ok(comments),
        _ => fetch_hn_comments_firebase(client, post),
    }
}

fn fetch_hn_comments_html(client: &reqwest::blocking::Client, post: &Post) -> Result<Vec<Comment>> {
    let url = format!("https://news.ycombinator.com/item?id={}", post.id);
    let html = client
        .get(&url)
        .send()
        .with_context(|| format!("GET {url}"))?
        .error_for_status()
        .with_context(|| format!("HTTP status for {url}"))?
        .text()
        .with_context(|| format!("read HTML from {url}"))?;

    Ok(parse_hn_comments_html(&html))
}

fn fetch_hn_comments_firebase(
    client: &reqwest::blocking::Client,
    post: &Post,
) -> Result<Vec<Comment>> {
    let id = post
        .id
        .parse::<u64>()
        .with_context(|| format!("parse Hacker News post id {}", post.id))?;
    let story = fetch_hn_item(client, id)?
        .with_context(|| format!("load Hacker News story item {}", post.id))?;
    let mut comments = Vec::new();
    if let Some(kids) = story.kids.as_deref() {
        collect_hn_comments(client, kids, 0, &mut comments)?;
    }
    Ok(comments)
}

pub(crate) fn parse_hn_comments_html(html: &str) -> Vec<Comment> {
    let mut comments = Vec::new();
    let mut rest = html;

    while let Some(start) = find_hn_comment_start(rest) {
        rest = &rest[start..];
        let next = find_hn_comment_start(&rest[1..])
            .map(|offset| offset + 1)
            .unwrap_or(rest.len());
        let chunk = &rest[..next];

        if let Some(comment) = parse_hn_comment_chunk(chunk) {
            comments.push(comment);
        }

        rest = &rest[next..];
    }

    comments
}

fn find_hn_comment_start(html: &str) -> Option<usize> {
    html.find("class=\"athing comtr\"")
        .or_else(|| html.find("class='athing comtr'"))
        .and_then(|class_pos| html[..class_pos].rfind("<tr").or(Some(class_pos)))
}

fn parse_hn_comment_chunk(chunk: &str) -> Option<Comment> {
    let depth = parse_usize_attr(chunk, "indent").unwrap_or(0);
    let author = find_hn_user(chunk).unwrap_or_else(|| "unknown".to_string());
    let html = expand_hn_comment_links(find_hn_commtext_html(chunk)?);
    let text = clean_comment_text(&html_to_text(&html));

    if text.is_empty() {
        return None;
    }

    let url = extract_first_url(&text);
    Some(Comment {
        author,
        depth,
        text,
        url,
    })
}

fn find_attr_value<'a>(html: &'a str, attr: &str) -> Option<&'a str> {
    let start = html.find(attr)? + attr.len();
    let value = html[start..].strip_prefix('=')?;
    let quote = value.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let value = &value[quote.len_utf8()..];
    Some(&value[..value.find(quote)?])
}

fn find_hn_user(chunk: &str) -> Option<String> {
    let class_pos = chunk
        .find("class=\"hnuser\"")
        .or_else(|| chunk.find("class='hnuser'"))?;
    let anchor_start = chunk[..class_pos].rfind("<a").unwrap_or(class_pos);
    let text = &chunk[anchor_start..];
    let text = &text[text.find('>')? + 1..];
    let author = html_to_text(&text[..text.find("</a>")?]);

    (!author.is_empty()).then_some(author)
}

fn find_hn_commtext_html(chunk: &str) -> Option<&str> {
    let start = chunk
        .find("<div class=\"commtext")
        .or_else(|| chunk.find("<div class='commtext"))?;
    let html = &chunk[start..];
    let html = &html[html.find('>')? + 1..];

    Some(&html[..html.find("</div>")?])
}

fn expand_hn_comment_links(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;

    while let Some(start) = rest.find("<a") {
        out.push_str(&rest[..start]);
        let anchor = &rest[start..];
        let Some(tag_end) = anchor.find('>') else {
            out.push_str(anchor);
            return out;
        };
        let tag = &anchor[..=tag_end];
        let after_tag = &anchor[tag_end + 1..];
        let Some(close) = after_tag.find("</a>") else {
            out.push_str(anchor);
            return out;
        };

        if let Some(href) = find_attr_value(tag, "href") {
            let decoded = html_to_text(href);
            if decoded.starts_with("http://") || decoded.starts_with("https://") {
                out.push_str(href);
                rest = &after_tag[close + "</a>".len()..];
                continue;
            }
        }

        out.push_str(tag);
        out.push_str(&after_tag[..close]);
        out.push_str("</a>");
        rest = &after_tag[close + "</a>".len()..];
    }

    out.push_str(rest);
    out
}

fn parse_usize_attr(html: &str, attr: &str) -> Option<usize> {
    find_attr_value(html, attr)?.parse().ok()
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
    let url = format!("https://lobste.rs/s/{}.json", post.id);
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
