use std::{error::Error, str::FromStr};

use interplex_common::identification::{Discoverability, NodeBuilder};
use libp2p::{
    Multiaddr, PeerId, SwarmBuilder,
    futures::StreamExt,
    identify, noise,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux,
};

#[derive(NetworkBehaviour)]
struct Behavior {
    identify: identify::Behaviour,
    rendezvous: interplex_common::rendezvous::client::Behaviour,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let rendezvous_address: Multiaddr = "/ip4/127.0.0.1/tcp/8080".parse().unwrap();
    let rendezvous_id: PeerId =
        PeerId::from_str("12D3KooWEA9r7SyAbHQjYtED3q9Xnt5CPBkQDu4oHgTNCdzi6T9t").unwrap();
    let mut swarm = SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_dns()?
        .with_behaviour(|key| {
            Ok(Behavior {
                identify: identify::Behaviour::new(identify::Config::new(
                    String::from("/interplex"),
                    key.public(),
                )),
                rendezvous: interplex_common::rendezvous::client::Behaviour::new(
                    NodeBuilder::new("interplex-test")
                        .peer_id(key.public().to_peer_id())
                        .alias("Test Node")
                        .discoverability(Discoverability::Namespace)
                        .group("subgroup")
                        .build()?,
                ),
            })
        })?
        .build();

    swarm.add_external_address("/ip4/0.0.0.0/tcp/0".parse()?);
    swarm.dial(rendezvous_address)?;

    loop {
        match swarm.select_next_some().await {
            SwarmEvent::ConnectionEstablished { peer_id, .. } if peer_id == rendezvous_id => {
                swarm.behaviour_mut().rendezvous.register(&peer_id).unwrap();
                swarm.behaviour_mut().rendezvous.discover(&peer_id, None);
            }
            e => {
                println!("{e:?}");
            }
        }
    }
}
