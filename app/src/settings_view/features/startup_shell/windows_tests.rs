use super::{format_version, path_suffix};
use std::path::Path;

#[test]
fn powershell_release_and_preview_are_distinct() {
    assert_eq!(
        format_version("7.6.10 SHA: deadbeef").as_deref(),
        Some("7.6.10")
    );
    assert_eq!(
        format_version("7.7.0-preview.2+build").as_deref(),
        Some("7.7.0 Preview 2")
    );
    assert_eq!(format_version("5.1").as_deref(), Some("5.1"));
    assert_eq!(format_version("not a version"), None);
}

#[test]
fn path_suffix_reaches_the_first_distinguishing_directory() {
    let stable = Path::new(r"C:\Program Files\PowerShell\7\pwsh.exe");
    let preview = Path::new(r"C:\Program Files\PowerShell\7-preview\pwsh.exe");
    let copy = Path::new(r"C:\Portable\PowerShell\7\pwsh.exe");

    assert_eq!(path_suffix(preview, &[stable]), "7-preview");
    assert_eq!(path_suffix(copy, &[stable]), r"Portable\PowerShell\7");
    assert_eq!(path_suffix(copy, &[copy]), copy.to_str().unwrap());
}
