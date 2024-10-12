pub mod conf;
pub mod ui;

use tcod::colors::*;
use tcod::console::*;
use tcod::input;
use tcod::input::Event;
use tcod::map::{FovAlgorithm, Map as FovMap};
use tcod::input::{Key, Mouse};
use ui::inventory_menu;
use ui::menu;
use ui::msgbox;
use ui::render_all;
use std::cmp;
use std::error::Error;
use std::fs::File;
use std::io::{Read, Write};
use rand::Rng;
use serde::{Deserialize, Serialize};
use conf::*;
use rand::distributions::{Distribution, WeightedIndex};

pub const PLAYER_ID: usize = 0;
const MAX_INV_SPACE: usize = 26;
const INVENTORY_WIDTH: i32 = 50;

const FOV_ALGO: FovAlgorithm = FovAlgorithm::Shadow;

const HEAL_AMOUNT: i32 = 12;
const LIGHTNING_DAMAGE: i32 = 40;
const LIGHTNING_RANGE: i32 = 5;
const CONFUSE_RANGE: i32 = 8;
const CONFUSE_NUM_TURNS: i32 = 10;
const FIREBALL_DAMAGE: i32 = 24;
const FIREBALL_RADIUS: i32 = 3;

// experience and level-ups
const LEVEL_UP_BASE: u32 = 50;
const LEVEL_UP_FACTOR: u32 = 150;
const LEVEL_SCREEN_WIDTH: i32 = 40;
const CHARACTER_SCREEN_WIDTH: i32 = 30;

pub struct Tcod {
    pub root: Root,
    pub con: Offscreen,
    pub panel: Offscreen,
    pub fov: FovMap,
    pub key: Key,
    pub mouse: Mouse,

    pub tables: Option<TransitionTables>
}

pub fn init_tcod(game_settings: &GameSettings) -> Tcod {
    let root = Root::initializer()
        .font("assets/prestige12x12.png", FontLayout::Tcod)
        .font_type(FontType::Greyscale)
        .size(game_settings.screen_w, game_settings.screen_h)
        .title("Tcod Tutorial")
        .init();

    let con = Offscreen::new(game_settings.map_w, game_settings.map_h);
    let panel = Offscreen::new(game_settings.screen_w, game_settings.screen_h);
    let fov = FovMap::new(game_settings.map_w, game_settings.map_h);

    let tcod = Tcod { root: root, 
        con: con, panel: panel, fov: fov, key: Default::default(), mouse: Default::default(), tables: None };

    tcod::system::set_fps(game_settings.fps_limit);

    tcod
}

fn init_fov(tcod: &mut Tcod, game: &Game){
    // populate the FOV map, according to the generated map
    for y in 0..game.game_settings.map_h {
        for x in 0..game.game_settings.map_w {
            tcod.fov.set(
                x,
                y,
                !game.map[x as usize][y as usize].block_sight,
                !game.map[x as usize][y as usize].blocked,
            );
        }
    }

    // unexplored areas start black (which is the default background color)
    tcod.con.clear();
}

pub fn new_game(tcod: &mut Tcod, game_settings: GameSettings) -> (Game, Vec<DisplayObj>){
    let mut player = DisplayObj::new(25, 23, '@', "player", WHITE, true);
    player.alive = true;
    player.fighter = Some(Fighter {
        base_max_hp: 100,
        hp: 100,
        base_defense: 1,
        base_power: 3,
        xp: 0,
        on_death: DeathCallback::Player
    });

    let mut objects = vec![player];

    let map: Vec<Vec<Tile>> = make_map(&tcod, &mut objects, &game_settings, 1);

    let mut game: Game = Game {
        game_settings: game_settings,
        map: map,
        messages: Messages::new(),
        inventory: vec![],
        dungeon_level: 1
    };

    init_fov(tcod, &game);

    // a warm welcoming message!
    game.messages.add(
        "Welcome stranger! Prepare to perish in the Tombs of the Ancient Kings.",
        RED,
    );

    // initial equipment: a dagger
    let mut dagger = DisplayObj::new(0, 0, '-', "dagger", SKY, false);
    dagger.item = Some(Item::Sword);
    dagger.equipment = Some(Equipment {
        equipped: true,
        slot: Slot::LeftHand,
        max_hp_bonus: 0,
        defense_bonus: 0,
        power_bonus: 1,
    });
    game.inventory.push(dagger);

    (game, objects)
}

pub fn play_game(tcod: &mut Tcod, game: &mut Game, objects: &mut Vec<DisplayObj>){
    // force FOV "recompute" first time through the game loop
    let mut previous_player_position = (-1, -1);

    while !tcod.root.window_closed() {
        tcod.con.clear();
        
        match input::check_for_event(input::MOUSE | input::KEY_PRESS) {
            Some((_, Event::Mouse(m))) => tcod.mouse = m,
            Some((_, Event::Key(k))) => tcod.key = k,
            _ => tcod.key = Default::default(),
        }

        let fov_recompute = previous_player_position != objects[PLAYER_ID].get_pos();
        render_all(tcod, game, &objects, fov_recompute);

        tcod.root.flush();
        // level up if needed
        level_up(tcod, game, objects);

        previous_player_position = objects[PLAYER_ID].get_pos();
        let player_action = handle_keys(tcod, game, objects);
        if player_action == PlayerAction::Exit {
            save_game(game, objects).unwrap();
            break;
        }

        // let monsters take their turn
        if objects[PLAYER_ID].alive && player_action != PlayerAction::DidntTakeTurn {
            for id in 0..objects.len() {
                // only if object is not player
                if objects[id].ai.is_some(){
                    ai_take_turn( id, &tcod, game, objects);
                }
            }
        }
    }
}

fn save_game(game: &Game, objects: &[DisplayObj]) -> Result<(), Box<dyn Error>> {
    let save_data = serde_json::to_string(&(game, objects))?;
    let mut file = File::create("savegame")?;
    file.write_all(save_data.as_bytes())?;
    Ok(())
}

fn load_game() -> Result<(Game, Vec<DisplayObj>), Box<dyn Error>> {
    let mut json_save_state = String::new();
    let mut file = File::open("savegame")?;
    file.read_to_string(&mut json_save_state)?;
    let result = serde_json
            ::from_str
            ::<(Game, Vec<DisplayObj>)>(&json_save_state)?;

    Ok(result)
}

fn next_level(tcod: &mut Tcod, game: &mut Game, objects: &mut Vec<DisplayObj>) {
    game.messages.add(
        "You descend down further into the crypt, where will it end...", 
        RED);
    
    let heal_hp = objects[PLAYER_ID].max_hp(game) / 2;
    objects[PLAYER_ID].heal(heal_hp, &game);

    game.dungeon_level += 1;
    game.map = make_map(&tcod, objects, &game.game_settings, game.dungeon_level);
    init_fov(tcod, game);
}

fn level_up(tcod: &mut Tcod, game: &mut Game, objects: &mut [DisplayObj]) {
    let player = &mut objects[PLAYER_ID];
    let level_up_xp = LEVEL_UP_BASE + player.level * LEVEL_UP_FACTOR;
    // see if the player's experience is enough to level-up
    if player.fighter.as_ref().map_or(0, |f| f.xp) >= level_up_xp {
        // it is! level up
        player.level += 1;
        game.messages.add(
            format!(
                "Your battle skills grow stronger! You reached level {}!",
                player.level
            ),
            YELLOW,
        );
        
        let fighter = player.fighter.as_mut().unwrap();
        let mut choice = None;
        while choice.is_none() {
            // keep asking until a choice is made
            choice = menu(
                "Level up! Choose a stat to raise:\n",
                &[
                    format!("Constitution (+20 HP, from {})", fighter.base_max_hp),
                    format!("Strength (+1 attack, from {})", fighter.base_power),
                    format!("Agility (+1 defense, from {})", fighter.base_defense),
                ],
                LEVEL_SCREEN_WIDTH,
                &mut tcod.root,
                &game.game_settings
            );
        }
        fighter.xp -= level_up_xp;
        match choice.unwrap() {
            0 => {
                fighter.base_max_hp += 20;
                fighter.hp += 20;
            }
            1 => {
                fighter.base_power += 1;
            }
            2 => {
                fighter.base_defense += 1;
            }
            _ => unreachable!(),
        }
    }
}

fn handle_keys(tcod: &mut Tcod, game: &mut Game, objects: &mut Vec<DisplayObj>) -> PlayerAction {
    use tcod::input::Key;
    use tcod::input::KeyCode::*;
    use PlayerAction::*;

    let player_alive = objects[PLAYER_ID].alive;

    match (tcod.key, tcod.key.text(), player_alive) {
        
        (Key {
            code: Enter,
            alt: true,
            ..
                },
                _,
                _) => {
            // Alt+Enter: toggle fullscreen
            let fullscreen = tcod.root.is_fullscreen();
            tcod.root.set_fullscreen(!fullscreen);
            return DidntTakeTurn
        }
        (Key { code: Escape, .. },
                _,
                _) => return Exit, // exit game
        // movement keys
        (Key { code: Up, .. }, _, true) | (Key { code: NumPad8, .. }, _, true) => {
            player_move_or_attack(0, -1, game, objects);
            TookTurn
        }
        (Key { code: Down, .. }, _, true) | (Key { code: NumPad2, .. }, _, true) => {
            player_move_or_attack(0, 1, game, objects);
            TookTurn
        }
        (Key { code: Left, .. }, _, true) | (Key { code: NumPad4, .. }, _, true) => {
            player_move_or_attack(-1, 0, game, objects);
            TookTurn
        }
        (Key { code: Right, .. }, _, true) | (Key { code: NumPad6, .. }, _, true) => {
            player_move_or_attack(1, 0, game, objects);
            TookTurn
        }
        (Key { code: Home, .. }, _, true) | (Key { code: NumPad7, .. }, _, true) => {
            player_move_or_attack(-1, -1, game, objects);
            TookTurn
        }
        (Key { code: PageUp, .. }, _, true) | (Key { code: NumPad9, .. }, _, true) => {
            player_move_or_attack(1, -1, game, objects);
            TookTurn
        }
        (Key { code: End, .. }, _, true) | (Key { code: NumPad1, .. }, _, true) => {
            player_move_or_attack(-1, 1, game, objects);
            TookTurn
        }
        (Key { code: PageDown, .. }, _, true) | (Key { code: NumPad3, .. }, _, true) => {
            player_move_or_attack(1, 1, game, objects);
            TookTurn
        }
        (Key { code: NumPad5, .. }, _, true) => {
            TookTurn // do nothing, i.e. wait for the monster to come to you
        },
        (Key { code: Text, .. }, "g", true) => {
            // pick up an item
            let item_id = objects
                .iter()
                .position(|object| object.get_pos() == objects[PLAYER_ID].get_pos() && object.item.is_some());
            if let Some(item_id) = item_id {
                pick_item_up(item_id, game, objects);
            }
            DidntTakeTurn
        },
        (Key { code: Text, .. }, "i", true) => {
            // show the inventory
            let inv_index = inventory_menu(
                &game.inventory,
                "Press the key next to an item to use it, or any other to cancel.\n",
                &mut tcod.root,
                &game);

            if let Some(inv_index) = inv_index {
                use_item(inv_index, tcod, game, objects);
            }
            TookTurn
        },
        (Key { code: Text, .. }, "d", true) => {
            // show the inventory; if an item is selected, drop it
            let inventory_index = inventory_menu(
                &game.inventory,
                "Press the key next to an item to drop it, or any other to cancel.\n'",
                &mut tcod.root,
                &game
            );
            if let Some(inventory_index) = inventory_index {
                drop_item(inventory_index, game, objects);
            }
            DidntTakeTurn
        },
        (Key { code: Text, .. }, "<", true) => {
            // go down stairs, if the player is on them
            let player_on_stairs = objects
                .iter()
                .any(|object| object.get_pos() == objects[PLAYER_ID].get_pos() && object.name == "stairs");
            if player_on_stairs {
                next_level(tcod, game, objects);
            }
            DidntTakeTurn
        },
        (Key { code: Text, .. }, "c", true) => {
            // show character information
            let player = &objects[PLAYER_ID];
            let level = player.level;
            let level_up_xp = LEVEL_UP_BASE + player.level * LEVEL_UP_FACTOR;
            if let Some(fighter) = player.fighter.as_ref() {
                let msg = format!(
"Character information

Level: {}
Experience: {}
Experience to level up: {}

Maximum HP: {}
Attack: {}
Defense: {}",
    level, fighter.xp, level_up_xp, player.max_hp(game), player.power(game), player.defense(game)
);
                msgbox(&msg, CHARACTER_SCREEN_WIDTH, &mut tcod.root, &game.game_settings);
            }
        
            DidntTakeTurn
        },

        _ => return DidntTakeTurn
    }
    
}

/// Returns a value that depends on level. the table specifies what
/// value occurs after each level, default is 0.
fn from_dungeon_level(table: &[Transition], level: u32) -> u32 {
    table
        .iter()
        .rev()
        .find(|transition| level >= transition.level)
        .map_or(0, |transition| transition.value)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DisplayObj {
    pub x: i32,
    pub y: i32,
    pub char: char,
    pub color: Color,
    pub name: String,
    pub blocks: bool,
    pub alive: bool,
    pub always_visible: bool,
    pub fighter: Option<Fighter>,
    pub ai: Option<Ai>,
    pub item: Option<Item>,
    pub level: u32,
    equipment: Option<Equipment>
}

impl DisplayObj {
    pub fn new(x: i32, y: i32, char: char, name: &str, color: Color, blocks: bool) -> Self{
        DisplayObj {x: x, y: y, char: char, color: color, name: name.into(), blocks: blocks, alive: false, always_visible: false, fighter: None, ai: None, item: None, level: 1, equipment: None}
    }

    pub fn draw(&self, con: &mut dyn Console){
        con.set_default_foreground(self.color);
        con.put_char(self.x, self.y, self.char, BackgroundFlag::None);
    }

    pub fn get_pos(&self) -> (i32, i32) {
        (self.x, self.y)
    }

    pub fn set_pos(&mut self, x: i32, y: i32) {
        self.x = x;
        self.y = y;
    }

    fn distance_to(&self, other: &DisplayObj) -> f32 {
        let dx = other.x - self.x;
        let dy = other.y - self.y;
        ((dx.pow(2) + dy.pow(2)) as f32).sqrt()
    }

    /// return the distance to some coordinates
    pub fn distance(&self, x: i32, y: i32) -> f32 {
        (((x - self.x).pow(2) + (y - self.y).pow(2)) as f32).sqrt()
    }

    pub fn take_damage(&mut self, damage: i32, game: &mut Game) -> Option<u32> {
        // apply damage if possible
        if let Some(fighter) = self.fighter.as_mut() {
            if damage > 0 {
                fighter.hp -= damage;
            }
        }

        if let Some(fighter) = self.fighter {
            if fighter.hp <= 0 {
                self.alive = false;
                fighter.on_death.callback(self, game);
                return Some(fighter.xp);
            }
        }
        None
    }

    pub fn attack(&mut self, target: &mut DisplayObj, game: &mut Game) {
        // a simple formula for attack damage
        let damage = self.power(game) - target.defense(game);
        if damage > 0 {
            // make the target take some damage
            game.messages.add(format!(
                "{} attacks {} for {} hit points.",
                self.name, target.name, damage),
                WHITE
            );
            if let Some(xp) = target.take_damage(damage, game){
                self.fighter.as_mut().unwrap().xp += xp;
            }
        } else {
            game.messages.add(format!(
                "{} attacks {} but it has no effect!",
                self.name, target.name),
                WHITE
            );
        }
    }

    /// heal by the given amount, without going over the maximum
    pub fn heal(&mut self, amount: i32, game: &Game) {
        let max_hp = self.max_hp(game); 
        if let Some(ref mut fighter) = self.fighter {
            fighter.hp += amount;
            if fighter.hp > max_hp {
                fighter.hp = max_hp;
            }
        }
    }

    /// Equip object and show a message about it
    pub fn equip(&mut self, messages: &mut Messages) {
        if self.item.is_none() {
            messages.add(
                format!("Can't equip {:?} because it's not an Item.", self),
                RED,
            );
            return;
        };
        if let Some(ref mut equipment) = self.equipment {
            if !equipment.equipped {
                equipment.equipped = true;
                messages.add(
                    format!("Equipped {} on {}.", self.name, equipment.slot),
                    LIGHT_GREEN,
                );
            }
        } else {
            messages.add(
                format!("Can't equip {:?} because it's not an Equipment.", self),
                RED,
            );
        }
    }

    /// Dequip object and show a message about it
    pub fn dequip(&mut self, messages: &mut Messages) {
        if self.item.is_none() {
            messages.add(
                format!("Can't dequip {:?} because it's not an Item.", self),
                RED,
            );
            return;
        };
        if let Some(ref mut equipment) = self.equipment {
            if equipment.equipped {
                equipment.equipped = false;
                messages.add(
                    format!("Dequipped {} from {}.", self.name, equipment.slot),
                    LIGHT_YELLOW,
                );
            }
        } else {
            messages.add(
                format!("Can't dequip {:?} because it's not an Equipment.", self),
                RED,
            );
        }
    }

    pub fn max_hp(&self, game: &Game) -> i32 {
        let base_max_hp = self.fighter.map_or(0, |f| f.base_max_hp);
        let bonus: i32 = self
            .get_all_equipped(game)
            .iter()
            .map(|e| e.max_hp_bonus)
            .sum();
        base_max_hp + bonus
    }

    pub fn power(&self, game: &Game) -> i32 {
        let base_power: i32 = self.fighter.map_or(0, |f| f.base_power);
        let bonus: i32 = self
                .get_all_equipped(game)
                .iter()
                .map(|e| e.power_bonus)
                .sum();

        base_power + bonus
    }

    pub fn defense(&self, game: &Game) -> i32 {
        let base_defense = self.fighter.map_or(0, |f| f.base_defense);
        let bonus: i32 = self
            .get_all_equipped(game)
            .iter()
            .map(|e| e.defense_bonus)
            .sum();
        base_defense + bonus
    }

    /// returns a list of equipped items
    pub fn get_all_equipped(&self, game: &Game) -> Vec<Equipment> {
        if self.name == "player" {
            game.inventory
                .iter()
                .filter(|item| item.equipment.map_or(false, |e| e.equipped))
                .map(|item| item.equipment.unwrap())
                .collect()
        } else {
            vec![] // other objects have no equipment
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PlayerAction {
    TookTurn,
    DidntTakeTurn,
    Exit,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Ai {
    Basic,
    Confused {
        previous_ai: Box<Ai>,
        num_turns: i32,
    }
}

// combat-related properties and methods (monster, player, NPC).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fighter {
    pub base_max_hp: i32,
    pub hp: i32,
    pub base_defense: i32,
    pub base_power: i32,
    pub xp: u32,
    pub on_death: DeathCallback
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum DeathCallback {
    Player,
    Monster,
}

impl DeathCallback {
    fn callback(&self, object: &mut DisplayObj, game: &mut Game) {
        use DeathCallback::*;
        let callback: fn(&mut DisplayObj, game: &mut Game) = match self {
            Player => player_death,
            Monster => monster_death
        };
        callback(object, game);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Item {
    Heal,
    Lightning,
    Confuse,
    Fireball,
    Sword,
    Shield,
    Helmet
}

enum UseResult {
    UsedUp,
    Cancelled,
    UsedAndKept
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
/// An object that can be equipped, yielding bonuses.
pub struct Equipment {
    pub slot: Slot,
    pub equipped: bool,
    pub max_hp_bonus: i32,
    pub power_bonus: i32,
    pub defense_bonus: i32
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Slot {
    LeftHand,
    RightHand,
    Head,
}

impl std::fmt::Display for Slot {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match *self {
            Slot::LeftHand => write!(f, "left hand"),
            Slot::RightHand => write!(f, "right hand"),
            Slot::Head => write!(f, "head"),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Tile {
    pub blocked: bool,
    pub block_sight: bool,
    pub explored: bool
}

impl Tile{
    pub fn empty() -> Self {
        Tile {
            blocked: false,
            block_sight: false,
            explored: false
        }
    }

    pub fn wall() -> Self {
        Tile {
            blocked: true,
            block_sight: true,
            explored: false
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct GameSettings{
    pub screen_w: i32,
    pub screen_h: i32,
    pub map_w: i32,
    pub map_h: i32,
    pub room_max_size: u32,
    pub room_min_size: u32,
    pub max_rooms: i32,
    pub max_room_monsters: i32,
    pub dark_wall_color: Color,
    pub light_wall_color: Color,
    pub dark_ground_color: Color,
    pub light_ground_color: Color,
    pub fps_limit: i32,
    pub fov_light_walls: bool,
    pub torch_radius: i32,
    pub bar_w: i32,
    pub panel_h: i32,
    pub panel_y: i32,
    pub msg_x: i32,
    pub msg_w: i32,
    pub msg_h: i32
}

impl GameSettings {
    pub fn new () -> Self {
        GameSettings {
            screen_w: 80,
            screen_h: 50,
            map_w: 80,
            map_h: 43,
            room_max_size: 10,
            room_min_size: 6,
            max_rooms: 20,
            max_room_monsters: 6,
            dark_wall_color: Color {r: 35, g: 35, b: 35 },
            light_wall_color: Color {r: 55, g: 55, b: 55 },
            dark_ground_color: Color { r: 85,g: 75,b: 55 },
            light_ground_color: Color {r: 100, g: 90,b: 70 },
            fps_limit: 20,
            fov_light_walls: true,
            torch_radius: 10,
            bar_w: 20,
            panel_h: 7,
            panel_y: 43,
            msg_x: 22,
            msg_w: 58,
            msg_h: 6
        }
    }
}

pub type Map = Vec<Vec<Tile>>;

#[derive(Serialize, Deserialize)] 
pub struct Game {
    pub game_settings: GameSettings,
    pub map: Map,
    pub messages: Messages,
    pub inventory: Vec<DisplayObj>,
    dungeon_level: u32
}

#[derive(Clone, Copy, Debug)]
pub struct Room {
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32
}

impl Room {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Room {
            x1: x,
            y1: y,
            x2: x + w,
            y2: y + h,
        }
    }

    pub fn center(&self) -> (i32, i32) {
        let center_x = (self.x1 + self.x2) /2;
        let center_y = (self.y1 + self.y2) /2;
        (center_x, center_y)
    }

    pub fn intersects_with(&self, other: &Room) -> bool {
        (self.x1 <= other.x2)
            && (self.x2 >= other.x1)
            && (self.y1 <= other.y2)
            && (self.y2 >= other.y1)
    }
}

fn create_room(room: Room, map: &mut Map) {
    // go through the tiles in the rectangle and make them passable
    for x in (room.x1 + 1)..room.x2 {
        for y in (room.y1 + 1)..room.y2 {
            map[x as usize][y as usize] = Tile::empty();
        }
    }
}

fn create_h_tunnel(x1: i32, x2: i32, y: i32, map: &mut Map) {
    // horizontal tunnel. `min()` and `max()` are used in case `x1 > x2`
    for x in cmp::min(x1, x2)..(cmp::max(x1, x2) + 1) {
        map[x as usize][y as usize] = Tile::empty();
    }
}

fn create_v_tunnel(y1: i32, y2: i32, x: i32, map: &mut Map) {
    // vertical tunnel
    for y in cmp::min(y1, y2)..(cmp::max(y1, y2) + 1) {
        map[x as usize][y as usize] = Tile::empty();
    }
}

fn make_map(tcod: &Tcod, objects: &mut Vec<DisplayObj>, game_settings: &GameSettings, level: u32) -> Map {
    let mut map = vec![vec![Tile::wall(); game_settings.map_h as usize]; game_settings.map_w as usize];

    // Player is the first element, remove everything else.
    // NOTE: works only when the player is the first object!
    assert_eq!(&objects[PLAYER_ID] as *const _, &objects[0] as *const _);
    objects.truncate(1);

    let mut rooms: Vec<Room> = vec![];
    for _ in 0..game_settings.max_rooms {
        let w = rand::thread_rng().gen_range(game_settings.room_min_size..game_settings.room_max_size+1) as i32;
        let h = rand::thread_rng().gen_range(game_settings.room_min_size..game_settings.room_max_size+1) as i32;

        let x = rand::thread_rng().gen_range(0..game_settings.map_w - w );
        let y = rand::thread_rng().gen_range(0..game_settings.map_h - h);

        let new_room = Room::new(x, y, w, h);

        // run through the other rooms and see if they intersect with this one
        let failed = rooms
            .iter()
            .any(|other_room| new_room.intersects_with(other_room));

        if !failed {
            create_room(new_room, &mut map);
            
            let (new_x, new_y) = new_room.center();

            if rooms.is_empty() {
                objects[PLAYER_ID].set_pos(new_x, new_y);
            } else {
                //Don't place monsters in the first room
                place_objects(tcod, new_room, &map, level, objects);
                let (prev_x, prev_y) = rooms[rooms.len() - 1].center();

                if rand::random() {
                    create_h_tunnel(prev_x, new_x, prev_y, &mut map);
                    create_v_tunnel(prev_y, new_y, new_x, &mut map);
                } else {
                    create_v_tunnel(prev_y, new_y, prev_x, &mut map);
                    create_h_tunnel(prev_x, new_x, new_y, &mut map);
                    
                }
            }
            rooms.push(new_room);
        }
    }

    // create stairs at the center of the last room
    let (last_room_x, last_room_y) = rooms[rooms.len() - 1].center();
    let mut stairs = DisplayObj::new(last_room_x, last_room_y, '<', "stairs", WHITE, false);
    stairs.always_visible = true;
    objects.push(stairs);

    map
}

fn place_objects(tcod: &Tcod, room: Room, map: &Map, level: u32, objects: &mut Vec<DisplayObj>) {
    // maximum number of monsters per room
    let max_spawn = from_dungeon_level(
        &tcod.tables.as_ref().unwrap().max_monsters,
        level,
    );
    generate_objects(max_spawn, 
        &tcod.tables.as_ref().unwrap().monsters, 
        room, map, level, objects);
    
    
    // maximum number of items per room
    let max_spawn = from_dungeon_level(
        &tcod.tables.as_ref().unwrap().max_items,
        level,
    );
    generate_objects(max_spawn, 
        &tcod.tables.as_ref().unwrap().items, 
        room, map, level, objects);
}

fn generate_objects(max_spawns: u32, conf_data: &Vec<ObjectConfiguration>, 
        room: Room, map: &Map, 
        level: u32, objects: &mut Vec<DisplayObj>){
    
    // choose random number of monsters
    let num_to_spawn = rand::thread_rng().gen_range(0..max_spawns + 1);

    let mut weights: Vec<u32> = Vec::new();

    for data in conf_data {
        weights.push(from_dungeon_level(&data.transition_table, level));
    }

    let choices = WeightedIndex::new(weights).unwrap();

    let mut rng = rand::thread_rng();

    for _ in 0..num_to_spawn {
        let x = rand::thread_rng().gen_range(room.x1+1..room.x2);
        let y = rand::thread_rng().gen_range(room.y1+1..room.y2);

        if is_blocked(x, y, map, objects) {
            continue;
        }

        let object_data = &conf_data[choices.sample(&mut rng)];

        objects.push(object_data.as_object(x, y));
    }
}

fn move_towards(id: usize, target_x: i32, target_y: i32, map: &Map, objects: &mut [DisplayObj]){
    let dx = target_x - objects[id].x;
    let dy = target_y - objects[id].y;
    let distance = ((dx.pow(2) + dy.pow(2)) as f32).sqrt();

    let dx = (dx as f32 / distance).round() as i32;
    let dy = (dy as f32 / distance).round() as i32;
    move_by(id, dx, dy, map, objects);
}

/// Mutably borrow two *separate* elements from the given slice.
/// Panics when the indexes are equal or out of bounds.
fn mut_two<T>(first_index: usize, second_index: usize, items: &mut [T]) -> (&mut T, &mut T) {
    assert!(first_index != second_index);
    let split_at_index = cmp::max(first_index, second_index);
    let (first_slice, second_slice) = items.split_at_mut(split_at_index);
    if first_index < second_index {
        (&mut first_slice[first_index], &mut second_slice[0])
    } else {
        (&mut second_slice[0], &mut first_slice[second_index])
    }
}

pub fn ai_take_turn(monster_id: usize, tcod: &Tcod, game: &mut Game, objects: &mut [DisplayObj]) {
    use Ai::*;
    if let Some(ai) = objects[monster_id].ai.take() {
        let new_ai = match ai {
            Basic => ai_basic(monster_id, tcod, game, objects),
            Confused {
                previous_ai,
                num_turns,
            } => ai_confused(monster_id, tcod, game, objects, previous_ai, num_turns),
        };
        objects[monster_id].ai = Some(new_ai);
    }
}

fn ai_basic(monster_id: usize, tcod: &Tcod, game: &mut Game, objects: &mut [DisplayObj]) -> Ai {
    // a basic monster takes its turn. If you can see it, it can see you
    let (monster_x, monster_y) = objects[monster_id].get_pos();
    if tcod.fov.is_in_fov(monster_x, monster_y) {
        if objects[monster_id].distance_to(&objects[PLAYER_ID]) >= 2.0 {
            // move towards player if far away
            let (player_x, player_y) = objects[PLAYER_ID].get_pos();
            move_towards(monster_id, player_x, player_y, &game.map, objects);
        } else if objects[PLAYER_ID].fighter.map_or(false, |f| f.hp > 0) {
            // close enough, attack! (if the player is still alive.)
            let (monster, player) = mut_two(monster_id, PLAYER_ID, objects);
            monster.attack(player, game);
        }
    }
    Ai::Basic
}

fn ai_confused(monster_id: usize, _tcod: &Tcod, game: &mut Game, objects: &mut [DisplayObj], 
        previous_ai: Box<Ai>, num_turns: i32) -> Ai {
    if num_turns >= 0 {
        // still confused ...
        // move in a random direction, and decrease the number of turns confused
        move_by(
            monster_id,
            rand::thread_rng().gen_range(-1..2),
            rand::thread_rng().gen_range(-1..2),
            &game.map,
            objects,
        );
        Ai::Confused {
            previous_ai: previous_ai,
            num_turns: num_turns - 1,
        }
    } else {
        // restore the previous AI (this one will be deleted)
        game.messages.add(
            format!("The {} is no longer confused!", objects[monster_id].name),
            RED,
        );
        *previous_ai
    }
}

fn player_death(player: &mut DisplayObj, game: &mut Game) {
    // the game ended!
    game.messages.add("You died!", RED);

    // for added effect, transform the player into a corpse!
    player.char = '%';
    player.color = DARK_RED;
}

fn monster_death(monster: &mut DisplayObj, game: &mut Game) {
    // transform it into a nasty corpse! it doesn't block, can't be
    // attacked and doesn't move
    // transform it into a nasty corpse! it doesn't block, can't be
    // attacked and doesn't move
    game.messages.add(
        format!(
            "{} is dead! You gain {} experience points.",
            monster.name,
            monster.fighter.unwrap().xp
        ),
        ORANGE,
    );
    monster.char = '%';
    monster.color = DARK_RED;
    monster.blocks = false;
    monster.fighter = None;
    monster.ai = None;
    monster.name = format!("remains of {}", monster.name);
}

fn move_by(id: usize, dx: i32, dy: i32, map: &Map, objects: &mut [DisplayObj]){
    let (x,y) = objects[id].get_pos();

    if !is_blocked(x + dx, y + dy, map, objects) {
        objects[id].set_pos(x + dx, y + dy);
    }
}

pub fn player_move_or_attack( dx: i32, dy: i32, game: &mut Game, objects: &mut [DisplayObj]) {
    // the coordinates the player is moving to/attacking
    let x = objects[PLAYER_ID].x + dx;
    let y = objects[PLAYER_ID].y + dy;

    // try to find an attackable object there
    let target_id = objects.iter().position(|object| object.fighter.is_some() && object.get_pos() == (x, y));

    // attack if target found, move otherwise
    match target_id {
        Some(target_id) => {
            let (player, target) = mut_two(PLAYER_ID, target_id, objects);
            player.attack(target, game);
        }
        None => {
            move_by(PLAYER_ID, dx, dy, &game.map, objects);
        }
    }
}

fn is_blocked(x: i32, y: i32, map: &Map, objects: &[DisplayObj]) -> bool {
    if map[x as usize][y as usize].blocked {
        return true;
    }

    objects.iter()
        .any(|object| object.blocks && object.get_pos() == (x,y))
}

pub fn pick_item_up(object_id: usize, game: &mut Game, objects: &mut Vec<DisplayObj>){
    if game.inventory.len() >= MAX_INV_SPACE {
        game.messages.add(format!("Inventory full, cannot pick up {}.", objects[object_id].name), RED);
    } else {
        let item = objects.swap_remove(object_id);
        game.messages
            .add(format!("You picked up a {}!", item.name), GREEN);
        let index = game.inventory.len();
        let slot = item.equipment.map(|e| e.slot);
        game.inventory.push(item);

        // automatically equip, if the corresponding equipment slot is unused
        if let Some(slot) = slot {
            if get_equipped_in_slot(slot, &game.inventory).is_none() {
                game.inventory[index].equip(&mut game.messages);
            }
        }
    }
}

pub fn drop_item(inventory_id: usize, game: &mut Game, objects: &mut Vec<DisplayObj>) {
    let mut item = game.inventory.remove(inventory_id);
    if item.equipment.is_some() {
        item.dequip(&mut game.messages);
    }

    item.set_pos(objects[PLAYER_ID].x, objects[PLAYER_ID].y);
    game.messages
        .add(format!("You dropped a {}.", item.name), YELLOW);
    objects.push(item);
}

//Handle GUI
#[derive(Serialize, Deserialize)]
pub struct Messages {
    messages: Vec<(String, Color)>,
}

impl Messages {
    pub fn new() -> Self {
        Self {messages: vec![]}
    }

    pub fn add<T: Into<String>>(&mut self, message: T, color: Color) {
        self.messages.push((message.into(), color));
    }

    pub fn iter (&self) -> impl DoubleEndedIterator<Item = &(String, Color)> {
        self.messages.iter()
    }
}

pub fn use_item(inventory_id: usize, tcod: &mut Tcod, game: &mut Game, objects: &mut [DisplayObj]) {
    use Item::*;
    // just call the "use_function" if it is defined
    if let Some(item) = game.inventory[inventory_id].item {
        let on_use = match item {
            Heal => cast_heal,
            Lightning => cast_lightning,
            Confuse => cast_confuse,
            Fireball => cast_fireball,
            Sword => toggle_equipment,
            Shield => toggle_equipment,
            Helmet => toggle_equipment,
        };
        match on_use(inventory_id, tcod, game, objects) {
            UseResult::UsedUp => {
                // destroy after use, unless it was cancelled for some reason
                game.inventory.remove(inventory_id);
            }
            UseResult::UsedAndKept => {} // do nothing
            UseResult::Cancelled => {
                game.messages.add("Cancelled", WHITE);
            }
        }
    } else {
        game.messages.add(
            format!("The {} cannot be used.", game.inventory[inventory_id].name),
            WHITE,
        );
    }
}

fn toggle_equipment(
    inventory_id: usize,
    _tcod: &mut Tcod,
    game: &mut Game,
    _objects: &mut [DisplayObj],
) -> UseResult {
    let equipment = match game.inventory[inventory_id].equipment {
        Some(equipment) => equipment,
        None => return UseResult::Cancelled,
    };

    // if the slot is already being used, dequip whatever is there first
    if let Some(current) = get_equipped_in_slot(equipment.slot, &game.inventory) {
        game.inventory[current].dequip(&mut game.messages);
    }
    
    if equipment.equipped {
        game.inventory[inventory_id].dequip(&mut game.messages);
    } else {
        game.inventory[inventory_id].equip(&mut game.messages);
    }
    UseResult::UsedAndKept
}

fn get_equipped_in_slot(slot: Slot, inventory: &[DisplayObj]) -> Option<usize> {
    for (inventory_id, item) in inventory.iter().enumerate() {
        if item
            .equipment
            .as_ref()
            .map_or(false, |e| e.equipped && e.slot == slot)
        {
            return Some(inventory_id);
        }
    }
    None
}

fn cast_heal(
    _inventory_id: usize,
    _tcod: &mut Tcod,
    game: &mut Game,
    objects: &mut [DisplayObj],
) -> UseResult {
    // heal the player
    let player = &mut objects[PLAYER_ID];
    if let Some(fighter) = player.fighter {
        if fighter.hp == player.max_hp(&game) {
            game.messages.add("You are already at full health.", RED);
            return UseResult::Cancelled;
        }
        game.messages
            .add("Your wounds start to feel better!", LIGHT_VIOLET);
        objects[PLAYER_ID].heal(HEAL_AMOUNT, &game);
        return UseResult::UsedUp;
    }
    UseResult::Cancelled
}

fn cast_lightning(
    _inventory_id: usize,
    tcod: &mut Tcod,
    game: &mut Game,
    objects: &mut [DisplayObj],
) -> UseResult {
    // find closest enemy (inside a maximum range and damage it)
    let monster_id = closest_monster(tcod, objects, LIGHTNING_RANGE);
    if let Some(monster_id) = monster_id {
        // zap it!
        game.messages.add(
            format!(
                "A lightning bolt strikes the {} with a loud thunder! \
                 The damage is {} hit points.",
                objects[monster_id].name, LIGHTNING_DAMAGE
            ),
            LIGHT_BLUE,
        );
        if let Some(xp) = objects[monster_id].take_damage(LIGHTNING_DAMAGE, game){
            objects[PLAYER_ID].fighter.as_mut().unwrap().xp += xp;
        }
        UseResult::UsedUp
    } else {
        // no enemy found within maximum range
        game.messages
            .add("No enemy is close enough to strike.", RED);
        UseResult::Cancelled
    }
}

fn cast_confuse(
    _inventory_id: usize,
    tcod: &mut Tcod,
    game: &mut Game,
    objects: &mut [DisplayObj],
) -> UseResult {
    // ask the player for a target to confuse
    game.messages.add(
        "Left-click an enemy to confuse it, or right-click to cancel.",
        LIGHT_CYAN,
    );
    let monster_id = target_monster(tcod, game, objects, Some(CONFUSE_RANGE as f32));

    if let Some(monster_id) = monster_id {
        let old_ai = objects[monster_id].ai.take().unwrap_or(Ai::Basic);
        // replace the monster's AI with a "confused" one; after
        // some turns it will restore the old AI
        objects[monster_id].ai = Some(Ai::Confused {
            previous_ai: Box::new(old_ai),
            num_turns: CONFUSE_NUM_TURNS,
        });
        game.messages.add(
            format!(
                "The eyes of {} look vacant, as he starts to stumble around!",
                objects[monster_id].name
            ),
            LIGHT_GREEN,
        );
        UseResult::UsedUp
    } else {
        // no enemy fonud within maximum range
        game.messages
            .add("No enemy is close enough to strike.", RED);
        UseResult::Cancelled
    }
}

fn cast_fireball(
    _inventory_id: usize,
    tcod: &mut Tcod,
    game: &mut Game,
    objects: &mut [DisplayObj]
) -> UseResult {
    game.messages.add(
        "Left-click a target tile to strike, right-click to cancel.",
        LIGHTER_CYAN
    );

    let (x, y) = match target_tile(tcod, game, objects, None) {
        Some(tile_pos) => tile_pos,
        None => return UseResult::Cancelled
    };

    game.messages.add(format!(
            "The fireball explodes, burning everything within {} tiles",
            FIREBALL_RADIUS
        ), ORANGE
    );

    let mut xp_to_gain: u32 = 0;

    for (id, obj) in objects.iter_mut().enumerate() {
        if obj.distance(x, y) <= FIREBALL_RADIUS as f32 && obj.fighter.is_some() {
            game.messages.add(
                format!(
                    "The {} gets burned for {} hit points",
                    obj.name, FIREBALL_DAMAGE
                ),
                ORANGE
            );
            if let Some(xp) = obj.take_damage(FIREBALL_DAMAGE, game){
                if id != PLAYER_ID {
                    xp_to_gain += xp;
                }
            }
        }
    }

    if xp_to_gain > 0 {
        objects[PLAYER_ID].fighter.as_mut().unwrap().xp += xp_to_gain;
    }

    UseResult::UsedUp
}

/// find closest enemy, up to a maximum range, and in the player's FOV
fn closest_monster(tcod: &Tcod, objects: &[DisplayObj], max_range: i32) -> Option<usize> {
    let mut closest_enemy = None;
    let mut closest_dist = (max_range + 1) as f32; // start with (slightly more than) maximum range

    for (id, object) in objects.iter().enumerate() {
        if (id != PLAYER_ID)
            && object.fighter.is_some()
            && object.ai.is_some()
            && tcod.fov.is_in_fov(object.x, object.y)
        {
            // calculate distance between this object and the player
            let dist = objects[PLAYER_ID].distance_to(object);
            if dist < closest_dist {
                // it's closer, so remember it
                closest_enemy = Some(id);
                closest_dist = dist;
            }
        }
    }
    closest_enemy
}

fn target_tile(
    tcod: &mut Tcod,
    game: &mut Game,
    objects: &[DisplayObj],
    max_range: Option<f32>
) -> Option<(i32, i32)> {
    use tcod::input::KeyCode::Escape;
    loop {
        tcod.root.flush();
        let event = input::check_for_event(input::KEY_PRESS | input::MOUSE)
                .map(|e| e.1);
        match event {
            Some(Event::Mouse(m)) => tcod.mouse = m,
            Some(Event::Key(k)) => tcod.key = k,
            None => tcod.key = Default::default()
        }

        render_all(tcod, game, objects, false);
        
        let (x, y) = (tcod.mouse.cx as i32, tcod.mouse.cy as i32);

        let in_fov = (x < game.game_settings.map_w) && (y < game.game_settings.map_h) && 
                tcod.fov.is_in_fov(x, y);
        let in_range = max_range.map_or(true, |range| objects[PLAYER_ID].distance(x, y) <= range);
        if tcod.mouse.lbutton_pressed && in_fov && in_range {
            return Some((x, y));
        }
        if tcod.mouse.rbutton_pressed || tcod.key.code == Escape {
            return None;
        }
    }
}

/// returns a clicked monster inside FOV up to a range, or None if right-clicked
fn target_monster(
    tcod: &mut Tcod,
    game: &mut Game,
    objects: &[DisplayObj],
    max_range: Option<f32>,
) -> Option<usize> {
    loop {
        match target_tile(tcod, game, objects, max_range) {
            Some((x, y)) => {
                // return the first clicked monster, otherwise continue looping
                for (id, obj) in objects.iter().enumerate() {
                    if obj.get_pos() == (x, y) && obj.fighter.is_some() && id != PLAYER_ID {
                        return Some(id);
                    }
                }
            }
            None => return None,
        }
    }
}