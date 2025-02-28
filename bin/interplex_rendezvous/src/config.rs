use std::{net::IpAddr, path::PathBuf};

use clap::Parser;

use crate::error::ServerError;

fn validate_expose(arg: &str) -> Result<(IpAddr, u16), ServerError> {
    if let Some((start, end)) = arg.split_once(':') {
        let address: IpAddr = start
            .parse()
            .or(Err(ServerError::InvalidExpose(arg.to_string())))?;
        let port: u16 = end
            .parse()
            .or(Err(ServerError::InvalidExpose(arg.to_string())))?;
        Ok((address, port))
    } else {
        Err(ServerError::InvalidExpose(arg.to_string()))
    }
}

#[derive(Parser, Clone, Debug)]
#[command(version, about = "Hosts an Interplex rendezvous/relay server", long_about = None)]
pub(crate) struct Config {
    #[arg(
        long,
        short,
        help = "Path to the database folder to store registrations in"
    )]
    pub database: PathBuf,

    #[arg(
        long,
        short,
        help = "Path to store the existing identity in. Will be created if non-existent."
    )]
    pub keypair: Option<PathBuf>,

    #[arg(long, short, help = "host:port to serve on. May provide multiple", value_parser = validate_expose)]
    pub expose: Vec<(IpAddr, u16)>,

    #[arg(
        long,
        short,
        help = "Number of hours to wait before expiring a non-refreshed registration",
        default_value_t = 12
    )]
    pub ttl: u16,
}
