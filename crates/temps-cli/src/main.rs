//! Temps CLI - Single entrypoint for all services
//!
//! This binary delegates to the `temps_cli` library. The Cli/Commands enums,
//! tracing init, and command dispatcher live in `lib.rs` so external binaries
//! (e.g. `temps-ee`) can reuse them without forking.

use clap::{CommandFactory, FromArgMatches};
use temps_cli::{dispatch_non_serve, init_tracing, Cli};

fn main() -> anyhow::Result<()> {
    // Inject the build-time version into clap's --version output. The Cli
    // struct in the library deliberately omits a literal version so that
    // each binary (temps, temps-ee) can stamp its own.
    let cmd = Cli::command().version(env!("TEMPS_VERSION"));
    let matches = cmd.get_matches();
    let cli = Cli::from_arg_matches(&matches)?;

    init_tracing(&cli.log_level, &cli.log_format);

    if let Some(serve_cmd) = dispatch_non_serve(cli.command)? {
        serve_cmd.execute()
    } else {
        Ok(())
    }
}
