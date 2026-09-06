//! Single-track transcript storage for public display surfaces.

use std::collections::HashMap;

use crate::display::{DisplayId, DisplayItem};

#[derive(Debug, Clone)]
pub struct TranscriptNode {
    pub item: DisplayItem,
    pub surface_seq: Option<u64>,
    revision: u64,
}

impl TranscriptNode {
    pub fn id(&self) -> &DisplayId {
        self.item.id()
    }

    pub fn unit(&self) -> Option<u64> {
        match &self.item {
            DisplayItem::Activity(_) => None,
            DisplayItem::Block(block) => block.unit,
            DisplayItem::Card(card) => card.unit,
            DisplayItem::Composite { detail, .. } => detail.unit,
            DisplayItem::Thinking(node) => node.unit,
        }
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }
}

#[derive(Debug, Default)]
pub struct TranscriptStore {
    nodes: Vec<TranscriptNode>,
    positions: HashMap<DisplayId, usize>,
    unit_owners: HashMap<u64, DisplayId>,
    surface_owners: HashMap<u64, DisplayId>,
    generation: u64,
}

impl TranscriptStore {
    pub fn nodes(&self) -> &[TranscriptNode] {
        &self.nodes
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn clear(&mut self) {
        if self.nodes.is_empty() {
            return;
        }
        self.nodes.clear();
        self.positions.clear();
        self.unit_owners.clear();
        self.surface_owners.clear();
        self.generation = self.generation.wrapping_add(1);
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn get(&self, id: &DisplayId) -> Option<&TranscriptNode> {
        self.positions
            .get(id)
            .and_then(|index| self.nodes.get(*index))
    }

    pub fn get_mut(&mut self, id: &DisplayId) -> Option<&mut TranscriptNode> {
        let index = *self.positions.get(id)?;
        self.nodes.get_mut(index)
    }

    pub fn position(&self, id: &DisplayId) -> Option<usize> {
        self.positions.get(id).copied()
    }

    pub fn unit_owner(&self, unit: u64) -> Option<&DisplayId> {
        self.unit_owners.get(&unit)
    }

    pub fn surface_owner(&self, seq: u64) -> Option<&DisplayId> {
        self.surface_owners.get(&seq)
    }

    pub fn append(&mut self, item: DisplayItem, surface_seq: Option<u64>) -> usize {
        self.insert(self.nodes.len(), item, surface_seq)
    }

    pub fn prepend(&mut self, item: DisplayItem, surface_seq: Option<u64>) -> usize {
        self.insert(0, item, surface_seq)
    }

    pub fn insert(&mut self, index: usize, item: DisplayItem, surface_seq: Option<u64>) -> usize {
        let id = item.id().clone();
        if let Some(existing) = self.positions.get(&id).copied() {
            self.remove(existing);
        }
        let index = index.min(self.nodes.len());
        self.generation = self.generation.wrapping_add(1);
        self.nodes.insert(
            index,
            TranscriptNode {
                item,
                surface_seq,
                revision: self.generation,
            },
        );
        self.reindex();
        index
    }

    pub fn replace(
        &mut self,
        id: &DisplayId,
        item: DisplayItem,
        surface_seq: Option<u64>,
    ) -> Option<usize> {
        let index = self.position(id)?;
        self.remove(index);
        Some(self.insert(index, item, surface_seq))
    }

    pub fn remove(&mut self, index: usize) -> Option<TranscriptNode> {
        if index >= self.nodes.len() {
            return None;
        }
        let removed = self.nodes.remove(index);
        self.reindex();
        self.generation = self.generation.wrapping_add(1);
        Some(removed)
    }

    pub fn remove_indices(&mut self, indices: &[usize]) -> Vec<TranscriptNode> {
        let mut sorted = indices.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        let mut removed = Vec::with_capacity(sorted.len());
        for index in sorted.into_iter().rev() {
            if index < self.nodes.len() {
                removed.push(self.nodes.remove(index));
            }
        }
        removed.reverse();
        if !removed.is_empty() {
            self.reindex();
            self.generation = self.generation.wrapping_add(1);
        }
        removed
    }

    pub fn remove_surfaces(&mut self, seqs: &[u64], start: u64, end: u64) -> usize {
        let indices = self
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| {
                node.surface_seq
                    .filter(|seq| {
                        if seqs.is_empty() {
                            (start..=end).contains(seq)
                        } else {
                            seqs.contains(seq)
                        }
                    })
                    .map(|_| index)
            })
            .collect::<Vec<_>>();
        self.remove_indices(&indices).len()
    }

    pub fn first_surface_position(&self, seqs: &[u64], start: u64, end: u64) -> Option<usize> {
        self.nodes.iter().position(|node| {
            node.surface_seq.is_some_and(|seq| {
                if seqs.is_empty() {
                    (start..=end).contains(&seq)
                } else {
                    seqs.contains(&seq)
                }
            })
        })
    }

    /// Record an in-place content/activity mutation without changing identity
    /// or structural positions. Cache layers may pair this with a tail/range
    /// dirty marker instead of treating it as a structural rebuild.
    pub fn touch(&mut self, id: &DisplayId) -> bool {
        let Some(index) = self.positions.get(id).copied() else {
            return false;
        };
        self.unit_owners.retain(|_, owner| owner != id);
        self.surface_owners.retain(|_, owner| owner != id);
        if let Some(unit) = self.nodes[index].unit() {
            self.unit_owners.insert(unit, id.clone());
        }
        if let Some(seq) = self.nodes[index].surface_seq {
            self.surface_owners.insert(seq, id.clone());
        }
        self.generation = self.generation.wrapping_add(1);
        self.nodes[index].revision = self.generation;
        true
    }

    fn reindex(&mut self) {
        self.positions.clear();
        self.unit_owners.clear();
        self.surface_owners.clear();
        for (index, node) in self.nodes.iter().enumerate() {
            let id = node.id().clone();
            self.positions.insert(id.clone(), index);
            if let Some(unit) = node.unit() {
                self.unit_owners.insert(unit, id.clone());
            }
            if let Some(seq) = node.surface_seq {
                self.surface_owners.insert(seq, id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::{DisplayTone, TranscriptBlock, TranscriptFormat};

    fn block(id: &str, unit: u64) -> DisplayItem {
        DisplayItem::Block(TranscriptBlock {
            id: DisplayId(id.into()),
            unit: Some(unit),
            content: id.into(),
            format: TranscriptFormat::Markdown,
            tone: DisplayTone::Normal,
            copy_source: id.into(),
            streaming: false,
        })
    }

    #[test]
    fn indexes_identity_unit_surface_and_generation() {
        let mut store = TranscriptStore::default();
        store.append(block("a", 1), Some(10));
        store.prepend(block("b", 2), Some(9));
        assert_eq!(store.position(&DisplayId("b".into())), Some(0));
        assert_eq!(store.unit_owner(1), Some(&DisplayId("a".into())));
        assert_eq!(store.surface_owner(9), Some(&DisplayId("b".into())));
        assert_eq!(store.generation(), 2);
    }

    #[test]
    fn node_revision_changes_only_when_that_node_changes() {
        let mut store = TranscriptStore::default();
        store.append(block("a", 1), None);
        let initial = store.get(&DisplayId("a".into())).unwrap().revision();

        store.append(block("b", 2), None);
        assert_eq!(
            store.get(&DisplayId("a".into())).unwrap().revision(),
            initial
        );

        store.touch(&DisplayId("a".into()));
        assert_ne!(
            store.get(&DisplayId("a".into())).unwrap().revision(),
            initial
        );
    }

    #[test]
    fn replacement_and_removal_preserve_consistent_indexes() {
        let mut store = TranscriptStore::default();
        store.append(block("a", 1), Some(1));
        store.append(block("b", 2), Some(2));
        store.replace(&DisplayId("a".into()), block("c", 3), Some(3));
        assert!(store.get(&DisplayId("a".into())).is_none());
        assert_eq!(store.position(&DisplayId("c".into())), Some(0));
        store.remove_indices(&[0]);
        assert_eq!(store.position(&DisplayId("b".into())), Some(0));
        assert!(store.surface_owner(3).is_none());
    }
}
