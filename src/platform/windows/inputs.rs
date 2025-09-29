use crate::common::*;
use crate::storage::backend::*;
use crate::StorageBackend;

use anyhow::Context;
use std::ffi::c_void;
use std::mem::size_of;
use tokio::sync::mpsc;
use tokio::sync::mpsc::channel;
use tokio::time::*;
use tracing::*;

use windows::core::{w, Error, HRESULT, PCWSTR};
use windows::Win32::Devices::HumanInterfaceDevice::HidD_GetProductString;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::{
    GetRawInputData, GetRawInputDeviceInfoW, GetRawInputDeviceList, RegisterRawInputDevices,
    HRAWINPUT, MOUSE_MOVE_RELATIVE, RAWINPUT, RAWINPUTDEVICE, RAWINPUTDEVICELIST, RAWINPUTHEADER,
    RIDEV_INPUTSINK, RIDI_DEVICENAME, RID_INPUT,
};
use windows::Win32::UI::WindowsAndMessaging::*;

/// List all raw input devices
fn list_raw_input_devices() -> anyhow::Result<Vec<RAWINPUTDEVICELIST>> {
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

#[derive(Debug, Clone, Copy)]
pub enum RawInputEvent {
    Keyboard {
        vkey: u16,
        flags: u16,
        message: u32,
    },
    Mouse {
        flags: u16,
        button_flags: u16,
        button_data: u16,
        x: i32,
        y: i32,
    },
}

unsafe fn get_human_readable_name(device_handle: HANDLE) -> anyhow::Result<String> {
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

pub async fn run(
    dpi: Option<u32>,
    update_interval: u32,
    backend: StorageBackend,
) -> anyhow::Result<()> {
    let mut inputs_data = InputLogger::new(&backend, dpi.unwrap_or(800))
        .await
        .with_context(|| {
            "Failed to initialize Inputs runtime because could not create InputLogger data"
        })?;

    let (events_tx, mut events_rx) = channel::<RawInputEvent>(256);
    let mut db_updates = interval(Duration::from_secs(update_interval as u64));

    // Log devices
    for (i, dev) in list_raw_input_devices()?.into_iter().enumerate() {
        let name = unsafe { get_human_readable_name(dev.hDevice) }
            .unwrap_or_else(|e| format!("[Error: {e}]"));
        debug!("Device {i}: type={:?}, name={}", dev.dwType, name);
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
                process_event(&mut inputs_data, event);
            }

            _ = db_updates.tick() => {
                if inputs_data.mouse_dpi > 0 {
                    const INCHES_TO_CM: f64 = 2.54;
                    inputs_data.cm_traveled = (inputs_data.pixels_traveled as f64 / inputs_data.mouse_dpi as f64 * INCHES_TO_CM) as u64;
                }

                if let Err(e) = backend.store_keys_data(&inputs_data).await {
                    error!("Failed to store inputs data in backend: {:?}", e);
                }
            }

            else => {
                error!("Event source channel closed. Shutting down.");
                break;
            }
        }
    }
    anyhow::bail!("Message Loop finished unexpectly!")
}

fn process_event(inputs: &mut InputLogger, event: RawInputEvent) {
    match event {
        RawInputEvent::Keyboard { flags, .. } if flags == RI_KEY_MAKE as u16 => {
            inputs.key_presses += 1;
        }
        RawInputEvent::Mouse {
            button_flags,
            button_data,
            x,
            y,
            flags,
        } => {
            if flags == MOUSE_MOVE_RELATIVE.0 {
                let dx = x as f64;
                let dy = y as f64;
                let dist = (dx * dx + dy * dy).sqrt();
                inputs.pixels_traveled += dist as u64;
            }
            if button_flags as u32 & RI_MOUSE_LEFT_BUTTON_DOWN != 0 {
                inputs.left_clicks += 1;
            }
            if button_flags as u32 & RI_MOUSE_RIGHT_BUTTON_DOWN != 0 {
                inputs.right_clicks += 1;
            }
            if button_flags as u32 & RI_MOUSE_MIDDLE_BUTTON_DOWN != 0 {
                inputs.middle_clicks += 1;
            }

            if button_flags as u32 & RI_MOUSE_WHEEL != 0 {
                let delta = button_data as i16;
                inputs.vertical_scroll_clicks += (delta.abs() as u32 / WHEEL_DELTA as u32) as u64;
            }
        }
        _ => {}
    }
}

fn run_message_loop(tx: mpsc::Sender<RawInputEvent>) -> anyhow::Result<()> {
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
    anyhow::bail!("");
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

unsafe fn handle_raw_input(
    handle: HRAWINPUT,
    tx: &mpsc::Sender<RawInputEvent>,
) -> anyhow::Result<()> {
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
            Some(RawInputEvent::Keyboard {
                vkey: kb.VKey,
                flags: kb.Flags,
                message: kb.Message,
            })
        }
        0 => {
            // RIM_TYPEMOUSE
            let mouse = raw.data.mouse;
            Some(RawInputEvent::Mouse {
                flags: mouse.usFlags.0,
                button_flags: mouse.Anonymous.Anonymous.usButtonFlags,
                button_data: mouse.Anonymous.Anonymous.usButtonData,
                x: mouse.lLastX,
                y: mouse.lLastY,
            })
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
