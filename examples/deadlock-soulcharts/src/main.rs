use haste::{
    entities::{deadlock_coord_from_cell, fkey_from_path, DeltaHeader, Entity},
    fxhash,
    fieldpath::FieldPath,
    fieldvalue::FieldValue,
    flattenedserializers::FlattenedSerializer,
    parser::{self, Context, Parser, Visitor},
};
use std::{
    borrow::{Borrow, BorrowMut}, collections::{hash_map::Entry, HashMap, HashSet}, fs::{File, OpenOptions}, io::{BufReader, Write}
};

fn get_entity_coord(entity: &Entity, cell_key: &u64, vec_key: &u64) -> Option<f32> {
    let cell: u16 = entity.get_value(cell_key)?;
    let vec: f32 = entity.get_value(vec_key)?;
    let coord = deadlock_coord_from_cell(cell, vec);
    Some(coord)
}

fn get_entity_position(entity: &Entity) -> Option<[f32; 3]> {
    const CX: u64 = fkey_from_path(&["CBodyComponent", "m_cellX"]);
    const CY: u64 = fkey_from_path(&["CBodyComponent", "m_cellY"]);
    const CZ: u64 = fkey_from_path(&["CBodyComponent", "m_cellZ"]);

    const VX: u64 = fkey_from_path(&["CBodyComponent", "m_vecX"]);
    const VY: u64 = fkey_from_path(&["CBodyComponent", "m_vecY"]);
    const VZ: u64 = fkey_from_path(&["CBodyComponent", "m_vecZ"]);

    let x = get_entity_coord(entity, &CX, &VX)?;
    let y = get_entity_coord(entity, &CY, &VY)?;
    let z = get_entity_coord(entity, &CZ, &VZ)?;

    Some([x, y, z])
}

fn get_value_info(serializer: &FlattenedSerializer, fp: &FieldPath) -> (Vec<String>, String) {
    let mut result = Vec::with_capacity(fp.last());

    let first_field_index = fp.get(0).unwrap_or(0);
    let mut field = unsafe { serializer
        .get_child(first_field_index)
        // NOTE: this may only throw if data is corrupted or something, but
        // never in normal circumbstances
        .unwrap_unchecked() };
    result.push(field.var_name.str.to_string());

    for field_index in fp.iter().skip(1) {
        if field.is_dynamic_array() {
            field = unsafe { field.get_child(0).unwrap_unchecked() };
            result.push(field_index.to_string());
        } else {
            // TODO: consider changing type of index arg in child*?
            // funcs from usize to u8 to be consistent with an actual
            // type of data in FieldPath
            field = unsafe { field.get_child(*field_index as usize).unwrap_unchecked() };
            result.push(field.var_name.str.to_string());
        }
    }

    (result, field.var_type.str.to_string())
}

fn insert_and_write_field(output_file: &mut File, permitted_fields: &mut HashMap<u64, i16>, path: &[&str]) -> () {
    let key = fkey_from_path(path);
    permitted_fields.insert(key, permitted_fields.len().try_into().unwrap());
    //let _ = writeln!(output_file, "{}\t{}", key, path.join("."));
}

const ENTITY_PLAYERPAWN: u64 = fxhash::hash_bytes(b"CCitadelPlayerPawn");
const ENTITY_TROOPER: u64 = fxhash::hash_bytes(b"CNPC_Trooper");
const ENTITY_TROOPERNEUTRAL: u64 = fxhash::hash_bytes(b"CNPC_TrooperNeutral");
const ENTITY_PLAYERCONTROLLER: u64 = fxhash::hash_bytes(b"CCitadelPlayerController");
const ENTITY_MIDBOSS: u64 = fxhash::hash_bytes(b"CNPC_MidBoss");
const ENTITY_TROOPERBOSS: u64 = fxhash::hash_bytes(b"CNPC_TrooperBoss");
const ENTITY_DESTROYABLEBUILDING: u64 = fxhash::hash_bytes(b"CCitadel_Destroyable_Building");
const ENTITY_BOSS_TIER2: u64 = fxhash::hash_bytes(b"CNPC_Boss_Tier2");
const ENTITY_BOSS_TIER3: u64 = fxhash::hash_bytes(b"CNPC_Boss_Tier3");

#[derive(/*Default, */Debug)]
struct MyVisitor {
    output_file: File,
    cmd_index: i32,
    cmd_printed: bool,
    valid_hero_id_entities: HashSet<i32>,
}

impl MyVisitor {
    fn new(mut output_file: File) -> MyVisitor {
        let valid_hero_id_entities: HashSet<i32> = HashSet::new();
        MyVisitor { output_file, cmd_index: 0, cmd_printed: false, valid_hero_id_entities }
    }

    fn print_cmd_if_needed(&mut self) -> anyhow::Result<()> {
        if !self.cmd_printed {
            writeln!(self.output_file, "C{}", self.cmd_index)?;
            self.cmd_printed = true;
        }
        Ok(())
    }

    fn handle_cmd(&mut self, _cmd_header: &haste::demofile::CmdHeader) -> anyhow::Result<()> {
        self.cmd_index += 1;
        self.cmd_printed = false;
        Ok(())
    }

    fn handle_player_controller(&mut self, delta_header: DeltaHeader, entity: &Entity) -> anyhow::Result<()> {
        if
            delta_header != DeltaHeader::UPDATE 
            || !self.valid_hero_id_entities.contains(&entity.index())
        {
            const KEY_PLAYER_NAME: u64 = fkey_from_path(&["m_iszPlayerName"]);
            const KEY_HERO_ID: u64 = fkey_from_path(&["m_nHeroID"]);

            let hero_id: u64 = entity.get_value(&KEY_HERO_ID).expect("hero_id");

            if delta_header != DeltaHeader::UPDATE || hero_id != 0 {
                let player_name: String = entity.get_value(&KEY_PLAYER_NAME).expect("player_name");

                self.print_cmd_if_needed()?;
                writeln!(self.output_file, "U{}\t{}\t{}\t{}", delta_header.value(), entity.index(), player_name, hero_id)?;
            }

            if delta_header == DeltaHeader::DELETE || hero_id == 0 {
                self.valid_hero_id_entities.remove(&entity.index());
            }
            else {
                self.valid_hero_id_entities.insert(entity.index());
            }
        }
        Ok(())
    }

    fn handle_player_pawn(&mut self, delta_header: DeltaHeader, entity: &Entity) -> anyhow::Result<()> {
        if 
            delta_header != DeltaHeader::UPDATE 
            || !self.valid_hero_id_entities.contains(&entity.index())
        {
            const KEY_HERO_ID: u64 = fkey_from_path(&["m_nHeroID"]);

            let hero_id: u64 = entity.get_value(&KEY_HERO_ID).expect("hero_id");

            if delta_header != DeltaHeader::UPDATE || hero_id != 0 {
                self.print_cmd_if_needed()?;
                writeln!(self.output_file, "P{}\t{}\t{}", delta_header.value(), entity.index(), hero_id)?;
            }

            if delta_header == DeltaHeader::DELETE || hero_id == 0 {
                self.valid_hero_id_entities.remove(&entity.index());
            }
            else {
                self.valid_hero_id_entities.insert(entity.index());
            }
        }
        Ok(())
    }

    fn handle_trooper(&mut self, delta_header: DeltaHeader, entity: &Entity) -> anyhow::Result<()> {
        if delta_header != DeltaHeader::UPDATE {
            const KEY_TEAM_NUM: u64 = fkey_from_path(&["m_iTeamNum"]);
            const KEY_LANE: u64 = fkey_from_path(&["m_iLane"]);

            let team_num: u64 = entity.get_value(&KEY_TEAM_NUM).expect("team_num");
            let lane: i64 = entity.get_value(&KEY_LANE).expect("lane");

            self.print_cmd_if_needed()?;
            writeln!(self.output_file, "T{}\t{}\t{}\t{}", delta_header.value(), entity.index(), team_num, lane)?;
        }
        Ok(())
    }

    fn handle_trooper_boss(&mut self, delta_header: DeltaHeader, entity: &Entity) -> anyhow::Result<()> {
        if delta_header != DeltaHeader::UPDATE {
            const KEY_TEAM_NUM: u64 = fkey_from_path(&["m_iTeamNum"]);
            const KEY_LANE: u64 = fkey_from_path(&["m_iLane"]);
            const KEY_MAX_HEALTH: u64 = fkey_from_path(&["m_iMaxHealth"]);

            let team_num: u64 = entity.get_value(&KEY_TEAM_NUM).expect("team_num");
            let lane: i64 = entity.get_value(&KEY_LANE).expect("lane");
            let max_health: i64 = entity.get_value(&KEY_MAX_HEALTH).expect("max_health");

            self.print_cmd_if_needed()?;
            writeln!(self.output_file, "B{}\t{}\t{}\t{}\t{}", delta_header.value(), entity.index(), team_num, lane, max_health)?;
        }
        Ok(())
    }

    fn handle_boss_tier2(&mut self, delta_header: DeltaHeader, entity: &Entity) -> anyhow::Result<()> {
        if delta_header != DeltaHeader::UPDATE {
            const KEY_TEAM_NUM: u64 = fkey_from_path(&["m_iTeamNum"]);
            const KEY_LANE: u64 = fkey_from_path(&["m_iLane"]);

            let team_num: u64 = entity.get_value(&KEY_TEAM_NUM).expect("team_num");
            let lane: i64 = entity.get_value(&KEY_LANE).expect("lane");

            self.print_cmd_if_needed()?;
            writeln!(self.output_file, "2{}\t{}\t{}\t{}", delta_header.value(), entity.index(), team_num, lane)?;
        }
        Ok(())
    }

    fn handle_boss_tier3(&mut self, delta_header: DeltaHeader, entity: &Entity) -> anyhow::Result<()> {
        if delta_header != DeltaHeader::UPDATE {
            const KEY_TEAM_NUM: u64 = fkey_from_path(&["m_iTeamNum"]);

            let team_num: u64 = entity.get_value(&KEY_TEAM_NUM).expect("team_num");

            self.print_cmd_if_needed()?;
            writeln!(self.output_file, "3{}\t{}\t{}", delta_header.value(), entity.index(), team_num)?;
        }
        Ok(())
    }

    fn handle_trooper_neutral(&mut self, delta_header: DeltaHeader, entity: &Entity) -> anyhow::Result<()> {
        if delta_header != DeltaHeader::UPDATE {
            self.print_cmd_if_needed()?;
            writeln!(self.output_file, "N{}\t{}", delta_header.value(), entity.index())?;
        }
        Ok(())
    }

    fn handle_mid_boss(&mut self, delta_header: DeltaHeader, entity: &Entity) -> anyhow::Result<()> {
        if delta_header != DeltaHeader::UPDATE {
            self.print_cmd_if_needed()?;
            writeln!(self.output_file, "R{}\t{}", delta_header.value(), entity.index())?;
        }
        Ok(())
    }

    fn handle_destroyable_building(&mut self, delta_header: DeltaHeader, entity: &Entity) -> anyhow::Result<()> {
        if delta_header != DeltaHeader::UPDATE {
            const KEY_TEAM_NUM: u64 = fkey_from_path(&["m_iTeamNum"]);
            const KEY_CELL_X: u64 = fkey_from_path(&["CBodyComponent", "m_cellX"]);

            let team_num: u64 = entity.get_value(&KEY_TEAM_NUM).expect("team_num");
            let cell_x: u64 = entity.get_value(&KEY_CELL_X).expect("cell_x");

            self.print_cmd_if_needed()?;
            writeln!(self.output_file, "S{}\t{}\t{}\t{}", delta_header.value(), entity.index(), team_num, cell_x)?;
        }
        Ok(())
    }

}

impl Visitor for &mut MyVisitor {
    fn on_cmd(
        &mut self, 
        _ctx: &Context, 
        cmd_header: &haste::demofile::CmdHeader, 
        _data: &[u8]
    ) -> parser::Result<()> {
        self.handle_cmd(cmd_header)?;
        Ok(())
    }

    fn on_entity(
        &mut self,
        _ctx: &Context,
        delta_header: DeltaHeader,
        entity: &Entity,
    ) -> parser::Result<()> {
        if entity.serializer_name_heq(ENTITY_TROOPER) { self.handle_trooper(delta_header, entity)?; }
        else if entity.serializer_name_heq(ENTITY_PLAYERPAWN) { self.handle_player_pawn(delta_header, entity)?; }
        else if entity.serializer_name_heq(ENTITY_TROOPERNEUTRAL) { self.handle_trooper_neutral(delta_header, entity)?; }
        else if entity.serializer_name_heq(ENTITY_MIDBOSS) { self.handle_mid_boss(delta_header, entity)?; }
        else if entity.serializer_name_heq(ENTITY_TROOPERBOSS) { self.handle_trooper_boss(delta_header, entity)?; }
        else if entity.serializer_name_heq(ENTITY_DESTROYABLEBUILDING) { self.handle_destroyable_building(delta_header, entity)?; }
        else if entity.serializer_name_heq(ENTITY_BOSS_TIER2) { self.handle_boss_tier2(delta_header, entity)?; }
        else if entity.serializer_name_heq(ENTITY_BOSS_TIER3) { self.handle_boss_tier3(delta_header, entity)?; }
        else if entity.serializer_name_heq(ENTITY_PLAYERCONTROLLER) { self.handle_player_controller(delta_header, entity)?; }
        else if delta_header != DeltaHeader::UPDATE {
            self.print_cmd_if_needed()?;
            writeln!(self.output_file, "O{}\t{}\t{}", delta_header.value(), entity.index(), entity.serializer().serializer_name.str.to_string())?;
        }
        Ok(())
    }
}

fn main() -> parser::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let filepath = args.get(1);
    let output_filepath = args.get(2);
    if filepath.is_none() || output_filepath.is_none() {
        eprintln!("usage: deadlock-position <filepath> <output_filepath>");
        std::process::exit(42);
    }

    let output_file = OpenOptions::new()
        .write(true)
        .append(false)
        .create(true)
        .open(output_filepath.unwrap_or(&String::from("")))?;

    let mut visitor = MyVisitor::new(output_file);

    let file = File::open(filepath.unwrap())?;
    let buf_reader = BufReader::new(file);
    let mut parser = Parser::from_reader_with_visitor(buf_reader, &mut visitor)?;
    parser.run_to_end()
}
