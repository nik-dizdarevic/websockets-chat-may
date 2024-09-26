use may::go;

fn main() {
    let handle = go!(|| {
        // tukaj opravimo nekaj dela
        "return value"
    });

    // tukaj opravimo nekaj drugega dela

    let out = handle.join().unwrap();
    println!("Got {}", out);
}