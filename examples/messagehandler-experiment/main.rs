use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;

use anyhow::{Context as _, Result};
use handler::HandlerVisitor;
use haste::demofile::DemoFile;
use haste::entities::{self, Entity};
use haste::parser::{Context, Parser};
use haste::stringtables::StringTable;
use haste::valveprotos::deadlock::{CCitadelUserMsgHeroKilled, CitadelUserMessageIds};

mod handler;

fn get_entity_name<'a>(entity: &'a Entity, entity_names: &'a StringTable) -> Option<&'a str> {
    const NAME_STRINGTABLE_INDEX_KEY: u64 =
        entities::fkey_from_path(&["m_pEntity", "m_nameStringableIndex"]);
    let name_stringtable_index: i32 = entity.get_value(&NAME_STRINGTABLE_INDEX_KEY)?;
    let name_stringtable_item = entity_names.get_item(&name_stringtable_index)?;
    let raw_string = name_stringtable_item.string.as_ref()?;
    std::str::from_utf8(raw_string).ok()
}

#[derive(Default)]
struct Score {
    kills: usize,
    deaths: usize,
}

#[derive(Default)]
struct State {
    hero_scores: HashMap<String, Score>,
}

fn hero_killed(state: &mut State, ctx: &Context, msg: &CCitadelUserMsgHeroKilled) -> Result<()> {
    let entities = ctx.entities().unwrap();

    let string_tables = ctx.string_tables().unwrap();
    let entity_names = string_tables.find_table("EntityNames").unwrap();

    let scorer_name = entities
        .get(&msg.entindex_scorer())
        .and_then(|entindex| get_entity_name(entindex, entity_names))
        .unwrap_or("<some-other-unit>");

    let victim_name = entities
        .get(&msg.entindex_victim())
        .and_then(|entindex| get_entity_name(entindex, entity_names))
        .unwrap();

    println!("{} killed {}", scorer_name, victim_name);

    state
        .hero_scores
        .entry(scorer_name.to_string())
        .or_default()
        .kills += 1;
    state
        .hero_scores
        .entry(victim_name.to_string())
        .or_default()
        .deaths += 1;

    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let filepath = args
        .get(1)
        .context("usage: experiemnt-messagehandler <filepath>")?;
    let file = File::open(filepath)?;
    let buf_reader = BufReader::new(file);
    let demo_file = DemoFile::start_reading(buf_reader)?;

    let state = State::default();
    let mut visitor = HandlerVisitor::with_state(state).with(
        CitadelUserMessageIds::KEUserMsgHeroKilled as u32,
        hero_killed,
    );

    let mut parser = Parser::from_stream_with_visitor(demo_file, &mut visitor)?;
    parser.run_to_end()?;

    println!();

    for (hero, score) in visitor.state().hero_scores.iter() {
        println!(
            "{} got {} kills and died {} times",
            hero, score.kills, score.deaths
        );
    }

    Ok(())
}
