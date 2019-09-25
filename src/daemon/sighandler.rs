use std::{
    fmt::{self, Display, Formatter},
    sync::atomic::{AtomicU8, Ordering},
};

static PENDING: AtomicU8 = AtomicU8::new(0);

#[repr(u8)]
pub enum Signal {
    Interrupt = 1,
    Hangup = 2,
    Terminate = 3,
    TermStop = 4,
}

pub fn status() -> Option<Signal> {
    match PENDING.swap(0, Ordering::SeqCst) {
        1 => Some(Signal::Interrupt),
        2 => Some(Signal::Hangup),
        3 => Some(Signal::Terminate),
        4 => Some(Signal::TermStop),
        _ => None,
    }
}

impl Display for Signal {
    fn fmt(&self, fmt: &mut Formatter) -> fmt::Result {
        let string = match *self {
            Signal::Interrupt => "interrupt",
            Signal::Hangup => "hangup",
            Signal::Terminate => "terminate",
            Signal::TermStop => "term stop",
        };

        fmt.write_str(string)
    }
}

pub fn init() {
    extern "C" fn handler(signal: i32) {
        let signal = match signal {
            libc::SIGINT => Signal::Interrupt,
            libc::SIGHUP => Signal::Hangup,
            libc::SIGTERM => Signal::Terminate,
            libc::SIGTSTP => Signal::TermStop,
            _ => unreachable!(),
        };

        warn!("caught {} signal", signal);
        PENDING.store(signal as u8, Ordering::SeqCst);
    }

    let handler = handler as libc::sighandler_t;

    unsafe {
        let _ = libc::signal(libc::SIGHUP, handler);
        let _ = libc::signal(libc::SIGTSTP, handler);
        let _ = libc::signal(libc::SIGINT, handler);
        let _ = libc::signal(libc::SIGTERM, handler);
    }
}
