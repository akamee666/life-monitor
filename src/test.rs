use std::fs::*;

use std::ffi::c_long;
use std::io::Result as IoResult;
use std::os::fd::AsFd;
use tracing::*;

use nix::unistd::read;
use tokio::io::unix::AsyncFd;

// I don't even know why i imported that i won't use it
#[allow(unused_imports)]
use crate::input_event_codes::*;

use crate::keylogger::KeyLogger;
// https://www.kernel.org/doc/html/v4.17/input/event-codes.html
/// `time` is the timestamp, it returns the time at which the event happened.
/// `type` is for example EV_REL for relative movement, EV_KEY for a keypress or release. More types are defined in include/uapi/linux/input-event-codes.h.
/// `code` is event code, for example REL_X or KEY_BACKSPACE, again a complete list is in include/uapi/linux/input-event-codes.h.
/// `value` is the value the event carries. Either a relative change for EV_REL, absolute new value for EV_ABS (joysticks ...), or 0 for EV_KEY for release, 1 for keypress and 2 for autorepeat.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct InputEvent {
    time: Time,
    typ: u16,
    code: u16,
    value: i32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct Time {
    tv_sec: c_long,
    tv_usec: c_long,
}

impl Time {
    fn new() -> Time {
        Time {
            tv_sec: 0,
            tv_usec: 0,
        }
    }
}

impl InputEvent {
    fn new() -> InputEvent {
        InputEvent {
            time: Time::new(),
            typ: 0,
            code: 0,
            value: 0,
        }
    }
}

pub async fn log_events() {
    // NOTE: must be in input group
    // TODO: Hardcoded
    // /dev/input/mouseX and /dev/input/eventX expose the same device but with different abstraction layers
    // /dev/input/mouseX → legacy PS/2-style interface.
    // Only provides relative motion + basic button clicks.
    // No access to high-resolution scroll wheels, side buttons, DPI switches, gestures, etc.
    // /dev/input/eventX → modern evdev interface.
    // Exposes the full capability set (all buttons, wheels, extra axes).
    // This is what you should use for anything beyond the bare minimum.
    let kbd = File::open("/dev/input/by-id/usb-BY_Tech_Gaming_Keyboard-event-kbd")
        .unwrap_or_else(|err| {
            if err.kind() == std::io::ErrorKind::PermissionDenied {
                error!("Permission denied when opening /dev/input/* file, are you in the inputs group?");
            };
            error!("failed to open /dev/input device");
            panic!("{err}");
        });

    let mouse = File::open("/dev/input/by-id/usb-Logitech_USB_Receiver-event-mouse")
        .unwrap_or_else(|err| {
            error!("failed to open /dev/input device");
            panic!("{err}");
        });

    nix::fcntl::fcntl(
        kbd.as_fd(),
        nix::fcntl::FcntlArg::F_SETFL(nix::fcntl::OFlag::O_NONBLOCK),
    )
    .unwrap();
    nix::fcntl::fcntl(
        mouse.as_fd(),
        nix::fcntl::FcntlArg::F_SETFL(nix::fcntl::OFlag::O_NONBLOCK),
    )
    .unwrap();

    let mut mouse_fd = AsyncFd::new(mouse).unwrap_or_else(|err| {
        error!("Failed to create asyncfd for mouse");
        panic!("fatal: {err}");
    });
    let mut kbd_fd = AsyncFd::new(kbd).unwrap_or_else(|err| {
        error!("Failed to create asyncfd for keyboard");
        panic!("fatal: {err}");
    });

    let mut keylogger = KeyLogger::default();

    loop {
        tokio::select! {
            res = handle_keyboard_events(&mut kbd_fd, &mut keylogger) => {
                if let Err(err) = res {
                    error!("Keyboard event handle failed: {err}");
                    break;
                }
            }
            res = handle_mouse_events(&mut mouse_fd) => {
                if let Err(err) = res {
                    error!("Mouse event handler failed: {err}");
                    break;
                }
            }
        }
    }
}

/// Waits for and processes all available keyboard events.
async fn handle_keyboard_events(
    kbd_fd: &mut AsyncFd<File>,
    keylogger: &mut KeyLogger,
) -> IoResult<()> {
    let _ = kbd_fd.readable_mut().await?;
    let mut kbd_event = InputEvent::new();

    loop {
        let buf = unsafe {
            std::slice::from_raw_parts_mut(
                &mut kbd_event as *mut _ as *mut u8,
                core::mem::size_of::<InputEvent>(),
            )
        };

        match read(kbd_fd.as_fd(), buf) {
            Ok(n) if n == core::mem::size_of::<InputEvent>() => {
                // value = 0 (release), 1 (press), 2 (repeat)
                // if kbd_event.typ == EV_KEY as u16 && kbd_event.value as u32 == 1 {
                if kbd_event.typ as u32 == EV_KEY && (kbd_event.value == 1 || kbd_event.value == 2)
                {
                    keylogger.t_kp += 1;
                    println!("Total keys pressed: {}", keylogger.t_kp);
                }
            }
            Ok(_) => break, // Partial read, wait for more data
            Err(nix::errno::Errno::EAGAIN) => {
                // Buffer is empty, we're done for now
                break;
            }
            Err(e) => return Err(std::io::Error::other(e)),
        }
    }

    Ok(())
}

/// Waits for and processes all available mouse events.
async fn handle_mouse_events(mouse_fd: &mut AsyncFd<File>) -> IoResult<()> {
    let _ = mouse_fd.readable_mut().await?;
    let mut mouse_event = InputEvent::new();

    loop {
        let buf = unsafe {
            std::slice::from_raw_parts_mut(
                &mut mouse_event as *mut _ as *mut u8,
                core::mem::size_of::<InputEvent>(),
            )
        };

        match read(mouse_fd.as_fd(), buf) {
            Ok(n) if n == core::mem::size_of::<InputEvent>() => {
                match mouse_event.typ as u32 {
                    EV_KEY => {
                        if mouse_event.value == 1 {
                            // Button press
                            match mouse_event.code as u32 {
                                BTN_LEFT => println!("Left mouse button pressed"),
                                BTN_RIGHT => println!("Right mouse button pressed"),
                                BTN_MIDDLE => println!("Middle mouse button pressed"),
                                BTN_SIDE => {
                                    println!("side button pressed")
                                }
                                BTN_EXTRA => {
                                    println!("extra button pressed")
                                }
                                BTN_FORWARD => {
                                    println!("forward button pressed")
                                }
                                BTN_BACK => {
                                    println!("back button pressed")
                                }

                                _ => {}
                            }
                        }
                    }
                    EV_REL => match mouse_event.code as u32 {
                        REL_X => {
                            println!("Mouse moved {} pixels to direction Y", mouse_event.value)
                        }
                        REL_Y => {
                            println!("Mouse moved {} pixels to direction X", mouse_event.value)
                        }
                        REL_WHEEL => println!("Mouse wheel: {}", mouse_event.value),
                        REL_HWHEEL => println!("Mouse wheel but H: {}", mouse_event.value),
                        _ => println!("Differnt event"),
                    },
                    _ => {}
                }
            }
            Ok(_) => break,                          // Partial read
            Err(nix::errno::Errno::EAGAIN) => break, // Drained
            Err(e) => return Err(std::io::Error::other(e)),
        }
    }

    Ok(())
}
