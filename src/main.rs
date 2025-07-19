use ggez::{
    GameResult,
    conf::{WindowMode, WindowSetup},
    event,
};
use rotchess_ggez::app::App;

#[tokio::main]
pub async fn main() -> GameResult {
    const BOARD_PX: f32 = 100.;
    let cb = ggez::ContextBuilder::new("super_simple", "ggez")
        .window_mode(WindowMode::default().dimensions(BOARD_PX, BOARD_PX))
        .window_setup(
            WindowSetup::default()
                .title("movable pieces on board")
                .icon("icon/icon_large.png"),
        );

    let (mut ctx, event_loop) = cb.build()?;

    let state = App::new().await?;

    event::run(ctx, event_loop, state)
}
