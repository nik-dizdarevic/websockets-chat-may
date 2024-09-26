use std::collections::HashMap;
use std::error::Error;
use std::io::{Cursor, Read, Write};
use bytes::{Buf, BytesMut};
use may::coroutine::JoinHandle;
use may::{go, loop_select};
use may::io::{SplitIo, SplitWriter};
use may::net::{TcpListener, TcpStream};
use may::sync::{mpsc};
use uuid::Uuid;
use websockets::{FragmentedMessage, Frame, Request, StatusCode, VecExt};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;
type ResponseFrame = Vec<u8>;
type Receiver<T> = mpsc::Receiver<T>;
type Sender<T> = mpsc::Sender<T>;
type Users = HashMap<Uuid, Sender<ResponseFrame>>;

enum Event {
    NewUser(Uuid, SplitWriter<TcpStream>),
    Message(ResponseFrame, Recipient),
}

#[derive(Debug)]
enum Recipient {
    All,
    User(Uuid),
}

fn main() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:7878")?;
    println!("Listening on 127.0.0.1:7878");

    let (broker_tx, broker_rx) = mpsc::channel();
    go!(move || {
        broker_loop(broker_rx).expect("Broker failure");
    });

    while let Ok((stream, _)) = listener.accept() {
        let broker_tx = broker_tx.clone();
        spawn_and_log_error(move || {
            handle_connection(stream, broker_tx)
        });
    }

    Ok(())
}

fn spawn_and_log_error<F>(f: F) -> JoinHandle<()>
where
    F: FnOnce() -> Result<()> + Send + 'static,
{
    go!(move || {
        if let Err(e) = f() {
            eprintln!("{}", e)
        }
    })
}

fn handle_connection(mut stream: TcpStream, mut broker_tx: Sender<Event>) -> Result<()> {
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
        let result = handle_websocket_frames(user, stream, buffer, &mut broker_tx);
        if result.is_err() {
            let close = Frame::Close(StatusCode::ProtocolError);
            broker_tx.send(Event::Message(close.response().unwrap(), Recipient::User(user))).unwrap();
        }
        result
    } else {
        Err("Not a valid websocket request".into())
    }
}

fn handle_websocket_frames(
    user: Uuid,
    stream: TcpStream,
    mut buffer: BytesMut,
    broker_tx: &mut Sender<Event>
) -> Result<()> {
    let (mut rd, wr) = stream.split().unwrap();
    broker_tx.send(Event::NewUser(user, wr)).unwrap();

    let mut read_buffer = vec![0; 4096];
    let mut fragmented_message = FragmentedMessage::Text(vec![]);
    loop {
        let mut buff = Cursor::new(&buffer[..]);
        match Frame::parse(&mut buff, &mut fragmented_message) {
            Ok(frame) => {
                if let Some(response) = frame.response() {
                    if frame.is_text() || frame.is_binary() || frame.is_continuation() {
                        broker_tx.send(Event::Message(response, Recipient::All)).unwrap();
                        fragmented_message = FragmentedMessage::Text(vec![]);
                    } else {
                        broker_tx.send(Event::Message(response, Recipient::User(user))).unwrap();
                        if frame.is_close() {
                            return Ok(());
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

fn writer_loop(user_rx: Receiver<ResponseFrame>, mut wr: SplitWriter<TcpStream>) -> Result<()> {
    while let Ok(message) = user_rx.recv() {
        wr.write_all(&message)?;
        wr.flush()?;
        if message.is_close() {
            break;
        }
    }
    Ok(())
}

fn broker_loop(broker_rx: Receiver<Event>) -> Result<()> {
    let (disconnect_tx, disconnect_rx) = mpsc::channel();
    let mut users= Users::new();

    loop_select!(
        event = broker_rx.recv() => match event {
            Ok(event) => {
                match event {
                    Event::NewUser(user, wr) => {
                        let (user_tx, user_rx) = mpsc::channel();
                        users.insert(user, user_tx);
                        let disconnect_tx = disconnect_tx.clone();
                        spawn_and_log_error(move || {
                            let result = writer_loop(user_rx, wr);
                            disconnect_tx.send(user).unwrap();
                            result
                        });
                    }
                    Event::Message(message, recipient) => match recipient {
                        Recipient::All => {
                            for user_tx in users.values() {
                                if let Err(e) = user_tx.send(message.clone()) {
                                    eprintln!("Failed sending to other users: {}", e);
                                }
                            }
                        }
                        Recipient::User(user) => {
                            if let Some(user_tx) = users.get(&user) {
                                user_tx.send(message).unwrap();
                            }
                        }
                    }
                }
            }
            Err(_) => break
        },
        user = disconnect_rx.recv() => match user {
            Ok(user) => {
                // println!("Goodbye user: {:?}", user);
                users.remove(&user);
            }
            Err(_) => break
        }
    );

    Ok(())
}