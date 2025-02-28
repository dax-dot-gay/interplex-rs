use std::{
    error::Error,
    fs::File,
    io::{Read, Write},
    path::PathBuf,
};

use chrono::TimeDelta;
use clap::Parser;
use config::Config;
use interplex_common::rendezvous;
use libp2p::{
    autonat, futures::StreamExt as _, identify, identity::ed25519::Keypair, multiaddr::Protocol, noise, ping, relay, swarm::NetworkBehaviour, tcp, tls, yamux, Multiaddr, SwarmBuilder
};

mod config;
mod error;

#[derive(NetworkBehaviour)]
struct RdvBehaviour {
    identify: identify::Behaviour,
    rendezvous: rendezvous::server::Behavior,
    ping: ping::Behaviour,
    relay: relay::Behaviour,
    autonat: autonat::Behaviour,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = Config::parse();

    let keypair = if let Ok(mut f) = File::open(
        config
            .keypair
            .clone()
            .unwrap_or(PathBuf::from("identity.key")),
    ) {
        let mut content: Vec<u8> = Vec::new();
        f.read_to_end(&mut content)
            .expect("Unable to read bytes from specified file");
        Keypair::try_from_bytes(&mut content.as_mut_slice())
            .expect("File did not contain a valid keypair")
    } else {
        let generated = Keypair::generate();
        let mut file = File::create(config.keypair.unwrap_or(PathBuf::from("identity.key")))
            .expect("Unable to create keyfile.");
        file.write_all(&generated.to_bytes())
            .expect("Unable to write to keyfile.");
        file.flush().unwrap();
        generated
    };

    let mut swarm = SwarmBuilder::with_existing_identity(keypair.into())
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            (tls::Config::new, noise::Config::new),
            yamux::Config::default,
        )?
        .with_dns()?
        .with_behaviour(|key| {
            Ok(RdvBehaviour {
                identify: identify::Behaviour::new(identify::Config::new(
                    String::from("/interplex"),
                    key.public(),
                )),
                rendezvous: rendezvous::server::Behavior::new(
                    rendezvous::server::ConfigBuilder::default()
                        .database(
                            config
                                .database
                                .to_str()
                                .expect("Expected a valid database path."),
                        )
                        .max_lifetime(TimeDelta::hours(config.ttl.into()))
                        .build()?,
                ),
                ping: ping::Behaviour::default(),
                relay: relay::Behaviour::new(key.public().to_peer_id(), Default::default()),
                autonat: autonat::Behaviour::new(
                    key.public().to_peer_id(),
                    autonat::Config::default(),
                ),
            })
        })?
        .build();
    
    if config.expose.len() > 0 {
        for (addr, port) in config.expose {
            let mut address = Multiaddr::from(addr);
            address.push(Protocol::Tcp(port));
            swarm.listen_on(address)?;
        }
    } else {
        swarm.listen_on("/ip4/0.0.0.0/tcp/8080".parse()?)?;
    }

    loop {
        match swarm.select_next_some().await {
            x => println!("{x:?}")
        }
    }
}
