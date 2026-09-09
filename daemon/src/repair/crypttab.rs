use super::CrypttabErr as Error;
use std::{fs, path::Path};

const CRYPTTAB: &str = "/etc/crypttab";

pub fn repair() -> Result<(), Error> {
    let crypttab_path = Path::new(Path::new(CRYPTTAB));
    if !crypttab_path.exists() {
        return Ok(());
    }

    let contents = fs::read_to_string(crypttab_path).map_err(Error::CrypttabRead)?;

    if let Some(new_contents) = cryptswap_plain_warning(&contents) {
        crate::fs::atomic_overwrite(crypttab_path, new_contents.as_bytes()).map_err(Error::CrypttabWrite)?;
    }

    Ok(())
}

fn cryptswap_plain_warning(input: &str) -> Option<String> {
    let mut correction_needed = false;
    let mut new = String::new();

    for line in input.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            new.push_str(line);
            new.push('\n');
            continue;
        }

        if let Some(options) = line.split_ascii_whitespace().nth(3) {
            let fields = options.split(',').collect::<Vec<_>>();
            if fields.iter().any(|&e| e == "swap") && !fields.iter().any(|&e| e == "plain") {
                new.push_str(&line.replace("swap,", "swap,plain,"));
                new.push('\n');
                correction_needed = true;
                continue;
            }
        }

        new.push_str(line);
        new.push('\n');
    }

    if correction_needed {
        Some(new)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BAD: &str = r#"cryptdata UUID=ed9e7007-b02b-48a6-b4ce-2207ee5fefd6 none luks
cryptswap UUID=ed9e7007-b02b-48a6-b4ce-2207ee5fefd6  /dev/urandom swap,offset=1024,cipher=aes-xts-plain64,size=512
"#;

    const GOOD: &str = r#"cryptdata UUID=ed9e7007-b02b-48a6-b4ce-2207ee5fefd6 none luks
cryptswap UUID=ed9e7007-b02b-48a6-b4ce-2207ee5fefd6  /dev/urandom swap,plain,offset=1024,cipher=aes-xts-plain64,size=512
"#;

    #[test]
    fn cryptswap() {
        assert_eq!(None, cryptswap_plain_warning(GOOD));
        assert_eq!(Some(GOOD.to_owned()), cryptswap_plain_warning(BAD));
    }
}
