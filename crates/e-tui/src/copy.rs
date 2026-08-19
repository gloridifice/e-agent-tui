//! Semantic Reading copy selection.
//!
//! Clipboard I/O remains in `e-dsh`; this leaf selects complete source from
//! the width-independent Reading Document and never reads rendered cells.

use crate::reading::{BlockId, ReadingCopyPayload, ReadingDocument};

pub fn block_payload<'a>(
    document: &'a ReadingDocument,
    block: &BlockId,
) -> Option<&'a ReadingCopyPayload> {
    document.block(block).map(|block| &block.copy)
}

pub fn block_text(document: &ReadingDocument, block: &BlockId) -> Option<String> {
    block_payload(document, block).map(|payload| payload.text.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        display::DisplayId,
        preview::{PreviewContent, PreviewKey, PreviewRef, PreviewRevision},
        reading::{ReadingBlock, ReadingBlockKind},
    };

    #[test]
    fn semantic_copy_returns_complete_atomic_source() {
        let id = BlockId("code".into());
        let source = "```rust\nfn main() {}\n```";
        let document = ReadingDocument {
            blocks: vec![ReadingBlock {
                id: id.clone(),
                owner: DisplayId("assistant".into()),
                unit: Some(1),
                kind: ReadingBlockKind::Code,
                copy: ReadingCopyPayload {
                    text: source.into(),
                    atomic: true,
                },
                preview: PreviewRef::Inline {
                    key: PreviewKey("code".into()),
                    revision: PreviewRevision(1),
                    content: PreviewContent::Markdown(source.into()),
                },
                items: Vec::new(),
            }],
        };
        assert_eq!(block_text(&document, &id).as_deref(), Some(source));
        assert!(block_payload(&document, &id).unwrap().atomic);
    }
}
