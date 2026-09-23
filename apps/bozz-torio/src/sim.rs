//! Deterministic, fixed-tick factory rules. No UI or Steam dependency lives here.
use serde::{Deserialize, Serialize};

pub const WIDTH: usize = 256;
pub const HEIGHT: usize = 256;
pub const PATCH_WIDTH: usize = 22;
pub const PATCH_HEIGHT: usize = 15;
pub const PATCH_X: usize = (WIDTH - PATCH_WIDTH) / 2;
pub const PATCH_Y: usize = (HEIGHT - PATCH_HEIGHT) / 2;
const HUB_X: usize = PATCH_X + 18;
const HUB_Y: usize = PATCH_Y + 7;

fn hash(seed: u64, x: usize, y: usize) -> u64 {
    let mut z = seed
        ^ (x as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ (y as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

fn biome(seed: u64, x: usize, y: usize) -> u8 {
    let gx = x as isize / 32;
    let gy = y as isize / 32;
    let mut nearest = i64::MAX;
    let mut selected = 0;
    for cell_y in gy - 1..=gy + 1 {
        for cell_x in gx - 1..=gx + 1 {
            let value = hash(seed ^ 0x51a9_a39b, cell_x as usize, cell_y as usize);
            let center_x = cell_x * 32 + 7 + (value % 19) as isize;
            let center_y = cell_y * 32 + 7 + ((value >> 8) % 19) as isize;
            let dx = x as isize - center_x;
            let dy = y as isize - center_y;
            let distance = (dx * dx + dy * dy) as i64;
            if distance < nearest {
                nearest = distance;
                selected = ((value >> 20) % 8) as u8;
            }
        }
    }
    selected
}

pub fn default_terrain() -> Vec<u8> {
    (0..HEIGHT)
        .flat_map(|y| (0..WIDTH).map(move |x| 22 + biome(1, x, y) * 8 + (hash(1, x, y) % 8) as u8))
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
pub enum Resource {
    IronOre,
    CopperOre,
    Coal,
}
impl Resource {
    pub const fn name(self) -> &'static str {
        match self {
            Self::IronOre => "Iron ore",
            Self::CopperOre => "Copper ore",
            Self::Coal => "Coal seam",
        }
    }
    pub const fn sprite(self) -> usize {
        match self {
            Self::IronOre => 12,
            Self::CopperOre => 13,
            Self::Coal => 17,
        }
    }
    pub const fn ore(self) -> Option<Item> {
        match self {
            Self::IronOre => Some(Item::IronOre),
            Self::CopperOre => Some(Item::CopperOre),
            Self::Coal => None,
        }
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
    Generator,
    PowerPole,
}
impl Kind {
    pub const BUILDABLE: [Self; 7] = [
        Self::Miner,
        Self::Furnace,
        Self::Assembler,
        Self::Belt,
        Self::Splitter,
        Self::Generator,
        Self::PowerPole,
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
            Self::Generator => "Generator",
            Self::PowerPole => "Power pole",
        }
    }
    pub const fn sprite(self) -> usize {
        match self {
            Self::Generator => 18,
            Self::PowerPole => 19,
            _ => 6 + self.index(),
        }
    }
    pub const fn price(self) -> u32 {
        match self {
            Self::Miner => 8,
            Self::Furnace => 7,
            Self::Assembler => 12,
            Self::Belt => 1,
            Self::Splitter => 4,
            Self::Hub => 0,
            Self::Generator => 22,
            Self::PowerPole => 6,
        }
    }
    pub const fn unlock_after(self) -> u32 {
        match self {
            Self::Generator | Self::PowerPole => 3,
            _ => 0,
        }
    }
    pub const fn upgrade_after(self) -> u32 {
        match self {
            Self::Miner => 1,
            Self::Furnace => 2,
            Self::Assembler => 4,
            Self::Generator => 5,
            Self::PowerPole => 6,
            Self::Belt => 7,
            Self::Splitter => 8,
            Self::Hub => u32::MAX,
        }
    }
    pub const fn upgrade_slot(self) -> u32 {
        match self {
            Self::Miner => 0,
            Self::Furnace => 1,
            Self::Assembler => 2,
            Self::Generator => 3,
            Self::PowerPole => 4,
            Self::Belt => 5,
            Self::Splitter => 6,
            Self::Hub => u32::MAX,
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
    #[serde(default = "default_level")]
    pub level: u8,
}
const fn default_level() -> u8 {
    1
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
            level: 1,
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
            Kind::Miner | Kind::Generator | Kind::PowerPole => false,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Tile {
    pub deposit: Option<Resource>,
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
    pub buildings: Vec<u16>,
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
    #[serde(default)]
    pub seed: u64,
    #[serde(default)]
    pub energy_capacity: u32,
    #[serde(default)]
    pub energy_used: u32,
}
impl Default for Game {
    fn default() -> Self {
        Self::new()
    }
}
impl Game {
    pub fn new() -> Self {
        Self::from_seed(1)
    }
    pub fn from_seed(seed: u64) -> Self {
        let mut tiles = vec![Tile::default(); WIDTH * HEIGHT];
        let mut terrain = Vec::with_capacity(WIDTH * HEIGHT);
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let random = hash(seed, x, y);
                terrain.push(22 + biome(seed, x, y) * 8 + (random % 8) as u8);
                if random.is_multiple_of(307) {
                    tiles[y * WIDTH + x].deposit = Some(match (random >> 12) % 5 {
                        0 | 1 => Resource::IronOre,
                        2 | 3 => Resource::CopperOre,
                        _ => Resource::Coal,
                    });
                }
            }
        }
        for y in PATCH_Y..PATCH_Y + PATCH_HEIGHT {
            for x in PATCH_X..PATCH_X + PATCH_WIDTH {
                tiles[y * WIDTH + x].deposit = None;
                terrain[y * WIDTH + x] = if (x + y).is_multiple_of(2) { 14 } else { 15 };
            }
        }
        for (x, y, kind) in [
            (PATCH_X + 4, PATCH_Y + 7, Resource::IronOre),
            (PATCH_X + 5, PATCH_Y + 3, Resource::IronOre),
            (PATCH_X + 4, PATCH_Y + 11, Resource::CopperOre),
            (PATCH_X + 8, PATCH_Y + 12, Resource::CopperOre),
            (PATCH_X + 12, PATCH_Y + 11, Resource::Coal),
        ] {
            tiles[y * WIDTH + x].deposit = Some(kind);
        }
        tiles[HUB_Y * WIDTH + HUB_X].building = Some(Building::new(Kind::Hub, Direction::West));
        Self {
            tiles,
            terrain,
            hub: default_hub(),
            buildings: vec![2, 2, 2, 24, 2, 0, 0, 0],
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
            seed,
            energy_capacity: 0,
            energy_used: 0,
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
            3 => Order {
                item: Item::IronBar,
                amount: 20,
                reward: 38,
                bonus_belts: 10,
            },
            4 => Order {
                item: Item::Gear,
                amount: 12,
                reward: 48,
                bonus_belts: 12,
            },
            5 => Order {
                item: Item::Circuit,
                amount: 12,
                reward: 60,
                bonus_belts: 14,
            },
            n => Order {
                item: match n % 3 {
                    0 => Item::IronBar,
                    1 => Item::Gear,
                    _ => Item::Circuit,
                },
                amount: 14 + (n - 6) * 4,
                reward: 20 + n * 8,
                bonus_belts: 6,
            },
        }
    }
    pub const fn tier(&self) -> u32 {
        self.order_index / 3 + 1
    }
    pub const fn phase(&self) -> u32 {
        self.order_index % 3 + 1
    }
    pub fn next_unlock(&self) -> String {
        match self.order_index {
            0 => "Miner Mk II".into(),
            1 => "Furnace Mk II".into(),
            2 => "Electricity: generator + power pole".into(),
            3 => "Assembler Mk II".into(),
            4 => "Generator Mk II".into(),
            5 => "Power pole Mk II".into(),
            6 => "Conveyor Mk II".into(),
            7 => "Splitter Mk II".into(),
            n => {
                let kinds = [
                    Kind::Miner,
                    Kind::Furnace,
                    Kind::Assembler,
                    Kind::Generator,
                    Kind::PowerPole,
                    Kind::Belt,
                    Kind::Splitter,
                ];
                let slot = ((n - 8) % 7) as usize;
                let level = 3 + (n - 8) / 7;
                format!("{} Mk {}", kinds[slot].name(), level)
            }
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
        if self.order_index < kind.unlock_after() {
            return Err("Complete the current tier to unlock this building");
        }
        if self.tiles[index].building.is_some() {
            return Err("That tile is occupied");
        }
        if kind == Kind::Miner && self.tiles[index].deposit.is_none_or(|r| r.ore().is_none()) {
            return Err("Miners need an ore deposit");
        }
        if kind == Kind::Generator && self.tiles[index].deposit != Some(Resource::Coal) {
            return Err("Generators need a coal seam");
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
        if self.order_index < kind.unlock_after() {
            return Err("Complete Tier 1 to unlock electricity");
        }
        if self.credits < kind.price() {
            return Err("Not enough credits. Complete an order to earn more.");
        }
        self.credits -= kind.price();
        self.buildings[kind.index()] += 1;
        self.notice = format!("Purchased one {}.", kind.name());
        Ok(())
    }
    pub fn max_level(&self, kind: Kind) -> u8 {
        if kind == Kind::Hub || self.order_index < kind.upgrade_after() {
            return 1;
        }
        let mut max_level = 2;
        while max_level < 12
            && self.order_index >= 9 + kind.upgrade_slot() + 7 * u32::from(max_level - 2)
        {
            max_level += 1;
        }
        max_level
    }
    pub fn upgrade(&mut self, x: usize, y: usize) -> Result<(), &'static str> {
        let index = Self::index(x, y).ok_or("Outside the factory")?;
        let kind = self.tiles[index]
            .building
            .as_ref()
            .ok_or("No building to upgrade")?
            .kind;
        let max_level = self.max_level(kind);
        let building = self.tiles[index]
            .building
            .as_mut()
            .ok_or("No building to upgrade")?;
        if building.kind == Kind::Hub {
            return Err("The hub cannot be upgraded");
        }
        if building.level >= max_level {
            return Err("Finish more phases to unlock the next upgrade level");
        }
        let price = building.kind.price() * u32::from(building.level + 1) + 8;
        if self.credits < price {
            return Err("Not enough credits for this upgrade");
        }
        self.credits -= price;
        building.level += 1;
        self.notice = format!(
            "{} upgraded to Mk {} for ¤{}.",
            building.kind.name(),
            building.level,
            price
        );
        Ok(())
    }
    fn cover(mask: &mut [bool], index: usize, radius: usize) {
        let cx = index % WIDTH;
        let cy = index / WIDTH;
        for y in cy.saturating_sub(radius)..=(cy + radius).min(HEIGHT - 1) {
            for x in cx.saturating_sub(radius)..=(cx + radius).min(WIDTH - 1) {
                if x.abs_diff(cx) + y.abs_diff(cy) <= radius {
                    mask[y * WIDTH + x] = true;
                }
            }
        }
    }
    pub fn power_network(&self) -> (Vec<bool>, u32) {
        let mut mask = vec![false; WIDTH * HEIGHT];
        let mut capacity = 0;
        let mut poles = Vec::new();
        for (index, tile) in self.tiles.iter().enumerate() {
            let Some(building) = &tile.building else {
                continue;
            };
            match building.kind {
                Kind::Generator if tile.deposit == Some(Resource::Coal) => {
                    capacity += 12 * u32::from(building.level);
                    Self::cover(&mut mask, index, 6 + usize::from(building.level - 1));
                }
                Kind::PowerPole => poles.push((index, building.level)),
                _ => {}
            }
        }
        let mut connected = vec![false; poles.len()];
        loop {
            let mut added = false;
            for (i, &(index, level)) in poles.iter().enumerate() {
                if !connected[i] && mask[index] {
                    connected[i] = true;
                    Self::cover(&mut mask, index, 6 + 2 * usize::from(level - 1));
                    added = true;
                }
            }
            if !added {
                break;
            }
        }
        (mask, capacity)
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
                        let unlocked = self.next_unlock();
                        self.order_index += 1;
                        self.order_progress = 0;
                        self.credits += order.reward;
                        self.buildings[Kind::Belt.index()] += order.bonus_belts;
                        self.notice = format!(
                            "Phase complete! +{} credits, +{} conveyors. Unlocked: {}.",
                            order.reward, order.bonus_belts, unlocked
                        );
                    }
                }
            }
            Kind::Furnace | Kind::Assembler => b.input[item.index()] += 1,
            Kind::Belt | Kind::Splitter => b.output = Some(item),
            Kind::Miner | Kind::Generator | Kind::PowerPole => {}
        }
    }
    fn move_outputs(&mut self, upgraded_only: bool, power_mask: &[bool], capacity: u32) {
        let mut proposals = Vec::new();
        for (index, &has_power) in power_mask.iter().enumerate().take(self.tiles.len()) {
            let Some(b) = &self.tiles[index].building else {
                continue;
            };
            if upgraded_only
                && (!matches!(b.kind, Kind::Belt | Kind::Splitter) || b.level < 2 || !has_power)
            {
                continue;
            }
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
            if upgraded_only && self.energy_used >= capacity {
                break;
            }
            if self.tiles[from].building.as_ref().and_then(|b| b.output) == Some(item)
                && self.tiles[to]
                    .building
                    .as_ref()
                    .is_some_and(|b| b.accepts(item))
            {
                if upgraded_only {
                    self.energy_used += 1;
                }
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
    }
    pub fn tick(&mut self) {
        if self.paused {
            return;
        }
        self.ticks += 1;
        let (power_mask, capacity) = self.power_network();
        self.energy_capacity = capacity;
        self.energy_used = 0;
        self.move_outputs(false, &power_mask, capacity);
        self.move_outputs(true, &power_mask, capacity);
        for (index, &has_power) in power_mask.iter().enumerate().take(self.tiles.len()) {
            let deposit = self.tiles[index].deposit.and_then(Resource::ore);
            let Some(b) = self.tiles[index].building.as_mut() else {
                continue;
            };
            if b.output.is_some() {
                continue;
            }
            let mut effective_level = b.level.min(2);
            if b.level > 1 && matches!(b.kind, Kind::Miner | Kind::Furnace | Kind::Assembler) {
                let demand = u32::from(b.level - 1) * 2;
                if has_power && self.energy_used + demand <= capacity {
                    self.energy_used += demand;
                    effective_level = b.level.saturating_add(1);
                }
            }
            match b.kind {
                Kind::Miner => {
                    if let Some(item) = deposit {
                        b.progress += 1;
                        if b.progress >= 5u8.saturating_sub(effective_level) {
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
                        if b.progress >= 4u8.saturating_sub(effective_level) {
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
                        if b.progress >= 6u8.saturating_sub(effective_level) {
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
    fn inspecting_locked_upgrades_does_not_underflow() {
        let mut game = Game::new();
        for kind in Kind::BUILDABLE {
            assert_eq!(game.max_level(kind), 1);
        }
        assert_eq!(game.max_level(Kind::Hub), 1);
        let (x, y) = (PATCH_X + 4, PATCH_Y + 7);
        game.place(x, y, Kind::Miner, Direction::East).unwrap();
        assert_eq!(
            game.upgrade(x, y),
            Err("Finish more phases to unlock the next upgrade level")
        );
        let hub = game.hub[1] * WIDTH + game.hub[0];
        for _ in 0..game.order().amount {
            game.accept(hub, Item::IronBar);
        }
        assert_eq!(game.max_level(Kind::Miner), 2);
        assert_eq!(game.max_level(Kind::Furnace), 1);
    }

    #[test]
    fn starter_inventory_can_build_and_complete_first_automated_order() {
        let mut game = Game::new();
        assert_eq!(game.buildings, [2, 2, 2, 24, 2, 0, 0, 0]);
        game.place(PATCH_X + 4, PATCH_Y + 7, Kind::Miner, Direction::East)
            .unwrap();
        game.place(PATCH_X + 5, PATCH_Y + 7, Kind::Furnace, Direction::East)
            .unwrap();
        for x in 6..18 {
            game.place(PATCH_X + x, PATCH_Y + 7, Kind::Belt, Direction::East)
                .unwrap();
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
        let (x, y) = (PATCH_X + 10, PATCH_Y + 6);
        game.place(x, y, Kind::Assembler, Direction::East).unwrap();
        game.stock[Item::IronBar.index()] = 3;
        game.inject(x, y, Item::IronBar).unwrap();
        game.inject(x, y, Item::IronBar).unwrap();
        for _ in 0..5 {
            game.tick();
        }
        assert_eq!(
            game.tiles[y * WIDTH + x].building.as_ref().unwrap().output,
            Some(Item::Gear)
        );
        game.take_output(x, y).unwrap();
        assert_eq!(game.stock[Item::Gear.index()], 1);
        game.set_recipe(x, y).unwrap();
        game.stock[Item::CopperBar.index()] = 1;
        game.inject(x, y, Item::IronBar).unwrap();
        game.inject(x, y, Item::CopperBar).unwrap();
        for _ in 0..5 {
            game.tick();
        }
        assert_eq!(
            game.tiles[y * WIDTH + x].building.as_ref().unwrap().output,
            Some(Item::Circuit)
        );
        game.remove(x, y).unwrap();
        assert_eq!(game.stock[Item::Circuit.index()], 1);
        assert_eq!(game.buildings[Kind::Assembler.index()], 2);
    }

    #[test]
    fn save_roundtrip_keeps_factory_layout_and_progress() {
        let mut game = Game::new();
        game.place(PATCH_X + 4, PATCH_Y + 7, Kind::Miner, Direction::East)
            .unwrap();
        for _ in 0..7 {
            game.tick();
        }
        let saved = serde_json::to_string(&game).unwrap();
        let loaded: Game = serde_json::from_str(&saved).unwrap();
        assert_eq!(loaded.ticks, game.ticks);
        assert_eq!(
            loaded.tiles[(PATCH_Y + 7) * WIDTH + PATCH_X + 4]
                .building
                .as_ref()
                .unwrap()
                .output,
            Some(Item::IronOre)
        );
        assert_eq!(loaded.buildings, game.buildings);
    }

    #[test]
    fn large_world_is_seeded_and_has_many_resource_nodes() {
        let a = Game::from_seed(42);
        let b = Game::from_seed(42);
        let c = Game::from_seed(43);
        assert_eq!(a.tiles.len(), 256 * 256);
        let resources = |game: &Game| {
            game.tiles
                .iter()
                .map(|tile| tile.deposit)
                .collect::<Vec<_>>()
        };
        assert_eq!(resources(&a), resources(&b));
        assert_ne!(resources(&a), resources(&c));
        assert!(a.tiles.iter().filter(|tile| tile.deposit.is_some()).count() > 150);
        let terrain_frames = a
            .terrain
            .iter()
            .copied()
            .filter(|frame| (22..=85).contains(frame))
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(terrain_frames.len(), 64);
    }

    #[test]
    fn phases_unlock_electricity_and_a_connected_pole_powers_upgrades() {
        let mut game = Game::new();
        assert!(game.buy(Kind::Generator).is_err());
        let hub = game.hub[1] * WIDTH + game.hub[0];
        for _ in 0..3 {
            let order = game.order();
            for _ in 0..order.amount {
                game.accept(hub, order.item);
            }
        }
        assert_eq!((game.tier(), game.phase()), (2, 1));
        game.buy(Kind::Generator).unwrap();
        game.buy(Kind::PowerPole).unwrap();
        game.place(PATCH_X + 12, PATCH_Y + 11, Kind::Generator, Direction::East)
            .unwrap();
        game.place(PATCH_X + 8, PATCH_Y + 9, Kind::PowerPole, Direction::East)
            .unwrap();
        game.place(PATCH_X + 4, PATCH_Y + 7, Kind::Miner, Direction::East)
            .unwrap();
        let miner = (PATCH_Y + 7) * WIDTH + PATCH_X + 4;
        assert!(game.power_network().0[miner]);
        game.upgrade(PATCH_X + 4, PATCH_Y + 7).unwrap();
        game.tick();
        assert_eq!(game.energy_capacity, 12);
        assert_eq!(game.energy_used, 2);
        assert_eq!(game.tiles[miner].building.as_ref().unwrap().level, 2);
    }
}
