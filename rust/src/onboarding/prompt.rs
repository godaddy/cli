use std::io::{self, BufRead, Write};

const TERMS_OF_USE: &str = "https://developer.commerce.godaddy.com/legal/agreements/terms-of-use";
const PRIVACY_POLICY: &str =
    "https://developer.commerce.godaddy.com/legal/agreements/privacy-policy";
const DEVELOPER_AGREEMENT: &str =
    "https://developer.commerce.godaddy.com/legal/agreements/developer-agreement";

pub fn prompt_accept_agreements<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
) -> io::Result<bool> {
    writeln!(writer)?;
    writeln!(
        writer,
        "By continuing, you agree to the GoDaddy Developer terms:"
    )?;
    writeln!(writer)?;
    writeln!(writer, "  Terms of Service:      {TERMS_OF_USE}")?;
    writeln!(writer, "  Privacy Policy:        {PRIVACY_POLICY}")?;
    writeln!(writer, "  Developer Agreement:   {DEVELOPER_AGREEMENT}")?;
    writeln!(writer)?;
    write!(writer, "Press Enter to accept and continue...")?;
    writer.flush()?;

    let mut line = String::new();
    let bytes = reader.read_line(&mut line)?;
    if bytes == 0 {
        return Ok(false);
    }

    Ok(line.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::prompt_accept_agreements;

    #[test]
    fn prompt_writes_all_links_to_stderr_and_accepts_enter() {
        let mut input = Cursor::new(b"\n");
        let mut output = Vec::new();
        assert!(prompt_accept_agreements(&mut input, &mut output).expect("prompt"));
        let text = String::from_utf8(output).expect("utf8");
        assert!(text.contains("terms-of-use"));
        assert!(text.contains("privacy-policy"));
        assert!(text.contains("developer-agreement"));
    }

    #[test]
    fn prompt_fails_closed_on_eof() {
        let mut input = Cursor::new([]);
        assert!(!prompt_accept_agreements(&mut input, &mut Vec::new()).expect("prompt"));
    }

    #[test]
    fn prompt_rejects_non_empty_response() {
        let mut input = Cursor::new(b"no\n");
        assert!(!prompt_accept_agreements(&mut input, &mut Vec::new()).expect("prompt"));
    }
}
