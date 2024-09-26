use std::collections::HashMap;
use std::error::Error;
use std::io::{Cursor, Read, Write};
use std::sync::Arc;
use bytes::{Buf, BytesMut};
use may::go;
use may::io::{SplitIo, SplitReader};
use may::net::{TcpListener, TcpStream};
use may::sync::{mpsc, RwLock};
use uuid::Uuid;
use websockets::{FragmentedMessage, Frame, Request, VecExt};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;
type ResponseFrame = Vec<u8>;
type Sender<T> = mpsc::Sender<T>;
type Users = Arc<RwLock<HashMap<Uuid, Sender<ResponseFrame>>>>;

fn main() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:7878")?;
    println!("Listening on 127.0.0.1:7878");

    let users = Users::default();
    while let Ok((stream, _)) = listener.accept() {
        let users = users.clone();
        go!(move || {
            handle_connection(stream, users).expect("Failure when handling connection");
        });
    }

    Ok(())
}

fn handle_connection(mut stream: TcpStream, users: Users) -> Result<()> {
    let user = Uuid::new_v4();
    let mut buffer = BytesMut::zeroed(4096);

    // println!("Welcome user {:?}", user);

    if 0 == stream.read(&mut buffer)? {
        return Err("Connection closed by remote.".into());
    }

    let request = Request::new(&buffer)?;
    if let Some(response) = request.response() {
        stream.write_all(response.as_bytes())?;
        stream.flush()?;
        buffer.clear();
        handle_websocket_frames(users, user, stream, buffer)
    } else {
        Err("Not a valid websocket request".into())
    }
}

fn handle_websocket_frames(
    users: Users,
    user: Uuid,
    stream: TcpStream,
    buffer: BytesMut
) -> Result<()> {
    let (tx, rx) = mpsc::channel();
    users.write().unwrap().insert(user, tx);

    let (rd, mut wr) = stream.split().unwrap();

    let users_rd = users.clone();
    go!(move || {
        if let Err(e) = read_loop(buffer, rd, &users_rd, user) {
            println!("Error: {:?}", e);
            disconnect(user, &users_rd);
        }
    });

    while let Ok(response) = rx.recv() {
        if let Err(e) = wr.write_all(&response) {
            println!("Error: {:?}", e);
            disconnect(user, &users);
            break;
        }
        wr.flush().unwrap();
        if response.is_close() {
            disconnect(user, &users);
            break;
        }
    }

    Ok(())
}

fn read_loop(
    mut buffer: BytesMut,
    mut rd: SplitReader<TcpStream>,
    users: &Users,
    user: Uuid
) -> Result<()> {
    let mut read_buffer = vec![0; 4096];
    let mut fragmented_message = FragmentedMessage::Text(vec![]);
    loop {
        let mut buff = Cursor::new(&buffer[..]);
        match Frame::parse(&mut buff, &mut fragmented_message) {
            Ok(frame) => {
                if let Some(response) = frame.response() {
                    if frame.is_text() || frame.is_binary() || frame.is_continuation() {
                        for tx in users.read().unwrap().values() {
                            tx.send(response.clone()).expect("Failed to send message");
                        }
                        fragmented_message = FragmentedMessage::Text(vec![]);
                    } else {
                        if let Some(tx) = users.read().unwrap().get(&user) {
                            tx.send(response).expect("Failed to send message");
                        }
                        if frame.is_close() {
                            return Ok(())
                        }
                    }
                }
                buffer.advance(buff.position() as usize);
            }
            Err(_) => {
                let n = rd.read(&mut read_buffer)?;
                if n == 0 {
                    return Err("Connection closed by remote.".into());
                }
                buffer.extend_from_slice(&read_buffer[..n]);
            }
        }
    }
}

fn disconnect(user: Uuid, users: &Users) {
    // println!("Goodbye user: {:?}", user);
    users.write().unwrap().remove(&user);
}