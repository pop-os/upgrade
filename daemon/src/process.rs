use std::io;
use std::process::Command;

pub fn exec(command: &mut Command) -> Result<(), CommandErr> {
    tracing::info!("exec: {command:?}");
    let cmd = command.get_program().to_string_lossy().into_owned();
    match command.output() {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => Err(CommandErr::Failed {
            cmd,
            code: output.status.code(),
            source: io::Error::other(String::from_utf8_lossy(&output.stderr)),
        }),
        Err(why) if why.kind() == io::ErrorKind::NotFound => Err(CommandErr::NotFound { cmd }),
        Err(why) => Err(CommandErr::Spawn { cmd, source: why }),
    }
}

error_set::error_set! {
    CommandErr := {
        #[display("`{cmd}` command exited in error ({code:?})")]
        Failed(io::Error) {
            cmd: String,
            code: Option<i32>,
        },
        #[display("`{cmd}` command not found")]
        NotFound { cmd: String },
        #[display("`{cmd}` command spawn failed")]
        Spawn(io::Error) { cmd: String },
    }
}
