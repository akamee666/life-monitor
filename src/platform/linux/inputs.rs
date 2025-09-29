use std::fs;
use std::fs::File;
use std::mem::MaybeUninit;
use std::os::fd::AsRawFd;
use std::os::unix::fs::FileTypeExt;
use std::time::Duration;

use anyhow::{Context, Result};

use nix::ioctl_read_buf;
use nix::unistd::read;

use tokio::io::unix::AsyncFd;
use tokio::sync::mpsc;
use tokio::sync::mpsc::channel;
use tokio::time::*;
use tracing::*;

use crate::common::*;
use crate::input_bindings::*;
use crate::storage::backend::DataStore;
use crate::storage::backend::StorageBackend;

// This is an ESTIMATE. A common default might be that one scroll click
// moves about 3 lines of text, which is roughly 0.4 to 0.5 cm.
// You can make this configurable.
const ASSUMED_CM_PER_SCROLL_CLICK: f64 = 0.4;
static mut IDLE_TIME: u64 = 0;

/// Either a relative change for EV_REL, absolute new value for EV_ABS (joysticks ...), or 0 for EV_KEY for release, 1 for keypress and 2 for autorepeat
/// https://docs.kernel.org/input/input.html
#[derive(PartialEq, Eq)]
enum KeyPressState {
    _Up = 0,
    Down = 1,
    _Repeat = 2,
}

enum InputEvent {
    Keyboard(input_event),
    Mouse(input_event),
}

struct DiscoveredDevices {
    keyboards: Vec<File>,
    mices: Vec<File>,
}

// ioctl defs
ioctl_read_buf!(eviocguniq, b'E', 0x08, u8);
ioctl_read_buf!(eviocgprop, b'E', 0x09, u8);
ioctl_read_buf!(eviocgmtslots, b'E', 0x0a, u8);
ioctl_read_buf!(eviocgkey, b'E', 0x18, u8);
ioctl_read_buf!(eviocgled, b'E', 0x19, u8);
ioctl_read_buf!(eviocgsnd, b'E', 0x1a, u8);
ioctl_read_buf!(eviocgsw, b'E', 0x1b, u8);
ioctl_read_buf!(eviocgbit_all, b'E', 0x20, u8);
ioctl_read_buf!(eviocgname, b'E', 0x06, u8);
ioctl_read_buf!(eviocgphys, b'E', 0x07, u8);
ioctl_read_buf!(eviocgbit_key, b'E', 0x20 + EV_KEY, u8); // key bitmask
ioctl_read_buf!(eviocgbit_rel, b'E', 0x20 + EV_REL, u8); // relative movement
ioctl_read_buf!(eviocgbit_abs, b'E', 0x20 + EV_ABS, u8); // absolute movement
ioctl_read_buf!(eviocgbit_rep, b'E', 0x20 + EV_REP, u8); // repeat
                                                         //

fn discover_devices() -> Result<DiscoveredDevices> {
    info!("Scanning /dev/input for devices...");
    let entries = fs::read_dir("/dev/input")?;
    let mut mices = Vec::new();
    let mut keyboards = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        let file_type = entry.file_type()?;

        if !file_type.is_char_device() {
            continue;
        }

        // Opening device is fine here (discover phase)
        let current_device = File::open(&path)?;
        if is_keyboard(&current_device) {
            info!(
                "Found Keyboard: [{}] -> {}",
                path.display(),
                get_device_name(&current_device).unwrap_or("N/A".to_string())
            );
            keyboards.push(current_device);
        } else if is_mouse(&current_device) {
            info!(
                "Found Mouse: [{}] -> {}",
                path.display(),
                get_device_name(&current_device).unwrap_or("N/A".to_string())
            );
            mices.push(current_device);
        }
    }

    if keyboards.is_empty() {
        anyhow::bail!("No keyboard devices found. This is required to run.");
    }
    if mices.is_empty() {
        anyhow::bail!("No mouse devices found. This is required to run.");
    }

    Ok(DiscoveredDevices { keyboards, mices })
}

// Returns the device name of a fd from: /dev/input/event*
fn get_device_name(fd: &File) -> Option<String> {
    let mut buf = vec![0u8; 256];
    match unsafe { eviocgname(fd.as_raw_fd(), buf.as_mut_slice()) } {
        Ok(len) if (len as usize) > buf.len() => {
            // should not happen with our buffer size, but guard anyway
            None
        }
        Ok(len) if len > 0 => {
            // ioctl returns length including trailing NUL; truncate to len and strip trailing zero
            let mut slice = &buf[..len as usize];
            if let Some(&0) = slice.last() {
                slice = &slice[..slice.len() - 1];
            }
            String::from_utf8(slice.to_vec()).ok()
        }
        _ => None,
    }
}

/// Helper: is `bit` set in `bytes` bitmask.
fn test_bit(bit: u32, bytes: &[u8]) -> bool {
    let byte_index = (bit / 8) as usize;
    let bit_in_byte = (bit % 8) as u8;
    if byte_index < bytes.len() {
        (bytes[byte_index] >> bit_in_byte) & 1 != 0
    } else {
        false
    }
}

/// Detect keyboard capabilities
fn is_keyboard(fd: &File) -> bool {
    // (EV_MAX + 7) / 8
    let mut ev_bitmask = vec![0u8; (EV_MAX as usize).div_ceil(8)];
    unsafe {
        if eviocgbit_all(fd.as_raw_fd(), ev_bitmask.as_mut_slice()).is_err() {
            // debug!("ioctl EVIOCGBIT(ALL) failed with error code: [{ret}]");
            return false;
        }
    }

    if !test_bit(EV_KEY, &ev_bitmask) {
        // debug!("Device is not a keyboard, EV_KEY test failed!");
        return false;
    }

    if !test_bit(EV_REP, &ev_bitmask) {
        // debug!("Device is not a keyboard, EV_REP test failed!");
        return false;
    }

    let mut key_bitmask = vec![0u8; (EV_MAX as usize).div_ceil(8)];
    unsafe {
        if eviocgbit_key(fd.as_raw_fd(), key_bitmask.as_mut_slice()).is_err() {
            // debug!("ioctl EVIOCGBIT(EV_KEYS) failed with error code: [{ret}]");
            return false;
        }
    }

    // check for alphabetic keys (Q..Y)
    let has_qwerty_keys = (KEY_Q..=KEY_Y).all(|k| test_bit(k, &key_bitmask));
    if !has_qwerty_keys {
        // debug!("Device is not a keyboard, can't handle alphabetic keys!");
        return false;
    }

    if is_mouse(fd) {
        // debug!("Device also has capabilities of a mouse, it isn't a keyboard.");
        return false;
    }

    true
}

/// Detect mouse capabilities
fn is_mouse(fd: &File) -> bool {
    let mut ev_types_bits = vec![0u8; (EV_MAX as usize).div_ceil(8)];
    unsafe {
        if eviocgbit_all(fd.as_raw_fd(), ev_types_bits.as_mut_slice()).is_err() {
            // debug!("ioctl EVIOCGBIT(ALL) failed with error code: [{ret}]");
            return false;
        }
    }

    if !test_bit(EV_REL, &ev_types_bits) {
        // debug!("Device can't handle relative movement, is not a mouse!");
        return false;
    }

    let mut rel_bits = vec![0u8; (EV_MAX as usize).div_ceil(8)];
    unsafe {
        if eviocgbit_rel(fd.as_raw_fd(), rel_bits.as_mut_slice()).is_err() {
            // debug!("ioctl EVIOCGBIT(EV_REL) failed with error code: [{ret}]");
            return false;
        }
    }

    if !test_bit(REL_X, &rel_bits) && !test_bit(REL_Y, &rel_bits) {
        // debug!("Device can't handle relative axes (X/Y). Not a mouse!");
        return false;
    }

    let mut ev_bitmask = vec![0u8; (EV_MAX as usize).div_ceil(8)];
    unsafe {
        if eviocgbit_all(fd.as_raw_fd(), ev_bitmask.as_mut_slice()).is_err() {
            // debug!("ioctl EVIOCGBIT(ALL) failed with error code: [{ret}]");
            return false;
        }
    }

    if !test_bit(EV_KEY, &ev_bitmask) {
        // debug!("Device can't handle EV_KEY events, not a mouse!");
        return false;
    }

    if is_keyboard(fd) {
        // debug!("Device seems to be an adapter; treat as non-mouse.");
        return false;
    }

    true
}

async fn keyboard_device_loop(mut file: AsyncFd<File>, tx: mpsc::Sender<InputEvent>) -> Result<()> {
    loop {
        let mut guard = file.readable_mut().await?;
        let mut event = MaybeUninit::<input_event>::uninit();
        let buf = unsafe {
            std::slice::from_raw_parts_mut(
                &mut event as *mut _ as *mut u8,
                core::mem::size_of::<input_event>(),
            )
        };

        match guard.try_io(|inner| {
            read(inner, buf).map_err(|err| std::io::Error::from_raw_os_error(err as i32))
        }) {
            Ok(Ok(n)) if n == core::mem::size_of::<input_event>() => {
                let kbd_event = unsafe { event.assume_init() };
                let _ = tx.try_send(InputEvent::Keyboard(kbd_event));
            }
            Ok(Ok(_)) => {} // partial read; ignore
            Ok(Err(err)) => return Err(anyhow::Error::from(err)),
            Err(_would_block) => continue, // fd not ready, await again
        }
    }
}

async fn mouse_device_loop(mut file: AsyncFd<File>, tx: mpsc::Sender<InputEvent>) -> Result<()> {
    loop {
        let mut guard = file.readable_mut().await?;
        let mut event = MaybeUninit::<input_event>::uninit();
        let buf = unsafe {
            std::slice::from_raw_parts_mut(
                &mut event as *mut _ as *mut u8,
                core::mem::size_of::<input_event>(),
            )
        };

        match guard.try_io(|inner| {
            read(inner, buf).map_err(|err| std::io::Error::from_raw_os_error(err as i32))
        }) {
            Ok(Ok(n)) if n == core::mem::size_of::<input_event>() => {
                let kbd_event = unsafe { event.assume_init() };
                let _ = tx.try_send(InputEvent::Mouse(kbd_event));
            }
            Ok(Ok(_)) => {} // partial read; ignore
            Ok(Err(err)) => return Err(anyhow::Error::from(err)),
            Err(_would_block) => continue, // fd not ready, await again
        }
    }
}

/// Spawn listeners: devices will send `InputEvent` to `tx`.
async fn spawn_input_listeners(tx: mpsc::Sender<InputEvent>) -> Result<()> {
    let devices = tokio::task::spawn_blocking(discover_devices).await??;

    for file in devices.keyboards {
        nix::fcntl::fcntl(
            &file,
            nix::fcntl::FcntlArg::F_SETFL(nix::fcntl::OFlag::O_NONBLOCK),
        )?;
        let async_file = AsyncFd::new(file)?;
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            if let Err(err) = keyboard_device_loop(async_file, tx_clone).await {
                error!("Keyboard device task failed: {err:?}");
            }
        });
    }

    for file in devices.mices {
        nix::fcntl::fcntl(
            &file,
            nix::fcntl::FcntlArg::F_SETFL(nix::fcntl::OFlag::O_NONBLOCK),
        )?;
        let async_file = AsyncFd::new(file)?;
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            if let Err(e) = mouse_device_loop(async_file, tx_clone).await {
                error!("Mouse device task failed: {}", e);
            }
        });
    }

    Ok(())
}

pub async fn run(dpi: Option<u32>, update_interval: u32, backend: StorageBackend) -> Result<()> {
    let mut inputs_data = InputLogger::new(&backend, dpi.unwrap_or(800))
        .await
        .with_context(|| {
            "Failed to initialize Inputs runtime because could not create InputLogger data"
        })?;

    let (tasks_tx, mut tasks_rx) = channel::<Signals>(32);
    let (events_tx, mut events_rx) = channel::<InputEvent>(256);

    // database updates
    let ticker_handle = spawn_ticker(
        tasks_tx.clone(),
        Duration::from_secs(update_interval.into()),
        Signals::DbUpdate,
    );

    tokio::spawn(async move {
        if let Err(err) = ticker_handle.await {
            error!("Ticker task panicked or was cancelled: {err:?}");
        } else {
            error!("Ticker task exited unexpectedly.");
        }
    });

    spawn_input_listeners(events_tx)
        .await
        .with_context(|| "Failed to spawn input listeners")?;
    let idle = Duration::from_secs(20);
    let mut ticker = interval(idle);
    let mut last_event: Option<input_event> = None;
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                debug!("Checking the time of the last event");
                if let Some(event) = last_event {
                    let event_timeval = event.time;
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::SystemTime::UNIX_EPOCH)
                        .unwrap();

                    let delta = now.checked_sub(Duration::new(event_timeval.tv_sec as u64, event_timeval.tv_usec as u32)).unwrap_or_default();
                    debug!("Time since last event: {:.6} seconds", delta.as_secs_f64());
                    unsafe {
                        IDLE_TIME = delta.as_secs()
                    }
                }
            }
            // An input event was received from a device.
            Some(event) = events_rx.recv() => {
                match event {
                    InputEvent::Keyboard(event) => {
                        last_event = Some(event);
                        if event.value == KeyPressState::Down as i32 {
                            inputs_data.key_presses += 1;
                        }
                    },
                    InputEvent::Mouse(event) => {
                        last_event = Some(event);
                        match event.type_ as u32 {
                            EV_KEY => {
                                if event.value == KeyPressState::Down as i32 {
                                    match event.code as u32 {
                                        BTN_LEFT => inputs_data.left_clicks += 1,
                                        BTN_RIGHT => inputs_data.right_clicks += 1,
                                        BTN_MIDDLE => inputs_data.middle_clicks += 1,
                                        // BTN_SIDE => logger.side_clicks += 1,
                                        // BTN_EXTRA => logger.extra_clicks +=1,
                                        // BTN_FORWARD => logger.forward_clicks +=1,
                                        // BTN_BACK => logger.back_clicks +=1,
                                        // Other buttons are ignored for now.
                                        _ => {}
                                    }
                                }
                            }
                            EV_REL => {
                                match event.code as u32 {
                                    REL_X | REL_Y => {
                                        inputs_data.pixels_traveled += event.value.unsigned_abs() as u64;
                                    }
                                    REL_WHEEL => { // Vertical scroll
                                        let clicks = event.value.unsigned_abs() as u64;
                                        inputs_data.vertical_scroll_clicks += clicks;
                                        inputs_data.vertical_scroll_cm += clicks as f64 * ASSUMED_CM_PER_SCROLL_CLICK;
                                    }
                                    REL_HWHEEL => { // Horizontal scroll
                                        let clicks = event.value.unsigned_abs() as u64;
                                        inputs_data.horizontal_scroll_clicks += clicks;
                                        inputs_data.horizontal_scroll_cm += clicks as f64 * ASSUMED_CM_PER_SCROLL_CLICK;
                                    }                                    _ => {}
                                }
                            }
                            _ => {}
                        }
                    },
                }
            }

            // A signal was received from another task.
            Some(signal) = tasks_rx.recv() => {
                if matches!(signal, Signals::DbUpdate) {
                    if inputs_data.mouse_dpi > 0 {
                        const INCHES_TO_CM: f64 = 2.54;
                        inputs_data.cm_traveled = (inputs_data.pixels_traveled as f64 / inputs_data.mouse_dpi as f64 * INCHES_TO_CM) as u64;
                    }

                    if let Err(e) = backend.store_keys_data(&inputs_data).await {
                        error!("Failed to store keylogger data in backend: {:?}", e);
                    }
                }
            }

            // If both channels close, it means all producers have exited.
            else => {
                error!("All event source channels closed. Shutting down.");
                break;
            }
        }
    }
    anyhow::bail!("Input listener unexpectedly stopped");
}

/// Returns true if the last input event happened +20 seconds ago
/// * What if user is watching something?
pub fn is_idle() -> bool {
    unsafe { IDLE_TIME > 20 }
}
