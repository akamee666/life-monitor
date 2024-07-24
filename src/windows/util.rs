use std::time::Duration;
use windows::{
    Win32::System::SystemInformation::GetTickCount,
    Win32::UI::{
        Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO},
        WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId},
    },
};

// god please bless the man who write this blog.
// https://hellocode.co/blog/post/tracking-active-process-windows-rust/
pub fn get_active_window() -> (u32, String) {
    unsafe {
        // That will give me a handle to the active window.
        let hwnd = GetForegroundWindow();
        let mut pid: u32 = 0;

        // retrieves the pid/indentifier of the thread that created that window.
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let mut bytes: [u16; 500] = [0; 500];
        let len = GetWindowTextW(hwnd, &mut bytes);
        let title = String::from_utf16_lossy(&bytes[..len as usize]);
        (pid, title)
    }
}

pub fn get_last_input_time() -> Duration {
    // Retrieves the number of milliseconds that have elapsed since the system was started, up to 49.7 days.
    // we will be using it to get how much time was went since the last user input
    let tick_count = unsafe { GetTickCount() };

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
