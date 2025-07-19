//! An app that lets users play and see (update/draw) chess, computed with help from [`rotchess_core`] and macroquad.

use std::{collections::HashMap, f32::consts::TAU, path::Path};

use ggez::{
    Context, GameResult,
    event::EventHandler,
    glam::Vec2,
    graphics::{Canvas, Color, DrawMode, DrawParam, Image, Mesh, MeshBuilder, Rect},
    winit::{
        keyboard::{Key, NamedKey},
        platform::modifier_supplement::KeyEventExtModifierSupplement,
    },
};
use rand::seq::SliceRandom;
use rotchess_core::{
    RotchessEmulator,
    emulator::{self, Event, TravelKind},
    piece::{PIECE_RADIUS, Piece, Pieces},
};

use crate::constants::*;

enum ChessLayout {
    Standard,
    Chess960,
}

impl ChessLayout {
    fn get_pieces(&self) -> Pieces {
        match self {
            ChessLayout::Standard => Pieces::standard_board(),
            ChessLayout::Chess960 => Pieces::chess960_board(|| {
                let mut ordering: [usize; 8] = std::array::from_fn(|i| i);
                ordering.shuffle(&mut rand::rng());
                ordering
            }),
        }
    }
}

/// The ID for an image is the file stem from its file path.
///
/// See [`App::load_images`], where they are canonically generated.
type ImageID = String;

pub struct App {
    chess: RotchessEmulator,
    runit_to_world_multiplier: f32,
    images: HashMap<ImageID, Image>,
    chess_layout: ChessLayout,
    mouse_pos: (f32, f32),
}

/// Misc utility functions
impl App {
    pub fn new(ctx: &mut Context) -> Self {
        Self {
            chess: RotchessEmulator::with(Pieces::standard_board()),
            runit_to_world_multiplier: 0.,
            images: Self::load_images(ctx),
            chess_layout: ChessLayout::Standard,
            mouse_pos: (0., 0.),
        }
    }

    fn update_runit_to_world_multiplier(&mut self, screen_width: f32, screen_height: f32) {
        self.runit_to_world_multiplier = f32::min(screen_width, screen_height) / 8.;
    }

    /// Converts from a rotchess unit to world unit (pixel).
    ///
    /// Must be run after we update the ratio after any screen resize, lest the value be outdated.
    fn cnv_r(&self, a: f32) -> f32 {
        a * self.runit_to_world_multiplier
    }

    /// Converts from a world unit (pixel) to rotchess unit.
    ///
    /// Must be run after we update the ratio after any screen resize, lest the value be outdated.
    fn cnv_w(&self, a: f32) -> f32 {
        a / self.runit_to_world_multiplier
    }
}

/// Helper functions for drawing
impl App {
    fn load_images(ctx: &mut Context) -> HashMap<ImageID, Image> {
        const IMAGE_PATHS: [&str; 12] = [
            "piece_bishopB1.png",
            "piece_bishopW1.png",
            "piece_kingB1.png",
            "piece_kingW1.png",
            "piece_knightB1.png",
            "piece_knightW1.png",
            "piece_pawnB1.png",
            "piece_pawnW1.png",
            "piece_queenB1.png",
            "piece_queenW1.png",
            "piece_rookB1.png",
            "piece_rookW1.png",
        ];
        let image_dir = Path::new("pieces_png");

        let mut images = HashMap::new();
        for path in IMAGE_PATHS {
            images.insert(
                Path::new(path)
                    .file_stem()
                    .expect("Hardcoded file stems exist.")
                    .to_str()
                    .expect("Hardcoded utf8 file names should convert to str.")
                    .to_string(),
                Image::from_path(ctx, image_dir.join(path))
                    .expect("Hardcoded file names/dir should yield a correct path."),
            );
        }

        images
    }

    fn draw_board(&self, (ctx, canvas): (&mut Context, &mut Canvas)) -> GameResult {
        let mut mb = MeshBuilder::new();
        mb.rectangle(
            DrawMode::fill(),
            Rect::new_i32(0, 0, 8, 8),
            LIGHT_TILE_COLOR,
        )?;

        let mut top = 0;
        let mut left = 1;
        let mut next_row_immediate_dark = true;

        const NUM_TILES: u8 = 8 * 8;
        const NUM_DARK_TILES: u8 = NUM_TILES / 2;

        for _ in 0..NUM_DARK_TILES {
            mb.rectangle(
                DrawMode::fill(),
                Rect::new_i32(left, top, 1, 1),
                DARK_TILE_COLOR,
            )?;

            left += 2;
            if left >= 8 {
                left = if next_row_immediate_dark { 0 } else { 1 };
                next_row_immediate_dark = !next_row_immediate_dark;
                top += 1;
            }
        }

        // TODO: creating new board mesh every frame.
        let board_mesh = Mesh::from_data(ctx, mb.build());
        canvas.draw(&board_mesh, Vec2::ZERO);

        Ok(())
    }

    fn draw_piece_outline(
        &self,
        (ctx, canvas): (&mut Context, &mut Canvas),
        x: f32,
        y: f32,
        color: Color,
    ) -> GameResult {
        canvas.draw(
            &Mesh::new_circle(
                ctx,
                DrawMode::stroke(1.),
                Vec2::ZERO,
                self.cnv_r(PIECE_RADIUS),
                CIRC_TOLERANCE,
                color,
            )?,
            Vec2::new(self.cnv_r(x), self.cnv_r(y)),
        );
        Ok(())
    }

    fn draw_piece_highlight(
        &self,
        (ctx, canvas): (&mut Context, &mut Canvas),
        x: f32,
        y: f32,
        color: Color,
    ) -> GameResult {
        /// Extra addition to the radius of the drawn circle.
        ///
        /// When highlighting a piece, there will be an outline over it. Without
        /// extra tolerance, there will be background poking in between the highlight
        /// and outline.
        const TOLERANCE: f32 = 0.5;

        canvas.draw(
            &Mesh::new_circle(
                ctx,
                DrawMode::fill(),
                Vec2::ZERO,
                self.cnv_r(PIECE_RADIUS) + TOLERANCE,
                CIRC_TOLERANCE,
                color,
            )?,
            Vec2::new(self.cnv_r(x), self.cnv_r(y)),
        );
        Ok(())
    }

    fn draw_movablepoint_indicator(
        &self,
        (ctx, canvas): (&mut Context, &mut Canvas),
        x: f32,
        y: f32,
    ) -> GameResult {
        canvas.draw(
            &Mesh::new_circle(
                ctx,
                DrawMode::fill(),
                Vec2::ZERO,
                self.cnv_r(0.12),
                CIRC_TOLERANCE,
                MOVE_HIGHLIGHT_COLOR,
            )?,
            Vec2::new(self.cnv_r(x), self.cnv_r(y)),
        );
        Ok(())
    }

    fn draw_capturablepoint_indicator(
        &self,
        (ctx, canvas): (&mut Context, &mut Canvas),
        x: f32,
        y: f32,
    ) -> GameResult {
        let x = self.cnv_r(x);
        let y = self.cnv_r(y);
        let dist = self.cnv_r(0.12);

        canvas.draw(
            // TODO: this func wants the points ordered clockwise, but which way that is depends if we go
            // off math (y up) or our eyes (y down)
            &Mesh::new_polygon(
                ctx,
                DrawMode::fill(),
                &[
                    Vec2::new(0., -dist),
                    Vec2::new(x - dist / 2. * f32::sqrt(3.), dist / 2.),
                    Vec2::new(x + dist / 2. * f32::sqrt(3.), y + dist / 2.),
                ],
                CAPTURE_HIGHLIGHT_COLOR,
            )?,
            Vec2::new(self.cnv_r(x), self.cnv_r(y)),
        );
        Ok(())
    }

    fn draw_pieces(
        &self,
        (ctx, canvas): (&mut Context, &mut Canvas),
        show_hitcircles: bool,
    ) -> GameResult {
        /// Size as fraction of 1.
        const PIECE_SIZE: f32 = 0.9;
        for piece in self.chess.pieces() {
            canvas.draw(
                self.images
                    .get(&format!(
                        "piece_{}{}1",
                        piece.kind().to_file_desc(),
                        piece.side().to_file_desc()
                    ))
                    .expect("Pieces should have correctly mapped to the file descrs."),
                DrawParam::new()
                    .dest_rect(Rect {
                        x: self.cnv_r(piece.x() - PIECE_SIZE / 2.),
                        y: self.cnv_r(piece.y() - PIECE_SIZE / 2.),
                        w: self.cnv_r(piece.x() - PIECE_SIZE / 2.),
                        h: self.cnv_r(piece.y() - PIECE_SIZE / 2.),
                    })
                    // .offset(Vec2::new(0.5, 0.5))
                    .rotation(TAU - piece.angle()),
            );

            if show_hitcircles {
                self.draw_piece_outline((ctx, canvas), piece.x(), piece.y(), HITCIRCLE_COLOR)?;
            }
        }
        Ok(())
    }
}

/// On update events, forward events to the chess emulator. Then, draw.
impl EventHandler for App {
    fn key_down_event(
        &mut self,
        _ctx: &mut Context,
        input: ggez::input::keyboard::KeyInput,
        _repeated: bool,
    ) -> GameResult {
        match input.event.key_without_modifiers() {
            Key::Named(NamedKey::ArrowLeft) => {
                if input.mods.shift_key() {
                    self.chess.handle_event(Event::FirstTurn);
                } else {
                    self.chess.handle_event(Event::PrevTurn);
                }
            }
            Key::Named(NamedKey::ArrowRight) => {
                if input.mods.shift_key() {
                    self.chess.handle_event(Event::LastTurn);
                } else {
                    self.chess.handle_event(Event::NextTurn);
                }
            }
            Key::Character(c) => match c.as_str() {
                "9" => {
                    self.chess_layout = ChessLayout::Chess960;
                    self.chess = RotchessEmulator::with(self.chess_layout.get_pieces());
                }
                "0" => {
                    self.chess_layout = ChessLayout::Standard;
                    self.chess = RotchessEmulator::with(self.chess_layout.get_pieces());
                }
                "r" => {
                    self.chess = RotchessEmulator::with(self.chess_layout.get_pieces());
                }
                _ => (),
            },
            _ => (),
        }

        Ok(())
    }

    fn mouse_button_down_event(
        &mut self,
        _ctx: &mut Context,
        button: ggez::winit::event::MouseButton,
        x: f32,
        y: f32,
    ) -> GameResult {
        let button = match button {
            ggez::winit::event::MouseButton::Left => Some(emulator::MouseButton::LEFT),
            ggez::winit::event::MouseButton::Right => Some(emulator::MouseButton::RIGHT),
            _ => None,
        };
        if let Some(button) = button {
            self.chess.handle_event(Event::ButtonDown { x, y, button });
        }
        Ok(())
    }

    fn mouse_button_up_event(
        &mut self,
        _ctx: &mut Context,
        button: ggez::winit::event::MouseButton,
        x: f32,
        y: f32,
    ) -> GameResult {
        let button = match button {
            ggez::winit::event::MouseButton::Left => Some(emulator::MouseButton::LEFT),
            ggez::winit::event::MouseButton::Right => Some(emulator::MouseButton::RIGHT),
            _ => None,
        };
        if let Some(button) = button {
            self.chess.handle_event(Event::ButtonUp { x, y, button });
        }
        Ok(())
    }

    fn mouse_motion_event(
        &mut self,
        _ctx: &mut Context,
        x: f32,
        y: f32,
        _dx: f32,
        _dy: f32,
    ) -> GameResult {
        self.mouse_pos = (x, y);
        self.chess.handle_event(Event::MouseMotion { x, y });
        Ok(())
    }

    fn resize_event(&mut self, _ctx: &mut Context, width: f32, height: f32) -> GameResult {
        self.update_runit_to_world_multiplier(width, height);
        Ok(())
    }

    fn update(&mut self, _ctx: &mut Context) -> GameResult {
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let mut canvas = Canvas::from_frame(ctx, BACKGROUND_COLOR);

        self.draw_board((ctx, &mut canvas))?;

        let selected = self.chess.selected();

        if let Some((piece, _)) = selected {
            self.draw_piece_highlight(
                (ctx, &mut canvas),
                piece.x(),
                piece.y(),
                SELECTED_PIECE_COLOR,
            )?;
        }

        // egui_macroquad::draw();
        self.draw_pieces((ctx, &mut canvas), selected.is_some())?;

        if let Some((_, travelpoints)) = selected {
            for tp in travelpoints {
                if tp.travelable {
                    let (xpix, ypix) = self.mouse_pos;
                    if Piece::collidepoint_generic(self.cnv_w(xpix), self.cnv_w(ypix), tp.x, tp.y) {
                        self.draw_piece_highlight(
                            (ctx, &mut canvas),
                            tp.x,
                            tp.y,
                            match tp.kind {
                                TravelKind::Capture => CAPTURE_HIGHLIGHT_COLOR,
                                TravelKind::Move => MOVE_HIGHLIGHT_COLOR,
                            },
                        )?;
                    } else {
                        match tp.kind {
                            TravelKind::Capture => {
                                self.draw_capturablepoint_indicator((ctx, &mut canvas), tp.x, tp.y)?
                            }
                            TravelKind::Move => {
                                self.draw_movablepoint_indicator((ctx, &mut canvas), tp.x, tp.y)?
                            }
                        }
                    }
                }
                self.draw_piece_outline(
                    (ctx, &mut canvas),
                    tp.x,
                    tp.y,
                    match tp.kind {
                        TravelKind::Capture => CAPTURE_OUTLINE_COLOR,
                        TravelKind::Move => MOVE_OUTLINE_COLOR,
                    },
                )?;
            }
        }
        canvas.finish(ctx)
    }
}
