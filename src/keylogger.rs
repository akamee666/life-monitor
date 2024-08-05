/* This code works both in linux and windows, thanks rdev for this. */
use rdev::{listen, Event};
static mut EVENTS_COUNTER: KeyLogger = KeyLogger::new();

#[derive(Debug)]
#[allow(dead_code)]
pub struct KeyLogger {
    pub left_clicks: i64,
    pub right_clicks: i64,
    pub middle_clicks: i64,
    pub keys_pressed: i64,
    pub pixels_moved: f64,
    pub mouse_moved_cm: f64,
}

impl KeyLogger {
    const fn new() -> KeyLogger {
        let left_clicks = 0;
        let right_clicks = 0;
        let middle_clicks = 0;
        let keys_pressed = 0;
        let mouse_moved_cm = 0.0;
        let pixels_moved = 0.0;

        /* Return the values */
        KeyLogger {
            left_clicks,
            right_clicks,
            middle_clicks,
            keys_pressed,
            pixels_moved,
            mouse_moved_cm,
        }
    }

    pub async fn init() {
        if let Err(error) = listen(callback) {
            println!("Error: {:?}", error)
        }
    }

    pub fn print_counters(&self) {
        println!("{:#?}", self);
    }
}

static mut LAST_X_PX: f64 = 0.0;
static mut LAST_Y_PX: f64 = 0.0;

pub fn callback(event: Event) {
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
                let mouse_dpi = 1600.0;
                let power_x: f64 = ((LAST_Y_PX - y).powf(2.0)) / mouse_dpi;
                let power_y: f64 = ((LAST_X_PX - x).powf(2.0)) / mouse_dpi;
                let pixels_moved = (power_x + power_y).sqrt();

                #[cfg(target_os = "windows")]
                {
                    // windows seems to fire more events than linux by some amount, so  by divinding by 3.
                    // it looks like accurate but i guess it's not a solution for the problem and
                    // it will come back to bite my ass later.
                    // This should be converted only when the data is being sent to api.
                    // EVENTS_COUNTER.pixels_moved += pixels_moved.ceil();
                    EVENTS_COUNTER.mouse_moved_cm += (pixels_moved.ceil() * 0.026) / 3.0;
                }

                #[cfg(target_os = "linux")]
                {
                    EVENTS_COUNTER.mouse_moved_cm += pixels_moved.ceil() * 0.026;
                    // EVENTS_COUNTER.pixels_moved += pixels_moved.ceil();
                }
            }

            LAST_X_PX = x;
            LAST_Y_PX = y;
        },

        _ => {}
    }
}
