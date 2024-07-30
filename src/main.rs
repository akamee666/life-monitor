/*
* The main purpose of this project is to create a program that will monitor the amount of times
* that i use the outputs of my setup, my keyboard, mouse and also monitor what i'm doing daily.
* The whole point of this is to create some graphs in an personal blog as i explained in README.md
*/
//#![windows_subsystem = "windows"]
mod keylogger;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "windows")]
mod win;

#[tokio::main]
async fn main() {
    println!("By now the program does not too much, it capture the active window each five seconds and display the amount of times that you have used you keyboard/mouse since the program had started.");

    #[cfg(target_os = "linux")]
    {
        tokio::spawn(crate::keylogger::KeyLogger::init());
        tokio::spawn(crate::linux::process::track_processes()).await;
    }

    #[cfg(target_os = "windows")]
    {
        tokio::spawn(crate::keylogger::KeyLogger::init());
        tokio::spawn(crate::win::systray::init());
        let _ = tokio::spawn(win::process::track_processes()).await;
    }
}
