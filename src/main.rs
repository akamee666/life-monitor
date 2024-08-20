// #![windows_subsystem = "windows"]
use env_logger;
use log::*;

pub mod db;
mod keylogger;
mod shutdown;
mod win;

use db::upload_data_to_db;
use std::ffi::OsString;
use std::time::Duration;
use windows_service::service::{
    Service, ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
    ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_dispatcher;
#[macro_use]
extern crate windows_service;

define_windows_service!(ffi_service_main, service_main);

#[tokio::main]
async fn main() -> Result<(), windows_service::Error> {
    env_logger::init();
    info!("Starting program");

    // Register generated `ffi_service_main` with the system and start the service, blocking
    // this thread until the service is stopped.
    //
    // sc create my_service binPath= "C:\path\to\your\service.exe"
    // sc start my_service
    service_dispatcher::start("akame_monitor", ffi_service_main)?;
    // TODO: FIX BUGS, SOMETIMES PROCESSES JUST STOPING TRACKING.
    // TODO: FIND WHERE MY CPU IS GOING CRAZY.
    tokio::spawn(crate::win::systray::init());
    tokio::spawn(crate::win::process::ProcessTracker::track_processes());
    tokio::spawn(crate::keylogger::KeyLogger::start_logging());

    Ok(())
}

// https://github.com/mullvad/windows-service-rs?tab=readme-ov-file#readme
fn service_main(arguments: Vec<OsString>) -> Result<(), windows_service::Error> {
    // The entry point where execution will start on a background thread after a call to
    // `service_dispatcher::start` from `main`.

    // TODO: FOLLOW THE EXAMPLES OF THIS PAGE HERE TO FIX THAT!!!
    // https://raw.githubusercontent.com/mullvad/windows-service-rs/main/examples/ping_service.rs
    let event_handler = move |control_event| -> ServiceControlHandlerResult {
        match control_event {
            ServiceControl::Stop => {
                info!("Stopping service...");

                let conn = upload_data_to_db();
                match conn {
                    Ok(conn) => conn,
                    Err(e) => {
                        info!("Could not send data to database {e:?}")
                    }
                }

                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Shutdown => {
                info!("System shutdown detected...");

                let conn = upload_data_to_db();
                match conn {
                    Ok(conn) => conn,
                    Err(e) => {
                        info!("Could not send data to database {e:?}")
                    }
                }

                ServiceControlHandlerResult::NoError
            }
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    // Register system service event handler
    let status_handle = service_control_handler::register("akame_monitor", event_handler)?;

    let next_status = ServiceStatus {
        // Should match the one from system service registry
        service_type: ServiceType::OWN_PROCESS,
        // The new state
        current_state: ServiceState::Running,
        // Accept stop events when running
        controls_accepted: ServiceControlAccept::STOP,
        // Used to report an error when starting or stopping only, otherwise must be zero
        exit_code: ServiceExitCode::Win32(0),
        // Only used for pending states, otherwise must be zero
        checkpoint: 0,
        // Only used for pending states, otherwise must be zero
        wait_hint: Duration::default(),
        // Unused for setting status
        process_id: None,
    };

    // Tell the system that the service is running now
    status_handle.set_service_status(next_status)?;

    info!("service running");

    #[allow(unreachable_code)]
    Ok(())
}
