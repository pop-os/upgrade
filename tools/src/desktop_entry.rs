//! Generates the desktop entry for this application.

use clap::Clap;
use freedesktop_desktop_entry::{Application, DesktopEntry, DesktopType};
use std::{
    fs::File,
    io::{self, Write},
    path::PathBuf,
};

/// generates desktop entries
#[derive(Clap)]
pub struct Command {
    #[clap(long)]
    args: Option<String>,

    #[clap(long)]
    appid: String,

    #[clap(long)]
    binary: String,

    #[clap(long)]
    categories: Vec<String>,

    #[clap(long)]
    comment: String,

    #[clap(long)]
    icon: String,

    #[clap(long, min_values = 0)]
    keywords: Vec<String>,

    #[clap(long)]
    name: String,

    #[clap(long)]
    prefix: PathBuf,

    #[clap(long)]
    startup_notify: bool,
}

impl Command {
    fn write_desktop_entry(&self) -> io::Result<()> {
        let categories: Vec<_> = self.categories.iter().map(String::as_str).collect();
        let keywords: Vec<_> = self.keywords.iter().map(String::as_str).collect();
        let exec = self.exec();

        let entry = DesktopEntry::new(
            &self.name,
            &self.icon,
            DesktopType::Application({
                let mut app = Application::new(&categories, &exec);

                app = app.keywords(&keywords);

                if self.startup_notify {
                    app = app.startup_notify();
                }

                app
            }),
        )
        .comment(&self.comment);

        let mut desktop = File::create(["target/", &self.appid, ".desktop"].concat().as_str())?;

        desktop.write_all(entry.to_string().as_bytes())
    }

    fn exec(&self) -> String {
        let exec_path = self.prefix.join("bin").join(&self.binary);
        let exec = exec_path.as_os_str().to_str().expect("path is not valid utf-8");
        if let Some(args) = &self.args {
            format!("{} {}", exec, args)
        } else {
            exec.to_string()
        }
    }
}

fn main() {
    let command = Command::parse();
    command.write_desktop_entry().expect("failed to write desktop entry");
}
