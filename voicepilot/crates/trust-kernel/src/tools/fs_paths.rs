//! Path canonicalization — V1.1 §4.4 step 1 + §6.2 prepare.
//!
//! Pure string transformation; does NOT touch the filesystem.
//! - Backslashes → forward slashes
//! - Lowercase drive letter (Windows)
//! - Collapse duplicate slashes
//! - Resolve `.` and `..` segments
//! - Preserve trailing slash (caller decides file vs dir)

/// Canonicalize a path string without touching the filesystem.
pub fn canonicalize(input: &str) -> String {
    // Step 1: backslashes → forward slashes.
    let with_forward = input.replace('\\', "/");

    // Step 2: lowercase drive letter if present (e.g. "C:/" → "c:/").
    let mut chars = with_forward.chars().collect::<Vec<_>>();
    let drive_lowered: String =
        if chars.len() >= 2 && chars[1] == ':' && chars[0].is_ascii_uppercase() {
            chars[0] = chars[0].to_ascii_lowercase();
            chars.into_iter().collect()
        } else {
            with_forward
        };

    // Step 3: split on '/', resolve "." and "..", collapse duplicates.
    let has_trailing_slash = drive_lowered.ends_with('/') && !drive_lowered.ends_with(":/");
    let mut parts: Vec<&str> = Vec::new();
    let mut prefix = String::new(); // holds "c:" or "" for relative
    let mut first_segment = true;
    for segment in drive_lowered.split('/') {
        if first_segment {
            first_segment = false;
            // If segment looks like "c:" treat as drive prefix.
            if segment.len() == 2 && segment.as_bytes()[1] == b':' {
                prefix = segment.to_string();
                continue;
            }
        }
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." {
            if let Some(last) = parts.last() {
                if *last != ".." {
                    parts.pop();
                    continue;
                }
            }
            // .. at root or in relative path with no prior segment: keep if relative, drop if absolute.
            if !prefix.is_empty() {
                // At filesystem root: drop the ..
                continue;
            }
            // Relative path: keep the ..
            parts.push("..");
            continue;
        }
        parts.push(segment);
    }

    let has_prefix = !prefix.is_empty();
    let mut out = prefix;
    if has_prefix {
        out.push('/');
    }
    out.push_str(&parts.join("/"));
    if out.is_empty() {
        out.push('.');
    }
    if has_trailing_slash && !out.ends_with('/') {
        out.push('/');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::canonicalize;

    #[test]
    fn empty_becomes_dot() {
        assert_eq!(canonicalize(""), ".");
    }
}
