use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::time::Duration;
use sysinfo::{Pid, ProcessRefreshKind, RefreshKind};
use windows::Win32::{
    System::SystemInformation::GetTickCount,
    UI::{
        Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO},
        WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId},
    },
};

// Returns window title and class in that order.
pub fn get_focused_window() -> Result<(String, String), windows::core::Error> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0 == 0 {
            return Err(windows::core::Error::from_win32());
        }

        let mut title: [u16; 256] = [0; 256];
        let title_len = GetWindowTextW(hwnd, &mut title);
        if title_len == 0 {
            return Err(windows::core::Error::from_win32());
        }

        // Convert the title from UTF-16 to String
        let window_title = OsString::from_wide(&title[..title_len as usize])
            .to_string_lossy()
            .into_owned();

        let mut process_pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut process_pid));
        let sys = sysinfo::System::new_with_specifics(
            RefreshKind::new().with_processes(ProcessRefreshKind::new()),
        );
        let proc = sys.processes().get(&Pid::from_u32(process_pid)).unwrap();
        let window_class = proc.name().to_string_lossy().to_string();
        Ok((window_title, window_class))
    }
}
pub fn get_last_input_time() -> Duration {
    // Retrieves the number of milliseconds that have elapsed since the system was started, up to 49.7 days.
    // we will be using it to get how much time was went since the last user input
    let tick_count = unsafe { GetTickCount() };

    // struct defined by windows.
    let mut last_input_info = LASTINPUTINFO {
        cbSize: 8,
        dwTime: 0,
    };

    let p_last_input_info = &mut last_input_info as *mut LASTINPUTINFO;

    let _sucess = unsafe {
        let _ = GetLastInputInfo(p_last_input_info);
    };

    let diff = tick_count - last_input_info.dwTime;
    return Duration::from_millis(diff.into());
}
