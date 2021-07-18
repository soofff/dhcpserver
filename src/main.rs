mod config;
mod error;
mod server;
mod sources;

use crate::server::Server;
use crate::config::{DhcpConfig, DhcpConfigOptions};
use simplelog::{CombinedLogger, TermLogger, Config, TerminalMode, ColorChoice};
use structopt::StructOpt;
use crate::error::{DhcpResult, DhcpError};

#[tokio::main]
async fn main() -> DhcpResult<()>{
    let options:DhcpConfigOptions = DhcpConfigOptions::from_args();

    CombinedLogger::init(
        vec![
            TermLogger::new(options.verbosity(),
                            Config::default(),
                            TerminalMode::Mixed,
                            ColorChoice::Auto),
        ]
    )?;

    let config_path = options.config().ok_or(DhcpError::ConfigFileNotFound)?;

    log::info!("using config file {}", config_path);

    let config = DhcpConfig::from_file(config_path)?;
    Server::listen(config).await
}