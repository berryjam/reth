//! Low level example of connecting to and communicating with a peer.
//!
//! Run with
//!
//! ```not_rust
//! cargo run -p manual-p2p
//! ```

use std::thread;
use std::time::Duration;

use futures::{Sink, SinkExt, StreamExt};
use reth_chainspec::{Chain, DEV, DML, LOCAL, MAINNET};
use reth_discv4::{DiscoveryUpdate, Discv4, Discv4ConfigBuilder, DEFAULT_DISCOVERY_ADDRESS, DML_ADDRESS};
use reth_ecies::stream::ECIESStream;
use reth_eth_wire::{EthMessage, EthStream, GetBlockHeaders, HeadersDirection, HelloMessage, P2PStream, Status, UnauthedEthStream, UnauthedP2PStream};
use reth_network::config::rng_secret_key;
use reth_network_peers::{mainnet_nodes, pk2id, NodeRecord, dml_node, local_node};
use reth_primitives::{EthereumHardfork, Head, MAINNET_GENESIS_HASH};
use secp256k1::{SecretKey, SECP256K1};
use std::sync::LazyLock;
use tokio::net::TcpStream;
use reth_eth_wire::message::RequestPair;
use reth_primitives::{b256, constants::DML_GENESIS_HASH, BlockHashOrNumber, Header, U256};
use uuid::Uuid;

type AuthedP2PStream = P2PStream<ECIESStream<TcpStream>>;
type AuthedEthStream = EthStream<P2PStream<ECIESStream<TcpStream>>>;

pub static MAINNET_BOOT_NODES: LazyLock<Vec<NodeRecord>> = LazyLock::new(mainnet_nodes);

#[tokio::main]
async fn main() -> eyre::Result<()> {
    let our_key = rng_secret_key();
    let peer = dml_node();
    let (p2p_stream, their_hello) = match handshake_p2p(peer, our_key).await {
        Ok(s) => {
            s
        },
        Err(e) => {
            println!("Failed P2P handshake with peer {}, {}", peer.address, e);
            return Ok(());
        }
    };
    println!("their_hello: {:?}", their_hello);

    let (eth_stream, their_status) = match handshake_eth_dml(p2p_stream).await {
        Ok(s) => {
            s
        },
        Err(e) => {
            println!("Failed ETH handshake with peer {}, {}", peer.address, e);
            return  Ok(());
        }
    };
    println!("their_status: {:?}", their_status);

    println!(
        "Successfully connected to a peer at {}:{} ({}) using eth-wire version eth/{}",
        peer.address, peer.tcp_port, their_hello.client_version, their_status.version
    );

    snoop(peer, eth_stream).await;

    // // Setup configs related to this 'node' by creating a new random
    // let our_key = rng_secret_key();
    // let our_enr = NodeRecord::from_secret_key(DEFAULT_DISCOVERY_ADDRESS, &our_key);
    //
    // // Setup discovery v4 protocol to find peers to talk to
    // let mut discv4_cfg = Discv4ConfigBuilder::default();
    // discv4_cfg.add_boot_nodes(MAINNET_BOOT_NODES.clone()).lookup_interval(Duration::from_secs(1));
    //
    // // Start discovery protocol
    // let discv4 = Discv4::spawn(our_enr.udp_addr(), our_enr, our_key, discv4_cfg.build()).await?;
    // let mut discv4_stream = discv4.update_stream().await?;
    //
    // while let Some(update) = discv4_stream.next().await {
    //     tokio::spawn(async move {
    //         if let DiscoveryUpdate::Added(peer) = update {
    //             // Boot nodes hard at work, lets not disturb them
    //             if MAINNET_BOOT_NODES.contains(&peer) {
    //                 return
    //             }
    //
    //             let (p2p_stream, their_hello) = match handshake_p2p(peer, our_key).await {
    //                 Ok(s) => s,
    //                 Err(e) => {
    //                     println!("Failed P2P handshake with peer {}, {}", peer.address, e);
    //                     return
    //                 }
    //             };
    //
    //             let (eth_stream, their_status) = match handshake_eth(p2p_stream).await {
    //                 Ok(s) => s,
    //                 Err(e) => {
    //                     println!("Failed ETH handshake with peer {}, {}", peer.address, e);
    //                     return
    //                 }
    //             };
    //
    //             println!(
    //                 "Successfully connected to a peer at {}:{} ({}) using eth-wire version eth/{}",
    //                 peer.address, peer.tcp_port, their_hello.client_version, their_status.version
    //             );
    //
    //             snoop(peer, eth_stream).await;
    //         }
    //     });
    // }

    Ok(())
}

// Perform a P2P handshake with a peer
async fn handshake_p2p(
    peer: NodeRecord,
    key: SecretKey,
) -> eyre::Result<(AuthedP2PStream, HelloMessage)> {
    let outgoing = TcpStream::connect((peer.address, peer.tcp_port)).await?;
    let ecies_stream = ECIESStream::connect(outgoing, key, peer.id).await?;

    let our_peer_id = pk2id(&key.public_key(SECP256K1));
    println!("our_peer_id: {:?}", our_peer_id);
    let our_hello = HelloMessage::builder(our_peer_id).build();

    Ok(UnauthedP2PStream::new(ecies_stream).handshake(our_hello).await?)
}

// Perform a ETH Wire handshake with a peer
async fn handshake_eth(p2p_stream: AuthedP2PStream) -> eyre::Result<(AuthedEthStream, Status)> {
    let fork_filter = MAINNET.fork_filter(Head {
        timestamp: MAINNET.fork(EthereumHardfork::Shanghai).as_timestamp().unwrap(),
        ..Default::default()
    });

    let status = Status::builder()
        .chain(Chain::mainnet())
        .genesis(MAINNET_GENESIS_HASH)
        .forkid(MAINNET.hardfork_fork_id(EthereumHardfork::Shanghai).unwrap())
        .build();

    let status = Status { version: p2p_stream.shared_capabilities().eth()?.version(), ..status };
    let eth_unauthed = UnauthedEthStream::new(p2p_stream);
    Ok(eth_unauthed.handshake(status, fork_filter).await?)
}

async fn handshake_eth_dml(p2p_stream: AuthedP2PStream) -> eyre::Result<(AuthedEthStream, Status)> {
    let fork_filter = DML.fork_filter(Head {
        timestamp: DML.fork(EthereumHardfork::Shanghai).as_timestamp().unwrap(),
        ..Default::default()
    });

    let status = Status::builder()
        .chain(999888222.into())
        .genesis(DML_GENESIS_HASH)
        .total_difficulty(U256::from(65536))
        .forkid(DML.hardfork_fork_id(EthereumHardfork::Shanghai).unwrap())
        .blockhash(DML_GENESIS_HASH)
        .build();

    let status = Status { version: p2p_stream.shared_capabilities().eth()?.version(), ..status };
    let eth_unauthed = UnauthedEthStream::new(p2p_stream);
    Ok(eth_unauthed.handshake(status, fork_filter).await?)
}

async fn handshake_eth_local(p2p_stream: AuthedP2PStream) -> eyre::Result<(AuthedEthStream, Status)> {
    let fork_filter = LOCAL.fork_filter(Head {
        timestamp: LOCAL.fork(EthereumHardfork::Shanghai).as_timestamp().unwrap(),
        ..Default::default()
    });

    let status = Status::builder()
        .chain(999888223.into())
        .genesis(DML_GENESIS_HASH)
        .total_difficulty(U256::from(65536))
        .forkid(LOCAL.hardfork_fork_id(EthereumHardfork::Shanghai).unwrap())
        .blockhash(DML_GENESIS_HASH)
        .build();

    let status = Status { version: p2p_stream.shared_capabilities().eth()?.version(), ..status };
    let eth_unauthed = UnauthedEthStream::new(p2p_stream);
    Ok(eth_unauthed.handshake(status, fork_filter).await?)
}

// Snoop by greedily capturing all broadcasts that the peer emits
// note: this node cannot handle request so will be disconnected by peer when challenged
async fn snoop(peer: NodeRecord, mut eth_stream: AuthedEthStream) {
    loop {
        match eth_stream.next().await {
            Some(Ok(update)) => match update {
                EthMessage::NewPooledTransactionHashes66(txs) => {
                    println!("Got {} new tx hashes from peer {}", txs.0.len(), peer.address);
                }
                EthMessage::NewBlock(block) => {
                    println!("Got new block hash {:?} data {:?} from peer {}", block.block.header.hash_slow(), block, peer.address);
                }
                EthMessage::Transactions(t) => {
                    println!("Got {} new txs from peer {}", t.0.len(), peer.address);
                }
                EthMessage::NewPooledTransactionHashes68(txs) => {
                    println!("Got {} new tx hashes from peer {}", txs.hashes.len(), peer.address);
                }
                EthMessage::NewBlockHashes(block_hashes) => {
                    println!(
                        "Got {} new block hashes from peer {}",
                        block_hashes.0.len(),
                        peer.address
                    );
                }
                EthMessage::GetNodeData(_) => {
                    println!("Unable to serve GetNodeData request to peer {}", peer.address);
                }
                EthMessage::GetReceipts(_) => {
                    println!("Unable to serve GetReceipts request to peer {}", peer.address);
                }
                EthMessage::GetBlockHeaders(headers) => {
                    println!("GetBlockHeaders headers: {:?}", headers);
                    // let mut send = false;
                    // match headers.message.start_block {
                    //     BlockHashOrNumber::Hash(hash) => {
                    //         println!("GetBlockHeaders by hash: {:?}", hash);
                    //         if hash == DML_GENESIS_HASH {
                    //             send = true;
                    //         }
                    //     }
                    //     BlockHashOrNumber::Number(number) => {
                    //         println!("GetBlockHeaders by number: {:?}", number);
                    //         if number == 0 {
                    //             send = true;
                    //         }
                    //     }
                    // }
                    // if send {
                    //     eth_stream.send(EthMessage::BlockHeaders(RequestPair{
                    //         request_id: headers.request_id,
                    //         message: vec![Header {
                    //             parent_hash: b256!("0000000000000000000000000000000000000000000000000000000000000000"),
                    //             state_root: b256!("904a08faad3ed1f0974f327cec04ec974e8869ded849e48cf9118a39b812659b"),
                    //             transactions_root: b256!("56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421"),
                    //             receipts_root: b256!("56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421"),
                    //             difficulty: U256::from(0x10000),
                    //             number: 0,
                    //             gas_limit: 3141592,
                    //             gas_used: 0,
                    //             timestamp: 0,
                    //             mix_hash: b256!("0000000000000000000000000000000000000000000000000000000000000000"),
                    //             nonce: 0x10000000042,
                    //             ..Default::default()
                    //         }].into(),
                    //     })).await.unwrap();
                    // } else {
                    //     eth_stream.send(EthMessage::BlockHeaders(RequestPair{
                    //         request_id: headers.request_id,
                    //         message: vec![].into(),
                    //     })).await.unwrap();
                    // }
                }
                EthMessage::GetBlockBodies(_) => {
                    println!("Unable to serve GetBlockBodies request to peer {}", peer.address);
                }
                EthMessage::GetPooledTransactions(_) => {
                    println!("Unable to serve GetPooledTransactions request to peer {}", peer.address);
                }
                EthMessage::Status(_) => {
                    println!("Unable to serve Status request to peer {}", peer.address);
                }
                EthMessage::BlockHeaders(_) => {
                    println!("Unable to serve BlockHeaders request to peer {}", peer.address);
                }
                EthMessage::BlockBodies(_) => {
                    println!("Unable to serve BlockBodies request to peer {}", peer.address);
                }
                EthMessage::PooledTransactions(_) => {
                    println!("Unable to serve PooledTransactions request to peer {}", peer.address);
                }
                EthMessage::NodeData(_) => {
                    println!("Unable to serve NodeData request to peer {}", peer.address);
                }
                EthMessage::Receipts(_) => {
                    println!("Unable to serve Receipts request to peer {}", peer.address);
                }
            },
            Some(Err(e)) => {
                println!("Error reading from peer {}: {}", peer.address, e);
                break;
            }
            None => {
                println!("Peer {} disconnected", peer.address);
                // let my_uuid = Uuid::new_v4();
                //
                // // 获取 UUID 前 8 字节，并将其转换为 u64
                // let first_8_bytes = &my_uuid.as_bytes()[..8]; // 取前8个字节
                // let u64_value = u64::from_be_bytes(first_8_bytes.try_into().expect("Slice with incorrect length"));
                //
                // eth_stream.send(EthMessage::GetBlockHeaders(RequestPair{
                //     request_id: u64_value,
                //     message: GetBlockHeaders {
                //         start_block:  BlockHashOrNumber::Number(0),
                //         limit: 1,
                //         skip: 0,
                //         direction: HeadersDirection::Rising,
                //     },
                // })).await.unwrap();
                let our_key = rng_secret_key();
                let peer = dml_node();
                let (p2p_stream, their_hello) = match handshake_p2p(peer, our_key).await {
                    Ok(s) => {
                        s
                    },
                    Err(e) => {
                        println!("Failed P2P handshake with peer {}, {}", peer.address, e);
                        continue;
                    }
                };
                println!("their_hello: {:?}", their_hello);

                let (new_eth_stream, their_status) = match handshake_eth_dml(p2p_stream).await {
                    Ok(s) => {
                        s
                    },
                    Err(e) => {
                        println!("Failed ETH handshake with peer {}, {}", peer.address, e);
                        continue;
                    }
                };
                println!("their_status: {:?}", their_status);

                println!(
                    "Successfully connected to a peer at {}:{} ({}) using eth-wire version eth/{}",
                    peer.address, peer.tcp_port, their_hello.client_version, their_status.version
                );
                eth_stream = new_eth_stream;
            }
        }
    }
}
