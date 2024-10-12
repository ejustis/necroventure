use tcod::input::Mouse;
use tcod::{colors::*, Map as FovMap, TextAlignment};
use tcod::console::{blit, Offscreen, Root};
use tcod::{BackgroundFlag, Color, Console};

use super::{init_fov, load_game, new_game, play_game, DisplayObj, Game, GameSettings, Tcod, FOV_ALGO, INVENTORY_WIDTH, PLAYER_ID};

pub fn main_menu(tcod: &mut Tcod, game_settings: &GameSettings) {
    let img = tcod::image::Image::from_file("assets/menu_background.png")
        .ok()
        .expect("Background image not found");

    let choices = &["Play new game", "Continue last game", "Quit"];

    while !tcod.root.window_closed() {
        tcod::image::blit_2x(&img, (0,0), (-1, -1), &mut tcod.root, (0, 0));

        tcod.root.set_default_foreground(LIGHTER_GREY);
        tcod.root.print_ex(game_settings.screen_w / 2, game_settings.screen_h / 2 - 4, 
                BackgroundFlag::None, 
                TextAlignment::Center, 
                "Necro-venture: The Beginning"
            );

        tcod.root.print_ex(game_settings.screen_w / 2, game_settings.screen_h -2, 
                BackgroundFlag::None, 
                TextAlignment::Center, 
                "Created By: JustisGames"
            );

        let choice = menu("", choices, 24, &mut tcod.root, game_settings);

        match choice {
            Some(0) => {
                let (mut game, mut objects) = new_game(tcod, *game_settings);

                play_game(tcod, &mut game, &mut objects);
            }
            Some(1) => {
                match load_game() {
                    Ok((mut game, mut objects)) => {
                        init_fov(tcod, &game);
                        play_game(tcod, &mut game, &mut objects);
                    }
                    Err(_e) => {
                        msgbox("\n No saved game to load. \n", 24, &mut tcod.root, &game_settings);
                        continue;
                    }
                }
            }
            Some(2) => {
                break;
            }
            _ => {}
        }
    }
}

fn render_bar(
    panel: &mut Offscreen,
    x: i32,
    y: i32,
    total_width: i32,
    name: &str,
    value: i32,
    maximum: i32,
    bar_color: Color,
    back_color: Color,
) {
    // render a bar (HP, experience, etc). First calculate the width of the bar
    let bar_width = (value as f32 / maximum as f32 * total_width as f32) as i32;

    // render the background first
    panel.set_default_background(back_color);
    panel.rect(x, y, total_width, 1, false, BackgroundFlag::Screen);

    // now render the bar on top
    panel.set_default_background(bar_color);
    if bar_width > 0 {
        panel.rect(x, y, bar_width, 1, false, BackgroundFlag::Screen);
    }

    // finally, some centered text with the values
    panel.set_default_foreground(WHITE);
    panel.print_ex(
        x + total_width / 2,
        y,
        BackgroundFlag::None,
        TextAlignment::Center,
        &format!("{}: {}/{}", name, value, maximum),
    );
}

pub fn render_all(tcod: &mut Tcod, game: &mut Game, objects: &[DisplayObj], fov_recompute: bool){
    if fov_recompute {
        tcod.fov
            .compute_fov(objects[PLAYER_ID].x, objects[PLAYER_ID].y, 
                game.game_settings.torch_radius, 
                game.game_settings.fov_light_walls, 
                FOV_ALGO);
    }

    let mut to_draw: Vec<_> = objects.iter()
        .filter(|o| {
            tcod.fov.is_in_fov(o.x, o.y) 
                || (o.always_visible && game.map[o.x as usize][o.y as usize].explored)
        })
        .collect();
    // sort so that non-blocking objects come first
    to_draw.sort_by(|o1, o2| { o1.blocks.cmp(&o2.blocks) });
    // draw the objects in the list
    for object in &to_draw {
        object.draw(&mut tcod.con);
    }

    for y in 0..game.game_settings.map_h
    {
        for x in 0..game.game_settings.map_w{
            let visible = tcod.fov.is_in_fov(x, y);
            let wall = game.map[x as usize][y as usize].block_sight;
            let color = match (visible, wall) {
                (false, true) => game.game_settings.dark_wall_color,
                (false, false) => game.game_settings.dark_ground_color,
                (true, true) => game.game_settings.light_wall_color,
                (true, false) => game.game_settings.light_ground_color
            };

            let explored = &mut game.map[x as usize][y as usize].explored;
            if visible {
                *explored = true;
            }

            if *explored {
                tcod.con
                    .set_char_background(x, y, color, BackgroundFlag::Set);
            }

        }
    }

    // show the player's stats
    // prepare to render the GUI panel
    tcod.panel.set_default_background(BLACK);
    tcod.panel.clear();

    // display names of objects under the mouse
    tcod.panel.set_default_foreground(LIGHT_GREY);
    tcod.panel.print_ex(
        1,
        0,
        BackgroundFlag::None,
        TextAlignment::Left,
        get_names_under_mouse(tcod.mouse, objects, &tcod.fov),
    );

    // show the player's stats
    let hp = objects[PLAYER_ID].fighter.map_or(0, |f| f.hp);
    let max_hp = objects[PLAYER_ID].max_hp(game);
    render_bar(
        &mut tcod.panel,
        1,
        1,
        game.game_settings.bar_w,
        "HP",
        hp,
        max_hp,
        LIGHT_RED,
        DARKER_RED,
    );

    tcod.panel.print_ex(
        1,
        3,
        BackgroundFlag::None,
        TextAlignment::Left,
        format!("Dungeon level: {}", game.dungeon_level),
    );

    // print the game messages, one line at a time
    let mut y = game.game_settings.msg_h as i32;
    for &(ref msg, color) in game.messages.iter().rev() {
        let msg_height = tcod.panel.get_height_rect(game.game_settings.msg_x, y, game.game_settings.msg_w, 0, msg);
        y -= msg_height;
        if y < 0 {
            break;
        }
        tcod.panel.set_default_foreground(color);
        tcod.panel.print_rect(game.game_settings.msg_x, y, game.game_settings.msg_w, 0, msg);
    }

    // blit the contents of `panel` to the root console
    blit(
        &tcod.panel,
        (0, 0),
        (game.game_settings.screen_w, game.game_settings.panel_h),
        &mut tcod.root,
        (0, game.game_settings.panel_y),
        1.0,
        1.0,
    );

    //Basically a double buffer
    blit(&tcod.con, (0, 0), (game.game_settings.map_w, game.game_settings.map_h), &mut tcod.root, (0, 0), 1.0, 1.0);
}

pub fn inventory_menu(inventory: &[DisplayObj], header: &str, root: &mut Root, game: &Game) -> Option<usize> {
    // how a menu with each item of the inventory as an option
    let options = if inventory.len() == 0 {
        vec!["Inventory is empty.".into()]
    } else {
        inventory
            .iter()
            .map(|item| {
                match item.equipment {
                    Some(equipment) if equipment.equipped => {
                        format!("{} (on {})", item.name, equipment.slot)
                    }
                    _ => item.name.clone(),
                }
            }).collect()
    };

    let inventory_index = menu(header, &options, INVENTORY_WIDTH, root, &game.game_settings);

    // if an item was chosen, return it
    if inventory.len() > 0 {
        inventory_index
    } else {
        None
    }
}

/// return a string with the names of all objects under the mouse
fn get_names_under_mouse(mouse: Mouse, objects: &[DisplayObj], fov_map: &FovMap) -> String {
    let (x, y) = (mouse.cx as i32, mouse.cy as i32);

    // create a list with the names of all objects at the mouse's coordinates and in FOV
    let names = objects
        .iter()
        .filter(|obj| obj.get_pos() == (x, y) && fov_map.is_in_fov(obj.x, obj.y))
        .map(|obj| obj.name.clone())
        .collect::<Vec<_>>();

    names.join(", ") // join the names, separated by commas
}

pub fn msgbox(text: &str, width: i32, root: &mut Root, game_settings: &GameSettings){
    let options : &[&str] = &[];
    menu(text, options, width, root, game_settings);
}

pub fn menu<T: AsRef<str>>(header: &str, options: &[T], width: i32, root: &mut Root, 
        game_settings: &GameSettings) -> Option<usize> {
    assert!(
        options.len() <= 26,
        "Cannot have a menu with more than 26 options."
    );

    // calculate total height for the header (after auto-wrap) and one line per option
    let header_height = if header.is_empty() {
        0
    } else {
        root.get_height_rect(0, 0, width, game_settings.screen_h, header)
    };
    let height = options.len() as i32 + header_height;

    // create an off-screen console that represents the menu's window
    let mut window = Offscreen::new(width, height);

    // print the header, with auto-wrap
    window.set_default_foreground(WHITE);
    window.print_rect_ex(
        0,
        0,
        width,
        height,
        BackgroundFlag::None,
        TextAlignment::Left,
        header,
    );

    // print all the options
    for (index, option_text) in options.iter().enumerate() {
        let menu_letter = (b'a' + index as u8) as char;
        let text = format!("({}) {}", menu_letter, option_text.as_ref());
        window.print_ex(
            0,
            header_height + index as i32,
            BackgroundFlag::None,
            TextAlignment::Left,
            text,
        );
    }

    // blit the contents of "window" to the root console
    let x = game_settings.screen_w / 2 - width / 2;
    let y = game_settings.screen_h / 2 - height / 2;
    blit(&window, (0, 0), (width, height), root, (x, y), 1.0, 0.7);

    // present the root console to the player and wait for a key-press
    root.flush();
    let key = root.wait_for_keypress(true);

    // convert the ASCII code to an index; if it corresponds to an option, return it
    if key.printable.is_alphabetic() {
        let index = key.printable.to_ascii_lowercase() as usize - 'a' as usize;
        if index < options.len() {
            Some(index)
        } else {
            None
        }
    } else {
        None
    }
}