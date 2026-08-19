//! Width-independent semantic Reading Document and shared-layout geometry.

use std::{
    collections::{HashMap, HashSet},
    ops::Range,
};

use crate::{
    config::Config,
    display::{DisplayId, DisplayItem, TranscriptFormat},
    input::InputState,
    preview::{PreviewContent, PreviewKey, PreviewRef, PreviewRevision},
    projection::TimelineModel,
    render_state::RenderState,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ItemId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadingBlockKind {
    Paragraph,
    Heading,
    Code,
    ListRow,
    Mermaid,
    Table,
    Custom,
    User,
    Reasoning,
    Tool,
    Notice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadingItemKind {
    Link,
    File,
    Path,
    Command,
    Text,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadingCopyPayload {
    pub text: String,
    pub atomic: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadingItem {
    pub id: ItemId,
    pub block_id: BlockId,
    pub kind: ReadingItemKind,
    pub label: String,
    pub preview: Option<PreviewRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadingBlock {
    pub id: BlockId,
    pub owner: DisplayId,
    pub unit: Option<u64>,
    pub kind: ReadingBlockKind,
    pub copy: ReadingCopyPayload,
    pub preview: PreviewRef,
    pub items: Vec<ReadingItem>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReadingDocument {
    pub blocks: Vec<ReadingBlock>,
}

impl ReadingDocument {
    pub fn derive(timeline: &TimelineModel, render: &RenderState, config: &Config) -> Self {
        let mut blocks = Vec::new();
        for node in timeline.transcript.nodes() {
            match &node.item {
                DisplayItem::Block(block)
                    if block.format == TranscriptFormat::Reasoning
                        && !config.thinking_display_mode().shows_reasoning() => {}
                DisplayItem::Block(block) if block.format == TranscriptFormat::Markdown => {
                    let mut seen = HashSet::new();
                    if let Some(lines) = render.markdown_layout.lines(&block.id) {
                        for line in lines {
                            if !seen.insert(line.unit) {
                                continue;
                            }
                            let source = render
                                .units
                                .get(&line.unit)
                                .cloned()
                                .unwrap_or_else(|| block.copy_source.clone());
                            if source.trim().is_empty() {
                                continue;
                            }
                            blocks.push(markdown_block(
                                &block.id,
                                line.unit,
                                source,
                                timeline.transcript.generation(),
                            ));
                        }
                    } else if !block.copy_source.trim().is_empty() {
                        blocks.push(markdown_block(
                            &block.id,
                            block.unit.unwrap_or_default(),
                            block.copy_source.clone(),
                            timeline.transcript.generation(),
                        ));
                    }
                }
                DisplayItem::Block(block) if !block.copy_source.trim().is_empty() => {
                    let kind = if block.format == TranscriptFormat::Reasoning {
                        ReadingBlockKind::Reasoning
                    } else {
                        ReadingBlockKind::Notice
                    };
                    blocks.push(plain_block(
                        block.id.clone(),
                        block.unit,
                        kind,
                        block.copy_source.clone(),
                        timeline,
                    ));
                }
                DisplayItem::Card(card) if !card.copy_source.trim().is_empty() => {
                    blocks.push(plain_block(
                        card.id.clone(),
                        card.unit,
                        if matches!(card.role, crate::display::CardRole::User) {
                            ReadingBlockKind::User
                        } else {
                            ReadingBlockKind::Custom
                        },
                        card.copy_source.clone(),
                        timeline,
                    ));
                }
                DisplayItem::Activity(row) => {
                    blocks.push(tool_block(
                        row.id.clone(),
                        [row.label.clone(), row.summary.clone()]
                            .into_iter()
                            .filter(|part| !part.is_empty())
                            .collect::<Vec<_>>()
                            .join(" "),
                        timeline,
                    ));
                }
                DisplayItem::Composite { activity, detail } => {
                    blocks.push(tool_block(
                        activity.id.clone(),
                        detail.copy_source.clone(),
                        timeline,
                    ));
                }
                _ => {}
            }
        }
        Self { blocks }
    }

    pub fn position(&self, id: &BlockId) -> Option<usize> {
        self.blocks.iter().position(|block| &block.id == id)
    }

    pub fn block(&self, id: &BlockId) -> Option<&ReadingBlock> {
        self.blocks.iter().find(|block| &block.id == id)
    }
}

fn markdown_block(owner: &DisplayId, unit: u64, source: String, revision: u64) -> ReadingBlock {
    let id = BlockId(format!("{}:unit:{unit}", owner.0));
    let kind = markdown_kind(&source);
    let atomic = matches!(
        kind,
        ReadingBlockKind::Code | ReadingBlockKind::Mermaid | ReadingBlockKind::Table
    );
    let items = markdown_links(&id, &source, revision);
    ReadingBlock {
        id,
        owner: owner.clone(),
        unit: Some(unit),
        kind,
        copy: ReadingCopyPayload {
            text: source.clone(),
            atomic,
        },
        preview: PreviewRef::Inline {
            key: PreviewKey(format!("markdown:{}:{unit}", owner.0)),
            revision: PreviewRevision(revision),
            content: PreviewContent::Markdown(source),
        },
        items,
    }
}

fn markdown_kind(source: &str) -> ReadingBlockKind {
    let trimmed = source.trim_start();
    if trimmed.starts_with("```mermaid") {
        ReadingBlockKind::Mermaid
    } else if trimmed.starts_with("```") || trimmed.starts_with("    ") {
        ReadingBlockKind::Code
    } else if trimmed.starts_with('|') && trimmed.lines().count() > 1 {
        ReadingBlockKind::Table
    } else if trimmed.starts_with('#') {
        ReadingBlockKind::Heading
    } else if trimmed.starts_with("- ")
        || trimmed.starts_with("* ")
        || trimmed.starts_with("+ ")
        || trimmed
            .split_once('.')
            .is_some_and(|(prefix, _)| prefix.chars().all(|ch| ch.is_ascii_digit()))
    {
        ReadingBlockKind::ListRow
    } else {
        ReadingBlockKind::Paragraph
    }
}

fn markdown_links(block_id: &BlockId, source: &str, revision: u64) -> Vec<ReadingItem> {
    let mut items = Vec::new();
    let mut search = 0usize;
    while let Some(open) = source[search..].find('[') {
        let open = search + open;
        let Some(close_rel) = source[open + 1..].find("](") else {
            break;
        };
        let close = open + 1 + close_rel;
        let url_start = close + 2;
        let Some(end_rel) = source[url_start..].find(')') else {
            break;
        };
        let end = url_start + end_rel;
        let label = source[open + 1..close].to_owned();
        let url = source[url_start..end].to_owned();
        let index = items.len();
        items.push(ReadingItem {
            id: ItemId(format!("{}:link:{index}", block_id.0)),
            block_id: block_id.clone(),
            kind: ReadingItemKind::Link,
            label: label.clone(),
            preview: Some(PreviewRef::Inline {
                key: PreviewKey(format!("link:{}:{index}", block_id.0)),
                revision: PreviewRevision(revision),
                content: PreviewContent::Link {
                    label: Some(label),
                    url,
                },
            }),
        });
        search = end + 1;
    }
    items
}

fn plain_block(
    owner: DisplayId,
    unit: Option<u64>,
    kind: ReadingBlockKind,
    source: String,
    timeline: &TimelineModel,
) -> ReadingBlock {
    let id = BlockId(match unit {
        Some(unit) => format!("{}:unit:{unit}", owner.0),
        None => format!("{}:block", owner.0),
    });
    let preview = timeline
        .preview_refs
        .get(&owner)
        .cloned()
        .unwrap_or_else(|| PreviewRef::Inline {
            key: PreviewKey(format!("source:{}", id.0)),
            revision: PreviewRevision(timeline.transcript.generation()),
            content: PreviewContent::PlainText(source.clone()),
        });
    ReadingBlock {
        id,
        owner,
        unit,
        kind,
        copy: ReadingCopyPayload {
            text: source,
            atomic: false,
        },
        preview,
        items: Vec::new(),
    }
}

fn tool_block(owner: DisplayId, source: String, timeline: &TimelineModel) -> ReadingBlock {
    let id = BlockId(format!("{}:tool", owner.0));
    let revision = PreviewRevision(timeline.transcript.generation());
    let items = timeline
        .tool_items
        .get(&owner)
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(index, item)| ReadingItem {
            id: ItemId(format!("{}:tool-item:{}", id.0, item.id)),
            block_id: id.clone(),
            kind: match item.reference {
                crate::agent::tool::ToolReference::Path { .. }
                | crate::agent::tool::ToolReference::Lines { .. } => ReadingItemKind::File,
                crate::agent::tool::ToolReference::Link { .. } => ReadingItemKind::Link,
                crate::agent::tool::ToolReference::Command { .. } => ReadingItemKind::Command,
                crate::agent::tool::ToolReference::Custom { .. } => ReadingItemKind::Custom,
                _ => ReadingItemKind::Text,
            },
            label: item.label.clone(),
            preview: item
                .reference
                .preview_reference(&format!("tool-item:{}:{index}", id.0), revision),
        })
        .collect();
    let preview = timeline
        .preview_refs
        .get(&owner)
        .cloned()
        .unwrap_or_else(|| PreviewRef::Inline {
            key: PreviewKey(format!("tool:{}", owner.0)),
            revision,
            content: PreviewContent::PlainText(source.clone()),
        });
    ReadingBlock {
        id,
        owner,
        unit: None,
        kind: ReadingBlockKind::Tool,
        copy: ReadingCopyPayload {
            text: source,
            atomic: false,
        },
        preview,
        items,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadingItemFragment {
    pub item_id: ItemId,
    pub row: usize,
    pub x: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadingBlockLayout {
    pub block_id: BlockId,
    pub rows: Range<usize>,
    pub gutter_x: usize,
    pub items: Vec<ReadingItemFragment>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReadingLayout {
    pub width: usize,
    pub blocks: Vec<ReadingBlockLayout>,
}

impl ReadingLayout {
    /// Derive semantic geometry from the exact transcript cache ranges and
    /// wrapped-row prefix used by rendering; no second wrapper is introduced.
    pub fn derive(
        timeline: &TimelineModel,
        render: &RenderState,
        document: &ReadingDocument,
    ) -> Self {
        let cache = &render.transcript_cache;
        let mut owner_positions = HashMap::new();
        for (index, node) in timeline.transcript.nodes().iter().enumerate() {
            owner_positions.insert(node.id().clone(), index);
        }
        let display_row = |base: usize| cache.layout.prefix.get(base).copied().unwrap_or(base);
        let mut blocks = Vec::new();
        for block in &document.blocks {
            let Some(index) = owner_positions.get(&block.owner).copied() else {
                continue;
            };
            let Some(message_range) = cache.message_ranges.get(index).copied().flatten() else {
                continue;
            };
            let base = if let (Some(unit), Some(lines)) =
                (block.unit, render.markdown_layout.lines(&block.owner))
            {
                let mut first = None;
                let mut last = None;
                for (offset, line) in lines.iter().enumerate() {
                    if line.unit == unit {
                        first.get_or_insert(offset);
                        last = Some(offset + 1);
                    }
                }
                match (first, last) {
                    (Some(first), Some(last)) => {
                        (message_range.start + first)..(message_range.start + last)
                    }
                    _ => message_range.start..message_range.end,
                }
            } else {
                message_range.start..message_range.end
            };
            let rows = display_row(base.start)..display_row(base.end);
            if rows.is_empty() {
                continue;
            }
            let item_count = block.items.len().max(1);
            let segment = cache.width.max(1).div_ceil(item_count);
            let items = block
                .items
                .iter()
                .enumerate()
                .flat_map(|(index, item)| {
                    let start = (index * segment).min(cache.width.saturating_sub(1));
                    let end = ((index + 1) * segment)
                        .min(cache.width.max(1))
                        .max(start + 1);
                    rows.clone().map(move |row| ReadingItemFragment {
                        item_id: item.id.clone(),
                        row,
                        x: start..end,
                    })
                })
                .collect();
            blocks.push(ReadingBlockLayout {
                block_id: block.id.clone(),
                rows,
                gutter_x: 0,
                items,
            });
        }
        Self {
            width: cache.width,
            blocks,
        }
    }

    pub fn block(&self, id: &BlockId) -> Option<&ReadingBlockLayout> {
        self.blocks.iter().find(|block| &block.block_id == id)
    }
}

fn item_anchor(layout: &ReadingBlockLayout, item: &ItemId) -> Option<(usize, usize)> {
    layout
        .items
        .iter()
        .filter(|fragment| &fragment.item_id == item)
        .map(|fragment| (fragment.row, fragment.x.start + fragment.x.len() / 2))
        .min()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadingDirection {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone)]
pub struct ReadingViewState {
    pub block_cursor: BlockId,
    pub item_cursor: Option<ItemId>,
    pub retained_x: Option<usize>,
    pub saved_input: InputState,
    preferred_index: usize,
}

impl ReadingViewState {
    pub fn enter(
        document: &ReadingDocument,
        layout: &ReadingLayout,
        viewport: Range<usize>,
        input: &InputState,
    ) -> Option<Self> {
        let center = viewport.start.saturating_add(viewport.len() / 2);
        let (preferred_index, block) =
            document
                .blocks
                .iter()
                .enumerate()
                .min_by_key(|(_, block)| {
                    layout
                        .block(&block.id)
                        .map(|geometry| {
                            let block_center = geometry.rows.start + geometry.rows.len() / 2;
                            block_center.abs_diff(center)
                        })
                        .unwrap_or(usize::MAX)
                })?;
        Some(Self {
            block_cursor: block.id.clone(),
            item_cursor: None,
            retained_x: None,
            saved_input: input.clone(),
            preferred_index,
        })
    }

    pub fn current<'a>(&self, document: &'a ReadingDocument) -> Option<&'a ReadingBlock> {
        document.block(&self.block_cursor)
    }

    pub fn enter_items(&mut self, document: &ReadingDocument, layout: &ReadingLayout) -> bool {
        let Some(block) = self.current(document) else {
            return false;
        };
        let Some(geometry) = layout.block(&block.id) else {
            return false;
        };
        let mut items = block.items.iter().collect::<Vec<_>>();
        items.sort_by_key(|item| {
            geometry
                .items
                .iter()
                .filter(|fragment| fragment.item_id == item.id)
                .map(|fragment| (fragment.row, fragment.x.start))
                .min()
                .unwrap_or((usize::MAX, usize::MAX))
        });
        let selected = if let Some(anchor) = self.retained_x {
            items.into_iter().min_by_key(|item| {
                item_anchor(geometry, &item.id)
                    .map(|(_, x)| x.abs_diff(anchor))
                    .unwrap_or(usize::MAX)
            })
        } else {
            items.into_iter().next()
        };
        let Some(item) = selected else { return false };
        self.item_cursor = Some(item.id.clone());
        self.retained_x = item_anchor(geometry, &item.id).map(|(_, x)| x);
        true
    }

    pub fn current_item<'a>(&self, document: &'a ReadingDocument) -> Option<&'a ReadingItem> {
        let id = self.item_cursor.as_ref()?;
        self.current(document)?
            .items
            .iter()
            .find(|item| &item.id == id)
    }

    pub fn leave_items(&mut self) -> bool {
        self.item_cursor.take().is_some()
    }

    pub fn move_item(
        &mut self,
        direction: ReadingDirection,
        document: &ReadingDocument,
        layout: &ReadingLayout,
    ) -> bool {
        let Some(current_item) = self.current_item(document).map(|item| item.id.clone()) else {
            return false;
        };
        let Some(block) = self.current(document) else {
            return false;
        };
        let Some(geometry) = layout.block(&block.id) else {
            return false;
        };
        let Some((current_row, current_x)) = item_anchor(geometry, &current_item) else {
            return false;
        };
        self.retained_x = Some(current_x);

        let horizontal = matches!(direction, ReadingDirection::Left | ReadingDirection::Right);
        if horizontal {
            let mut candidates = block
                .items
                .iter()
                .filter(|item| item.id != current_item)
                .filter_map(|item| {
                    let (row, x) = item_anchor(geometry, &item.id)?;
                    let primary = match direction {
                        ReadingDirection::Left if x < current_x => current_x - x,
                        ReadingDirection::Right if x > current_x => x - current_x,
                        _ => return None,
                    };
                    Some((primary, row.abs_diff(current_row), row, x, item.id.clone()))
                })
                .collect::<Vec<_>>();
            candidates
                .sort_by_key(|candidate| (candidate.0, candidate.1, candidate.2, candidate.3));
            if let Some((_, _, _, x, id)) = candidates.into_iter().next() {
                self.item_cursor = Some(id);
                self.retained_x = Some(x);
                return true;
            }
            if direction == ReadingDirection::Left {
                self.item_cursor = None;
                return true;
            }
            return false;
        }

        let mut vertical = block
            .items
            .iter()
            .filter(|item| item.id != current_item)
            .filter_map(|item| {
                let (row, x) = item_anchor(geometry, &item.id)?;
                let primary = match direction {
                    ReadingDirection::Up if row < current_row => current_row - row,
                    ReadingDirection::Down if row > current_row => row - current_row,
                    _ => return None,
                };
                Some((primary, x.abs_diff(current_x), row, x, item.id.clone()))
            })
            .collect::<Vec<_>>();
        vertical.sort_by_key(|candidate| (candidate.0, candidate.1, candidate.2, candidate.3));
        if let Some((_, _, _, x, id)) = vertical.into_iter().next() {
            self.item_cursor = Some(id);
            self.retained_x = Some(x);
            return true;
        }

        let current_block = document.position(&self.block_cursor).unwrap_or_default();
        let delta = if direction == ReadingDirection::Up {
            -1
        } else {
            1
        };
        let adjacent = current_block.saturating_add_signed(delta);
        if adjacent >= document.blocks.len() || adjacent == current_block {
            return false;
        }
        let next_block = &document.blocks[adjacent];
        self.block_cursor = next_block.id.clone();
        self.preferred_index = adjacent;
        let Some(next_geometry) = layout.block(&next_block.id) else {
            self.item_cursor = None;
            return true;
        };
        let anchor = self.retained_x.unwrap_or(current_x);
        self.item_cursor = next_block
            .items
            .iter()
            .filter_map(|item| {
                item_anchor(next_geometry, &item.id)
                    .map(|(_, x)| (x.abs_diff(anchor), x, item.id.clone()))
            })
            .min_by_key(|candidate| (candidate.0, candidate.1))
            .map(|(_, x, id)| {
                self.retained_x = Some(x);
                id
            });
        true
    }

    pub fn move_block(&mut self, document: &ReadingDocument, delta: isize) -> bool {
        let Some(current) = document.position(&self.block_cursor) else {
            return false;
        };
        let next = current
            .saturating_add_signed(delta)
            .min(document.blocks.len().saturating_sub(1));
        if next == current {
            return false;
        }
        self.preferred_index = next;
        self.block_cursor = document.blocks[next].id.clone();
        self.item_cursor = None;
        true
    }

    pub fn reconcile(&mut self, document: &ReadingDocument) -> bool {
        if document.block(&self.block_cursor).is_some() {
            if self.item_cursor.as_ref().is_some_and(|item| {
                !self
                    .current(document)
                    .is_some_and(|block| block.items.iter().any(|candidate| &candidate.id == item))
            }) {
                self.item_cursor = None;
            }
            return true;
        }
        let Some(block) = document.blocks.get(
            self.preferred_index
                .min(document.blocks.len().saturating_sub(1)),
        ) else {
            return false;
        };
        self.block_cursor = block.id.clone();
        self.item_cursor = None;
        true
    }

    pub fn copy_payload<'a>(
        &self,
        document: &'a ReadingDocument,
    ) -> Option<&'a ReadingCopyPayload> {
        self.current(document).map(|block| &block.copy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        agent::tool::{ToolItem, ToolReference},
        app::TuiApp,
        display::{ActivityRow, ContentCard, DisplayTone, TranscriptBlock},
        input::InputState,
        theme::Theme,
        ui::{render, RenderOverlays, ScrollState},
    };
    use ratatui::{backend::TestBackend, Terminal};

    fn document(app: &TuiApp) -> ReadingDocument {
        ReadingDocument::derive(&app.timeline, &app.render, &app.config)
    }

    fn reading_layout(app: &TuiApp, document: &ReadingDocument) -> ReadingLayout {
        ReadingLayout::derive(&app.timeline, &app.render, document)
    }

    fn materialize(app: &mut TuiApp, width: u16) {
        let input = InputState::new(&app.config);
        let mut scroll = ScrollState::default();
        let theme = app.theme();
        let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    app,
                    &input,
                    &mut scroll,
                    &theme,
                    RenderOverlays {
                        help_visible: false,
                        toast: None,
                        input_page: None,
                        settings: None,
                        login: None,
                    },
                );
            })
            .unwrap();
    }

    #[test]
    fn ids_and_copy_payload_survive_width_independent_derivation() {
        let mut app = TuiApp::default();
        app.config.resolved_theme = Theme::ferra();
        app.timeline.transcript.append(
            DisplayItem::Card(ContentCard {
                id: DisplayId::correlated("user", "1"),
                unit: Some(7),
                header: None,
                content: "hello".into(),
                role: crate::display::CardRole::User,
                tone: DisplayTone::Normal,
                horizontal_padding: 2,
                copy_source: "hello".into(),
            }),
            None,
        );
        let first = document(&app);
        app.render.transcript_cache.width = 40;
        let second = document(&app);
        assert_eq!(first.blocks[0].id, second.blocks[0].id);
        assert_eq!(first.blocks[0].copy.text, "hello");
    }

    #[test]
    fn markdown_links_are_items_owned_by_one_block() {
        let id = DisplayId::correlated("assistant", "1");
        let block = markdown_block(&id, 4, "see [docs](https://example.com)".into(), 1);
        assert_eq!(block.items.len(), 1);
        assert_eq!(block.items[0].block_id, block.id);
        assert_eq!(block.items[0].kind, ReadingItemKind::Link);
    }

    #[test]
    fn markdown_units_cover_semantic_kinds_and_share_render_geometry() {
        let mut app = TuiApp::default();
        let source = "paragraph [very long documentation link that wraps across terminal rows and remains one item](https://example.com)\n\n- item\n\n```rust\nfn main() {}\n```\n\n```mermaid\ngraph TD; A-->B\n```\n\n| A | B |\n|---|---|\n| 1 | 2 |";
        app.timeline.transcript.append(
            DisplayItem::Block(TranscriptBlock {
                id: DisplayId::correlated("assistant", "semantic"),
                unit: None,
                content: source.into(),
                format: TranscriptFormat::Markdown,
                tone: DisplayTone::Normal,
                copy_source: source.into(),
                streaming: false,
            }),
            None,
        );
        materialize(&mut app, 70);
        let document = document(&app);
        let kinds = document
            .blocks
            .iter()
            .map(|block| block.kind)
            .collect::<Vec<_>>();
        assert!(kinds.contains(&ReadingBlockKind::Paragraph));
        assert!(kinds.contains(&ReadingBlockKind::ListRow));
        assert!(kinds.contains(&ReadingBlockKind::Code));
        assert!(kinds.contains(&ReadingBlockKind::Mermaid));
        assert!(kinds.contains(&ReadingBlockKind::Table));
        assert!(document.blocks.iter().any(|block| !block.items.is_empty()));
        let layout = reading_layout(&app, &document);
        assert_eq!(layout.blocks.len(), document.blocks.len());
        assert!(layout.blocks.iter().all(|block| !block.rows.is_empty()));
        assert!(layout
            .blocks
            .iter()
            .flat_map(|block| &block.items)
            .all(|item| !item.x.is_empty()));
        let mut fragments = HashMap::<ItemId, usize>::new();
        for fragment in layout.blocks.iter().flat_map(|block| &block.items) {
            *fragments.entry(fragment.item_id.clone()).or_default() += 1;
        }
        assert!(fragments.values().any(|count| *count > 1));
    }

    #[test]
    fn one_tool_lifecycle_owns_adapter_items_and_preview() {
        let mut app = TuiApp::default();
        let id = DisplayId::correlated("tool-call", "read-1");
        let mut row = ActivityRow::root(id.clone(), "read");
        row.summary = "src/lib.rs".into();
        app.timeline
            .transcript
            .append(DisplayItem::Activity(row), None);
        app.timeline.tool_items.insert(
            id.clone(),
            vec![ToolItem {
                id: "path".into(),
                label: "src/lib.rs".into(),
                reference: ToolReference::Lines {
                    path: "src/lib.rs".into(),
                    start: 1,
                    lines: vec!["fn main() {}".into()],
                },
            }],
        );
        materialize(&mut app, 70);
        let document = document(&app);
        assert_eq!(document.blocks.len(), 1);
        assert_eq!(document.blocks[0].kind, ReadingBlockKind::Tool);
        assert_eq!(document.blocks[0].items.len(), 1);
        assert_eq!(document.blocks[0].items[0].block_id, document.blocks[0].id);
        assert!(document.blocks[0].items[0].preview.is_some());
    }

    #[test]
    fn surface_replacement_and_tool_settlement_reconcile_by_canonical_owner() {
        let mut app = TuiApp::default();
        let old = DisplayId::correlated("tool-call", "1");
        app.timeline.transcript.append(
            DisplayItem::Activity(ActivityRow::root(old.clone(), "read")),
            None,
        );
        materialize(&mut app, 70);
        let first = document(&app);
        let stable = first.blocks[0].id.clone();
        if let Some(node) = app.timeline.transcript.get_mut(&old) {
            if let DisplayItem::Activity(row) = &mut node.item {
                row.state = crate::display::ActivityState::Success;
            }
        }
        assert_eq!(document(&app).blocks[0].id, stable);

        app.timeline.transcript.remove(0);
        let replacement = DisplayId::correlated("assistant", "summary");
        app.timeline.transcript.append(
            DisplayItem::Block(TranscriptBlock {
                id: replacement.clone(),
                unit: Some(8),
                content: "summary".into(),
                format: TranscriptFormat::Plain,
                tone: DisplayTone::Normal,
                copy_source: "summary".into(),
                streaming: false,
            }),
            None,
        );
        let after = document(&app);
        assert!(!after.blocks.iter().any(|block| block.owner == old));
        assert!(after.blocks.iter().any(|block| block.owner == replacement));
    }

    #[test]
    fn spatial_item_navigation_clamps_boundaries_and_preserves_ownership() {
        let preview = |key: &str| PreviewRef::Inline {
            key: PreviewKey(key.into()),
            revision: PreviewRevision(1),
            content: PreviewContent::PlainText(key.into()),
        };
        let make_item = |block: &BlockId, name: &str| ReadingItem {
            id: ItemId(format!("{}:{name}", block.0)),
            block_id: block.clone(),
            kind: ReadingItemKind::Link,
            label: name.into(),
            preview: Some(preview(name)),
        };
        let b1 = BlockId("b1".into());
        let b2 = BlockId("b2".into());
        let b3 = BlockId("b3".into());
        let document = ReadingDocument {
            blocks: vec![
                ReadingBlock {
                    id: b1.clone(),
                    owner: DisplayId("o1".into()),
                    unit: None,
                    kind: ReadingBlockKind::Paragraph,
                    copy: ReadingCopyPayload {
                        text: "one".into(),
                        atomic: false,
                    },
                    preview: preview("b1"),
                    items: vec![make_item(&b1, "a"), make_item(&b1, "b")],
                },
                ReadingBlock {
                    id: b2.clone(),
                    owner: DisplayId("o2".into()),
                    unit: None,
                    kind: ReadingBlockKind::Paragraph,
                    copy: ReadingCopyPayload {
                        text: "two".into(),
                        atomic: false,
                    },
                    preview: preview("b2"),
                    items: Vec::new(),
                },
                ReadingBlock {
                    id: b3.clone(),
                    owner: DisplayId("o3".into()),
                    unit: None,
                    kind: ReadingBlockKind::Paragraph,
                    copy: ReadingCopyPayload {
                        text: "three".into(),
                        atomic: false,
                    },
                    preview: preview("b3"),
                    items: vec![make_item(&b3, "c")],
                },
            ],
        };
        let layout = ReadingLayout {
            width: 30,
            blocks: vec![
                ReadingBlockLayout {
                    block_id: b1.clone(),
                    rows: 0..2,
                    gutter_x: 0,
                    items: vec![
                        ReadingItemFragment {
                            item_id: ItemId("b1:a".into()),
                            row: 0,
                            x: 0..5,
                        },
                        ReadingItemFragment {
                            item_id: ItemId("b1:b".into()),
                            row: 0,
                            x: 10..15,
                        },
                    ],
                },
                ReadingBlockLayout {
                    block_id: b2.clone(),
                    rows: 2..4,
                    gutter_x: 0,
                    items: Vec::new(),
                },
                ReadingBlockLayout {
                    block_id: b3.clone(),
                    rows: 4..6,
                    gutter_x: 0,
                    items: vec![ReadingItemFragment {
                        item_id: ItemId("b3:c".into()),
                        row: 4,
                        x: 9..14,
                    }],
                },
            ],
        };
        let input = InputState::new(&Config::default());
        let mut reading = ReadingViewState::enter(&document, &layout, 0..2, &input).unwrap();
        assert!(reading.enter_items(&document, &layout));
        assert_eq!(reading.item_cursor, Some(ItemId("b1:a".into())));
        assert!(reading.move_item(ReadingDirection::Right, &document, &layout));
        assert_eq!(reading.item_cursor, Some(ItemId("b1:b".into())));
        assert!(!reading.move_item(ReadingDirection::Right, &document, &layout));
        assert!(reading.move_item(ReadingDirection::Left, &document, &layout));
        assert!(reading.move_item(ReadingDirection::Left, &document, &layout));
        assert!(reading.item_cursor.is_none());
        reading.enter_items(&document, &layout);
        assert!(reading.move_item(ReadingDirection::Down, &document, &layout));
        assert_eq!(reading.block_cursor, b2);
        assert!(reading.item_cursor.is_none());
        reading.move_block(&document, 1);
        reading.enter_items(&document, &layout);
        assert_eq!(reading.block_cursor, b3);
        assert_eq!(reading.item_cursor, Some(ItemId("b3:c".into())));
        assert_eq!(reading.copy_payload(&document).unwrap().text, "three");
    }

    #[test]
    fn ids_survive_resize_streaming_and_history_prepend_while_hidden_reasoning_is_excluded() {
        let mut app = TuiApp::default();
        let id = DisplayId::correlated("assistant", "stable");
        app.timeline.transcript.append(
            DisplayItem::Block(TranscriptBlock {
                id: id.clone(),
                unit: None,
                content: "streaming paragraph".into(),
                format: TranscriptFormat::Markdown,
                tone: DisplayTone::Normal,
                copy_source: "streaming paragraph".into(),
                streaming: true,
            }),
            None,
        );
        app.timeline.transcript.append(
            DisplayItem::Block(TranscriptBlock {
                id: DisplayId::correlated("reasoning", "hidden"),
                unit: None,
                content: "private thought".into(),
                format: TranscriptFormat::Reasoning,
                tone: DisplayTone::Dim,
                copy_source: "private thought".into(),
                streaming: false,
            }),
            None,
        );
        materialize(&mut app, 70);
        let first = document(&app);
        assert_eq!(first.blocks.len(), 1);
        let stable = first.blocks[0].id.clone();
        materialize(&mut app, 60);
        assert_eq!(document(&app).blocks[0].id, stable);
        if let Some(node) = app.timeline.transcript.get_mut(&id) {
            if let DisplayItem::Block(block) = &mut node.item {
                block.content.push_str(" grows");
                block.copy_source.push_str(" grows");
            }
        }
        app.timeline.transcript.touch(&id);
        app.render.transcript_cache.mark_tail_dirty();
        materialize(&mut app, 60);
        assert_eq!(document(&app).blocks[0].id, stable);
        app.timeline.transcript.prepend(
            DisplayItem::Card(ContentCard {
                id: DisplayId::correlated("user", "older"),
                unit: Some(99),
                header: None,
                content: "older".into(),
                role: crate::display::CardRole::User,
                tone: DisplayTone::Normal,
                horizontal_padding: 2,
                copy_source: "older".into(),
            }),
            None,
        );
        materialize(&mut app, 60);
        let after = document(&app);
        assert!(after.blocks.iter().any(|block| block.id == stable));
    }
}
