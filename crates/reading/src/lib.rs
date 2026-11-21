//! The reading core without egui (ADR 0021): a Resource opened as one document, the reading
//! view over it (virtualized blocks, heights, scrolling, jumps, the Current location),
//! selection, Annotation Marks, interlinear lines, the layout workers, the Page tone
//! palettes and CPU-drawn page tiles. The desktop's `obs-ui-egui` paints it; the Mobile app
//! shares it.
//!
//! Layer L5: no egui, eframe, winit or rfd (see `docs/dev/reading-core.md`).

pub mod doc;
pub mod heights;
pub mod interlinear;
pub mod marks;
pub mod palette;
pub mod reading_tokens;
pub mod selection;
pub mod tile;
pub mod view;
pub mod workers;

pub use doc::{DocBlock, ReadingDoc};
pub use heights::HeightIndex;
pub use interlinear::InterlinearSource;
pub use marks::{HighlightLooks, Mark, MarkKind, marks_for};
pub use palette::{ContrastShort, StylePaint, TonePalette, contrast};
pub use selection::{
    DocPos, Granularity, Selection, copy_text, range_rects, selected_text, selection_rects, word_at,
};
pub use tile::{Tile, compose_block_tile, compose_tile, text_column};
pub use view::{Frame, FrameBlock, FrameStats, ReadingView, SYNC_BUDGET, VisibleBlock};
pub use workers::{LayoutClient, SnippetCache, lay_out, set_layout_slowdown};
