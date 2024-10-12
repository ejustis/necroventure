use std::{error::Error, fs::File, io::Read};

use serde::{Deserialize, Serialize};
use tcod::Color;

use super::{Ai, DisplayObj, Equipment, Fighter, Item};

const SETTINGS_FILE: &str = "settings.json";

#[derive(Debug, Serialize, Deserialize)]
pub struct Transition {
    pub level: u32,
    pub value: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ObjectConfiguration {
    name: String,
    char: char,
    color: Color,
    pub transition_table: Vec<Transition>,
    pub fighter: Option<Fighter>,
    pub ai: Option<Ai>,
    pub item: Option<Item>,
    pub equipment: Option<Equipment>
}

impl ObjectConfiguration {
    pub fn new(name: String, char: char, color: Color, tables: Vec<Transition>, 
            fighter: Option<Fighter>, ai: Option<Ai>, item: Option<Item>, equipment: Option<Equipment>) -> Self {
        ObjectConfiguration {
            name: name,
            char: char,
            color: color,
            transition_table: tables,
            fighter: fighter,
            ai: ai,
            item: item,
            equipment: equipment
        }
    }

    pub fn as_object(&self, x: i32, y: i32) -> DisplayObj {
        let mut object = DisplayObj::new(x, y, self.char, &self.name,  self.color, true);

        if self.fighter != None {
            object.fighter = self.fighter;
            object.ai = self.ai.clone();
            object.alive = true;
        } else if self.item != None {
            object.item = self.item;
            object.equipment = self.equipment;
            object.always_visible = true;
            object.blocks = false;
        }
        object
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TransitionTables {
    pub max_monsters: Vec<Transition>,
    pub max_items: Vec<Transition>,
    pub monsters: Vec<ObjectConfiguration>,
    pub items: Vec<ObjectConfiguration>
}

impl TransitionTables {
    pub fn new(max_monsters: Vec<Transition>, max_items: Vec<Transition>) -> Self {
        TransitionTables {
            max_monsters : max_monsters,
            max_items: max_items,
            monsters : Vec::new(),
            items: Vec::new()
        }
    }
}

pub fn load_weighted_tables() -> Result<TransitionTables, Box<dyn Error>> {
    let mut json_settings = String::new();
    let mut file = File::open(SETTINGS_FILE)?;
    file.read_to_string(&mut json_settings)?;

    let tables = serde_json
            ::from_str
            ::<TransitionTables>(&json_settings)?;

    Ok(tables)
}