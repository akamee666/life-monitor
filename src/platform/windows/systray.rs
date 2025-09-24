use std::process::Command;
use std::thread;
use tracing::debug;

#[allow(non_snake_case)]
pub fn LOWORD(l: usize) -> usize {
    l & 0xffff
}

#[allow(non_snake_case)]
pub fn HIWORD(l: usize) -> usize {
    (l >> 16) & 0xffff
}

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
const ID_GO_TO: u16 = 1001;
const ID_QUIT: u16 = 1002;

pub async fn init() {
    debug!("Spawning native Win32 systray thread");

    thread::spawn(move || unsafe {
        let instance = GetModuleHandleW(None).unwrap();
        let class_name = w!("AKAME_SPY_WND_CLASS");

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: class_name,
            ..Default::default()
        };

        if RegisterClassExW(&wc) == 0 {
            return;
        }

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("akame.666 spy"),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .unwrap();

        if hwnd.0 == std::ptr::null_mut() {
            return;
        }

        let h_icon = LoadIconW(Some(instance.into()), w!("makima_icon"))
            .unwrap_or_else(|_| LoadIconW(None, IDI_APPLICATION).unwrap());

        let mut nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: WM_TRAYICON,
            hIcon: h_icon,
            ..Default::default()
        };
        let tip = "akame.666 spy"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>();
        nid.szTip[..tip.len()].copy_from_slice(&tip);

        Shell_NotifyIconW(NIM_ADD, &nid);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        Shell_NotifyIconW(NIM_DELETE, &nid);
    });
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TRAYICON => {
            if lparam.0 as u32 == WM_RBUTTONUP {
                let mut point = POINT::default();
                GetCursorPos(&mut point);

                let hmenu = CreatePopupMenu().unwrap();
                AppendMenuW(hmenu, MF_STRING, ID_GO_TO as usize, w!("Project Source"));
                AppendMenuW(hmenu, MF_SEPARATOR, 0, None);
                AppendMenuW(hmenu, MF_STRING, ID_QUIT as usize, w!("Quit"));

                SetForegroundWindow(hwnd);

                TrackPopupMenu(
                    hmenu,
                    TPM_BOTTOMALIGN | TPM_LEFTALIGN,
                    point.x,
                    point.y,
                    Some(0),
                    hwnd,
                    None,
                );

                DestroyMenu(hmenu);
            }
            LRESULT(0)
        }

        WM_COMMAND => {
            match LOWORD(wparam.0) as u16 {
                ID_GO_TO => {
                    let _ = Command::new("cmd.exe")
                        .args([
                            "/C",
                            "start",
                            "",
                            "http://www.github.com/akamee666/life-monitor",
                        ])
                        .spawn();
                }
                ID_QUIT => {
                    PostQuitMessage(0);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
