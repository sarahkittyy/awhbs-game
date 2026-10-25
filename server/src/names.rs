//! Player names: 1..=16 printable ASCII characters (the game's font has nothing else), letters,
//! digits, spaces and a little punctuation, with runs of spaces collapsed. A short list of slurs
//! is refused, matched after folding case, common digit/symbol swaps and separators.

pub const MAX_LEN: usize = 16;

const ALLOWED_PUNCT: &str = " _-.!?'";

/// Substrings refused anywhere in a folded name.
const BLOCKED: &[&str] = &[
    "nigger", "nigga", "faggot", "fagot", "tranny", "retard", "chink", "kike", "gook", "wetback",
    "hitler", "nazi", "kkk",
];

pub fn clean(raw: &str) -> Result<String, &'static str> {
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return Err("type a name first");
    }
    if collapsed.chars().count() > MAX_LEN {
        return Err("that name is too long");
    }
    if !collapsed.chars().all(|c| c.is_ascii_alphanumeric() || ALLOWED_PUNCT.contains(c)) {
        return Err("letters, numbers and _-.!?' only");
    }
    if blocked(&collapsed) {
        return Err("pick a different name");
    }
    Ok(collapsed)
}

fn blocked(name: &str) -> bool {
    let folded: String = name
        .chars()
        .filter_map(|c| match c.to_ascii_lowercase() {
            '0' => Some('o'),
            '1' | '!' => Some('i'),
            '3' => Some('e'),
            '4' => Some('a'),
            '5' => Some('s'),
            '7' => Some('t'),
            c if c.is_ascii_lowercase() => Some(c),
            _ => None,
        })
        .collect();
    BLOCKED.iter().any(|b| folded.contains(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(clean("  awhbs   fan ").unwrap(), "awhbs fan");
        assert!(clean("").is_err());
        assert!(clean("seventeen chars!!").is_err());
        assert!(clean("bad<script>").is_err());
        assert!(clean("n1 gg3r").is_err());
        assert!(clean("sproutmole").is_ok());
    }
}
