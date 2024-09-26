use may::sync::Mutex;
use may::go;
use std::sync::Arc;

fn main() {
    // kreiramo in ovijemo Mutex v Arc
    let data1 = Arc::new(Mutex::new(0));

    // kloniramo Arc
    let data2 = Arc::clone(&data1);

    let handle = go!(move || {
        // pridobimo zaklep
        let mut guard = data2.lock().unwrap();
        *guard += 1;
    });

    handle.join().unwrap();

    println!("Result: {}", *data1.lock().unwrap());
}