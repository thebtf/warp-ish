use super::*;

fn local_match(line_text: &str, submatches: &[(usize, usize)]) -> GlobalSearchMatch {
    GlobalSearchMatch {
        location: LocalOrRemotePath::Local(PathBuf::from("result.rs")),
        line_number: 7,
        column_num: None,
        line_text: line_text.to_string(),
        submatches: submatches
            .iter()
            .map(|&(byte_start, byte_end)| Submatch {
                byte_start: ByteOffset::from(byte_start),
                byte_end: ByteOffset::from(byte_end),
            })
            .collect(),
    }
}

#[test]
fn local_matches_expand_one_row_per_submatch() {
    let results = GlobalSearch::expand_submatches(local_match("foo foo", &[(0, 3), (4, 7)]));
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].column_num, Some(1));
    assert_eq!(results[1].column_num, Some(5));
    assert!(results.iter().all(|result| result.submatches.len() == 1));
    assert_eq!(
        results[0].location.to_local_path(),
        Some(std::path::Path::new("result.rs"))
    );
    assert_eq!(results[0].line_number, 7);
}

#[test]
fn local_match_leading_whitespace_is_trimmed_per_submatch() {
    let results = GlobalSearch::expand_submatches(local_match("    foo", &[(4, 7)]));
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].line_text, "foo");
    assert_eq!(results[0].column_num, Some(5));
    assert_eq!(results[0].submatches[0].byte_start.as_usize(), 0);
    assert_eq!(results[0].submatches[0].byte_end.as_usize(), 3);
}

#[test]
fn local_match_column_counts_characters_not_bytes() {
    let results = GlobalSearch::expand_submatches(local_match("€foo", &[(3, 6)]));
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].column_num, Some(2));
}
