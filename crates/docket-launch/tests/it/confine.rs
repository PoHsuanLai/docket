//! What a person is told of how far an agent is held: the words for every network and every
//! level the exec stage can report, and the strict reading of its report line.

use bulkhead::{LandlockLevel, NetworkMode};
use docket_launch::Confinement;
use docket_launch::procs::level_in;

const MODES: [NetworkMode; 4] = [
    NetworkMode::None,
    NetworkMode::EndpointOnly,
    NetworkMode::Allowlist,
    NetworkMode::Host,
];

fn sites() -> Vec<String> {
    vec!["api.example.com".to_owned(), "*.example.org".to_owned()]
}

#[test]
fn the_summary_for_every_network_and_level() {
    const NOT: &str = "Not limited by the kernel";
    const FILES: &str = "Files limited, network not limited by the kernel";
    const BOTH: &str = "Files and network limited";
    let rows = [
        // No report (no exec stage, or silent), and a stage that found nothing to use.
        (None, [NOT, NOT, NOT, NOT]),
        (Some(LandlockLevel::None), [NOT, NOT, NOT, NOT]),
        (Some(LandlockLevel::FilesOnly), [FILES, FILES, FILES, FILES]),
        (Some(LandlockLevel::Full), [BOTH, BOTH, BOTH, FILES]),
    ];
    for (level, want) in rows {
        for (mode, want) in MODES.into_iter().zip(want) {
            let held = Confinement::new(level, mode, sites());
            assert_eq!(held.summary(), want, "{level:?} {mode:?}");
        }
    }
}

#[test]
fn the_network_line_for_every_network() {
    let want = [
        "Cannot reach the network",
        "Can reach only its model",
        "Can reach only: api.example.com, *.example.org",
        "Can reach the whole network",
    ];
    for (mode, want) in MODES.into_iter().zip(want) {
        assert_eq!(
            Confinement::new(None, mode, sites()).network(),
            want,
            "{mode:?}"
        );
    }
}

#[test]
fn no_word_a_person_reads_is_a_developer_word() {
    let levels = [
        None,
        Some(LandlockLevel::None),
        Some(LandlockLevel::FilesOnly),
        Some(LandlockLevel::Full),
    ];
    for level in levels {
        for mode in MODES {
            let held = Confinement::new(level, mode, sites());
            let said = format!("{} {}", held.summary(), held.network()).to_lowercase();
            for word in [
                "landlock",
                "proxy",
                "abi",
                "allowlist",
                "namespace",
                "endpoint",
            ] {
                assert!(!said.contains(word), "{level:?} {mode:?}: {word} in {said}");
            }
        }
    }
}

#[test]
fn the_report_line_is_read_strictly() {
    let rows: [(&[u8], Option<LandlockLevel>); 12] = [
        (
            b"bulkhead-enter: landlock=full\n",
            Some(LandlockLevel::Full),
        ),
        (
            b"bulkhead-enter: landlock=files-only\n",
            Some(LandlockLevel::FilesOnly),
        ),
        (
            b"bulkhead-enter: landlock=none\n",
            Some(LandlockLevel::None),
        ),
        (b"bulkhead-enter: landlock=full", Some(LandlockLevel::Full)),
        (
            b"bulkhead-enter: landlock=full\r\n",
            Some(LandlockLevel::Full),
        ),
        (b"bulkhead-enter: landlock=maybe\n", None),
        (b"bulkhead-enter: landlock=full extra\n", None),
        (b" bulkhead-enter: landlock=full\n", None),
        (b"the agent says hello\n", None),
        (b"\xff\xfe\n", None),
        (b"", None),
        (b"\n", None),
    ];
    for (line, want) in rows {
        assert_eq!(level_in(line), want, "{}", String::from_utf8_lossy(line));
    }
}
