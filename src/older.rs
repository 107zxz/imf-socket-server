use std::{env, fs};
use std::cell::RefCell;
use std::io::BufReader;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::ops::DerefMut;
use std::rc::Rc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::spawn;
use tungstenite::{accept, Message, WebSocket};
use log::{error, info};
use serde::Deserialize;
use std::sync::mpsc;

/// A WebSocket echo server
fn main () {
    env_logger::init();

    GameServer::new().run();
}


enum ConnectionState {
    DUMMY
}
struct Connection {
    state: ConnectionState,
    peer_name: String
}

impl Connection {
    pub fn new() -> Self {
        Connection {
            peer_name: "stoad".into(),
            state: ConnectionState::DUMMY
        }
    }
}

#[derive(Deserialize)]
struct GameServer {
    default_port: u16,
    room_names: Vec<String>,
}

impl GameServer {

    pub fn new() -> Self {
        let mut conf: GameServer = serde_yaml::from_str(&fs::read_to_string("config.yml").expect("You need a config.yml file!")).expect("Failed to parse config");
        conf
    }

    fn generate_room_code() -> String {
        "Obama".to_owned()
    }
    fn handle_message(conn: &mut Connection, ws: &mut WebSocket<TcpStream>) {
        let message = match ws.read() {
            Ok(msg) => msg.to_string(),
            Err(_) => return
        };

        info!("Recieved message {:?}", message);

        let mut message_chunks = message.split_whitespace().fuse();
        
        ws.send(
            Message::Text(
                match message_chunks.next() {
                    // Host request: Specify a room code to try and host
                    // If none is specified you get one free
                    Some("H:") => match message_chunks.next() {
                        Some(code) => format!("H: {}\n", code),
                        None => "H: Obama\n".into()
                    },
                    Some(_) => "E: Invalid opcode or syntax\n".into(),
                    None => "E: Message did not contain whitespace\n".into()
                }
            )
        ).unwrap()
    }
    pub fn run(&mut self) {
        let port = match env::var("PORT") {
            Ok(port_str) => port_str.parse().unwrap(),
            Err(_) => self.default_port
        };

        let server = TcpListener::bind(SocketAddr::from((
            [0, 0, 0, 0], port
        ))).unwrap();

        info!("Listening on port {}!", port);

        // let thingy = Mutex::new(Vec::<String>::new());

        let th = Arc::new(Mutex::new(Vec::<String>::new()));


        for stream in server.incoming() {
            spawn (|| {
                let mut connection = Connection::new();
                let mut websocket = accept(stream.unwrap()).unwrap();

                let th2 = th.clone();
                // let mut th3 = th2.lock().unwrap();

                loop {
                    Self::handle_message(&mut connection, &mut websocket);
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::thread::{sleep, spawn};
    use tungstenite::{connect, Message};
    use url::Url;
    use crate::GameServer;

    #[test]
    fn test_join() {
        // Run server
        spawn(|| {
            GameServer::new().run()
        });

        // Connect to server
        let (mut socket, response) = connect(Url::parse("ws://localhost:9081").unwrap()).unwrap();

        // Expect a websocket upgrade response
        assert_eq!(response.status().as_u16(), 101);

        // Request a random lobby
        socket.send(Message::Text("H: ".to_string())).unwrap();

        // Did we get a lobby code back?
        assert_eq!("H: Obama", socket.read().unwrap().to_string());
    }
}