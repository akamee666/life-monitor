// TODO: Implement windows key logger
use crate::common::*;
use crate::storage::backend::StorageBackend;

use std::time::Duration;
use tracing::*;

use anyhow::{Context, Result};
use windows::core::Error;

use tokio::sync::mpsc::channel;

use windows::Win32::UI::Input::*;

use windows::Win32::Devices::DeviceAndDriverInstallation::SetupDiGetCustomDevicePropertyW;

use anyhow::*;

pub async fn run(dpi: Option<u32>, update_interval: u32, backend: StorageBackend) -> Result<()> {
    let mut num_devices: u32 = 0;
    let rid_list_size = std::mem::size_of::<RAWINPUTDEVICELIST>() as u32;
    unsafe {
        if GetRawInputDeviceList(None, &mut num_devices, rid_list_size) != 0 {
            return Err(anyhow!(Error::from_win32()));
        };

        if num_devices == 0 {
            println!("No raw input devices found.");
            return Ok(());
        }

        let mut devices: Vec<RAWINPUTDEVICELIST> = Vec::with_capacity(num_devices as usize);
        let devices_ret =
            GetRawInputDeviceList(Some(devices.as_mut_ptr()), &mut num_devices, rid_list_size);

        if devices_ret == u32::MAX {
            error!("Failed to get raw input device list");
            return Err(anyhow!(Error::from_win32()));
        }

        devices.set_len(devices_ret as usize);

        for (i, device) in devices.iter().enumerate() {
            // RIM_TYPEHID
            // 2
            // The device is an HID that is not a keyboard and not a mouse.
            // RIM_TYPEKEYBOARD
            // 1
            // The device is a keyboard.
            // RIM_TYPEMOUSE
            // 0
            // The device is a mouse.

            let mut size = 0;
            let result =
                GetRawInputDeviceInfoW(Some(device.hDevice), RIDI_DEVICENAME, None, &mut size);
            if result == u32::MAX {
                error!("Failed to get raw input device name size");
                return Err(anyhow!(Error::from_win32()));
            }

            let mut name_buffer: Vec<u16> = vec![0; size as usize];

            let result = GetRawInputDeviceInfoW(
                Some(device.hDevice),
                RIDI_DEVICENAME,
                Some(name_buffer.as_mut_ptr() as *mut _),
                &mut size,
            );
            if result == u32::MAX {
                error!("Failed to get raw input device name");
                return Err(anyhow!(Error::from_win32()));
            }

            let str = String::from_utf16_lossy(&name_buffer);

            println!(
                "  Device {}: Type={}, Handle={:?}, Name: {}",
                i, device.dwType.0, device.hDevice, str
            );
        }
    }

    // Get name
    // GetRawInputDeviceInfo();
    //  Get file descriptor daidaij
    // RegisterRawInputDevices();
    // WM_INPUT receive input
    //
    let mut inputs_data = InputLogger::new(&backend, dpi.unwrap_or(800))
        .await
        .with_context(|| {
            "Failed to initialize Inputs runtime because could not create InputLogger data"
        })?;

    let (tasks_tx, mut tasks_rx) = channel::<Signals>(32);

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

    let idle = Duration::from_secs(20);
    // loop {}

    Ok(())
}
