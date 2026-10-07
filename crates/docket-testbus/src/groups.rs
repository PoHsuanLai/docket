//! The process groups under a process: a daemon may start children that lead groups of their own
//! (inferd's engines do), and a daemon that dies without ending them leaves them running. Read
//! from `/proc` by PID and parent PID; nothing is matched by name.

use std::collections::{BTreeSet, HashMap};

/// One `/proc/<pid>/stat` line's process, parent and group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Proc {
    pub pid: u32,
    pub parent: u32,
    pub group: u32,
}

/// Reads pid, ppid and pgrp from a `stat` line (the command name, in parentheses, may hold spaces
/// and parentheses, so the fields are read after the last `)`).
pub(crate) fn parse_stat(line: &str) -> Option<Proc> {
    let pid = line.split_whitespace().next()?.parse().ok()?;
    let rest = &line[line.rfind(')')? + 1..];
    let mut fields = rest.split_whitespace().skip(1);
    let parent = fields.next()?.parse().ok()?;
    let group = fields.next()?.parse().ok()?;
    Some(Proc { pid, parent, group })
}

/// The groups of every process below `root` (not `root`'s own), from a process table.
pub(crate) fn groups_below(root: u32, table: &[Proc]) -> BTreeSet<u32> {
    let mut children: HashMap<u32, Vec<Proc>> = HashMap::new();
    for proc in table {
        children.entry(proc.parent).or_default().push(*proc);
    }
    let mut groups = BTreeSet::new();
    let mut todo = vec![root];
    while let Some(pid) = todo.pop() {
        for child in children.get(&pid).into_iter().flatten() {
            if child.group != root {
                groups.insert(child.group);
            }
            todo.push(child.pid);
        }
    }
    groups
}

/// The live process table, as far as `/proc` can be read.
pub(crate) fn table() -> Vec<Proc> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|e| std::fs::read_to_string(e.path().join("stat")).ok())
        .filter_map(|line| parse_stat(&line))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stat_line_with_spaces_and_parentheses_in_the_name_reads() {
        let line = "4242 (VLLM::Engine (core)) S 77 4100 4100 0 -1 4194560 0 0";
        assert_eq!(
            parse_stat(line),
            Some(Proc {
                pid: 4242,
                parent: 77,
                group: 4100
            })
        );
    }

    #[test]
    fn groups_below_follow_parents_and_skip_the_roots_own() {
        let p = |pid, parent, group| Proc { pid, parent, group };
        let table = [
            p(10, 1, 10),
            p(11, 10, 10),
            p(12, 10, 12),
            p(13, 12, 12),
            p(14, 13, 14),
            p(20, 1, 20),
        ];
        assert_eq!(groups_below(10, &table), BTreeSet::from([12, 14]));
    }
}
