use lazy_static::lazy_static;
use log::{error, info, LevelFilter};
use nix::{
    libc::c_int,
    sys::{
        signal::{signal, SigHandler, Signal},
        wait::waitpid,
    },
    unistd::{fork, getpid, ForkResult, Pid},
};
use std::{convert::TryFrom, panic, sync::Mutex, thread::sleep, time::Duration, time::SystemTime};
use syslog::{BasicLogger, Facility, Formatter3164};

enum State {
    Starting,
    AwaitingChild(Pid),
    ShuttingDown,
}

lazy_static! {
    static ref STATE: Mutex<State> = Mutex::new(State::Starting);
}

extern "C" fn on_signal(signal: c_int) {
    let signal = Signal::try_from(signal).expect("invalid signal");
    let mut state = STATE.lock().expect("failed to acquire lock");
    if let State::AwaitingChild(child) = *state {
        let _ = nix::sys::signal::kill(child, signal);
    }
    *state = if signal == Signal::SIGTERM {
        State::ShuttingDown
    } else {
        State::Starting
    };
}

pub fn run_as_daemon<T, I, R>(init: I, run: R) -> Result<(), String>
where
    I: Fn() -> Result<T, String>,
    R: FnOnce(T),
{
    // Log to syslog.
    let formatter = Formatter3164 {
        facility: Facility::LOG_USER,
        hostname: None,
        process: "autokey-rs".into(),
        pid: getpid().into(),
    };
    let logger = syslog::unix(formatter).unwrap();
    log::set_boxed_logger(Box::new(BasicLogger::new(logger))).unwrap();
    log::set_max_level(LevelFilter::Info);

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
    let mut sleep_time = min_sleep_time;
    loop {
        let mut state = STATE.lock().unwrap();
        match *state {
            State::Starting => {}
            State::AwaitingChild(_) => {
                sleep(sleep_time);
                sleep_time *= 2;
            }
            State::ShuttingDown => {
                break;
            }
        }
        match unsafe { fork() }.unwrap() {
            ForkResult::Parent { child } => {
                *state = State::AwaitingChild(child);
                drop(state);
                unsafe {
                    let msg = "failed to install signal handler";
                    signal(Signal::SIGQUIT, SigHandler::Handler(on_signal)).expect(msg);
                    signal(Signal::SIGTERM, SigHandler::Handler(on_signal)).expect(msg);
                }
                info!("monitoring child {}", child);
                let start_time = SystemTime::now();
                while let Err(err) = waitpid(child, None) {
                    error!("error waiting for child: {:?}", err);
                }
                match SystemTime::now().duration_since(start_time) {
                    Ok(t) if t > sleep_time => {
                        // Child seems to be behaving, stop sleeping a long time
                        // before trying to restart it.
                        sleep_time = min_sleep_time;
                    }
                    _ => {}
                }
                match init() {
                    Ok(data) => init_data = data,
                    Err(msg) => error!("{}", msg),
                }
            }
            ForkResult::Child => {
                drop(state);
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
