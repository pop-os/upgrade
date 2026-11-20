use super::CrypttabErr as Error;
use std::fs;
use std::path::Path;

const CRYPTTAB: &str = "/etc/crypttab";

pub fn repair() -> Result<(), Error> {
    let crypttab_path = Path::new(Path::new(CRYPTTAB));
    if !crypttab_path.exists() {
        return Ok(());
    }

    let contents = fs::read_to_string(crypttab_path).map_err(Error::CrypttabRead)?;
    if let Some(new_contents) = crypttab_update(&contents) {
        crate::fs::atomic_overwrite(crypttab_path, new_contents.as_bytes())
            .map_err(Error::CrypttabWrite)?;
    }

    Ok(())
}

fn crypttab_update(input: &str) -> Option<String> {
    let mut modified = false;
    let mut new = String::new();

    for line in input.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            new.push_str(line);
            new.push('\n');
            continue;
        }

        let mut fields = line.split_ascii_whitespace();
        if let Some(cmd) = fields.next()
            && let Some(source) = fields.next()
            && let Some(keyfile) = fields.next()
            && let Some(options) = fields.next()
        {
            const HAS_SWAP: u32 = 0b0001;
            const HAS_PLAIN: u32 = 0b0010;
            const HAS_LUKS: u32 = 0b0100;
            const HAS_DISCARD: u32 = 0b1000;
            let mut flags = 0;
            for field in options.split(',') {
                match field {
                    "swap" => flags |= HAS_SWAP,
                    "plain" => flags |= HAS_PLAIN,
                    "luks" => flags |= HAS_LUKS,
                    "discard" => flags |= HAS_DISCARD,
                    _ => (),
                }
            }

            let add_plain_to_swap = flags & (HAS_SWAP | HAS_PLAIN) == HAS_SWAP;
            let add_discard_to_luks = flags & (HAS_LUKS | HAS_DISCARD) == HAS_LUKS;
            if add_plain_to_swap || add_discard_to_luks {
                modified = true;
                let mut add_comma = false;
                new.push_str(cmd);
                new.push(' ');
                new.push_str(source);
                new.push(' ');
                new.push_str(keyfile);
                new.push(' ');

                for field in options.split(',') {
                    if add_comma {
                        new.push(',');
                    }

                    add_comma = true;
                    match field {
                        "swap" if add_plain_to_swap => {
                            new.push_str("swap,plain");
                        }
                        "luks" if add_discard_to_luks => {
                            new.push_str("luks,discard");
                        }
                        _ => new.push_str(field),
                    }
                }

                new.push('\n');
                continue;
            }
        }

        new.push_str(line);
        new.push('\n');
    }

    modified.then_some(new)
}

#[cfg(test)]
mod tests {
    const BAD: &str = r#"cryptdata UUID=ed9e7007-b02b-48a6-b4ce-2207ee5fefd6 none luks
cryptswap UUID=ed9e7007-b02b-48a6-b4ce-2207ee5fefd6  /dev/urandom swap,offset=1024,cipher=aes-xts-plain64,size=512
"#;

    const GOOD: &str = r#"cryptdata UUID=ed9e7007-b02b-48a6-b4ce-2207ee5fefd6 none luks,discard
cryptswap UUID=ed9e7007-b02b-48a6-b4ce-2207ee5fefd6 /dev/urandom swap,plain,offset=1024,cipher=aes-xts-plain64,size=512
"#;

    #[test]
    fn craypttab_updates() {
        assert_eq!(None, super::crypttab_update(GOOD));
        assert_eq!(Some(GOOD.to_owned()), super::crypttab_update(BAD));
    }
}
