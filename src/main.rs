mod app;
mod fetch;
mod input;
mod model;
mod text;
mod ui;

#[cfg(test)]
mod tests;

use std::time::Duration;

use anyhow::Result;

use app::App;
use input::run;

const USER_AGENT: &str = "thwack/0.1";

fn main() -> Result<()> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(15))
        .build()?;
    let mut app = App::new(client);
    app.refresh();
    ratatui::run(|terminal| run(terminal, &mut app))?;
    Ok(())
}
