use ggez::graphics::Color;

/// Generically acceptable tolerance for e.g. [`ggez::graphics::Mesh::new_circle`].
pub const CIRC_TOLERANCE: f32 = 0.1;

pub const DARK_TILE_COLOR: Color = Color::new(0.70980, 0.53333, 0.38824, 1.00000);
pub const LIGHT_TILE_COLOR: Color = Color::new(0.94118, 0.85098, 0.70980, 1.00000);
pub const BACKGROUND_COLOR: Color = Color::new(0.90196, 0.90196, 0.90196, 1.00000);

/// yellowish
pub const SELECTED_PIECE_COLOR: Color = Color::new(1.00000, 1.00000, 0.60000, 0.78431);
/// cyanish
pub const MOVE_OUTLINE_COLOR: Color = Color::new(0.67843, 1.00000, 0.95686, 1.00000);
pub const MOVE_HIGHLIGHT_COLOR: Color = Color::new(0.67843, 1.00000, 0.95686, 0.78431);
/// red
pub const CAPTURE_OUTLINE_COLOR: Color = Color::new(1.00000, 0.00000, 0.00000, 1.00000);
pub const CAPTURE_HIGHLIGHT_COLOR: Color = Color::new(1.00000, 0.00000, 0.00000, 0.78431);
/// springgreen
pub const HITCIRCLE_COLOR: Color = Color::new(0.00000, 1.00000, 0.49804, 1.00000);

/// Size of window in pixels
pub const STARTING_WINDOW_SIZE: f32 = 800.;

/// Source: my eyes at file explorer.
///
/// Yes, it's square.
pub const PIECE_PNG_SIZE_PX: u32 = 200;
