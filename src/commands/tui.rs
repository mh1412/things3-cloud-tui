mod calendar;
mod editor;
mod model;
mod mutations;
mod screen;
mod theme;
mod worker;

use std::io::{IsTerminal, Write};

use anyhow::{Result, ensure};
use clap::Args;
use iocraft::prelude::*;

use crate::{app::Cli, cmd_ctx::CmdCtx, commands::Command};
use model::List;

#[derive(Debug, Default, Args)]
pub struct TuiArgs {
    /// Icon style (nerd requires a Nerd Font in your terminal)
    #[arg(long, value_enum, default_value_t = theme::Icons::Nerd)]
    pub icons: theme::Icons,
    /// List to open initially
    #[arg(long, value_enum, default_value_t = List::Today)]
    pub view: List,
}

impl Command for TuiArgs {
    fn run_with_ctx(&self, cli: &Cli, _out: &mut dyn Write, _ctx: &mut dyn CmdCtx) -> Result<()> {
        ensure!(!cli.json, "tui does not support --json");
        ensure!(
            std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
            "tui requires an interactive terminal (stdin and stdout)"
        );
        ensure!(
            cli.load_journal.as_deref() != Some(std::path::Path::new("-")),
            "tui requires a journal file path, not stdin"
        );
        let local = cli.no_cloud || cli.load_journal.is_some();
        let (requests, events) = worker::start(cli)?;
        let mut app = element! {
            screen::App(
                initial_view: self.view,
                requests: Some(requests),
                events: Some(events),
                local,
                cached: cli.no_sync,
                no_color: cli.no_color,
                icons: self.icons,
            )
        };
        smol::block_on(app.fullscreen())?;
        Ok(())
    }
}
