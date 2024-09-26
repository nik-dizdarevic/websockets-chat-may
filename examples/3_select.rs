use may::sync::spsc;
use may::go;
use may::select;

fn main() {
    // kreiramo dva kanala
    let (tx1, rx1) = spsc::channel();
    let (tx2, rx2) = spsc::channel();

    go!(move || {
        let _ = tx1.send("one");
    });

    go!(move || {
        let _ = tx2.send("two");
    });

    // hkrati čakamo na oba kanala
    let out = select!(
        val = rx1.recv() => {
            println!("rx1 completed first with {:?}", val.unwrap())
        },
        val = rx2.recv() => {
            println!("rx2 completed first with {:?}", val.unwrap())
        }
    );

    println!("Got = {}", out);
}