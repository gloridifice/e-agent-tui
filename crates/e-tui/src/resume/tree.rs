//! Pure parent-first session ordering; IDs are opaque adapter-owned identities.

use std::collections::HashMap;

pub type SessionParents = HashMap<String, String>;

pub struct SessionTreeRow {
    pub index: usize,
    pub parent: Option<usize>,
    pub prefix: String,
}

/// Input order is recency order. A family's newest member determines its rank.
pub fn session_tree(ids: &[&str], links: &SessionParents) -> Vec<SessionTreeRow> {
    let indices: HashMap<_, _> = ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let mut parents: Vec<_> = ids
        .iter()
        .map(|id| {
            links
                .get(*id)
                .and_then(|parent| indices.get(parent.as_str()).copied())
        })
        .collect();
    let mut visited = vec![0_u8; ids.len()];
    for start in 0..ids.len() {
        let mut path = Vec::new();
        let mut cursor = Some(start);
        while let Some(index) = cursor {
            match visited[index] {
                2 => break,
                1 => {
                    parents[index] = None;
                    break;
                }
                _ => {
                    visited[index] = 1;
                    path.push(index);
                    cursor = parents[index];
                }
            }
        }
        for index in path {
            visited[index] = 2;
        }
    }

    let mut children = vec![Vec::new(); ids.len()];
    let mut roots = Vec::new();
    for (index, parent) in parents.iter().enumerate() {
        if let Some(parent) = parent {
            children[*parent].push(index);
        } else {
            roots.push(index);
        }
    }
    let mut walk = Vec::with_capacity(ids.len());
    let mut stack = roots.clone();
    while let Some(index) = stack.pop() {
        walk.push(index);
        stack.extend(children[index].iter().copied());
    }
    let mut rank: Vec<_> = (0..ids.len()).collect();
    for &index in walk.iter().rev() {
        if let Some(parent) = parents[index] {
            rank[parent] = rank[parent].min(rank[index]);
        }
    }
    roots.sort_by_key(|index| rank[*index]);
    for siblings in &mut children {
        siblings.sort_by_key(|index| rank[*index]);
    }

    let mut stack: Vec<_> = roots.iter().rev().map(|index| (*index, 0, true)).collect();
    let mut last_at_depth = Vec::new();
    let mut rows = Vec::with_capacity(ids.len());
    while let Some((index, depth, last)) = stack.pop() {
        last_at_depth.truncate(depth);
        last_at_depth.push(last);
        let mut prefix = String::new();
        for &ancestor_last in last_at_depth.iter().take(depth.min(9)).skip(1) {
            prefix.push_str(if ancestor_last { "   " } else { "│  " });
        }
        if depth > 9 {
            prefix.push_str("… ");
        }
        if depth > 0 {
            prefix.push_str(if last { "└─ " } else { "├─ " });
        }
        rows.push(SessionTreeRow {
            index,
            parent: parents[index],
            prefix,
        });
        let siblings = &children[index];
        stack.extend(
            siblings
                .iter()
                .enumerate()
                .rev()
                .map(|(position, child)| (*child, depth + 1, position + 1 == siblings.len())),
        );
    }
    rows
}
