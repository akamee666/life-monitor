// The main purpose of this project is to create a program that will monitor the amount of times
// that i use the outputs of my setup, my keyboard, mouse and also monitor what i'm doing daily.
// The whole point of this is to create some graphs in an personal blog as i explained in README.md
use rdev::{listen, Event};

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "windows")]
mod windows;

static mut EVENTS_COUNTER: EventsCounter = EventsCounter::new();

#[derive(Debug)]
struct EventsCounter {
    pub left_clicks: i64,
    pub right_clicks: i64,
    pub middle_clicks: i64,
    pub keys_pressed: i64,
    pub mouse_moved_cm: f64,
}

impl EventsCounter {
    const fn new() -> EventsCounter {
        let left_clicks = 0;
        let right_clicks = 0;
        let middle_clicks = 0;
        let keys_pressed = 0;
        let mouse_moved_cm = 0.0;

        /* Return the values */
        EventsCounter {
            left_clicks,
            right_clicks,
            middle_clicks,
            keys_pressed,
            mouse_moved_cm,
        }
    }

    pub fn print_counters(&self) {
        println!("{:#?}", self);
    }
}

static mut LAST_X_PX: f64 = 0.0;
static mut LAST_Y_PX: f64 = 0.0;
fn callback(event: Event) {
    // each time an event occurs, increment the global var by one.
    match event.event_type {
        rdev::EventType::ButtonPress(button) => match button {
            rdev::Button::Left => unsafe { EVENTS_COUNTER.left_clicks += 1 },
            rdev::Button::Right => unsafe { EVENTS_COUNTER.right_clicks += 1 },
            rdev::Button::Middle => unsafe { EVENTS_COUNTER.middle_clicks += 1 },
            _ => {}
        },

        rdev::EventType::KeyPress(key) => match key {
            rdev::Key::KeyS => unsafe {
                EVENTS_COUNTER.keys_pressed += 1;
                EVENTS_COUNTER.print_counters();
            },
            _ => unsafe { EVENTS_COUNTER.keys_pressed += 1 },
        },

        rdev::EventType::MouseMove { x, y } => unsafe {
            if LAST_X_PX != 0.0 {
                // this is not accurate aodmwoamodawodawmod
                let power_x: f64 = ((LAST_Y_PX - y).powf(2.0)) / 1600.0;
                let power_y: f64 = ((LAST_X_PX - x).powf(2.0)) / 1600.0;
                let pixels_moved = (power_x + power_y).sqrt();
                // I would like this being i64 but not working just parsing to
                // so i figure it out how to do it later i guess.
                EVENTS_COUNTER.mouse_moved_cm += pixels_moved.ceil() * 0.026;
            }

            LAST_X_PX = x;
            LAST_Y_PX = y;
        },

        _ => {}
    }
}

#[tokio::main]
async fn main() {
    println!("By now the program does not too much, it capture the active window each five seconds and display the amount of times that you have used you keyboard/mouse since the program had started.");

    //tokio::spawn(linux::process::track_processes());
    if let Err(error) = listen(callback) {
        println!("Error: {:?}", error)
    }
}
