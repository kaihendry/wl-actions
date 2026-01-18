use {error_reporter::Report, std::io, thiserror::Error, wl_proxy::simple::SimpleProxyError};

mod actions;
mod cli;

#[derive(Debug, Error)]
enum ActionsError {
    #[error("could not create a simple server")]
    CreateServer(#[source] SimpleProxyError),
    #[error("could not spawn child")]
    SpawnChild(#[source] io::Error),
    #[error("the server terminated")]
    ServerFailed(#[source] SimpleProxyError),
}

fn main() -> Result<(), Report<ActionsError>> {
    cli::main().map_err(Report::new)
}
