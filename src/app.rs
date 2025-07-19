//! An app that lets users play and see (update/draw) chess, computed with help from [`rotchess_core`] and macroquad.

use std::{collections::HashMap, f32::consts::TAU, path::Path};

use ggez::{
    Context, GameError, GameResult,
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
    emulator::{self, Event, ThingHappened, TravelKind},
    piece::{PIECE_RADIUS, Piece, Pieces},
};
use sfn_tpn::{Config, NetcodeInterface};
use tokio::sync::oneshot;

use crate::constants::*;

// TODO: pull this out into a sfn_tpn::get_netcode_interface_naive() or such.
async fn get_netcode_interface() -> GameResult<NetcodeInterface<TURN_SIZE>> {
    /// Return whether our process is a client.
    ///
    /// If not, we must be the server.
    ///
    /// Decides based on command line arguments. If no arguments
    /// are supplied, we assume the user wants the process to be
    /// a server.
    fn is_client() -> GameResult<bool> {
        let mut is_client = false;
        let mut is_server = false;
        for arg in std::env::args() {
            if arg == "client" {
                is_client = true;
            }
            if arg == "server" {
                is_server = true;
            }
        }
        if is_client && is_server {
            Err(GameError::CustomError(
                "This process cannot be both the client and the server.".to_string(),
            ))
        } else {
            Ok(is_client)
        }
    }

    /// Gets the first ticket string from the command line arguments.
    fn ticket() -> GameResult<String> {
        for arg in std::env::args() {
            if let Some(("--ticket", t)) = arg.split_once("=") {
                return Ok(t.to_string());
            }
        }

        Err(GameError::CustomError(
            "No ticket provided. Clients must provide a ticket to find a server.".to_string(),
        ))
    }

    if is_client()? {
        Ok(NetcodeInterface::new(Config::Ticket(ticket()?)))
    } else {
        let (send, recv) = oneshot::channel();
        let net = NetcodeInterface::<TURN_SIZE>::new(Config::TicketSender(send));
        println!(
            "hosting game. another player may join with \n\n\
            cargo run client --ticket={}",
            recv.await.unwrap()
        );
        Ok(net)
    }
}

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

#[derive(PartialEq, Eq)]
enum TurnPhase {
    Move,
    Rotate,
    Wait,
}

pub struct App {
    chess: RotchessEmulator,
    runit_to_world_multiplier: f32,
    images: HashMap<ImageID, Image>,
    chess_layout: ChessLayout,
    /// ERM TODO I FORGOR IF THIS IS ROT UNITS OR PX UNITS. DOUBLE CHECK ON ME WHERE IM INSTANTIATED.
    mouse_pos: (f32, f32),
    netcode: NetcodeInterface<TURN_SIZE>,
    turn_phase: TurnPhase,
}

/// Misc utility functions
impl App {
    pub async fn new(ctx: &mut Context) -> GameResult<Self> {
        let mut s = Self {
            chess: RotchessEmulator::with(Pieces::standard_board()),
            runit_to_world_multiplier: 0.,
            images: Self::load_images(ctx),
            chess_layout: ChessLayout::Standard,
            mouse_pos: (0., 0.),
            netcode: get_netcode_interface().await?,
            turn_phase: TurnPhase::Wait,
        };

        s.turn_phase = if s.netcode.my_turn() {
            TurnPhase::Move
        } else {
            TurnPhase::Wait
        };

        s.update_runit_to_world_multiplier(STARTING_WINDOW_SIZE, STARTING_WINDOW_SIZE);

        Ok(s)
    }

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
                Image::from_path(ctx, Path::new("/").join(image_dir.join(path)))
                    .expect("Hardcoded file names/dir should yield a correct path."),
            );
        }

        images
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

/// Netcode related stuff for our app.
impl App {
    /// Sends an event to our inner chess emulator, unless it is not our turn.
    ///
    /// If a thing happened under the hood, send it to the other player.
    /// If we did an illegal turn phase action, revert it.
    fn try_send_event(&mut self, e: Event) {
        if self.netcode.my_turn()
            && let Some(thing_happened) = self.chess.handle_event(e)
        {
            match thing_happened {
                ThingHappened::Move(_, _, _) => {
                    if let TurnPhase::Rotate = self.turn_phase {
                        // disallow move on rotation phase
                        println!(
                            "Player turns consist of a move and a rotation in that order.
                             No moving in your rotation phase!"
                        );
                        self.chess.handle_event(Event::PrevTurn);
                        return;
                    }
                    self.turn_phase = TurnPhase::Rotate;
                }
                // if we rotated, use a little (evil) hack to deselect the piece
                // that we're rotating. I, the dev of rotchess-core, know right button
                // down can only select. so, we send a select click to narnia (-1000,-1000)
                // Nothing should be selectable there, so we deselect.
                ThingHappened::Rotate(_, _) => {
                    if let TurnPhase::Move = self.turn_phase {
                        // disallow rotation on move phase
                        println!(
                            "Player turns consist of a move and a rotation in that order.
                             No rotating in your move phase!"
                        );
                        self.chess.handle_event(Event::PrevTurn);
                        return;
                    }
                    debug_assert!(
                        self.chess
                            .handle_event(Event::ButtonDown {
                                x: -1000.,
                                y: -1000.,
                                button: emulator::MouseButton::RIGHT,
                            })
                            .is_none(),
                        "Nothing should have happened as detectable by the NothingHappened enum.",
                    );
                    self.turn_phase = TurnPhase::Wait;
                }
                _ => (),
            };
            self.netcode
                .send_turn(&Self::ser_thing(Some(&thing_happened)));
        }
    }

    // yes, we're doing these manually. huzzah!

    /// Serialize a Thing into a netcode byte buffer turn.
    fn ser_thing(thing: Option<&ThingHappened>) -> [u8; TURN_SIZE] {
        // we really don't need to have
        // a usize be the piece index, we don't have enough pieces on
        // the board. a single u8 is enough. but for type convenience,
        // we're leaving it as a usize. If someone manages to get
        // more than 256 pieces on the board, that probably violates
        // some invariant somewhere. (aren't pieces supposed to not
        // stack?)
        let mut ans = [0; TURN_SIZE];
        match thing {
            Some(ThingHappened::FirstTurn) => ans[0] = 1,
            Some(ThingHappened::PrevTurn) => ans[0] = 2,
            Some(ThingHappened::NextTurn) => ans[0] = 3,
            Some(ThingHappened::LastTurn) => ans[0] = 4,
            Some(ThingHappened::Rotate(piece_idx, r)) => {
                ans[0] = 5;
                ans[1] = (*piece_idx).try_into().expect("See above");
                ans[2..6].copy_from_slice(&r.to_be_bytes());
            }
            Some(ThingHappened::Move(piece_idx, x, y)) => {
                ans[0] = 6;
                ans[1] = (*piece_idx).try_into().expect("See above");
                ans[2..6].copy_from_slice(&x.to_be_bytes());
                ans[6..10].copy_from_slice(&y.to_be_bytes());
            }
            None => ans[0] = 7,
        }
        ans
    }

    /// Deserialize a Thing from a netcode byte buffer turn.
    fn de_thing(thing: &[u8; TURN_SIZE]) -> Option<ThingHappened> {
        match thing[0] {
            1 => Some(ThingHappened::FirstTurn),
            2 => Some(ThingHappened::PrevTurn),
            3 => Some(ThingHappened::NextTurn),
            4 => Some(ThingHappened::LastTurn),
            5 => {
                let piece_idx = thing[1] as usize;

                let mut r_bytes = [0; size_of::<f32>()];
                r_bytes.copy_from_slice(&thing[2..6]);
                let r = f32::from_be_bytes(r_bytes);

                Some(ThingHappened::Rotate(piece_idx, r))
            }
            6 => {
                let piece_idx = thing[1] as usize;

                let mut x_bytes = [0; size_of::<f32>()];
                x_bytes.copy_from_slice(&thing[2..6]);
                let x = f32::from_be_bytes(x_bytes);

                let mut y_bytes = [0; size_of::<f32>()];
                y_bytes.copy_from_slice(&thing[6..10]);
                let y = f32::from_be_bytes(y_bytes);

                Some(ThingHappened::Move(piece_idx, x, y))
            }
            7 => None,
            _ => panic!("Received malformed data from opponent."),
        }
    }
}

#[cfg(test)]
mod test_serde_thinghappened {
    use super::App;
    use parameterized::parameterized;
    use rotchess_core::emulator::ThingHappened;

    /// .
    ///
    /// Well, ThingHappened doesnt have PartialEq so I guess we're comparing
    /// the byte buffers.
    fn assert_deser_bijective(thing: Option<&ThingHappened>) {
        assert_eq!(
            App::ser_thing(App::de_thing(&App::ser_thing(thing)).as_ref()),
            App::ser_thing(thing)
        )
    }

    #[test]
    fn none_serialization_is_bijective() {
        assert_deser_bijective(None);
    }

    #[test]
    fn firstturn_serialization_is_bijective() {
        assert_deser_bijective(Some(&ThingHappened::FirstTurn));
    }

    #[test]
    fn prevturn_serialization_is_bijective() {
        assert_deser_bijective(Some(&ThingHappened::PrevTurn));
    }

    #[test]
    fn nextturn_serialization_is_bijective() {
        assert_deser_bijective(Some(&ThingHappened::NextTurn));
    }

    #[test]
    fn lastturn_serialization_is_bijective() {
        assert_deser_bijective(Some(&ThingHappened::LastTurn));
    }
    #[parameterized(rotate_thing = {
        &ThingHappened::Rotate(2, 91.246876218913),
        &ThingHappened::Rotate(6, 01.797548620909),
        &ThingHappened::Rotate(5, 08.147878140881),
        &ThingHappened::Rotate(1, 21.581176862643),
        &ThingHappened::Rotate(7, 32.217517844368),
        &ThingHappened::Rotate(4, 90.522625314885),
        &ThingHappened::Rotate(1, 23.927154940674),
        &ThingHappened::Rotate(8, 53.959229741122),
        &ThingHappened::Rotate(8, 60.743439712343),
        &ThingHappened::Rotate(8, 82.152850235763),
    })]
    fn rotate_serialization_is_bijective(rotate_thing: &ThingHappened) {
        assert_deser_bijective(Some(rotate_thing));
    }

    #[parameterized(move_thing = {
        &ThingHappened::Move(28, 11.352279394256, 81.647432982848),
        &ThingHappened::Move(74, 30.000701136234, 90.218648211692),
        &ThingHappened::Move(47, 56.161192888566, 02.448786090013),
        &ThingHappened::Move(86, 54.106803274653, 61.299734032137),
        &ThingHappened::Move(86, 28.528662175474, 48.520872175935),
        &ThingHappened::Move(26, 66.300609468152, 85.435537391159),
        &ThingHappened::Move(77, 73.688001636818, 68.715058900751),
        &ThingHappened::Move(10, 68.328589705709, 11.444493994595),
        &ThingHappened::Move(29, 65.925913814140, 87.078698941045),
        &ThingHappened::Move(85, 38.747317527971, 20.528927188939),
    })]
    fn move_serialization_is_bijective(move_thing: &ThingHappened) {
        assert_deser_bijective(Some(move_thing));
    }
}

/// Helper functions for drawing
impl App {
    fn draw_board(&self, (ctx, canvas): (&mut Context, &mut Canvas)) -> GameResult {
        let mut mb = MeshBuilder::new();
        mb.rectangle(
            DrawMode::fill(),
            Rect::new(0., 0., self.cnv_r(8.), self.cnv_r(8.)),
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
                Rect::new(
                    self.cnv_r(left as f32),
                    self.cnv_r(top as f32),
                    self.cnv_r(1.),
                    self.cnv_r(1.),
                ),
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
            &Mesh::from_triangles(
                ctx,
                &[
                    Vec2::new(x, y - dist),
                    Vec2::new(x - dist / 2. * f32::sqrt(3.), y + dist / 2.),
                    Vec2::new(x + dist / 2. * f32::sqrt(3.), y + dist / 2.),
                ],
                CAPTURE_HIGHLIGHT_COLOR,
            )?,
            DrawParam::new(),
        );
        Ok(())
    }

    fn draw_pieces(
        &self,
        (ctx, canvas): (&mut Context, &mut Canvas),
        show_hitcircles: bool,
    ) -> GameResult {
        let tile_size_px = self.runit_to_world_multiplier; // I did the math.
        const SHRINK: f32 = 0.9;
        for piece in self.chess.pieces() {
            // if (piece.angle() % PI).abs() > 0.001 {
            //     // println!("{}", (piece.angle() % PI).abs());
            //     println!("piece angle is not up or down: {}", piece.angle());
            // }
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
                        x: self.cnv_r(piece.x()),                            // x
                        y: self.cnv_r(piece.y()),                            // y
                        w: tile_size_px / PIECE_PNG_SIZE_PX as f32 * SHRINK, // scale x multiplier
                        h: tile_size_px / PIECE_PNG_SIZE_PX as f32 * SHRINK, // scale y multiplier
                                                                             // again, I did the math.
                    })
                    .offset(Vec2::new(0.5, 0.5))
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
                    self.try_send_event(Event::FirstTurn);
                } else {
                    self.try_send_event(Event::PrevTurn);
                }
            }
            Key::Named(NamedKey::ArrowRight) => {
                if input.mods.shift_key() {
                    self.try_send_event(Event::LastTurn);
                } else {
                    self.try_send_event(Event::NextTurn);
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
        if let Some(button) = match button {
            ggez::winit::event::MouseButton::Left => Some(emulator::MouseButton::LEFT),
            ggez::winit::event::MouseButton::Right => Some(emulator::MouseButton::RIGHT),
            _ => None,
        } {
            let (x, y) = (self.cnv_w(x), self.cnv_w(y));
            self.try_send_event(Event::ButtonDown { x, y, button });
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
        if let Some(button) = match button {
            ggez::winit::event::MouseButton::Left => Some(emulator::MouseButton::LEFT),
            ggez::winit::event::MouseButton::Right => Some(emulator::MouseButton::RIGHT),
            _ => None,
        } {
            let (x, y) = (self.cnv_w(x), self.cnv_w(y));
            self.try_send_event(Event::ButtonUp { x, y, button });
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
        let (x, y) = (self.cnv_w(x), self.cnv_w(y));
        self.mouse_pos = (x, y);
        self.try_send_event(Event::MouseMotion { x, y });
        Ok(())
    }

    fn resize_event(&mut self, _ctx: &mut Context, width: f32, height: f32) -> GameResult {
        self.update_runit_to_world_multiplier(width, height);
        Ok(())
    }

    fn update(&mut self, _ctx: &mut Context) -> GameResult {
        // don't use turn phase for this check, the turn phase can be Wait even though netcode
        // isn't done yet (ie when it's my turn)
        if !self.netcode.my_turn()
            && let Ok(turn) = self.netcode.try_recv_turn()
        {
            match Self::de_thing(&turn) {
                Some(ThingHappened::FirstTurn) => self.chess.handle_event(Event::FirstTurn),
                Some(ThingHappened::PrevTurn) => self.chess.handle_event(Event::PrevTurn),
                Some(ThingHappened::NextTurn) => self.chess.handle_event(Event::NextTurn),
                Some(ThingHappened::LastTurn) => self.chess.handle_event(Event::LastTurn),
                Some(ThingHappened::Rotate(piece_idx, r)) => {
                    assert!(self.turn_phase == TurnPhase::Wait);
                    self.turn_phase = TurnPhase::Move;
                    self.chess
                        .handle_event(Event::RotateUnchecked(piece_idx, r))
                }
                Some(ThingHappened::Move(piece_idx, x, y)) => {
                    assert!(self.turn_phase == TurnPhase::Wait);
                    self.chess
                        .handle_event(Event::MoveUnchecked(piece_idx, x, y));
                    self.netcode.send_turn(&Self::ser_thing(None));
                    None
                }
                None => None,
            };
        }
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
