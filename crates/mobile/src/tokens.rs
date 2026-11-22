//! The Mobile app's own design tokens (ADR 0015, plan A5): the few sizes its core decides that
//! the reading core's tokens (`obs_reading::reading_tokens`) do not. The design-token lint
//! (`design_tokens_tests.rs` in `obs-ui-egui`) scans this crate and fails on a size literal
//! anywhere else in it.

/// The Text size, in points, for each of UIKit's content size categories (Dynamic Type,
/// `UIContentSizeCategory` raw values), smallest first, within the app's 12–40 pt: Large is the
/// desktop's default, 20 pt. Android's font scale can use the same steps later (plan A4).
pub const DYNAMIC_TYPE: [(&str, f32); 12] = [
    ("UICTContentSizeCategoryXS", 15.0),
    ("UICTContentSizeCategoryS", 17.0),
    ("UICTContentSizeCategoryM", 18.0),
    ("UICTContentSizeCategoryL", 20.0),
    ("UICTContentSizeCategoryXL", 22.0),
    ("UICTContentSizeCategoryXXL", 24.0),
    ("UICTContentSizeCategoryXXXL", 26.0),
    ("UICTContentSizeCategoryAccessibilityM", 29.0),
    ("UICTContentSizeCategoryAccessibilityL", 32.0),
    ("UICTContentSizeCategoryAccessibilityXL", 35.0),
    ("UICTContentSizeCategoryAccessibilityXXL", 38.0),
    ("UICTContentSizeCategoryAccessibilityXXXL", 40.0),
];

/// The width of the text input's caret, in points.
pub const CARET_WIDTH: f32 = 2.0;
