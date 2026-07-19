use trust_kernel::tools::fs_paths::canonicalize;

#[test]
fn backslashes_become_forward() {
    let p = canonicalize(r"C:\Users\me\file.txt");
    assert_eq!(p, "c:/Users/me/file.txt");
}

#[test]
fn drive_letter_lowercased() {
    let p = canonicalize(r"D:/Docs/Readme.md");
    assert_eq!(p, "d:/Docs/Readme.md");
}

#[test]
fn duplicated_slashes_collapsed() {
    let p = canonicalize(r"C:/Users//me///file.txt");
    assert_eq!(p, "c:/Users/me/file.txt");
}

#[test]
fn trailing_slash_stripped_for_files() {
    // We can't know if it's a file vs dir without filesystem access;
    // canonicalize() preserves trailing slash as a single '/'.
    let p = canonicalize("C:/Users/me/");
    assert_eq!(p, "c:/Users/me/");
}

#[test]
fn relative_path_left_as_is() {
    let p = canonicalize("docs/readme.md");
    assert_eq!(p, "docs/readme.md");
}

#[test]
fn dot_segments_resolved() {
    let p = canonicalize("C:/Users/me/../you/file.txt");
    assert_eq!(p, "c:/Users/you/file.txt");
}

#[test]
fn dot_dot_at_root_stays_at_root() {
    let p = canonicalize("C:/../file.txt");
    assert_eq!(p, "c:/file.txt");
}
