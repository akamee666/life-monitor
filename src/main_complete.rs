/*
* The main purpose of this project is to create a program that will monitor the amount of times
* that i use the outputs of my setup, my keyboard, mouse and also monitor what i'm doing daily.
* The whole point of this is to create some graphs in an personal blog as i explained in README.md
*/

#![windows_subsystem = "windows"]
mod keylogger;

#[cfg(target_os = "linux")]
mod linux;

mod time_tracking;
#[cfg(target_os = "windows")]
mod win;

/*
*   This is actually my first time writing parallel code or having more than one thread going on so
*   i went pretty much knowing nothing and i reach a point that is all working but i dont like that
*   way that the code is struct cause is becoming quite hard to add new things and refactor other
*   code parts. I'm not a hundred percentage sure but i think there is better ways to do what i'm
*   doing so the next tasks is actually go through the tokio documentation and see if i can figure
*   out better ways to do what i want.
*
*/

#[tokio::main]
async fn main() {
    let connection = sqlite::open(":memory:").unwrap();
    let create_query = "create table keys_input (key text, press_count integer);";
    connection.execute(create_query).unwrap();
    if false {
        let query = "SELECT * FROM keys_input";
        println!("printing database.");
        connection
            .iterate(query, |pairs| {
                for &(name, value) in pairs.iter() {
                    println!("{} = {}", name, value.unwrap());
                }
                true
            })
            .unwrap();
    }

    #[cfg(target_os = "linux")]
    {
        // that doesn't mean that we are starting this code in a different thread.
        tokio::spawn(crate::keylogger::KeyLogger::init());
    }

    #[cfg(target_os = "windows")]
    {
        tokio::spawn(crate::keylogger::KeyLogger::init());
        tokio::spawn(crate::win::systray::init());
    }

    time_tracking::start_tracking();
}
