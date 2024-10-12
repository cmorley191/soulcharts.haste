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
    let _ = writeln!(output_file, "{}\t{}", key, path.join("."));
}

const ENTITY_PLAYERPAWN: u64 = fxhash::hash_bytes(b"CCitadelPlayerPawn");
const ENTITY_TROOPER: u64 = fxhash::hash_bytes(b"CNPC_Trooper");
const ENTITY_TROOPERNEUTRAL: u64 = fxhash::hash_bytes(b"CNPC_TrooperNeutral");
const ENTITY_PLAYERCONTROLLER: u64 = fxhash::hash_bytes(b"CCitadelPlayerController");
const ENTITY_MIDBOSS: u64 = fxhash::hash_bytes(b"CNPC_MidBoss");
const ENTITY_TROOPERBOSS: u64 = fxhash::hash_bytes(b"CNPC_TrooperBoss");
const ENTITY_BASEDEFENSESENTRY: u64 = fxhash::hash_bytes(b"CNPC_BaseDefenseSentry");
const ENTITY_BOSS_TIER2: u64 = fxhash::hash_bytes(b"CNPC_Boss_Tier2");
const ENTITY_BOSS_TIER3: u64 = fxhash::hash_bytes(b"CNPC_Boss_Tier3");

#[derive(/*Default, */Debug)]
struct MyVisitor {
    output_file: File,
    positions: HashMap<i32, [f32; 3]>,
    entity_fields: HashMap<i32, HashMap<u64, FieldValue>>,
    permitted_fields: HashMap<u64, i16>,
    cmd_index: i32,
}

impl MyVisitor {
    fn new(mut output_file: File) -> MyVisitor {
        let positions: HashMap<i32, [f32; 3]> = HashMap::new();
        let entity_fields: HashMap<i32, HashMap<u64, FieldValue>> = HashMap::new();
        let mut permitted_fields: HashMap<u64, i16> = HashMap::new();
        insert_and_write_field(output_file.borrow_mut(), permitted_fields.borrow_mut(), &["CBodyComponent", "m_cellX"]);
        insert_and_write_field(output_file.borrow_mut(), permitted_fields.borrow_mut(), &["CBodyComponent", "m_cellY"]);
        insert_and_write_field(output_file.borrow_mut(), permitted_fields.borrow_mut(), &["CBodyComponent", "m_cellZ"]);
        insert_and_write_field(output_file.borrow_mut(), permitted_fields.borrow_mut(), &["CBodyComponent", "m_vecX"]);
        insert_and_write_field(output_file.borrow_mut(), permitted_fields.borrow_mut(), &["CBodyComponent", "m_vecY"]);
        insert_and_write_field(output_file.borrow_mut(), permitted_fields.borrow_mut(), &["CBodyComponent", "m_vecZ"]);
        insert_and_write_field(output_file.borrow_mut(), permitted_fields.borrow_mut(), &["m_iszPlayerName"]);
        insert_and_write_field(output_file.borrow_mut(), permitted_fields.borrow_mut(), &["m_nAssignedLane"]);
        insert_and_write_field(output_file.borrow_mut(), permitted_fields.borrow_mut(), &["m_nHeroID"]);
        insert_and_write_field(output_file.borrow_mut(), permitted_fields.borrow_mut(), &["m_nHeroID"]);
        insert_and_write_field(output_file.borrow_mut(), permitted_fields.borrow_mut(), &["m_nHeroID"]);
        insert_and_write_field(output_file.borrow_mut(), permitted_fields.borrow_mut(), &["m_nHeroID"]);
        insert_and_write_field(output_file.borrow_mut(), permitted_fields.borrow_mut(), &["m_nHeroID"]);
        insert_and_write_field(output_file.borrow_mut(), permitted_fields.borrow_mut(), &["m_nHeroID"]);

        let _ = writeln!(output_file, "");
        MyVisitor { output_file, positions, entity_fields, permitted_fields, cmd_index: 0 }
    }

    fn handle_cmd(&mut self, cmd_header: &haste::demofile::CmdHeader) -> anyhow::Result<()> {
        let _ = writeln!(self.output_file, "\tC{}\t{:?}", self.cmd_index, cmd_header.command);
        self.cmd_index += 1;
        Ok(())
    }

    fn handle_player_pawn(&mut self, delta_header: DeltaHeader, entity: &Entity) -> anyhow::Result<()> {
        match self.entity_fields.entry(entity.index()) {
            Entry::Occupied(mut oe) => {
                let fields = oe.get_mut();

                let mut wrote_entity_header = false;

                entity
                    .iter()
                    .for_each(|(key, field_value)| {
                        match self.permitted_fields.entry(*key) {
                            Entry::Occupied(oe_keymap) => {
                                let mapped_key = oe_keymap.get();

                                let changed_field = match fields.entry(*key) {
                                    Entry::Occupied(mut field_oe) => {
                                        if (*field_oe.get()) == (*field_value) {
                                            false
                                        }
                                        else {
                                            field_value.clone_into(field_oe.get_mut());
                                            true
                                        }
                                    }
                                    Entry::Vacant(field_ve) => {
                                        field_ve.insert(field_value.clone());
                                        true
                                    }
                                };

                                if changed_field {
                                    if !wrote_entity_header {
                                        wrote_entity_header = true;
                                        let _ = writeln!(self.output_file, "\tE{}\t{}\t{}", entity.index(), delta_header.value(), entity.serializer().serializer_name.str.to_string());
                                    }

                                    /*
                                    let fp: &FieldPath = unsafe { entity
                                        .get_path(key)
                                        // NOTE: this should never throw because if entity
                                        // was returned it means that it exists thus path
                                        // exists
                                        .unwrap_unchecked() };
                                    let (named_path, var_type) = get_value_info(entity.serializer(), fp);
                                    */

                                    let _ = match field_value {
                                        FieldValue::I64(value) => writeln!(self.output_file, "I{}\t{}", mapped_key, value),
                                        FieldValue::U64(value) => writeln!(self.output_file, "U{}\t{}", mapped_key, value),
                                        FieldValue::F32(value) => writeln!(self.output_file, "F{}\t{}", mapped_key, value),
                                        FieldValue::Bool(value) => writeln!(self.output_file, "B{}\t{}", mapped_key, value),
                                        FieldValue::Vector3(value) => writeln!(self.output_file, "2{}\t{:?}", mapped_key, value),
                                        FieldValue::Vector2(value) => writeln!(self.output_file, "3{}\t{:?}", mapped_key, value),
                                        FieldValue::Vector4(value) => writeln!(self.output_file, "4{}\t{:?}", mapped_key, value),
                                        FieldValue::QAngle(value) => writeln!(self.output_file, "Q{}\t{:?}", mapped_key, value),
                                        FieldValue::String(value) => writeln!(self.output_file, "S{}\t{}", mapped_key, value),
                                    };
                                }
                            }
                            Entry::Vacant(ve_keymap) => {
                            }
                        }
                    });
            }
            Entry::Vacant(ve) => {
                let _ = writeln!(self.output_file, "\tE{}\t{}\t{}", entity.index(), delta_header.value(), entity.serializer().serializer_name.str.to_string());

                let fields = ve.insert(HashMap::new());

                entity
                    .iter()
                    .for_each(|(key, field_value)| {
                        match self.permitted_fields.entry(*key) {
                            Entry::Occupied(oe_keymap) => {
                                let mapped_key = oe_keymap.get();

                                fields.insert(*key, field_value.clone());

                                /*
                                let fp: &FieldPath = unsafe { entity
                                    .get_path(key)
                                    // NOTE: this should never throw because if entity
                                    // was returned it means that it exists thus path
                                    // exists
                                    .unwrap_unchecked() };
                                let (named_path, var_type) = get_value_info(entity.serializer(), fp);
                                */

                                let _ = match field_value {
                                    FieldValue::I64(value) => writeln!(self.output_file, "I{}\t{}", mapped_key, value),
                                    FieldValue::U64(value) => writeln!(self.output_file, "U{}\t{}", mapped_key, value),
                                    FieldValue::F32(value) => writeln!(self.output_file, "F{}\t{}", mapped_key, value),
                                    FieldValue::Bool(value) => writeln!(self.output_file, "B{}\t{}", mapped_key, value),
                                    FieldValue::Vector3(value) => writeln!(self.output_file, "2{}\t{:?}", mapped_key, value),
                                    FieldValue::Vector2(value) => writeln!(self.output_file, "3{}\t{:?}", mapped_key, value),
                                    FieldValue::Vector4(value) => writeln!(self.output_file, "4{}\t{:?}", mapped_key, value),
                                    FieldValue::QAngle(value) => writeln!(self.output_file, "Q{}\t{:?}", mapped_key, value),
                                    FieldValue::String(value) => writeln!(self.output_file, "S{}\t{}", mapped_key, value),
                                };
                            }
                            Entry::Vacant(ve_keymap) => {
                            }
                        }
                    });
            }
        };

        /*
        let position = get_entity_position(entity).expect("player pawn position");

        // TODO: get rid of hashmap, parser must supply a list of updated fields.
        match self.positions.entry(entity.index()) {
            Entry::Occupied(mut oe) => {
                let prev_position = oe.insert(position);
                if prev_position != position {
                    writeln!(
                        self.output_file,
                        "{} moved from {:?} to {:?}",
                        entity.index(),
                        prev_position,
                        position
                    );
                }
            }
            Entry::Vacant(ve) => {
                ve.insert(position);
            }
        };
        */

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
        if
            entity.serializer_name_heq(ENTITY_PLAYERPAWN) 
            || entity.serializer_name_heq(ENTITY_TROOPER) 
            || entity.serializer_name_heq(ENTITY_PLAYERCONTROLLER)
            || entity.serializer_name_heq(ENTITY_TROOPERNEUTRAL)
            || entity.serializer_name_heq(ENTITY_TROOPER) 
            || entity.serializer_name_heq(ENTITY_MIDBOSS)
            || entity.serializer_name_heq(ENTITY_TROOPERBOSS)
            || entity.serializer_name_heq(ENTITY_BASEDEFENSESENTRY) 
            || entity.serializer_name_heq(ENTITY_BOSS_TIER2)
            || entity.serializer_name_heq(ENTITY_BOSS_TIER3)
        {
            self.handle_player_pawn(delta_header, entity)?;
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
