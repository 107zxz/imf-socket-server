use std::{env, fs};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use futures_util::lock::Mutex;
use futures_util::stream::{SplitSink, SplitStream};
use log::{error, info};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::{Message};
use tokio_tungstenite::WebSocketStream;

macro_rules! send_msg {
    ($ctx: expr, $target_idx: expr, $msg: expr) => {
        $ctx.writers.get_mut($target_idx).unwrap()
            .send(Message::Text(serde_json::to_string($msg).unwrap())).await.unwrap()
    };
}

#[derive(Deserialize)]
struct ServerConfig {
    default_port: u16,
    username_regex: String,
}

struct ServerContext {
    config: ServerConfig,
    writers: HashMap<u16, SplitSink<WebSocketStream<TcpStream>, Message>>,
    lobby_names: HashMap<String, u16>
}

impl ServerContext {
    fn new() -> Self {
        Self {
            config: serde_yaml::from_str(
                &fs::read_to_string("config.yml").unwrap()
            ).unwrap(),
            writers: HashMap::new(),
            lobby_names: HashMap::new()
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

    let mut next_socket_idx = 0;
    let ctx_ref = Arc::new(Mutex::new(ctx));

    // Accept connections: This is what the main thread must do...
    while let Ok((stream, _)) = listener.accept().await {

        // Split the websocket into readers and writers.
        let ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        let (writer, reader) = ws.split();

        ctx_ref.lock().await.writers.insert(next_socket_idx, writer);

        tokio::spawn(accept_connection(
            reader,
            Connection::new(
            next_socket_idx,
            // reader,
            ctx_ref.clone()
        )));

        next_socket_idx += 1;
    }
}

struct Connection {
    socket_idx: u16,
    lobby_name: Option<String>,
    // reader: SplitStream<WebSocketStream<TcpStream>>,
    ctx: Arc<Mutex<ServerContext>>
}
impl Connection {

    fn new(
        socket_idx: u16,
        // reader: SplitStream<WebSocketStream<TcpStream>>,
        ctx: Arc<Mutex<ServerContext>>
    ) -> Self {

        info!("Connection created with idx: {socket_idx}");

        Self {
            lobby_name: None,
            socket_idx,
            // reader,
            ctx
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag="type", rename_all="snake_case")]
struct IceCandidate {
    mid: String,
    index: u16,
    sdp: String
}

#[derive(Deserialize)]
#[serde(tag="type", rename_all="snake_case")]
enum ClientMessage {
    DebugPrint { data: Value },

    RegisterLobby { lobby_name: String },

    GetLobbies,

    SendOffer { to: u16, offer: String },
    SendResponse {to: u16, response: String},
    SendIce { to: u16, ice: IceCandidate }
}

#[derive(Serialize)]
#[serde(tag="type", rename_all="snake_case")]
enum ServerMessage {
    ErrorParse {msg: String},

    TakeANumber { id: u16 },

    AckRegisterLobby,
    ErrorInvalidLobbyName,
    ErrorLobbyExists,

    SendLobbyData { lobbies: HashMap<String, u16> },

    ReceiveOffer { from: u16, offer: String },
    ReceiveResponse {from: u16, response: String},
    ReceiveIce { from: u16, ice: IceCandidate },

    AckSendOffer,
    AckSendResponse,
    AckSendIce
}


// Each socket should be stored with their own mutexes and only used when comm is necessary.
// Unsure how this would interact with polling / reading
async fn accept_connection(mut reader: SplitStream<WebSocketStream<TcpStream>>, mut connection: Connection) {
    // Don't poll too fast

    // TODO: Maybe implement a timeout to kill the thread. Ref this:
    // https://github.com/snapview/tokio-tungstenite/blob/master/examples/interval-server.rs
    // let mut interval = tokio::time::interval(Duration::from_millis(1000));

    let lobby_regex = Regex::new(&connection.ctx.lock().await.config.username_regex).unwrap();

    // Give every new user their number on connection. It's only polite
    {
        let mut ctx = connection.ctx.lock().await;
        send_msg!(ctx, &connection.socket_idx, &ServerMessage::TakeANumber { id: connection.socket_idx });
    }

    loop {
        match reader.next().await {
            Some(msg_res) => {
                let mut ctx = connection.ctx.lock().await;

                match msg_res {
                    Ok(msg) => match msg {
                        Message::Text(message_text) => {

                            match serde_json::from_str::<ClientMessage>(&message_text) {
                                Ok(ClientMessage::RegisterLobby{ lobby_name }) => {

                                    if !lobby_regex.is_match(&lobby_name) {
                                        send_msg!(ctx, &connection.socket_idx, &ServerMessage::ErrorInvalidLobbyName);
                                        continue;
                                    }

                                    if ctx.lobby_names.contains_key(&lobby_name) {
                                        send_msg!(ctx, &connection.socket_idx, &ServerMessage::ErrorLobbyExists);
                                        continue;
                                    }

                                    // Let people change their current lobby
                                    if let Some(existing_lobby) = &connection.lobby_name {
                                        ctx.lobby_names.remove(existing_lobby);
                                    }

                                    ctx.lobby_names.insert(lobby_name.clone(), connection.socket_idx);
                                    connection.lobby_name = Some(lobby_name);

                                    info!("{:?}", ctx.lobby_names);

                                    send_msg!(ctx, &connection.socket_idx, &ServerMessage::AckRegisterLobby);
                                },
                                Ok(ClientMessage::GetLobbies) => {
                                    let lobbies =  ctx.lobby_names.clone();
                                    send_msg!(ctx, &connection.socket_idx, &ServerMessage::SendLobbyData {lobbies});
                                },
                                Ok(ClientMessage::SendOffer { to, offer }) => {
                                    send_msg!(ctx, &to, &ServerMessage::ReceiveOffer { from: connection.socket_idx, offer });
                                    send_msg!(ctx, &connection.socket_idx, &ServerMessage::AckSendOffer);
                                },
                                Ok(ClientMessage::SendResponse { to, response }) => {
                                    send_msg!(ctx, &to, &ServerMessage::ReceiveResponse { from: connection.socket_idx, response });
                                    send_msg!(ctx, &connection.socket_idx, &ServerMessage::AckSendResponse);
                                },
                                Ok(ClientMessage::SendIce { to, ice }) => {
                                    send_msg!(ctx, &to, &ServerMessage::ReceiveIce { from: connection.socket_idx, ice });
                                    send_msg!(ctx, &connection.socket_idx, &ServerMessage::AckSendIce);
                                },
                                Ok(ClientMessage::DebugPrint { data }) => {
                                    info!("Message from {}: {:?}", &connection.socket_idx, data);
                                },
                                Err(e) => {send_msg!(ctx, &connection.socket_idx, &ServerMessage::ErrorParse{msg: e.to_string()})}
                            }

/*
                            let mut parts = message.split_whitespace();

                            // We'll need the context here
                            match parts.next() {
                                Some("R:") => match parts.next() {
                                    None => {
                                        send_msg!(ctx, &connection.socket_idx, "E: No username specified!");
                                    },
                                    Some(username) => {
                                        if connection.player_name.is_some() {
                                            send_msg!(ctx, &connection.socket_idx, "E: You're already registered!");
                                            continue;
                                        }

                                        if !username_regex.is_match(username) {
                                            send_msg!(ctx, &connection.socket_idx, "E: Username invalid!");
                                            continue;
                                        }

                                        if ctx.player_names.contains_key(username.into()) {
                                            send_msg!(ctx, &connection.socket_idx, "E: Username taken!");
                                            continue;
                                        }

                                        ctx.player_names.insert(username.into(), connection.socket_idx);
                                        connection.player_name = Some(username.into());

                                        send_msg!(ctx, &connection.socket_idx, format!("R: Registered as {}!", username));

                                        info!("Player {} registered!", username);
                                    }
                                },
                                Some("S:") => {
                                    match connection.player_name {
                                        Some(_) => {
                                            let player_names = ctx.player_names.clone();
                                            let keys = player_names.keys();

                                            send_msg!(ctx, &connection.socket_idx, format!("{:?}", keys));
                                        }
                                        None => send_msg!(ctx, &connection.socket_idx, "E: Register first!")
                                    }
                                },
                                Some("1:") => match connection.player_name {
                                    Some(_) => match parts.next() {
                                        Some(connect_to) => {
                                            if connection.player_name.clone().unwrap() == connect_to {
                                                send_msg!(ctx, &connection.socket_idx, "E: Cannot connect to yourself!");
                                                continue;
                                            }
                                            // let ctx = ctx_arc.lock().await;
                                            if !ctx.player_names.contains_key(connect_to) {
                                                send_msg!(ctx, &connection.socket_idx, "E: No player with that name!");
                                                continue;
                                            }

                                            let sid = ctx.player_names.get_mut(connect_to).unwrap().clone();

                                            // Send to other player!
                                            send_msg!(ctx, &sid, message);

                                            send_msg!(ctx, &connection.socket_idx, "1: Sent to other player!");
                                        },
                                        None => send_msg!(ctx, &connection.socket_idx, "E: Specify a player name and offer!")
                                    },
                                    None => send_msg!(ctx, &connection.socket_idx, "E: Register first!")
                                },
                                Some("2:") => match connection.player_name {
                                    Some(_) => match parts.next() {
                                        Some(connect_to) => {
                                            if connection.player_name.clone().unwrap() == connect_to {
                                                send_msg!(ctx, &connection.socket_idx, "E: Cannot connect to yourself!");
                                                continue;
                                            }
                                            // let ctx = ctx_arc.lock().await;
                                            if !ctx.player_names.contains_key(connect_to) {
                                                send_msg!(ctx, &connection.socket_idx, "E: No player with that name!");
                                                continue;
                                            }

                                            let sid = ctx.player_names.get_mut(connect_to).unwrap().clone();

                                            // Send to other player!
                                            send_msg!(ctx, &sid, message);

                                            send_msg!(ctx, &connection.socket_idx, "2: Sent to other player!");
                                        },
                                        None => send_msg!(ctx, &connection.socket_idx, "E: Specify a player name and offer!")
                                    },
                                    None => send_msg!(ctx, &connection.socket_idx, "E: Register first!")
                                },
                                Some("3:") => match connection.player_name {
                                    Some(_) => match parts.next() {
                                        Some(connect_to) => {
                                            if connection.player_name.clone().unwrap() == connect_to {
                                                send_msg!(ctx, &connection.socket_idx, "E: Cannot connect to yourself!");
                                                continue;
                                            }
                                            // let ctx = ctx_arc.lock().await;
                                            if !ctx.player_names.contains_key(connect_to) {
                                                send_msg!(ctx, &connection.socket_idx, "E: No player with that name!");
                                                continue;
                                            }

                                            let sid = ctx.player_names.get_mut(connect_to).unwrap().clone();

                                            // Send to other player!
                                            send_msg!(ctx, &sid, message);

                                            send_msg!(ctx, &connection.socket_idx, "3: Sent to other player!");
                                        },
                                        None => send_msg!(ctx, &connection.socket_idx, "E: Specify a player name and offer!")
                                    },
                                    None => send_msg!(ctx, &connection.socket_idx, "E: Register first!")
                                },
                                Some("4:") => match connection.player_name {
                                    Some(_) => match parts.next() {
                                        Some(connect_to) => {
                                            if connection.player_name.clone().unwrap() == connect_to {
                                                send_msg!(ctx, &connection.socket_idx, "E: Cannot connect to yourself!");
                                                continue;
                                            }
                                            // let ctx = ctx_arc.lock().await;
                                            if !ctx.player_names.contains_key(connect_to) {
                                                send_msg!(ctx, &connection.socket_idx, "E: No player with that name!");
                                                continue;
                                            }

                                            let sid = ctx.player_names.get_mut(connect_to).unwrap().clone();

                                            // Send to other player!
                                            send_msg!(ctx, &sid, message);

                                            send_msg!(ctx, &connection.socket_idx, "4: Sent to other player!");
                                        },
                                        None => send_msg!(ctx, &connection.socket_idx, "E: Specify a player name and offer!")
                                    },
                                    None => send_msg!(ctx, &connection.socket_idx, "E: Register first!")
                                },
                                Some("P:") => {
                                    info!("{message}");
                                },
                                Some(_) => send_msg!(ctx, &connection.socket_idx, "E: Invalid command!"),
                                None => send_msg!(ctx, &connection.socket_idx, "E: Command must include whitespace!")
                            }
*/
                        },
                        Message::Close(_reason) => {
                            ctx.writers.remove(&connection.socket_idx);
                            match connection.lobby_name {
                                Some(name) => {ctx.lobby_names.remove(&name);},
                                None => {}
                            };
                            break;
                        },
                        _ => {}
                    },
                    Err(e) => {
                        error!("{}", e);
                        ctx.writers.remove(&connection.socket_idx);
                        match connection.lobby_name {
                            Some(name) => {ctx.lobby_names.remove(&name);},
                            None => {}
                        };
                        break;
                    }
                }
            },
            None => {
                let mut ctx = connection.ctx.lock().await;
                ctx.writers.remove(&connection.socket_idx);
                match connection.lobby_name {
                    Some(name) => {ctx.lobby_names.remove(&name);},
                    None => {}
                };
                break;
            }
        }
    }
}
