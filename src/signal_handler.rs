use std::{
    fmt::{self, Display, Formatter},
    sync::atomic::{AtomicUsize, Ordering, ATOMIC_USIZE_INIT},
};

pub static PENDING: AtomicUsize = ATOMIC_USIZE_INIT;

#[repr(u8)]
pub enum Signal {
    Interrupt,
    Hangup,
    Terminate,
}

impl Display for Signal {
    fn fmt(&self, fmt: &mut Formatter) -> fmt::Result {
        let string = match *self {
            Signal::Interrupt => "interrupt",
            Signal::Hangup => "hangup",
            Signal::Terminate => "terminate",
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
            _ => unreachable!(),
        };

        warn!("caught {} signal", signal);
        PENDING.store(signal as usize, Ordering::SeqCst);
    }

    unsafe {
        let _ = libc::signal(libc::SIGHUP, handler as libc::sighandler_t);
        let _ = libc::signal(libc::SIGINT, handler as libc::sighandler_t);
        let _ = libc::signal(libc::SIGTERM, handler as libc::sighandler_t);
    }
}
