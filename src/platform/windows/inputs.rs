use crate::StorageBackend;
use anyhow::Context;
use std::mem::size_of;
use tokio::sync::mpsc;

use tracing::*;
use windows::core::w;
use windows::core::{Error, HRESULT, PCWSTR};
use windows::Win32::Devices::HumanInterfaceDevice::HidD_GetProductString;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, LRESULT, WPARAM};

use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, OPEN_EXISTING};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::{
    GetRawInputData, GetRawInputDeviceInfoW, GetRawInputDeviceList, RegisterRawInputDevices,
    HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTDEVICELIST, RAWINPUTHEADER, RIDEV_INPUTSINK,
    RIDI_DEVICENAME, RID_INPUT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, PostQuitMessage,
    RegisterClassW, TranslateMessage, HWND_MESSAGE, MSG, WM_DESTROY, WM_INPUT, WNDCLASSW,
};

#[derive(Debug)]
pub enum RawInputEvent {
    Keyboard {
        vkey: u16,
        flags: u16,
        message: u32,
    },
    Mouse {
        flags: u16,
        button_flags: u16,
        x: i32,
        y: i32,
    },
}

use windows::Win32::Foundation::HANDLE;

unsafe fn get_human_readable_name(device_handle: HANDLE) -> anyhow::Result<String> {
    let mut size: u32 = 0;
    if GetRawInputDeviceInfoW(Some(device_handle), RIDI_DEVICENAME, None, &mut size) != 0 {
        return Err(anyhow::anyhow!(
            "GetRawInputDeviceInfoW (size query) failed"
        ));
    }
    if size == 0 {
        return Err(anyhow::anyhow!("Device name is empty"));
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
        0,
        FILE_SHARE_READ,
        None,
        OPEN_EXISTING,
        Default::default(),
        None,
    )?;

    let mut name_buffer: Vec<u16> = vec![0; 127];

    let result = HidD_GetProductString(
        hid_handle,
        name_buffer.as_mut_ptr() as *mut _,
        (name_buffer.len() * size_of::<u16>()) as u32,
    );

    CloseHandle(hid_handle)?;

    if !result {
        return Ok("[Not a HID or no product string]".to_string());
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
    let (tx, mut rx) = mpsc::channel::<RawInputEvent>(128);

    unsafe {
        let devices: Vec<RAWINPUTDEVICELIST> = {
            let mut num_devices: u32 = 0;
            let rid_list_size: u32 = size_of::<RAWINPUTDEVICELIST>() as u32;

            GetRawInputDeviceList(None, &mut num_devices, rid_list_size);

            if num_devices == 0 {
                println!("No raw input devices found.");
            };

            let mut device_vec: Vec<RAWINPUTDEVICELIST> = Vec::with_capacity(num_devices as usize);

            let devices_written = GetRawInputDeviceList(
                Some(device_vec.as_mut_ptr()),
                &mut num_devices,
                rid_list_size,
            );

            if devices_written == u32::MAX {
                return Err(anyhow::anyhow!(
                    "Failed to get raw input device list: {}",
                    Error::from_win32()
                ));
            }

            device_vec.set_len(devices_written as usize);
            device_vec
        };

        for (i, device) in devices.iter().enumerate() {
            let product_name = match get_human_readable_name(device.hDevice) {
                Ok(name) => name,
                Err(e) => format!("[Error getting name: {}]", e),
            };

            println!(
                "Device {}: Type={:?}, Name: {}",
                i, device.dwType, product_name
            );
        }
    }

    tokio::task::spawn_blocking(|| {
        if let Err(e) = run_message_loop(tx) {
            error!("Win32 message loop thread failed: {:?}", e);
        }
    });

    info!("Raw input thread started. Listening for events...");

    // TOOD: IMPLEMENT THIS LATER not sure how bc hwnd_proc does not receive any klidna of data
    // maybe global?
    while let Some(event) = rx.recv().await {
        info!("Received event");
        match event {
            RawInputEvent::Keyboard {
                vkey,
                flags,
                message,
            } => {
                println!(
                    "ASYNC - Keyboard Event: VKey={}, Flags={:#x}, Message={}",
                    vkey, flags, message
                );
            }
            RawInputEvent::Mouse {
                flags,
                button_flags,
                x,
                y,
            } => {
                println!(
                    "ASYNC - Mouse Event: Flags={:#x}, ButtonFlags={:#x}, X={}, Y={}",
                    flags, button_flags, x, y
                );
            }
        }
    }

    Ok(())
}

fn run_message_loop(_: mpsc::Sender<RawInputEvent>) -> anyhow::Result<()> {
    unsafe {
        let hinstance = GetModuleHandleW(None)?;
        let class_name = w!("RawInputSink");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinstance.into(),
            lpszClassName: class_name,
            ..Default::default()
        };

        if RegisterClassW(&wc) == 0 {
            if Error::from_win32().code() == HRESULT(1410) {
                info!("Already exist");
            } else {
                return Err(anyhow::anyhow!(Error::from_win32()))
                    .with_context(|| "failed to register class");
            }
        }

        let hwnd = CreateWindowExW(
            Default::default(),
            class_name,
            w!(""),
            Default::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(hinstance.into()),
            None,
        )?;

        if hwnd.is_invalid() {
            let error = Error::from_win32();
            return Err(anyhow::anyhow!(error)).with_context(|| "CreateWindowExW failed");
        }

        let devices = [
            RAWINPUTDEVICE {
                usUsagePage: 1,
                usUsage: 6,
                dwFlags: RIDEV_INPUTSINK,
                hwndTarget: hwnd,
            },
            RAWINPUTDEVICE {
                usUsagePage: 1,
                usUsage: 2,
                dwFlags: RIDEV_INPUTSINK,
                hwndTarget: hwnd,
            },
        ];

        RegisterRawInputDevices(&devices, size_of::<RAWINPUTDEVICE>() as u32)?;
        info!("devices registered!");
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_INPUT => {
            info!("INPUT");
            if let Err(e) = handle_raw_input(HRAWINPUT(lparam.0 as _)) {
                eprintln!("Failed to handle raw input: {:?}", e);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            info!("Quit message!");
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => {
            info!("Def Window ProcW!");
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
    }
}

unsafe fn handle_raw_input(handle: HRAWINPUT) -> anyhow::Result<()> {
    let mut size: u32 = 0;

    if GetRawInputData(
        handle,
        RID_INPUT,
        None,
        &mut size,
        size_of::<RAWINPUTHEADER>() as u32,
    ) != 0
    {
        return Err(anyhow::anyhow!("GetRawInputData (size query) failed"));
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
            let kb = raw.data.keyboard;
            Some(RawInputEvent::Keyboard {
                vkey: kb.VKey,
                flags: kb.Flags,
                message: kb.Message,
            })
        }
        0 => {
            let mouse = raw.data.mouse;
            Some(RawInputEvent::Mouse {
                flags: mouse.usFlags.0,
                button_flags: mouse.Anonymous.Anonymous.usButtonFlags,
                x: mouse.lLastX,
                y: mouse.lLastY,
            })
        }
        _ => None,
    };

    println!("event: {:#?}", event.unwrap());

    Ok(())
}
