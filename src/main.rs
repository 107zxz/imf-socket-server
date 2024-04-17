use std::net::TcpListener;
use std::thread::spawn;
use tungstenite::{accept, Message};

/// A WebSocket echo server
fn main () {
    let server = TcpListener::bind("127.0.0.1:80").unwrap();
    for stream in server.incoming() {
        spawn (move || {
            let mut websocket = accept(stream.unwrap()).unwrap();
            loop {
                let msg = websocket.read().unwrap();

                // Only handle text-y messages
                match msg.to_text() {
                    Ok(msg_text) => match handle_message(msg_text) {
                        Ok(response) => websocket.send(Message::Text(response)).unwrap(),
                        Err(err_msg) => eprintln!("Error! {}", err_msg)
                    }
                    _ => {}
                }
            }
        });
    }
}

fn handle_message(msg: &str) -> Result<String, String> {
    println!("Recieved message {:?}", msg);

    let mut char_iter = msg.chars();

    // Match message code
    return match char_iter.next() {
        Some('J') => {
            println!("Recieved join req");
            Ok("J: Obama\n".to_owned())
        },
        None => Err("Empty message!".to_owned()),
        _ => Err("Invalid control code!".to_owned())
    }
}