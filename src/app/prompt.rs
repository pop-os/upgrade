use std::{
    io,
    io::{BufRead, Write},
};

// Write a prompt to the terminal, and wait for an answer.
pub fn get_bool(message: &str, default: bool) -> bool {
    let std_in = io::stdin();
    let mut std_in_lock = std_in.lock();

    let std_out = io::stdout();
    let mut std_out_lock = std_out.lock();

    let mut prompt = || -> io::Result<Option<bool>> {
        let response = get_input(&mut std_in_lock, &mut std_out_lock, message)
            .map(|input| parse_bool(&input, Some(default)))?;

        if response.is_none() {
            writeln!(std_out_lock, "The answer must be either `y` or `n`.")?;
        }
        Ok(response)
    };

    loop {
        if let Some(result) = prompt().unwrap_or(Some(default)) {
            return result;
        }
    }
}

fn get_input(
    mut std_in: impl BufRead,
    mut std_out: impl Write,
    message: &str,
) -> io::Result<String> {
    std_out.write_all(message.as_bytes())?;
    std_out.flush()?;

    let mut buf = String::default();
    std_in.read_line(&mut buf)?;
    Ok(buf)
}

fn parse_bool(input: &str, default: Option<bool>) -> Option<bool> {
    match input.trim().to_lowercase().chars().next() {
        None => default,
        Some('y') => Some(true),
        Some('n') => Some(false),
        Some(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use test_case::test_case;

    #[test_case("y\n", None => Some(true) ; "returns true if y is entered")]
    #[test_case("yes\n", None => Some(true) ; "returns true if yes is entered")]
    #[test_case("n\n", None => Some(false) ; "returns false if n is entered")]
    #[test_case("no\n", None => Some(false) ; "returns false if no is entered")]
    #[test_case("\n", Some(false) => Some(false) ; "returns default if nothing is entered")]
    #[test_case("\n", None => None ; "returns None if nothing is entered and no default set")]
    #[test_case("YeS\n", None => Some(true) ; "is case insensitive")]
    fn parse_bool(input: &str, default: Option<bool>) -> Option<bool> {
        super::super::parse_bool(input, default)
    }
}
