use {
    crate::{ActionsError, actions},
    clap::{CommandFactory, Parser, ValueHint},
    clap_complete::Shell,
    std::io::stdout,
};

/// Count input actions (key presses, mouse clicks, scroll events, touch taps)
/// for a wrapped Wayland application.
#[derive(Parser, Debug)]
#[command(name = "wl-actions")]
pub struct WlActions {
    /// Generate shell completions instead of running the program.
    #[clap(long, value_enum, value_name = "SHELL")]
    generate_completion: Option<Shell>,

    /// Suppress live output, only show summary on exit.
    #[clap(short, long)]
    quiet: bool,

    /// The program to run (and its arguments).
    #[clap(
        trailing_var_arg = true,
        value_hint = ValueHint::CommandWithArguments,
        required_unless_present = "generate_completion",
    )]
    program: Option<Vec<String>>,
}

pub fn main() -> Result<(), ActionsError> {
    let args = WlActions::parse();
    if let Some(shell) = args.generate_completion {
        let stdout = stdout();
        let mut stdout = stdout.lock();
        clap_complete::generate(shell, &mut WlActions::command(), "wl-actions", &mut stdout);
        return Ok(());
    }
    actions::main(args.quiet, args.program.unwrap())
}
