use dbus;
use std::io;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DaemonError {
    #[error("failed to make a private dbus connection to the system bus")]
    PrivateConnection(#[source] dbus::Error),

    #[error("failed to register dbus name")]
    RegisterName(#[source] dbus::Error),

    #[error("failed to register object paths in the dbus tree")]
    TreeRegister(#[source] dbus::Error),

    #[error("failure to create {}", _0)]
    VarLibDirectory(&'static str, #[source] io::Error),
}
