use std::io;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};

use crate::app::App;
use crate::model::{Mode, Source};
use crate::ui::render;

pub(crate) fn run(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> io::Result<()> {
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

pub(crate) fn handle_key(app: &mut App, key: KeyEvent) -> bool {
    if key.kind != KeyEventKind::Press {
        return false;
    }

    if key.code == KeyCode::Char('q')
        || key.code == KeyCode::Char('c') && key.modifiers.contains(event::KeyModifiers::CONTROL)
        || key.code == KeyCode::Char('d') && key.modifiers.contains(event::KeyModifiers::CONTROL)
    {
        return true;
    }

    match key.code {
        KeyCode::Char('j') | KeyCode::Down => app.move_down(),
        KeyCode::Char('k') | KeyCode::Up => app.move_up(),
        KeyCode::Char('g') => app.move_top(),
        KeyCode::Char('G') => app.move_bottom(),
        KeyCode::Char('o') => app.open_selected_link(),
        KeyCode::Char('c') => app.open_discussion(),
        KeyCode::Char('r') => match app.mode {
            Mode::Posts => app.refresh(),
            Mode::Comments => app.load_comments(),
        },
        _ => match app.mode {
            Mode::Posts => match key.code {
                KeyCode::Enter => app.load_comments(),
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
                KeyCode::Esc | KeyCode::Char('b') => app.back_to_posts(),
                KeyCode::Left | KeyCode::Char('h') => app.select_previous_comment(),
                KeyCode::Right | KeyCode::Char('l') => app.select_next_comment(),
                KeyCode::Char(' ') | KeyCode::Enter => app.toggle_comment_collapse(),
                _ => {}
            },
        },
    }

    false
}
