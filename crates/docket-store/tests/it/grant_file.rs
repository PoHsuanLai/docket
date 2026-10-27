//! The grant file's disk rules: a missing or blank file holds nothing, a damaged one is a fault
//! (never an empty list), and a text that could not be encoded writes nothing.

use docket_store::{GrantFileError, read_grant_list, write_grant_list};
use std::num::ParseIntError;

fn decode(text: &str) -> Result<Vec<u32>, ParseIntError> {
    text.split(',').map(|n| n.trim().parse()).collect()
}

fn unencodable() -> Result<String, ParseIntError> {
    "x".parse::<u32>().map(|n| n.to_string())
}

#[test]
fn a_missing_or_blank_file_holds_nothing_and_a_damaged_one_is_a_fault() {
    let dir = tempfile::tempdir().expect("scratch");
    let path = dir.path().join("grants");
    assert_eq!(
        read_grant_list(&path, decode).expect("missing"),
        Vec::<u32>::new()
    );
    std::fs::write(&path, "  \n").expect("blank");
    assert_eq!(
        read_grant_list(&path, decode).expect("blank"),
        Vec::<u32>::new()
    );
    std::fs::write(&path, "1, x").expect("damaged");
    assert!(matches!(
        read_grant_list(&path, decode),
        Err(GrantFileError::Corrupt { .. })
    ));
}

#[test]
fn a_list_is_written_whole_and_an_unencodable_one_writes_nothing() {
    let dir = tempfile::tempdir().expect("scratch");
    let path = dir.path().join("a/grants");
    write_grant_list(&path, Ok::<_, ParseIntError>("1, 2".to_owned())).expect("write");
    assert_eq!(read_grant_list(&path, decode).expect("read"), vec![1, 2]);
    assert!(matches!(
        write_grant_list(&path, unencodable()),
        Err(GrantFileError::Encode { .. })
    ));
    assert_eq!(read_grant_list(&path, decode).expect("kept"), vec![1, 2]);
}
