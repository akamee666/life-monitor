// The main purpose of this project is to create a program that will monitor the amount of times
// that i use the outputs of my setup, my keyboard, mouse and also monitor what i'm doing daily.
// The whole point of this is to create some graphs in an personal blog as i explained in README.md
use rdev::{listen, Event};

// pub struct Event {
//     pub time: SystemTime,
//     pub name: Option<String>,
//     pub event_type: EventType,
// }
//
//
fn callback(event: Event) {
    println!("My callback {:?}", event);
    match event.name {
        Some(string) => println!("User wrote {:?}", string),
        None => (),
    }
}

fn main() {
    // This will block.
    if let Err(error) = listen(callback) {
        println!("Error: {:?}", error)
    }
}
