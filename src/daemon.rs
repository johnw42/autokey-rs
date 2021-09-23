use lazy_static::lazy_static;
use log::{error, info, LevelFilter};
use nix::{
    libc::c_int,
    sys::{
        signal::{signal, SigHandler, Signal},
        wait::waitpid,
    },
    unistd::{fork, ForkResult, Pid},
};
use std::{convert::TryFrom, panic, sync::Mutex, thread::sleep, time::Duration, time::SystemTime};
use syslog::{BasicLogger, Formatter3164};

enum State {
    StartingChild,

    /// The parent is waiting for the child process to terminate.
    MonitoringChild(Pid),

    /// The parent process has been asked to shut down.
    ShuttingDown,
}

lazy_static! {
    static ref STATE: Mutex<State> = Mutex::new(State::StartingChild);
}

extern "C" fn on_signal(signal: c_int) {
    let signal = Signal::try_from(signal).expect("invalid signal");
    let mut state = STATE.lock().expect("failed to acquire lock");
    if let State::MonitoringChild(child) = *state {
        // Pass signals on to the child.
        let _ = nix::sys::signal::kill(child, signal);
    }
    *state = if signal == Signal::SIGTERM {
        State::ShuttingDown
    } else {
        State::StartingChild
    };
}

/// Execute the function `run` in a separate process which is restarted if it
/// dies. Prior to invoking `run`, `init` is called in the parent process.
pub fn run_as_daemon<InitData, InitFn, RunFn>(
    init: InitFn,
    run: RunFn,
    formatter: Option<Formatter3164>,
) -> Result<(), String>
where
    InitFn: Fn() -> Result<InitData, String>,
    RunFn: FnOnce(InitData),
{
    // Log to syslog if requested.
    if let Some(formatter) = formatter {
        let logger = syslog::unix(formatter).unwrap();
        log::set_boxed_logger(Box::new(BasicLogger::new(logger))).unwrap();
        log::set_max_level(LevelFilter::Info);
    }

    // Make sure panics are logged.
    panic::set_hook(Box::new(|info| {
        let message: &str = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| *s)
            .or_else(|| info.payload().downcast_ref::<String>().map(|s| s.as_str()))
            .unwrap_or("(no message)");
        if let Some(loc) = info.location() {
            error!("panic at {}:{}: {}", loc.file(), loc.line(), message);
        } else {
            error!("panic: {}", message);
        }
    }));

    let mut init_data = init()?;
    let min_sleep_time = Duration::from_millis(50);

    // This is how long to wait before trying to restart the child when it dies.
    // Initially this is a short time, but if the child keeps dying right away,
    // the time is increased exponentially to avoid restarting the child too
    // many times if it's broken.
    let mut sleep_time = min_sleep_time;

    loop {
        let mut state = STATE.lock().unwrap();
        match *state {
            State::StartingChild => {}
            State::MonitoringChild(_) => {
                // The child died while it was being monitored.  Increase the
                // delay before re-starting it.
                sleep(sleep_time);
                info!("increasing delay before restart");
                sleep_time *= 2;
            }
            State::ShuttingDown => {
                // The daemon process was killed by the user.
                break;
            }
        }
        match unsafe { fork() }.unwrap() {
            ForkResult::Parent { child } => {
                *state = State::MonitoringChild(child);
                drop(state);

                // Install signal handlers for the case where the parent process
                // is deliberately killed.
                unsafe {
                    let msg = "failed to install signal handler";
                    signal(Signal::SIGQUIT, SigHandler::Handler(on_signal)).expect(msg);
                    signal(Signal::SIGTERM, SigHandler::Handler(on_signal)).expect(msg);
                }

                info!("monitoring child {}", child);
                let start_time = SystemTime::now();

                // Run the child until it terminates.
                while let Err(err) = waitpid(child, None) {
                    error!("error waiting for child: {:?}", err);
                }

                match SystemTime::now().duration_since(start_time) {
                    Ok(t) if t > sleep_time => {
                        // The child didn't die right away (for some definition
                        // of "right away"), so we assume it's in a healthy
                        // enough state that if it dies again, we don't need to
                        // wait a long time before restarting it.
                        sleep_time = min_sleep_time;
                    }
                    _ => {}
                }

                // Try to re-read init data for when child is restarted.  If
                // this step fails, just keep using the old init data.
                match init() {
                    Ok(data) => init_data = data,
                    Err(msg) => error!("{}", msg),
                }
            }
            ForkResult::Child => {
                drop(state);
                // Clear any signal handlers the parent installed.
                unsafe {
                    let msg = "failed to clear signal handler";
                    signal(Signal::SIGQUIT, SigHandler::SigDfl).expect(msg);
                    signal(Signal::SIGTERM, SigHandler::SigDfl).expect(msg);
                }
                run(init_data);
                break;
            }
        }
    }
    Ok(())
}
