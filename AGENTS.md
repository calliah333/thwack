# Thwack Agent Guide

## What this repo is

`thwack` is a small read-only terminal news reader. It shows Hacker News top stories and Lobsters hottest stories, lets the user read comment threads, collapse replies, and open links in the system browser.

Keep it small. Prefer deleting code over adding knobs. Do not add async runtimes, background workers, caches, config files, logging stacks, or extra crates unless the current behavior needs them.

## Crate shape

- Single binary crate: `src/main.rs`.
- No `lib.rs`; tests are in `src/tests.rs` behind `#[cfg(test)]`.
- All internal APIs are `pub(crate)` because there is no public library surface.
- Rust edition: 2024.
- Dependencies in use:
  - `anyhow` for fallible top-level/network/parser plumbing.
  - `reqwest` blocking client with JSON support.
  - `serde` derive for API response structs.
  - `ratatui` for rendering.
  - `crossterm` for keyboard events.

## Source map

- `src/main.rs`
  - Declares modules.
  - Builds one blocking `reqwest::Client` with `USER_AGENT = "thwack/0.1"` and a 15 second timeout.
  - Creates `App`, calls `app.refresh()`, then enters `ratatui::run`.

- `src/model.rs`
  - Core data only: `Source`, `Mode`, `Post`, `Comment`.
  - `Source` is `HackerNews | Lobsters`.
  - `Mode` is `Posts | Comments`.
  - `source_label` and `source_title` are the UI labels. Add a new source here first, then follow the compiler.

- `src/app.rs`
  - Owns app state and state transitions.
  - Fetch entry points are `refresh()` for posts and `load_comments()` for comments.
  - Selection and scrolling are state-only methods: `move_*`, `select_*`, `toggle_comment_collapse`.
  - `switch_source()` clears posts/comments, resets selection, then refreshes.
  - URL opening is intentionally simple: macOS `open`, Windows `cmd /C start`, Unix `xdg-open`, otherwise an error status.
  - Keep network and terminal rendering out of tests where possible; state methods are easy to test directly.

- `src/fetch.rs`
  - All network/API work lives here.
  - `fetch_posts()` and `fetch_comments()` dispatch on `Source`.
  - Hacker News posts come from Firebase top stories, limited by `POST_LIMIT = 30`.
  - Hacker News comments try the HTML page first so full link URLs survive; if that produces no comments for a post with comments, fallback is Firebase item recursion.
  - The HN HTML parser is deliberately a small string scanner, not a DOM dependency. Keep parser tests tight if changing it.
  - Lobsters posts and comments come from JSON endpoints.
  - Network errors should keep `anyhow::Context` with the URL and action.

- `src/text.rs`
  - Text cleanup helpers.
  - `html_to_text()` handles the small HTML/entity subset this app needs and collapses whitespace.
  - `extract_first_url()` trims common surrounding/trailing punctuation.
  - `clean_comment_text()` normalizes newlines and strips Markdown code-fence/backtick noise for terminal display.

- `src/ui.rs`
  - Pure-ish ratatui rendering and line construction.
  - `render()` draws title, content, and status/help.
  - `render_posts()` owns the post list.
  - `render_comments()` owns the selected post header, comment line wrapping, scroll bounds, and selection visibility.
  - Comment tree layout is built by `comment_text_lines()` plus helpers for rails, separators, wrapping, collapsed replies, URL spans, and selected-line styling.
  - Keep layout helpers deterministic; most UI behavior should be testable without a real terminal.

- `src/input.rs`
  - Event loop and key bindings.
  - Quits on `q`, Ctrl-C, or Ctrl-D.
  - Global movement: `j/k` and arrow up/down; `g/G` top/bottom; `o` open selected link; `c` open discussion; `r` refresh/reload.
  - Posts mode: Enter loads comments; Tab toggles source; `1` selects Hacker News; `2` selects Lobsters.
  - Comments mode: Esc/`b` returns to posts; left/`h` and right/`l` move selected visible comment; Space/Enter collapses/expands selected comment.

- `src/tests.rs`
  - Unit tests for text cleanup, URL span styling, HN HTML parsing, app state transitions, comment tree formatting/collapse, scroll-to-selection, keyboard handling, and empty-comment rendering.
  - Tests should stay offline. Use direct helpers and synthetic `Post`/`Comment` values.

## Behavior invariants

- The app is read-only toward news sites. Opening a browser is the only side effect outside the terminal.
- Empty post/comment lists must not panic.
- Source switches reset posts, comments, selection, scroll, collapsed comments, and mode.
- Comment scrolling is line-based; left/right changes selected visible comment.
- Collapsing a comment hides all descendants and reports the number hidden.
- Selected comments should scroll into view only when selection changes, not on every line scroll.
- URLs displayed in comments should be underlined and should not include trailing punctuation.
- HN comment links with shortened anchor text should preserve the full `href` URL.

## Development rules for this repo

- Keep the blocking architecture unless a real requirement forces async. A TUI that fetches 30 posts synchronously is simpler and matches the current crate.
- Do not introduce traits or service abstractions for `fetch`, `App`, or rendering without multiple real implementations.
- Do not add config for constants that have one obvious value (`POST_LIMIT`, timeout, user agent) unless users can actually change them.
- Prefer small pure helpers over wider stateful changes. Existing tests are built around that.
- Avoid `unwrap()` in production code. Tests may use `expect()` when a failure is a broken test setup.
- Preserve `anyhow::Context` on external I/O and network calls.
- Add a test when changing parsing, key handling, selection/scroll behavior, comment tree formatting, or URL extraction.
- Run `cargo fmt` after Rust edits and `cargo test` for behavioral changes.
