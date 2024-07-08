// The main purpose of this project is to create a program that will monitor the amount of times
// that i use the outputs of my setup, my keyboard, mouse and also monitor what i'm doing daily.
// The whole point of this is to create some graphs in an personal blog as i explained in README.md
use rdev::{listen, Event};
static mut LEFT_CLICKS: i64 = 0;
static mut MIDDLE_CLICKS: i64 = 0;
static mut RIGHT_CLICKS: i64 = 0;
static mut KEY_PRESSED: i64 = 0;
static mut MOUSE_MOV: i64 = 0;
static mut LAST_X_PX: i64 = 0;
static mut LAST_Y_PX: i64 = 0;

fn callback(event: Event) {
    match event.event_type {
        rdev::EventType::ButtonPress(button) => {
            // println!("My callback {:?}", button);

            match button {
                rdev::Button::Left => unsafe {
                    LEFT_CLICKS += 1;
                    println!("amount of left clicks: {}", LEFT_CLICKS);
                },
                rdev::Button::Right => unsafe {
                    RIGHT_CLICKS += 1;
                    println!("amount of right clicks: {}", RIGHT_CLICKS);
                },
                rdev::Button::Middle => unsafe {
                    MIDDLE_CLICKS += 1;
                    println!("amount of middle clicks: {}", MIDDLE_CLICKS);
                },
                _ => {}
            }
        }

        rdev::EventType::KeyPress(_) => unsafe {
            KEY_PRESSED += 1;
            println!("amount of key pressed: {}", KEY_PRESSED);
        },

        rdev::EventType::MouseMove { x, y } => unsafe {
            // you cannot just increase cause sometimes the amount o pixel between events jumps
            // like 5 or more px, depends on the speed you are moving your mouse.
            // maybe keep track of last cordinates and calculate the difference based on that, here
            // there is a link to help you: https://stackoverflow.com/questions/68288183/detect-mousemove-of-x-pixels

            println!("x: {}", x);
            println!("y: {}", y);
            println!("MOUSE MOV: {}px", MOUSE_MOV);

            // 1000 pixel (X)	26.4583333333 cm
        },

        _ => {}
    }
}

fn main() {
    // This will block.
    if let Err(error) = listen(callback) {
        println!("Error: {:?}", error)
    }
}
