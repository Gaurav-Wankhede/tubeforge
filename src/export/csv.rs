//! Minimal RFC-4180 CSV writer for `tubeforge export`.
//!
//! Quoting rule: a field is quoted iff it contains a comma, a double quote,
//! CR or LF; embedded quotes are doubled. This is the subset that keeps
//! `videos.csv`/`channels.csv`/`tags.csv`/`keywords.csv` round-trippable
//! through any spreadsheet tool.

/// Escape one CSV field per RFC-4180 (comma / quote / newline handling).
pub fn field(v: &str) -> String {
    if v.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", v.replace('"', "\"\""))
    } else {
        v.to_string()
    }
}

/// One CSV record (fields joined with commas, terminated with `\n`).
pub fn record(fields: &[String]) -> String {
    let mut out = String::new();
    for (i, f) in fields.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&field(f));
    }
    out.push('\n');
    out
}

/// Convenience: build a record from &str slices.
pub fn record_strs(fields: &[&str]) -> String {
    record(&fields.iter().map(|s| s.to_string()).collect::<Vec<_>>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_fields_pass_through() {
        assert_eq!(field("abc"), "abc");
        assert_eq!(field(""), "");
        assert_eq!(field("with space"), "with space");
    }

    #[test]
    fn comma_quote_and_newline_are_quoted_and_escaped() {
        assert_eq!(field("a,b"), "\"a,b\"");
        assert_eq!(field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(field("line1\nline2"), "\"line1\nline2\"");
        assert_eq!(field("cr\rlf"), "\"cr\rlf\"");
    }

    #[test]
    fn record_joins_and_terminates() {
        assert_eq!(record_strs(&["a", "b", "c"]), "a,b,c\n");
        assert_eq!(
            record_strs(&["x", "y,z", "\"q\""]),
            "x,\"y,z\",\"\"\"q\"\"\"\n"
        );
        assert_eq!(record_strs(&[]), "\n");
    }
}
