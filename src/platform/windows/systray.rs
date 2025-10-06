use anyhow::{anyhow, Result};
use std::process::Command;
use tracing::*;
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Shell::{
                Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
                NOTIFYICONDATAW,
            },
            WindowsAndMessaging::*,
        },
    },
};

const WM_TRAYICON: u32 = WM_APP + 1;

#[derive(Debug, Clone, Copy)]
enum TrayCommand {
    GoTo = 1001,
    Quit = 1002,
}

pub async fn init_tray() -> Result<()> {
    debug!("Spawning native Win32 systray thread");

    tokio::spawn(async move {
        if let Err(e) = run_tray_loop() {
            error!("Tray loop failed: {e:?}");
        }
    })
    .await?;

    Ok(())
}

fn run_tray_loop() -> Result<()> {
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class_name = w!("LIFE_MONITOR_TRAY");

        register_tray_class(instance.into(), class_name)?;

        let hwnd = create_tray_window(instance.into(), class_name)?;
        let h_icon = load_icon(instance.into());

        let nid = create_notify_icon(hwnd, h_icon)?;
        Shell_NotifyIconW(NIM_ADD, &nid).ok()?;

        info!("System tray running...");

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        Shell_NotifyIconW(NIM_DELETE, &nid).ok()?;
        Ok(())
    }
}

unsafe fn register_tray_class(hinstance: HINSTANCE, class_name: PCWSTR) -> Result<()> {
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(wnd_proc),
        hInstance: hinstance,
        lpszClassName: class_name,
        ..Default::default()
    };

    if RegisterClassExW(&wc) == 0 {
        let err = Error::from_win32();
        if err.code().0 as u32 != ERROR_CLASS_ALREADY_EXISTS.0 {
            return Err(anyhow!("Failed to register class: {err:?}"));
        }
    }
    Ok(())
}

unsafe fn create_tray_window(hinstance: HINSTANCE, class_name: PCWSTR) -> Result<HWND> {
    let hwnd = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        class_name,
        w!("Life Monitor"),
        WS_OVERLAPPEDWINDOW,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        None,
        None,
        Some(hinstance),
        None,
    )?;

    if hwnd.is_invalid() {
        return Err(anyhow!("Failed to create tray window"));
    }
    Ok(hwnd)
}

unsafe fn load_icon(hinstance: HINSTANCE) -> HICON {
    LoadIconW(Some(hinstance), w!("makima_icon"))
        .unwrap_or_else(|_| LoadIconW(None, IDI_APPLICATION).unwrap())
}

unsafe fn create_notify_icon(hwnd: HWND, h_icon: HICON) -> Result<NOTIFYICONDATAW> {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: WM_TRAYICON,
        hIcon: h_icon,
        ..Default::default()
    };

    let tip = "Life Monitor"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<u16>>();
    nid.szTip[..tip.len()].copy_from_slice(&tip);

    Ok(nid)
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        // How do we handle errors here?
        WM_TRAYICON if lparam.0 as u32 == WM_RBUTTONUP => {
            show_context_menu(hwnd).expect("Failed to handle message in systray");
            LRESULT(0)
        }

        WM_COMMAND => {
            handle_command(LOWORD(wparam.0) as u16);
            LRESULT(0)
        }

        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn show_context_menu(hwnd: HWND) -> Result<()> {
    let mut point = POINT::default();
    GetCursorPos(&mut point)?;

    if let Ok(hmenu) = CreatePopupMenu() {
        AppendMenuW(
            hmenu,
            MF_STRING,
            TrayCommand::GoTo as usize,
            w!("Project Source"),
        )?;
        AppendMenuW(hmenu, MF_SEPARATOR, 0, None)?;
        AppendMenuW(hmenu, MF_STRING, TrayCommand::Quit as usize, w!("Quit"))?;

        let _ = SetForegroundWindow(hwnd);
        let _ = TrackPopupMenu(
            hmenu,
            TPM_BOTTOMALIGN | TPM_LEFTALIGN,
            point.x,
            point.y,
            Some(0),
            hwnd,
            None,
        );
        DestroyMenu(hmenu)?;
    }

    Ok(())
}

fn handle_command(cmd: u16) {
    match cmd {
        x if x == TrayCommand::GoTo as u16 => {
            let _ = Command::new("cmd.exe")
                .args([
                    "/C",
                    "start",
                    "",
                    "https://github.com/akamee666/life-monitor",
                ])
                .spawn();
        }
        x if x == TrayCommand::Quit as u16 => unsafe {
            PostQuitMessage(0);
            std::process::exit(0);
        },
        _ => {}
    }
}

#[allow(non_snake_case)]
pub fn LOWORD(l: usize) -> usize {
    l & 0xffff
}
