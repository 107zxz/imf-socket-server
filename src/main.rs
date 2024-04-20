use std::{env, fs};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::hash::Hash;
use std::net::SocketAddr;
use std::rc::Rc;
use std::sync::{Arc};

use futures_util::{future, SinkExt, StreamExt, TryStreamExt};
use futures_util::lock::Mutex;
use futures_util::stream::TryChunks;
use log::{error, info};
use regex::Regex;
use serde::{Deserialize, Deserializer};
use tokio::net::{TcpListener, TcpStream, tcp::WriteHalf};
use tokio::net::tcp::ReadHalf;
use tokio_tungstenite::tungstenite::{Message};
use tokio_tungstenite::WebSocketStream;


#[derive(Deserialize)]
struct ServerConfig {
    default_port: u16,
    username_regex: String,
}

struct ServerContext {
    config: ServerConfig,
    streams: HashMap<u32, Arc<Mutex<TcpStream>>>
}

impl ServerContext {
    fn new() -> Self {
        Self {
            config: serde_yaml::from_str(
                &fs::read_to_string("config.yml").unwrap()
            ).unwrap(),
            streams: HashMap::new()
        }
    }
}


#[tokio::main]
async fn main() {
    let _ = env_logger::try_init();

    let ctx = ServerContext::new();

    // Create the event loop and TCP listener we'll accept connections on.
    let port = match env::var("PORT") {
        Ok(port_str) => port_str.parse().unwrap(),
        Err(_) => ctx.config.default_port
    };

    let addr = SocketAddr::from((
        [0, 0, 0, 0], port
    ));

    let try_socket = TcpListener::bind(addr).await;

    let listener = try_socket.expect("Failed to bind");
    info!("Listening on: {}", addr.to_string());

    // Async multithreading my beloved
    let ctx_ref = Arc::new(Mutex::new(ctx));

    // Track here: List of sending sockets. Each sending socket can be snapped up by mutex.
    // Each socket now has a unique ID that will be used to identify its sending stream
    // let sending_sockets: Arc<Mutex<HashMap<u32, TcpStream>>> = Arc::new(Mutex::new(HashMap::new()));
    //
    let mut next_key = 0;

    // Accept connections: This is what the main thread must do...
    while let Ok((stream, _)) = listener.accept().await {

        // Save the TcpStream for future use by all threads
        {
            let mut ctx = ctx_ref.lock().await;
            (*ctx).streams.insert(next_key, Arc::new(Mutex::new(stream)));
        }

        tokio::spawn(accept_connection(next_key, ctx_ref.clone()));
    }
}

// Each socket should be stored with their own mutexes and only used when comm is necessary.
// Unsure how this would interact with polling / reading
async fn accept_connection(socket_key: u32, context: Arc<Mutex<ServerContext>>) {
    let mut ws_stream = tokio_tungstenite::accept_async(stream)
        .await?;

    info!("New WebSocket connection: {}", addr);

    let mut connection_state = ConnectionState::new();

    while let Some(mr) = ws_stream.next().await {}
    }
}

/*
struct ConnectionState {
    player_name: Option<String>
}

impl ConnectionState {
    fn new() -> Self {
        Self {
            player_name: None
        }
    }
}

async fn remove_player(ctx_arc: Arc<Mutex<ServerContext>>, player_name: &Option<String>) {
    match player_name {
        Some(name) => {
            let mut ctx = ctx_arc.lock().await;
            ctx.connected_players.remove(name);
        },
        None => {}
    }
}

async fn accept_connection(stream: TcpStream, ctx_arc: Arc<Mutex<ServerContext>>) -> Result<(), tokio_tungstenite::tungstenite::error::Error> {
    // Config that gotta go here Mr. Thread
    let username_regex = Regex::new(r"^[\w ,\-']{1,20}$").unwrap();

    let addr = stream.peer_addr()?;
    info!("Peer address: {}", addr);

    let mut ws_stream = tokio_tungstenite::accept_async(stream)
        .await?;

    info!("New WebSocket connection: {}", addr);

    let mut connection_state = ConnectionState::new();

    while let Some(mr) = ws_stream.next().await {
        match mr {
            Ok(msg) => match msg {
                Message::Text(message) => {

                    let mut parts = message.split_whitespace();

                    match parts.next() {
                        Some("R:") => match parts.next() {
                            None => {
                                ws_stream.send(Message::Text("E: No username specified!".into())).await?;
                            },
                            Some(username) => {
                                if connection_state.player_name.is_some() {
                                    ws_stream.send(Message::Text("E: You're already registered!".into())).await?;
                                    continue;
                                }

                                if !username_regex.is_match(username) {
                                    ws_stream.send(Message::Text("E: Username invalid!".into())).await?;
                                    continue;
                                }

                                let mut ctx = ctx_arc.lock().await;

                                if ctx.connected_players.contains(username.into()) {
                                    ws_stream.send(Message::Text("E: Username taken!".into())).await?;
                                    continue;
                                }

                                ctx.connected_players.insert(username.into());
                                connection_state.player_name = Some(username.into());

                                ws_stream.send(Message::Text(format!("R: Registered as {}!", username))).await?;

                                info!("Player {} registered!", username);
                            }
                        },
                        Some("S:") => {
                            match connection_state.player_name {
                                Some(_) => {
                                    let ctx = ctx_arc.lock().await;
                                    ws_stream.send(Message::Text(format!("{:?}", ctx.connected_players))).await?;
                                }
                                None => ws_stream.send(Message::Text("E: Register first!".into())).await?
                            }
                        },
                        // Some("J:") => match connection_state.player_name {
                        //     Some(_) => match parts.next() {
                        //         Some(connect_to) => {
                        //             if connection_state.player_name.clone().unwrap() == connect_to {
                        //                 ws_stream.send(Message::Text("E: Cannot connect to yourself!".into())).await?;
                        //                 continue;
                        //             }
                        //             let ctx = ctx_arc.lock().await;
                        //             if !ctx.connected_players.contains(connect_to) {
                        //                 ws_stream.send(Message::Text("E: No player with that name!".into())).await?;
                        //                 continue;
                        //             }
                        //             ws_stream.send(Message::Text("J: Join requested!".into())).await?;
                        //         },
                        //         None => ws_stream.send(Message::Text("E: Register first!".into())).await?
                        //     },
                        //     None => ws_stream.send(Message::Text("E: Register first!".into())).await?
                        // },
                        Some("O:") => match connection_state.player_name.clone() {
                            Some(username) => match message.split_once(' ') {
                                Some((_, offer)) => {
                                    info!("Offer from {}: {}", username, offer);
                                },
                                None => {}
                            },
                            None => ws_stream.send(Message::Text("E: Register first!".into())).await?
                        },
                        Some(_) => ws_stream.send(Message::Text("E: Invalid command".into())).await?,
                        None => ws_stream.send(Message::Text("E: Command must include whitespace".into())).await?
                    }
                },
                Message::Close(_reason) => {
                    remove_player(ctx_arc.clone(), &connection_state.player_name).await;
                },
                _ => {}
            },
            Err(e) => {
                error!("Socket thread error! {}", e);

                remove_player(ctx_arc.clone(), &connection_state.player_name).await;

                break;
            }
        }
    }
    Ok(())
}
 */