use crate::common::*;
use crate::storage::backend::*;
use crate::StorageBackend;

use anyhow::{Context, Result};
use std::ffi::c_void;
use std::mem::size_of;

use tokio::{sync::mpsc::*, sync::*, time::*};

use tracing::*;
use windows::Win32::Graphics::Gdi::{GetDC, GetDeviceCaps, ReleaseDC, HORZSIZE, VERTSIZE};

use windows::core::{w, Error, HRESULT, PCWSTR};
use windows::Win32::Devices::HumanInterfaceDevice::HidD_GetProductString;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::{
    GetRawInputData, GetRawInputDeviceInfoW, GetRawInputDeviceList, RegisterRawInputDevices,
    HRAWINPUT, MOUSE_MOVE_ABSOLUTE, RAWINPUT, RAWINPUTDEVICE, RAWINPUTDEVICELIST, RAWINPUTHEADER,
    RAWKEYBOARD, RAWMOUSE, RIDEV_INPUTSINK, RIDI_DEVICENAME, RID_INPUT,
};
use windows::Win32::UI::WindowsAndMessaging::*;

/// List all raw input devices
fn list_raw_input_devices() -> Result<Vec<RAWINPUTDEVICELIST>> {
    unsafe {
        let mut num_devices: u32 = 0;
        let size = size_of::<RAWINPUTDEVICELIST>() as u32;
        GetRawInputDeviceList(None, &mut num_devices, size);
        if num_devices == 0 {
            return Ok(vec![]);
        }

        let mut vec = Vec::with_capacity(num_devices as usize);
        let written = GetRawInputDeviceList(Some(vec.as_mut_ptr()), &mut num_devices, size);
        if written == u32::MAX {
            return Err(Error::from_win32()).context("GetRawInputDeviceList failed");
        }
        vec.set_len(written as usize);
        Ok(vec)
    }
}

#[derive(Clone, Copy)]
pub enum RawInputEvent {
    Keyboard(RAWKEYBOARD),
    Mouse(RAWMOUSE),
}

unsafe fn get_human_readable_name(device_handle: HANDLE) -> Result<String> {
    let mut size: u32 = 0;
    if GetRawInputDeviceInfoW(Some(device_handle), RIDI_DEVICENAME, None, &mut size) != 0 {
        return Err(anyhow::anyhow!(
            "GetRawInputDeviceInfoW (size query) failed"
        ));
    }
    if size == 0 {
        return Ok("[No device name]".to_string());
    }

    let mut device_path_buffer: Vec<u16> = vec![0; size as usize];
    if GetRawInputDeviceInfoW(
        Some(device_handle),
        RIDI_DEVICENAME,
        Some(device_path_buffer.as_mut_ptr() as *mut _),
        &mut size,
    ) == u32::MAX
    {
        return Err(anyhow::anyhow!(
            "GetRawInputDeviceInfoW (data fetch) failed"
        ));
    }

    let hid_handle = CreateFileW(
        PCWSTR(device_path_buffer.as_ptr()),
        0, // no access to the device itself, just for querying
        FILE_SHARE_READ | FILE_SHARE_WRITE,
        None,
        OPEN_EXISTING,
        Default::default(),
        None,
    )?;

    if hid_handle.is_invalid() {
        // could be a composite device parent, often doesn't have a product string.
        return Ok("[Not a direct HID or access denied]".to_string());
    }

    let mut name_buffer: Vec<u16> = vec![0; 127];
    let result = HidD_GetProductString(
        hid_handle,
        name_buffer.as_mut_ptr() as *mut _,
        (name_buffer.len() * size_of::<u16>()) as u32,
    );

    let _ = CloseHandle(hid_handle);

    if !result {
        return Ok("[HID with no product string]".to_string());
    }

    let len = name_buffer
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(name_buffer.len());
    let name = String::from_utf16_lossy(&name_buffer[..len]);

    Ok(name)
}

pub async fn run(dpi: Option<u32>, update_interval: u32, backend: StorageBackend) -> Result<()> {
    let mouse_dpi = dpi.unwrap_or(DEFAULT_MOUSE_DPI).max(1) as f64;
    let mut inputs_data = InputLogger::default();
    let mut input_buffer =
        InputBucketBuffer::new(backend.source_id(), backend.bucket_granularity_minutes());

    let (events_tx, mut events_rx) = channel::<RawInputEvent>(256);
    let mut db_updates = interval(Duration::from_secs(update_interval as u64));

    // Log devices
    for (i, dev) in list_raw_input_devices()?.into_iter().enumerate() {
        let name = unsafe { get_human_readable_name(dev.hDevice) }
            .unwrap_or_else(|e| format!("[Error: {e}]"));
        debug!("Device {i}: type={:?}, name={}", dev.dwType, name);
    }

    {
        let screen_dc = unsafe { GetDC(None) };
        if screen_dc.is_invalid() {
            anyhow::bail!("Failed to get screen size!");
        }

        inputs_data.w.screen_width_mm = unsafe { GetDeviceCaps(Some(screen_dc), HORZSIZE).into() };
        inputs_data.w.screen_height_mm = unsafe { GetDeviceCaps(Some(screen_dc), VERTSIZE).into() };

        let _ = unsafe { ReleaseDC(None, screen_dc) };
    }

    // This is required because GetMessageW is a blocking call.
    tokio::task::spawn_blocking(move || {
        if let Err(e) = run_message_loop(events_tx) {
            error!("Win32 message loop thread failed: {:?}", e);
        }
    });

    info!("Raw input thread started. Listening for events...");

    loop {
        tokio::select! {
            Some(event) = events_rx.recv() => {
                process_event(&mut inputs_data, &mut input_buffer, mouse_dpi, event);
            }

            _ = db_updates.tick() => {
                let pending_rows = input_buffer.drain();
                if let Err(e) = backend.store_keys_data(&pending_rows).await {
                    error!("Failed to store inputs data in backend: {:?}", e);
                }
            }

            else => {
                error!("Event source channel closed. Shutting down.");
                break;
            }
        }
    }
    anyhow::bail!("Message Loop finished unexpectedly!")
}

// https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-rawmouse#remarks
fn process_event(
    inputs: &mut InputLogger,
    input_buffer: &mut InputBucketBuffer,
    mouse_dpi: f64,
    event: RawInputEvent,
) {
    match event {
        RawInputEvent::Keyboard(event) => {
            apply_keyboard_event(
                inputs,
                input_buffer,
                chrono::Utc::now(),
                event.VKey,
                (event.Flags & RI_KEY_BREAK as u16) == 0,
            );
        }

        RawInputEvent::Mouse(event) => {
            let now = chrono::Utc::now();
            // Relative movement we already get the delta for x and y
            if (event.usFlags.0 & MOUSE_MOVE_ABSOLUTE.0) == 0 {
                apply_relative_mouse_move(
                    input_buffer,
                    now,
                    mouse_dpi,
                    event.lLastX as f64,
                    event.lLastY as f64,
                );
            }

            // win32 doesn't use pixels as movement in this case
            if (event.usFlags.0 & MOUSE_MOVE_ABSOLUTE.0) != 0 {
                apply_absolute_mouse_move(inputs, input_buffer, now, event.lLastX, event.lLastY);
            }

            unsafe {
                apply_mouse_buttons(input_buffer, now, event.Anonymous.Anonymous.usButtonFlags);

                if (event.Anonymous.Anonymous.usButtonFlags & RI_MOUSE_WHEEL as u16) != 0 {
                    apply_vertical_wheel(
                        input_buffer,
                        now,
                        event.Anonymous.Anonymous.usButtonData as i16,
                    );
                }

                if (event.Anonymous.Anonymous.usButtonFlags & RI_MOUSE_HWHEEL as u16) != 0 {
                    apply_horizontal_wheel(
                        input_buffer,
                        now,
                        event.Anonymous.Anonymous.usButtonData as i16,
                    );
                }
            }
        }
    }
}

fn apply_keyboard_event(
    inputs: &mut InputLogger,
    input_buffer: &mut InputBucketBuffer,
    now: chrono::DateTime<chrono::Utc>,
    vkey: u16,
    is_key_down: bool,
) {
    if is_key_down {
        if inputs.w.pressed_keys_state.insert(vkey) {
            input_buffer.record_key_press(now);
        }
    } else {
        inputs.w.pressed_keys_state.remove(&vkey);
    }
}

fn apply_relative_mouse_move(
    input_buffer: &mut InputBucketBuffer,
    now: chrono::DateTime<chrono::Utc>,
    mouse_dpi: f64,
    dx: f64,
    dy: f64,
) {
    input_buffer.record_mouse_distance_cm(now, relative_counts_to_centimeters(dx, dy, mouse_dpi));
}

fn apply_absolute_mouse_move(
    inputs: &mut InputLogger,
    input_buffer: &mut InputBucketBuffer,
    now: chrono::DateTime<chrono::Utc>,
    current_x: i32,
    current_y: i32,
) {
    if let (Some(last_x), Some(last_y)) = (inputs.w.last_abs_x, inputs.w.last_abs_y) {
        let raw_dx = (current_x - last_x) as f64;
        let raw_dy = (current_y - last_y) as f64;

        let dx_mm = raw_dx * (inputs.w.screen_width_mm / 65535.0);
        let dy_mm = raw_dy * (inputs.w.screen_height_mm / 65535.0);
        let dist_cm = millimeters_to_centimeters(dx_mm, dy_mm);
        const JITTER_THRESHOLD_MM: f64 = 0.3;
        if dist_cm > JITTER_THRESHOLD_MM / 10.0 {
            input_buffer.record_mouse_distance_cm(now, dist_cm);
        }
    }

    inputs.w.last_abs_x = Some(current_x);
    inputs.w.last_abs_y = Some(current_y);
}

fn apply_mouse_buttons(
    input_buffer: &mut InputBucketBuffer,
    now: chrono::DateTime<chrono::Utc>,
    button_flags: u16,
) {
    if (button_flags & RI_MOUSE_LEFT_BUTTON_DOWN as u16) != 0 {
        input_buffer.record_left_click(now);
    }
    if (button_flags & RI_MOUSE_RIGHT_BUTTON_DOWN as u16) != 0 {
        input_buffer.record_right_click(now);
    }
    if (button_flags & RI_MOUSE_MIDDLE_BUTTON_DOWN as u16) != 0 {
        input_buffer.record_middle_click(now);
    }
}

fn apply_vertical_wheel(
    input_buffer: &mut InputBucketBuffer,
    now: chrono::DateTime<chrono::Utc>,
    delta: i16,
) {
    let distance_cm = scroll_steps_to_centimeters(delta as f64 / 120.0);
    input_buffer.record_vertical_scroll_cm(now, distance_cm);
}

fn apply_horizontal_wheel(
    input_buffer: &mut InputBucketBuffer,
    now: chrono::DateTime<chrono::Utc>,
    delta: i16,
) {
    let distance_cm = scroll_steps_to_centimeters(delta as f64 / 120.0);
    input_buffer.record_horizontal_scroll_cm(now, distance_cm);
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn empty_input_logger() -> InputLogger {
        InputLogger::default()
    }

    #[test]
    fn keyboard_event_counts_unique_keydown_only_once() {
        let mut inputs = empty_input_logger();
        let mut buffer = InputBucketBuffer::new(DEFAULT_SOURCE_ID, DEFAULT_BUCKET_MINUTES as u32);
        let now = Utc.with_ymd_and_hms(2026, 4, 18, 12, 0, 0).unwrap();

        apply_keyboard_event(&mut inputs, &mut buffer, now, 65, true);
        apply_keyboard_event(&mut inputs, &mut buffer, now, 65, true);
        apply_keyboard_event(&mut inputs, &mut buffer, now, 65, false);

        let rows = buffer.drain();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].key_presses, 1);
    }

    #[test]
    fn keyboard_event_counts_a_new_press_after_release() {
        let mut inputs = empty_input_logger();
        let mut buffer = InputBucketBuffer::new(DEFAULT_SOURCE_ID, DEFAULT_BUCKET_MINUTES as u32);
        let now = Utc.with_ymd_and_hms(2026, 4, 18, 12, 0, 0).unwrap();

        apply_keyboard_event(&mut inputs, &mut buffer, now, 65, true);
        apply_keyboard_event(&mut inputs, &mut buffer, now, 65, false);
        apply_keyboard_event(&mut inputs, &mut buffer, now, 65, true);

        let rows = buffer.drain();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].key_presses, 2);
    }

    #[test]
    fn relative_mouse_move_converts_distance_to_centimeters() {
        let mut buffer = InputBucketBuffer::new(DEFAULT_SOURCE_ID, DEFAULT_BUCKET_MINUTES as u32);
        let now = Utc.with_ymd_and_hms(2026, 4, 18, 12, 0, 0).unwrap();

        apply_relative_mouse_move(&mut buffer, now, 800.0, 3.0, 4.0);

        let rows = buffer.drain();
        assert_eq!(rows.len(), 1);
        assert!((rows[0].mouse_distance_cm - (5.0 / 800.0 * 2.54)).abs() < 1e-6);
    }

    #[test]
    fn absolute_mouse_move_ignores_small_jitter_and_tracks_real_motion() {
        let mut inputs = empty_input_logger();
        inputs.w.screen_width_mm = 500.0;
        inputs.w.screen_height_mm = 300.0;
        let mut buffer = InputBucketBuffer::new(DEFAULT_SOURCE_ID, DEFAULT_BUCKET_MINUTES as u32);
        let now = Utc.with_ymd_and_hms(2026, 4, 18, 12, 0, 0).unwrap();

        apply_absolute_mouse_move(&mut inputs, &mut buffer, now, 100, 100);
        apply_absolute_mouse_move(&mut inputs, &mut buffer, now, 101, 101);
        assert!(buffer.drain().is_empty());

        apply_absolute_mouse_move(&mut inputs, &mut buffer, now, 1000, 1000);
        let rows = buffer.drain();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].mouse_distance_cm > 0.0);
    }

    #[test]
    fn button_and_wheel_helpers_record_bucket_metrics() {
        let now = Utc.with_ymd_and_hms(2026, 4, 18, 12, 0, 0).unwrap();
        let mut buffer = InputBucketBuffer::new(DEFAULT_SOURCE_ID, DEFAULT_BUCKET_MINUTES as u32);
        apply_mouse_buttons(
            &mut buffer,
            now,
            RI_MOUSE_LEFT_BUTTON_DOWN as u16
                | RI_MOUSE_RIGHT_BUTTON_DOWN as u16
                | RI_MOUSE_MIDDLE_BUTTON_DOWN as u16,
        );
        apply_vertical_wheel(&mut buffer, now, 240);
        apply_horizontal_wheel(&mut buffer, now, 120);

        let rows = buffer.drain();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].left_clicks, 1);
        assert_eq!(rows[0].right_clicks, 1);
        assert_eq!(rows[0].middle_clicks, 1);
        assert!((rows[0].scroll_vertical_cm - 0.8).abs() < 1e-6);
        assert!((rows[0].scroll_horizontal_cm - 0.4).abs() < 1e-6);
    }
}

fn run_message_loop(tx: mpsc::Sender<RawInputEvent>) -> Result<()> {
    unsafe {
        let hinstance = GetModuleHandleW(None)?;
        let class_name = w!("RawInputSinkWindowClass");

        let wc = WNDCLASSW {
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinstance.into(),
            lpszClassName: class_name,
            ..Default::default()
        };

        if RegisterClassW(&wc) == 0 {
            // Class already registered is not a fatal error.
            if Error::from_win32().code() != HRESULT(1410) {
                return Err(anyhow::anyhow!(Error::from_win32()))
                    .with_context(|| "Failed to register window class");
            }
        }

        // This is needed  so we can access it inside wnd_proc function.
        let tx_ptr = Box::into_raw(Box::new(tx));

        // create a message-only window.
        let hwnd = CreateWindowExW(
            Default::default(),
            class_name,
            w!("RawInputMessageWindow"),
            Default::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(hinstance.into()),
            Some(tx_ptr as *mut c_void),
        )?;

        if hwnd.is_invalid() {
            let _ = Box::from_raw(tx_ptr);
            return Err(anyhow::anyhow!(Error::from_win32()))
                .with_context(|| "CreateWindowExW failed");
        }

        let devices = [
            RAWINPUTDEVICE {
                usUsagePage: 1, // Generic Desktop
                usUsage: 6,     // Keyboard
                dwFlags: RIDEV_INPUTSINK,
                hwndTarget: hwnd,
            },
            RAWINPUTDEVICE {
                usUsagePage: 1, // Generic Desktop
                usUsage: 2,     // Mouse
                dwFlags: RIDEV_INPUTSINK,
                hwndTarget: hwnd,
            },
        ];

        RegisterRawInputDevices(&devices, size_of::<RAWINPUTDEVICE>() as u32)?;
        info!("Win32 message loop running...");
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        info!("Win32 message loop finished.");
    }
    anyhow::bail!("Win32 raw input message loop stopped")
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        // WM_CREATE is called when the window is first created.
        WM_CREATE => {
            let create_struct = &*(lparam.0 as *const CREATESTRUCTW);
            let tx_ptr = create_struct.lpCreateParams;
            // Store the pointer in the window's user data area for later access.
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, tx_ptr as isize);
            LRESULT(0)
        }

        WM_INPUT => {
            // Retrieve the sender pointer we stored earlier.
            let tx_ptr =
                GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const mpsc::Sender<RawInputEvent>;
            if !tx_ptr.is_null() {
                let tx = &*tx_ptr;
                if let Err(e) = handle_raw_input(HRAWINPUT(lparam.0 as _), tx) {
                    error!("Failed to handle raw input: {:?}", e);
                }
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }

        WM_DESTROY => {
            info!("Destroying window and cleaning up sender...");
            let tx_ptr =
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) as *mut mpsc::Sender<RawInputEvent>;
            if !tx_ptr.is_null() {
                let _ = Box::from_raw(tx_ptr);
            }
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn handle_raw_input(handle: HRAWINPUT, tx: &mpsc::Sender<RawInputEvent>) -> Result<()> {
    let mut size: u32 = 0;
    GetRawInputData(
        handle,
        RID_INPUT,
        None,
        &mut size,
        size_of::<RAWINPUTHEADER>() as u32,
    );
    if size == 0 {
        return Ok(());
    }

    let mut buf = vec![0u8; size as usize];
    let bytes_written = GetRawInputData(
        handle,
        RID_INPUT,
        Some(buf.as_mut_ptr() as *mut _),
        &mut size,
        size_of::<RAWINPUTHEADER>() as u32,
    );

    if bytes_written == u32::MAX || bytes_written == 0 {
        return Err(anyhow::anyhow!("GetRawInputData (data fetch) failed"));
    }

    let raw = &*(buf.as_ptr() as *const RAWINPUT);

    let event = match raw.header.dwType {
        1 => {
            // RIM_TYPEKEYBOARD
            let kb = raw.data.keyboard;
            Some(RawInputEvent::Keyboard(kb))
        }
        0 => {
            // RIM_TYPEMOUSE
            let mouse = raw.data.mouse;
            Some(RawInputEvent::Mouse(mouse))
        }
        _ => None,
    };

    if let Some(e) = event {
        if let Err(send_err) = tx.try_send(e) {
            warn!(
                "Failed to send raw input event, channel may be full: {}",
                send_err
            );
        }
    }

    Ok(())
}
