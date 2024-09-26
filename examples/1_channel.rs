use may::sync::mpsc;
use may::go;

fn main() {
    // kreiramo kanal
    let (tx, rx) = mpsc::channel();

    // kloniramo oddajnik
    let tx2 = tx.clone();

    go!(move || {
        // pošljemo vrednost
        tx.send("sending from first handle").unwrap();
    });

    go!(move || {
        // pošljemo vrednost
        tx2.send("sending from second handle").unwrap();
    });

    // sprejmemo vrednost
    // blokira glavno nit
    while let Ok(message) = rx.recv() {
        println!("Got = {}", message);
    }
}