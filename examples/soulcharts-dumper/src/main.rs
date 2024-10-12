use anyhow::Error;
use haste::{
    entities::{fkey_from_path, DeltaHeader, Entity},
    fxhash,
    parser::{self, Context, Parser, Visitor},
    protos::{
        self, 
        prost::Message, // provides <msgtype>::decode(...) function
    },
};
use std::{
    fs::{File, OpenOptions}, 
    io::BufReader
};

const ENTITYH_GAME_RULES: u64 = fxhash::hash_bytes(b"CCitadelGameRulesProxy");

#[derive(/*Default, */Debug)]
struct MyVisitor {
    output_file: File,

    frame_index: i32,
    message_index: i32,
    previous_frame_ticks: Vec<u32>,

    fm_index_server_info: Option<(i32, i32)>,
    tick_interval: Option<f32>,

    frame_index_sync_tick: Option<i32>,

    message_index_first_user_damage: Option<i32>,
    message_index_first_user_bullet_hit: Option<i32>,

    message_index_tick: Option<i32>,
    tick: u32,

    message_index_entities: Option<i32>,
}

impl MyVisitor {
    fn new(output_file: File) -> MyVisitor {
        MyVisitor {
            output_file,

            frame_index: -1,
            message_index: -1,
            previous_frame_ticks: Vec::new(),

            fm_index_server_info: None,
            tick_interval: None,

            frame_index_sync_tick: None,

            message_index_first_user_damage: None,
            message_index_first_user_bullet_hit: None,

            message_index_tick: None,
            tick: 0,

            message_index_entities: None,
        }
    }

    fn handle_frame(&mut self, ctx: &Context, header: &haste::demofile::CmdHeader, _data: &[u8]) -> anyhow::Result<()> {
        // new frame, so first finish out the previous frame: (if there was one)
        if self.frame_index >= 0 {
            self.handle_frame_end()?;
        }

        self.frame_index += 1;
        self.message_index = -1;
        self.message_index_first_user_damage = None;
        self.message_index_first_user_bullet_hit = None;
        self.message_index_tick = None;
        self.message_index_entities = None;

        if header.command == protos::EDemoCommands::DemSyncTick {
            if self.frame_index_sync_tick.is_some() {
                eprintln!("Duplicate DEM_SyncTick (frames {}, {})", self.frame_index_sync_tick.unwrap(), self.frame_index);
                return Err(Error::msg(""));
            }

            self.frame_index_sync_tick = Some(self.frame_index);

            if self.fm_index_server_info.is_none() {
                eprintln!("DEM_SyncTick (frame {}) before svc_ServerInfo", self.frame_index);
                return Err(Error::msg(""));
            }

            // this would've been parsed during the svc_ServerInfo parse
            self.tick_interval = Some(ctx.tick_interval());
        }

        Ok(())
    }

    fn handle_frame_end(&mut self) -> anyhow::Result<()> {
        if self.message_index_tick.is_none() || self.message_index_entities.is_none() {
            if self.message_index_first_user_damage.is_some() {
                eprintln!("(retroactive) k_EUserMsg_Damage (message {}-{}-{}) without one of net_Tick or svc_PacketEntities later in frame", self.frame_index, self.tick, self.message_index_first_user_damage.unwrap());
                return Err(Error::msg(""));
            }
            if self.message_index_first_user_bullet_hit.is_some() {
                eprintln!("(retroactive) k_EUserMsg_BulletHit (message {}-{}-{}) without one of net_Tick or svc_PacketEntities later in frame", self.frame_index, self.tick, self.message_index_first_user_bullet_hit.unwrap());
                return Err(Error::msg(""));
            }
        }

        self.previous_frame_ticks.push(self.tick);

        Ok(())
    }

    fn handle_message(&mut self, _ctx: &Context, message_type: u32, data: &[u8]) -> anyhow::Result<()> {
        self.message_index += 1;

        match message_type {
            t if t == protos::SvcMessages::SvcServerInfo as u32 => {
                if self.fm_index_server_info.is_some() {
                    let (frame_index_server_info, message_index_server_info) = self.fm_index_server_info.unwrap();
                    eprintln!("Duplicate svc_ServerInfo (messages {}-{}-{}, {}-{}-{})", 
                        frame_index_server_info, 
                        self.previous_frame_ticks[frame_index_server_info as usize],
                        message_index_server_info, 
                        self.frame_index,
                        self.tick,
                        self.message_index
                    );
                    return Err(Error::msg(""));
                }
    
                self.fm_index_server_info = Some((self.frame_index, self.message_index));
            }

            t if t == protos::CitadelUserMessageIds::KEUserMsgDamage as u32 => {
                if self.message_index_tick.is_some() {
                    eprintln!("k_EUserMsg_Damage (message {}-{}-{}) after net_Tick (message {})", 
                        self.frame_index, 
                        self.tick,
                        self.message_index, 
                        self.message_index_tick.unwrap()
                    );
                    return Err(Error::msg(""));
                }

                if self.message_index_first_user_damage.is_none() {
                    self.message_index_first_user_damage = Some(self.message_index);
                }
            }

            t if t == protos::CitadelUserMessageIds::KEUserMsgBulletHit as u32 => {
                if self.message_index_tick.is_some() {
                    eprintln!("k_EUserMsg_BulletHit (message {}-{}-{}) after net_Tick (message {})", 
                        self.frame_index, 
                        self.tick,
                        self.message_index, 
                        self.message_index_tick.unwrap()
                    );
                    return Err(Error::msg(""));
                }

                if self.message_index_first_user_bullet_hit.is_none() {
                    self.message_index_first_user_bullet_hit = Some(self.message_index);
                }
            }

            t if t == protos::NetMessages::NetTick as u32 => {
                if self.message_index_tick.is_some() {
                    eprintln!("Duplicate net_Tick (messages {}-{}-{}, {})", 
                        self.frame_index, 
                        self.tick,
                        self.message_index_tick.unwrap(), 
                        self.message_index
                    );
                    return Err(Error::msg(""));
                }
    
                self.message_index_tick = Some(self.message_index);

                let msg = protos::CnetMsgTick::decode(data)?;
                if msg.tick.is_none() {
                    eprintln!("net_Tick empty (message {}-{}-{})", self.frame_index, self.tick, self.message_index);
                    return Err(Error::msg(""));
                }

                self.tick = msg.tick.unwrap();
            }

            t if t == protos::SvcMessages::SvcPacketEntities as u32 => {
                if self.message_index_tick.is_none() {
                    eprintln!("svc_PacketEntities (message {}-{}-{}) before net_Tick", self.frame_index, self.tick, self.message_index);
                    return Err(Error::msg(""));
                }

                if self.message_index_entities.is_some() {
                    eprintln!("Duplicate svc_PacketEntities (messages {}-{}-{}, {})", 
                        self.frame_index, 
                        self.tick,
                        self.message_index_entities.unwrap(), 
                        self.message_index
                    );
                    return Err(Error::msg(""));
                }
    
                self.message_index_entities = Some(self.message_index);
            }

            t if t == protos::ECitadelGameEvents::GeFireBullets as u32 => {
                if self.message_index_entities.is_none() {
                    eprintln!("GE_FireBullets (message {}-{}-{}) before svc_PacketEntities", 
                        self.frame_index, 
                        self.tick,
                        self.message_index
                    );
                    return Err(Error::msg(""));
                }
            }

            t if t == protos::ECitadelGameEvents::GeBulletImpact as u32 => {
                if self.message_index_entities.is_none() {
                    eprintln!("GE_BulletImpact (message {}-{}-{}) before svc_PacketEntities", 
                        self.frame_index,
                        self.tick,
                        self.message_index
                    );
                    return Err(Error::msg(""));
                }
            }

            _ => {

            }
        }

        Ok(())
    }

    fn handle_entity(&mut self, _ctx: &Context, _delta_header: DeltaHeader, entity: &Entity) -> anyhow::Result<()> {
        if entity.serializer_name_heq(ENTITYH_GAME_RULES) {
            eprintln!("Game rules (message {}-{}-{})", self.frame_index, self.tick, self.message_index);

            let game_start_time: f32 =
                entity.try_get_value(&fkey_from_path(&["m_pGameRules", "m_flGameStartTime"]))?;
            
            // NOTE: 0.001 is an arbitrary number; nothing special.
            if game_start_time < 0.001 {
                eprintln!("game_start_time={}", game_start_time);
                return Ok(());
            }

            let game_paused: bool =
                entity.try_get_value(&fkey_from_path(&["m_pGameRules", "m_bGamePaused"]))?;
            let pause_start_tick: i32 =
                entity.try_get_value(&fkey_from_path(&["m_pGameRules", "m_nPauseStartTick"]))?;
            let total_paused_ticks: i32 =
                entity.try_get_value(&fkey_from_path(&["m_pGameRules", "m_nTotalPausedTicks"]))?;

            eprintln!("game_start_time={}, game_paused={}, pause_start_tick={}, total_paused_ticks={}", game_start_time, game_paused, pause_start_tick, total_paused_ticks);
        }

        Ok(())
    }

}

impl Visitor for &mut MyVisitor {

    // called by the parser for every frame, *just before* any processing on the frame's data is performed
    fn on_cmd(
        &mut self, 
        ctx: &Context, 
        cmd_header: &haste::demofile::CmdHeader, 
        data: &[u8]
    ) -> parser::Result<()> {
        self.handle_frame(ctx, cmd_header, data)?;
        Ok(())
    }

    // called by the parser for every message in a packet frame, *just before* any processing on the message's data is performed
    fn on_packet(
        &mut self, 
        ctx: &Context, 
        packet_type: u32,
        data: &[u8]
    ) -> parser::Result<()> {
        self.handle_message(ctx, packet_type, data)?;
        Ok(())
    }

    fn on_entity(
        &mut self,
        ctx: &Context,
        delta_header: DeltaHeader,
        entity: &Entity,
    ) -> parser::Result<()> {
        self.handle_entity(ctx, delta_header, entity)?;
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
