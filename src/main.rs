// The main purpose of this project is to create a program that will monitor the amount of times
// that i use the outputs of my setup, my keyboard, mouse and also monitor what i'm doing daily.
// The whole point of this is to create some graphs in an personal blog as i explained in README.md
use rdev::{listen, Event};
mod process;
use crate::process::*;

#[cfg(target_os = "windows")]
mod windows;

static mut LEFT_CLICKS: i64 = 0;
static mut MIDDLE_CLICKS: i64 = 0;
static mut RIGHT_CLICKS: i64 = 0;
static mut KEY_PRESSED: i64 = 0;
static mut MOUSE_MOV: f64 = 0.0;
static mut LAST_X_PX: f64 = 0.0;
static mut LAST_Y_PX: f64 = 0.0;

fn callback(event: Event) {
    // each time an event occurs, increment the global var by one.
    match event.event_type {
        rdev::EventType::ButtonPress(button) => match button {
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
        },

        rdev::EventType::KeyPress(_) => unsafe {
            KEY_PRESSED += 1;
            println!("amount of key pressed: {}", KEY_PRESSED);
        },

        rdev::EventType::MouseMove { x, y } => unsafe {
            // https://stackoverflow.com/questions/68288183/detect-mousemove-of-x-pixels
            if LAST_X_PX != 0.0 {
                let power_x: f64 = (LAST_Y_PX - y).powf(2.0);
                let power_y: f64 = (LAST_X_PX - x).powf(2.0);

                MOUSE_MOV += (power_x + power_y).sqrt();
            }

            LAST_X_PX = x;
            LAST_Y_PX = y;

            //Find out the DPI of the output device in question. (It's frequently 96 [96 dots per inch], but you cannot assume that.) This thread may help you do that, if it's a Windows Forms app. Also, the Graphics class has the DpiX and DpiY members, so you can use those.
            //Convert the DPI to DPC [dots-per-centimeter] (DPC = DPI / 2.54).
            //Multiply your number of centimeters by your DPC value.
        },

        _ => {}
    }
}

#[tokio::main]
async fn main() {
    println!("By now the program does not too much, it capture the active window each five seconds and display the amount of times that you have used you keyboard/mouse since the program had started.");
    // looks like it is working.
    tokio::spawn(track_processes());

    if let Err(error) = listen(callback) {
        println!("Error: {:?}", error)
    }
}
