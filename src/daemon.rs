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
use std::{convert::TryFrom, panic, sync::Mutex};
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
    if signal == Signal::SIGTERM {
        *state = State::ShuttingDown;
    }
}

pub fn run_as_daemon<F: FnOnce()>(f: F) {
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

    loop {
        let mut state = STATE.lock().unwrap();
        if let State::ShuttingDown = *state {
            break;
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
                while let Err(err) = waitpid(child, None) {
                    error!("error waiting child: {:?}", err);
                }
            }
            ForkResult::Child => {
                drop(state);
                unsafe {
                    let msg = "failed to clear signal handler";
                    signal(Signal::SIGQUIT, SigHandler::SigDfl).expect(msg);
                    signal(Signal::SIGTERM, SigHandler::SigDfl).expect(msg);
                }
                info!("spawned child: {}", getpid());
                f();
                break;
            }
        }
    }
}
