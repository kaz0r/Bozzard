//! Deterministic, fixed-tick factory rules. No UI or Steam dependency lives here.
use serde::{Deserialize, Serialize};

pub const WIDTH: usize = 22;
pub const HEIGHT: usize = 15;
const HUB_X: usize = 18;
const HUB_Y: usize = 7;

pub fn default_terrain() -> Vec<u8> {
    (0..HEIGHT)
        .flat_map(|y| (0..WIDTH).map(move |x| if (x + y) % 2 == 0 { 14 } else { 15 }))
        .collect()
}

fn default_hub() -> [usize; 2] {
    [HUB_X, HUB_Y]
}

fn default_first_order_amount() -> u32 {
    8
}

fn default_first_order_reward() -> u32 {
    16
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Item {
    IronOre,
    CopperOre,
    IronBar,
    CopperBar,
    Gear,
    Circuit,
}
impl Item {
    pub const ALL: [Self; 6] = [
        Self::IronOre,
        Self::CopperOre,
        Self::IronBar,
        Self::CopperBar,
        Self::Gear,
        Self::Circuit,
    ];
    pub const fn index(self) -> usize {
        self as usize
    }
    pub const fn name(self) -> &'static str {
        match self {
            Self::IronOre => "Iron ore",
            Self::CopperOre => "Copper ore",
            Self::IronBar => "Iron ingot",
            Self::CopperBar => "Copper ingot",
            Self::Gear => "Gear",
            Self::Circuit => "Circuit",
        }
    }
    pub const fn sprite(self) -> usize {
        self.index()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Miner,
    Furnace,
    Assembler,
    Belt,
    Splitter,
    Hub,
}
impl Kind {
    pub const BUILDABLE: [Self; 5] = [
        Self::Miner,
        Self::Furnace,
        Self::Assembler,
        Self::Belt,
        Self::Splitter,
    ];
    pub const fn index(self) -> usize {
        self as usize
    }
    pub const fn name(self) -> &'static str {
        match self {
            Self::Miner => "Miner",
            Self::Furnace => "Furnace",
            Self::Assembler => "Assembler",
            Self::Belt => "Conveyor",
            Self::Splitter => "Splitter",
            Self::Hub => "Hub",
        }
    }
    pub const fn sprite(self) -> usize {
        6 + self.index()
    }
    pub const fn price(self) -> u32 {
        match self {
            Self::Miner => 8,
            Self::Furnace => 7,
            Self::Assembler => 12,
            Self::Belt => 1,
            Self::Splitter => 4,
            Self::Hub => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    North,
    East,
    South,
    West,
}
impl Direction {
    pub const fn next(self) -> Self {
        match self {
            Self::North => Self::East,
            Self::East => Self::South,
            Self::South => Self::West,
            Self::West => Self::North,
        }
    }
    pub const fn delta(self) -> (isize, isize) {
        match self {
            Self::North => (0, -1),
            Self::East => (1, 0),
            Self::South => (0, 1),
            Self::West => (-1, 0),
        }
    }
    pub const fn glyph(self) -> &'static str {
        match self {
            Self::North => "↑",
            Self::East => "→",
            Self::South => "↓",
            Self::West => "←",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Recipe {
    Gear,
    Circuit,
}
impl Recipe {
    pub const fn next(self) -> Self {
        match self {
            Self::Gear => Self::Circuit,
            Self::Circuit => Self::Gear,
        }
    }
    pub const fn item(self) -> Item {
        match self {
            Self::Gear => Item::Gear,
            Self::Circuit => Item::Circuit,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Building {
    pub kind: Kind,
    pub direction: Direction,
    pub recipe: Recipe,
    pub input: [u16; 6],
    pub output: Option<Item>,
    pub progress: u8,
    pub split_next: bool,
}
impl Building {
    pub(crate) fn new(kind: Kind, direction: Direction) -> Self {
        Self {
            kind,
            direction,
            recipe: Recipe::Gear,
            input: [0; 6],
            output: None,
            progress: 0,
            split_next: false,
        }
    }
    fn accepts(&self, item: Item) -> bool {
        match self.kind {
            Kind::Furnace => {
                matches!(item, Item::IronOre | Item::CopperOre) && self.input[item.index()] < 8
            }
            Kind::Assembler => {
                matches!(item, Item::IronBar | Item::CopperBar) && self.input[item.index()] < 8
            }
            Kind::Belt | Kind::Splitter => self.output.is_none(),
            Kind::Hub => true,
            Kind::Miner => false,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Tile {
    pub deposit: Option<Item>,
    pub building: Option<Building>,
}

#[derive(Clone, Copy, Debug)]
pub struct Order {
    pub item: Item,
    pub amount: u32,
    pub reward: u32,
    pub bonus_belts: u16,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Game {
    pub tiles: Vec<Tile>,
    #[serde(default = "default_terrain")]
    pub terrain: Vec<u8>,
    #[serde(default = "default_hub")]
    pub hub: [usize; 2],
    pub buildings: [u16; 5],
    pub stock: [u16; 6],
    pub delivered: [u32; 6],
    pub produced: [u32; 6],
    pub credits: u32,
    pub order_index: u32,
    pub order_progress: u32,
    #[serde(default = "default_first_order_amount")]
    pub first_order_amount: u32,
    #[serde(default = "default_first_order_reward")]
    pub first_order_reward: u32,
    pub ticks: u64,
    pub placed: u32,
    pub paused: bool,
    pub notice: String,
}
impl Default for Game {
    fn default() -> Self {
        Self::new()
    }
}
impl Game {
    pub fn new() -> Self {
        let mut tiles = vec![Tile::default(); WIDTH * HEIGHT];
        for (x, y, kind) in [
            (4, 7, Item::IronOre),
            (5, 3, Item::IronOre),
            (4, 11, Item::CopperOre),
            (8, 12, Item::CopperOre),
        ] {
            tiles[y * WIDTH + x].deposit = Some(kind);
        }
        tiles[HUB_Y * WIDTH + HUB_X].building = Some(Building::new(Kind::Hub, Direction::West));
        Self {
            tiles,
            terrain: default_terrain(),
            hub: default_hub(),
            buildings: [2, 2, 2, 24, 2],
            stock: [4, 4, 0, 0, 0, 0],
            delivered: [0; 6],
            produced: [0; 6],
            credits: 4,
            order_index: 0,
            order_progress: 0,
            first_order_amount: default_first_order_amount(),
            first_order_reward: default_first_order_reward(),
            ticks: 0,
            placed: 0,
            paused: false,
            notice: "Place a miner on the iron deposit, then connect it to a furnace.".into(),
        }
    }
    pub const fn index(x: usize, y: usize) -> Option<usize> {
        if x < WIDTH && y < HEIGHT {
            Some(y * WIDTH + x)
        } else {
            None
        }
    }
    pub fn order(&self) -> Order {
        match self.order_index {
            0 => Order {
                item: Item::IronBar,
                amount: self.first_order_amount,
                reward: self.first_order_reward,
                bonus_belts: 8,
            },
            1 => Order {
                item: Item::Gear,
                amount: 6,
                reward: 25,
                bonus_belts: 10,
            },
            2 => Order {
                item: Item::Circuit,
                amount: 5,
                reward: 35,
                bonus_belts: 12,
            },
            n => Order {
                item: if n % 2 == 1 {
                    Item::Gear
                } else {
                    Item::Circuit
                },
                amount: 8 + (n - 3) * 3,
                reward: 20 + n * 8,
                bonus_belts: 6,
            },
        }
    }
    pub fn place(
        &mut self,
        x: usize,
        y: usize,
        kind: Kind,
        dir: Direction,
    ) -> Result<(), &'static str> {
        let index = Self::index(x, y).ok_or("Outside the factory")?;
        if kind == Kind::Hub {
            return Err("The hub is fixed");
        }
        if self.tiles[index].building.is_some() {
            return Err("That tile is occupied");
        }
        if kind == Kind::Miner && self.tiles[index].deposit.is_none() {
            return Err("Miners need an ore deposit");
        }
        if self.buildings[kind.index()] == 0 {
            return Err("No buildings left. Buy another in Inventory.");
        }
        self.buildings[kind.index()] -= 1;
        self.tiles[index].building = Some(Building::new(kind, dir));
        self.placed += 1;
        self.notice = format!("{} placed. Outputs travel {}.", kind.name(), dir.glyph());
        Ok(())
    }
    pub fn remove(&mut self, x: usize, y: usize) -> Result<(), &'static str> {
        let index = Self::index(x, y).ok_or("Outside the factory")?;
        let building = self.tiles[index]
            .building
            .take()
            .ok_or("Nothing to remove")?;
        if building.kind == Kind::Hub {
            self.tiles[index].building = Some(building);
            return Err("The delivery hub stays in place");
        }
        self.buildings[building.kind.index()] += 1;
        for item in Item::ALL {
            self.stock[item.index()] =
                self.stock[item.index()].saturating_add(building.input[item.index()]);
        }
        if let Some(item) = building.output {
            self.stock[item.index()] = self.stock[item.index()].saturating_add(1);
        }
        self.notice = format!("{} recovered to inventory.", building.kind.name());
        Ok(())
    }
    pub fn buy(&mut self, kind: Kind) -> Result<(), &'static str> {
        if kind == Kind::Hub {
            return Err("The hub cannot be purchased");
        }
        if self.credits < kind.price() {
            return Err("Not enough credits. Complete an order to earn more.");
        }
        self.credits -= kind.price();
        self.buildings[kind.index()] += 1;
        self.notice = format!("Purchased one {}.", kind.name());
        Ok(())
    }
    pub fn inject(&mut self, x: usize, y: usize, item: Item) -> Result<(), &'static str> {
        let index = Self::index(x, y).ok_or("Outside the factory")?;
        if self.stock[item.index()] == 0 {
            return Err("No item in inventory");
        }
        if !self.tiles[index]
            .building
            .as_ref()
            .is_some_and(|b| b.accepts(item))
        {
            return Err("That building cannot take this item");
        }
        self.stock[item.index()] -= 1;
        self.accept(index, item);
        self.notice = format!("Loaded {} by hand.", item.name());
        Ok(())
    }
    pub fn take_output(&mut self, x: usize, y: usize) -> Result<(), &'static str> {
        let index = Self::index(x, y).ok_or("Outside the factory")?;
        let item = self.tiles[index]
            .building
            .as_mut()
            .and_then(|b| b.output.take())
            .ok_or("No finished item here")?;
        self.stock[item.index()] = self.stock[item.index()].saturating_add(1);
        self.notice = format!(
            "Picked up {}. Select it in Inventory to load or deliver.",
            item.name()
        );
        Ok(())
    }
    pub fn rotate(&mut self, x: usize, y: usize) -> Result<(), &'static str> {
        let index = Self::index(x, y).ok_or("Outside the factory")?;
        let b = self.tiles[index]
            .building
            .as_mut()
            .ok_or("Nothing to rotate")?;
        if b.kind == Kind::Hub {
            return Err("The hub cannot rotate");
        }
        b.direction = b.direction.next();
        Ok(())
    }
    pub fn set_recipe(&mut self, x: usize, y: usize) -> Result<(), &'static str> {
        let index = Self::index(x, y).ok_or("Outside the factory")?;
        let b = self.tiles[index]
            .building
            .as_mut()
            .ok_or("No assembler here")?;
        if b.kind != Kind::Assembler {
            return Err("Only assemblers have recipes");
        }
        b.recipe = b.recipe.next();
        b.progress = 0;
        self.notice = format!("Assembler set to {}.", b.recipe.item().name());
        Ok(())
    }
    fn next(&self, index: usize, direction: Direction) -> Option<usize> {
        let x = index % WIDTH;
        let y = index / WIDTH;
        let (dx, dy) = direction.delta();
        Self::index(x.checked_add_signed(dx)?, y.checked_add_signed(dy)?)
    }
    fn accept(&mut self, index: usize, item: Item) {
        let b = self.tiles[index].building.as_mut().unwrap();
        match b.kind {
            Kind::Hub => {
                self.delivered[item.index()] += 1;
                if self.order().item == item {
                    self.order_progress += 1;
                    let order = self.order();
                    if self.order_progress >= order.amount {
                        self.order_index += 1;
                        self.order_progress = 0;
                        self.credits += order.reward;
                        self.buildings[Kind::Belt.index()] += order.bonus_belts;
                        self.notice = format!(
                            "Order complete! +{} credits, +{} conveyors. Next: {}.",
                            order.reward,
                            order.bonus_belts,
                            self.order().item.name()
                        );
                    }
                }
            }
            Kind::Furnace | Kind::Assembler => b.input[item.index()] += 1,
            Kind::Belt | Kind::Splitter => b.output = Some(item),
            Kind::Miner => {}
        }
    }
    pub fn tick(&mut self) {
        if self.paused {
            return;
        }
        self.ticks += 1;
        let mut proposals = Vec::new();
        for index in 0..self.tiles.len() {
            let Some(b) = &self.tiles[index].building else {
                continue;
            };
            let Some(item) = b.output else { continue };
            let directions = [b.direction, b.direction.next()];
            let candidates = if b.kind == Kind::Splitter && b.split_next {
                [directions[1], directions[0]]
            } else {
                directions
            };
            let options = if b.kind == Kind::Splitter { 2 } else { 1 };
            for dir in candidates.into_iter().take(options) {
                let Some(to) = self.next(index, dir) else {
                    continue;
                };
                if self.tiles[to]
                    .building
                    .as_ref()
                    .is_some_and(|target| target.accepts(item))
                {
                    proposals.push((index, to, item));
                    break;
                }
            }
        }
        for (from, to, item) in proposals {
            if self.tiles[from].building.as_ref().and_then(|b| b.output) == Some(item)
                && self.tiles[to]
                    .building
                    .as_ref()
                    .is_some_and(|b| b.accepts(item))
            {
                self.tiles[from].building.as_mut().unwrap().output = None;
                if self.tiles[from]
                    .building
                    .as_ref()
                    .is_some_and(|b| b.kind == Kind::Splitter)
                {
                    self.tiles[from].building.as_mut().unwrap().split_next =
                        !self.tiles[from].building.as_ref().unwrap().split_next;
                }
                self.accept(to, item);
            }
        }
        for index in 0..self.tiles.len() {
            let deposit = self.tiles[index].deposit;
            let Some(b) = self.tiles[index].building.as_mut() else {
                continue;
            };
            if b.output.is_some() {
                continue;
            }
            match b.kind {
                Kind::Miner => {
                    if let Some(item) = deposit {
                        b.progress += 1;
                        if b.progress >= 4 {
                            b.progress = 0;
                            b.output = Some(item);
                            self.produced[item.index()] += 1;
                        }
                    }
                }
                Kind::Furnace => {
                    let ore = if b.input[Item::IronOre.index()] > 0 {
                        Some(Item::IronOre)
                    } else if b.input[Item::CopperOre.index()] > 0 {
                        Some(Item::CopperOre)
                    } else {
                        None
                    };
                    if let Some(ore) = ore {
                        b.progress += 1;
                        if b.progress >= 3 {
                            b.progress = 0;
                            b.input[ore.index()] -= 1;
                            let out = if ore == Item::IronOre {
                                Item::IronBar
                            } else {
                                Item::CopperBar
                            };
                            b.output = Some(out);
                            self.produced[out.index()] += 1;
                        }
                    }
                }
                Kind::Assembler => {
                    let iron = b.input[Item::IronBar.index()];
                    let copper = b.input[Item::CopperBar.index()];
                    let ready = match b.recipe {
                        Recipe::Gear => iron >= 2,
                        Recipe::Circuit => iron >= 1 && copper >= 1,
                    };
                    if ready {
                        b.progress += 1;
                        if b.progress >= 5 {
                            b.progress = 0;
                            b.input[Item::IronBar.index()] -= match b.recipe {
                                Recipe::Gear => 2,
                                Recipe::Circuit => 1,
                            };
                            if b.recipe == Recipe::Circuit {
                                b.input[Item::CopperBar.index()] -= 1;
                            }
                            let out = b.recipe.item();
                            b.output = Some(out);
                            self.produced[out.index()] += 1;
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starter_inventory_can_build_and_complete_first_automated_order() {
        let mut game = Game::new();
        assert_eq!(game.buildings, [2, 2, 2, 24, 2]);
        game.place(4, 7, Kind::Miner, Direction::East).unwrap();
        game.place(5, 7, Kind::Furnace, Direction::East).unwrap();
        for x in 6..18 {
            game.place(x, 7, Kind::Belt, Direction::East).unwrap();
        }
        for _ in 0..400 {
            game.tick();
        }
        assert!(game.order_index >= 1);
        assert!(game.delivered[Item::IronBar.index()] >= 8);
        assert!(game.credits >= 20);
    }

    #[test]
    fn assembler_recipes_and_inventory_recovery_are_real_resources() {
        let mut game = Game::new();
        game.place(10, 6, Kind::Assembler, Direction::East).unwrap();
        game.stock[Item::IronBar.index()] = 3;
        game.inject(10, 6, Item::IronBar).unwrap();
        game.inject(10, 6, Item::IronBar).unwrap();
        for _ in 0..5 {
            game.tick();
        }
        assert_eq!(
            game.tiles[6 * WIDTH + 10].building.as_ref().unwrap().output,
            Some(Item::Gear)
        );
        game.take_output(10, 6).unwrap();
        assert_eq!(game.stock[Item::Gear.index()], 1);
        game.set_recipe(10, 6).unwrap();
        game.stock[Item::CopperBar.index()] = 1;
        game.inject(10, 6, Item::IronBar).unwrap();
        game.inject(10, 6, Item::CopperBar).unwrap();
        for _ in 0..5 {
            game.tick();
        }
        assert_eq!(
            game.tiles[6 * WIDTH + 10].building.as_ref().unwrap().output,
            Some(Item::Circuit)
        );
        game.remove(10, 6).unwrap();
        assert_eq!(game.stock[Item::Circuit.index()], 1);
        assert_eq!(game.buildings[Kind::Assembler.index()], 2);
    }

    #[test]
    fn save_roundtrip_keeps_factory_layout_and_progress() {
        let mut game = Game::new();
        game.place(4, 7, Kind::Miner, Direction::East).unwrap();
        for _ in 0..7 {
            game.tick();
        }
        let saved = serde_json::to_string(&game).unwrap();
        let loaded: Game = serde_json::from_str(&saved).unwrap();
        assert_eq!(loaded.ticks, game.ticks);
        assert_eq!(
            loaded.tiles[7 * WIDTH + 4]
                .building
                .as_ref()
                .unwrap()
                .output,
            Some(Item::IronOre)
        );
        assert_eq!(loaded.buildings, game.buildings);
    }
}
