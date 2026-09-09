use std::io::{self, Write};

const LUKS_CONF_PATH: &str = "/etc/dracut.conf.d/luks.conf";

pub fn configure() -> io::Result<()> {
    if let Some(luks_uuid) = cryptdata_uuid() {
        create_luks_config(&luks_uuid)?;
        add_kernelstub_option(&luks_uuid)?;
    }

    Ok(())
}

pub fn cryptdata_uuid() -> Option<String> {
    let crypttab = std::fs::read_to_string("/etc/crypttab").ok()?;
    for line in crypttab.lines() {
        let mut fields = line.split_ascii_whitespace();

        if fields.next() == Some("cryptdata") {
            let Some(source) = fields.next() else {
                continue;
            };
            if let Some(uuid) = source.strip_prefix("UUID=") {
                return Some(uuid.to_owned());
            }
        }
    }

    None
}

pub fn create_luks_config(luks_uuid: &str) -> io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(LUKS_CONF_PATH)?;

    writeln!(&mut file, r#"add_dracutmodules+=" crypt lvm mdraid plymouth "
install_items+=" /etc/crypttab "
kernel_cmdline+=" rd.luks.uuid={luks_uuid} "
"#)
}

pub fn add_kernelstub_option(root_uuid: &str) -> io::Result<()> {
    std::process::Command::new("kernelstub")
        .args(["-a", &format!("rd.luks.uuid={root_uuid}")])
        .status()?;
    Ok(())
}
