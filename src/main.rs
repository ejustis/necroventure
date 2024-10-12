use roguelike_tut::engine::*;
use roguelike_tut::engine::conf::load_weighted_tables;
use ui::{main_menu, msgbox};

fn main() {
    let game_settings = GameSettings::new();
 
    let mut tcod = init_tcod(&game_settings);

    match load_weighted_tables() {
        Ok(table) => {
            tcod.tables = Some(table);
        }
        Err(e) => {
            println!("{}", e);
            msgbox("\n No game settings loaded. \n", 24, &mut tcod.root, &game_settings);
        }
    }

    main_menu(&mut tcod, &game_settings);
}