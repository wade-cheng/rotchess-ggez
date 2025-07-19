use std::path::PathBuf;

use ggez::{
    GameResult,
    conf::{WindowMode, WindowSetup},
    event,
};
use rotchess_ggez::{app::App, constants::STARTING_WINDOW_SIZE};

#[tokio::main]
pub async fn main() -> GameResult {
    let mut cb = ggez::ContextBuilder::new("super_simple", "ggez")
        .window_mode(
            WindowMode::default()
                .dimensions(STARTING_WINDOW_SIZE, STARTING_WINDOW_SIZE)
                .resizable(true),
        )
        .window_setup(
            WindowSetup::default()
                .title("Rotating Chess")
                .icon("/icon/icon_large.png"),
        );

    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let mut path = PathBuf::from(manifest_dir);
        path.push("resources");
        cb = cb.add_resource_path(path);
    }

    let (mut ctx, event_loop) = cb.build()?;

    let state = App::new(&mut ctx);

    event::run(ctx, event_loop, state)
}
