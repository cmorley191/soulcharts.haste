use crate::{
    bitbuf::{self, BitReader},
    nohash::NoHashHasherBuilder,
};
use hashbrown::HashMap;
use haste_dota2_protos::c_demo_string_tables;
use std::{
    intrinsics::{likely, unlikely},
    mem::MaybeUninit,
};

// NOTE: some info about string tables is available at
// https://developer.valvesoftware.com/wiki/Networking_Events_%26_Messages#String_Tables

#[derive(thiserror::Error, Debug)]
pub enum Error {
    // 3rd party crates
    #[error(transparent)]
    Snap(#[from] snap::Error),
    // crate
    #[error(transparent)]
    BitBuf(#[from] bitbuf::Error),
    // mod
    #[error("tried to create string table '{0}' twice")]
    DuplicateStringTable(String),
}

pub type Result<T> = std::result::Result<T, Error>;

const HISTORY_SIZE: usize = 32;
const HISTORY_BITMASK: usize = HISTORY_SIZE - 1;

const MAX_STRING_BITS: usize = 5;
const MAX_STRING_SIZE: usize = 1 << MAX_STRING_BITS;

#[derive(Debug)]
struct StringHistoryEntry {
    string: [u8; MAX_STRING_SIZE],
}

impl StringHistoryEntry {
    unsafe fn new_uninit() -> Self {
        Self {
            // NOTE: the thick is not use this correctly xd
            #[allow(invalid_value)]
            string: MaybeUninit::uninit().assume_init(),
        }
    }
}

const MAX_USERDATA_BITS: usize = 17;
const MAX_USERDATA_SIZE: usize = 1 << MAX_USERDATA_BITS;

#[derive(Debug)]
pub struct StringTableItem {
    pub string: Option<Vec<u8>>,
    pub user_data: Option<Vec<u8>>,
}

#[derive(Debug)]
pub struct StringTable {
    pub name: Box<str>,
    user_data_fixed_size: bool,
    user_data_size: i32,
    user_data_size_bits: i32,
    flags: i32,
    using_varint_bitcounts: bool,
    pub(crate) items: HashMap<i32, StringTableItem, NoHashHasherBuilder<i32>>,

    history: Vec<StringHistoryEntry>,
    string_buf: Vec<u8>,
    user_data_buf: Vec<u8>,
    user_data_uncompressed_buf: Vec<u8>,
}

impl StringTable {
    pub fn new(
        name: &str,
        user_data_fixed_size: bool,
        user_data_size: i32,
        user_data_size_bits: i32,
        flags: i32,
        using_varint_bitcounts: bool,
    ) -> Self {
        #[inline(always)]
        unsafe fn make_vec<T>(size: usize) -> Vec<T> {
            let mut vec = Vec::with_capacity(size);
            vec.set_len(size);
            vec
        }

        Self {
            name: name.into(),
            user_data_fixed_size,
            user_data_size,
            user_data_size_bits,
            flags,
            using_varint_bitcounts,
            items: HashMap::with_capacity_and_hasher(1024, NoHashHasherBuilder::default()),

            history: unsafe { make_vec(HISTORY_SIZE) },
            string_buf: unsafe { make_vec(1024) },
            user_data_buf: unsafe { make_vec(MAX_USERDATA_SIZE) },
            user_data_uncompressed_buf: unsafe { make_vec(MAX_USERDATA_SIZE) },
        }
    }

    // void ParseUpdate( bf_read &buf, int entries );
    //
    // some pieces are ported from csgo, some are stolen from butterfly, some
    // comments are stolen from manta.
    pub fn parse_update(&mut self, br: &mut BitReader, num_entries: i32) -> Result<()> {
        let mut entry_index: i32 = -1;

        // TODO: feature flag or something for a static allocation of history,
        // string_buf and user_data_buf in single threaded environment (similar
        // to what butterfly does).
        // NOTE: making thing static wins us 10ms

        // > cost of zero-initializing a buffer of 1024 bytes on the stack can
        // be disproportionately high
        // https://www.reddit.com/r/rust/comments/9ozddb/comment/e7z2qi1/?utm_source=share&utm_medium=web2x&context=3

        let history = &mut self.history;
        let mut history_delta_index: usize = 0;

        let string_buf = &mut self.string_buf;
        let user_data_buf = &mut self.user_data_buf;
        let user_data_uncompressed_buf = &mut self.user_data_uncompressed_buf;

        // Loop through entries in the data structure
        //
        // Each entry is a tuple consisting of {index, key, value}
        //
        // Index can either be incremented from the previous position or
        // overwritten with a given entry.
        //
        // Key may be omitted (will be represented here as "")
        //
        // Value may be omitted
        for _ in 0..num_entries as usize {
            // Read a boolean to determine whether the operation is an increment
            // or has a fixed index position. A fixed index position of zero
            // should be the last data in the buffer, and indicates that all
            // data has been read.
            entry_index = if br.read_bool()? {
                entry_index + 1
            } else {
                br.read_uvarint32()? as i32 + 1
            };

            let has_string = br.read_bool()?;
            let string = if has_string {
                let mut size: usize = 0;

                // Some entries use reference a position in the key history for
                // part of the key. If referencing the history, read the
                // position and size from the buffer, then use those to build
                // the string combined with an extra string read (null
                // terminated). Alternatively, just read the string.
                if br.read_bool()? {
                    // NOTE: valve uses their CUtlVector which shifts elements
                    // to the left on delete. they maintain max len of 32. they
                    // don't allow history to grow beyond 32 elements, once it
                    // reaches len of 32 they remove item at index 0. i'm
                    // stealing following approach from butterfly, even thought
                    // rust's Vec has remove method which does exactly same
                    // thing, butterfly's way is more efficient, thanks!
                    let mut history_delta_zero = 0;
                    if history_delta_index > HISTORY_SIZE {
                        history_delta_zero = history_delta_index & HISTORY_BITMASK;
                    };

                    let index =
                        (history_delta_zero + br.read_ubitlong(5)? as usize) & HISTORY_BITMASK;
                    let bytestocopy = br.read_ubitlong(MAX_STRING_BITS)? as usize;
                    size += bytestocopy;

                    string_buf[..bytestocopy]
                        .copy_from_slice(&history[index].string[..bytestocopy]);
                    size += br.read_string(&mut string_buf[bytestocopy..], false)?;
                } else {
                    size += br.read_string(string_buf, false)?;
                }

                let mut she = unsafe { StringHistoryEntry::new_uninit() };
                she.string.copy_from_slice(&string_buf[..MAX_STRING_SIZE]);

                history[history_delta_index & HISTORY_BITMASK] = she;
                history_delta_index += 1;

                Some(&string_buf[..size])
            } else {
                None
            };

            let has_user_data = br.read_bool()?;
            let user_data = if has_user_data {
                if self.user_data_fixed_size {
                    // Don't need to read length, it's fixed length and the length was networked down already.
                    br.read_bits(user_data_buf, self.user_data_size_bits as usize)?;
                    Some(&user_data_buf[..self.user_data_size as usize])
                } else {
                    let mut is_compressed = false;
                    if (self.flags & 0x1) != 0 {
                        is_compressed = br.read_bool()?;
                    }

                    // NOTE: using_varint_bitcounts bool was introduced in the
                    // new frontiers update on smaypril twemmieth of 2023,
                    // https://github.com/SteamDatabase/GameTracking-Dota2/commit/8851e24f0e3ef0b618e3a60d276a3b0baf88568c#diff-79c9dd229c77c85f462d6d85e29a65f5daf6bf31f199554438d42bd643e89448R405
                    let size = if likely(self.using_varint_bitcounts) {
                        br.read_ubitvar()
                    } else {
                        br.read_ubitlong(MAX_USERDATA_BITS)
                    }? as usize;

                    br.read_bytes(&mut user_data_buf[..size])?;

                    if is_compressed {
                        snap::raw::Decoder::new()
                            .decompress(&user_data_buf[..size], user_data_uncompressed_buf)?;
                        let size = snap::raw::decompress_len(user_data_buf)?;
                        Some(&user_data_uncompressed_buf[..size])
                    } else {
                        Some(&user_data_buf[..size])
                    }
                }
            } else {
                None
            };

            // TODO: try to use .entry + .or_inser_with
            if let Some(entry) = self.items.get_mut(&entry_index) {
                if let Some(dst) = entry.user_data.as_mut() {
                    if let Some(src) = user_data {
                        dst.resize(src.len(), 0);
                        dst.clone_from_slice(src);
                    }
                } else {
                    entry.user_data = user_data.map(|v| v.to_vec());
                }
            } else {
                let sti = StringTableItem {
                    string: string.map(|src| {
                        let mut dst = Vec::with_capacity(src.len());
                        unsafe { dst.set_len(src.len()) };
                        dst.clone_from_slice(src);
                        dst
                    }),
                    user_data: user_data.map(|v| v.to_vec()),
                };
                self.items.insert(entry_index, sti);
            }
        }

        Ok(())
    }

    pub fn do_full_update(&mut self, table: &c_demo_string_tables::TableT) {
        debug_assert!(
            self.name.as_ref().eq(table.table_name()),
            "trying to do a full update on the wrong table"
        );
        // copying clarity's behaviour
        // src/main/java/skadistats/clarity/processor/stringtables/BaseStringTableEmitter.java
        debug_assert!(
            table.items.len() >= self.items.len(),
            "removing entries is not supported"
        );

        for (i, incoming) in table.items.iter().enumerate() {
            self.items
                .entry(i as i32)
                .and_modify(|existing| existing.user_data = incoming.data.clone())
                .or_insert_with(|| StringTableItem {
                    string: incoming.str.as_ref().map(|v| v.as_bytes().to_vec()),
                    user_data: incoming.data.clone(),
                });
        }
    }

    // NOTE: might need those for fast seeks
    // // HLTV change history & rollback
    // void EnableRollback();
    // void RestoreTick(int tick);
}

// NOTE: this is modelled after CNetworkStringTableContainer
#[derive(Default)]
pub struct StringTables {
    tables: Vec<StringTable>,
}

impl StringTables {
    // INetworkStringTable *CreateStringTable( const char *tableName, int maxentries, int userdatafixedsize = 0, int userdatanetworkbits = 0, int flags = NSF_NONE );
    pub fn create_string_table_mut(
        &mut self,
        name: &str,
        user_data_fixed_size: bool,
        user_data_size: i32,
        user_data_size_bits: i32,
        flags: i32,
        using_varint_bitcounts: bool,
    ) -> Result<&mut StringTable> {
        let table = self.find_table(name);
        // TODO: should this check exist?
        if unlikely(table.is_some()) {
            return Err(Error::DuplicateStringTable(name.to_string()));
        }

        let table = StringTable::new(
            name,
            user_data_fixed_size,
            user_data_size,
            user_data_size_bits,
            flags,
            using_varint_bitcounts,
        );

        // TODO: push
        let len = self.tables.len();
        self.tables.insert(len, table);
        Ok(&mut self.tables[len])
    }

    pub fn do_full_update(&mut self, cmd: haste_dota2_protos::CDemoStringTables) {
        for incoming in &cmd.tables {
            if let Some(existing) = self.find_table_mut(incoming.table_name()) {
                existing.do_full_update(incoming);
            }
        }
    }

    // INetworkStringTable *FindTable( const char *tableName ) const ;
    pub fn find_table(&self, name: &str) -> Option<&StringTable> {
        self.tables
            .iter()
            // TODO: this used to be |&table| .., does this affect performance?
            .find(|table| table.name.as_ref().eq(name))
    }

    pub fn find_table_mut(&mut self, name: &str) -> Option<&mut StringTable> {
        self.tables
            .iter_mut()
            .find(|table| table.name.as_ref().eq(name))
    }

    // INetworkStringTable	*GetTable( TABLEID stringTable ) const;
    pub fn get_table(&self, id: usize) -> Option<&StringTable> {
        self.tables.get(id)
    }

    pub fn get_table_mut(&mut self, id: usize) -> Option<&mut StringTable> {
        self.tables.get_mut(id)
    }

    // clear clears underlying storage, but this has no effect on the allocated
    // capacity.
    //
    // NOTE: it seems like valve's name `RemoveAllTables` has relation to or
    // based on name of CUtlVector's `Remove` method, which does not de-allocate
    // memory; - rust's variant would be `clear`.
    //
    // void             RemoveAllTables( void );
    pub fn clear(&mut self) {
        self.tables.clear();
    }

    // NOTE: the goal is to check if there are any string tables,
    // CNetworkStringTableContainer has `GetNumTables` method which would be
    // equivalent to `len` in idiomatic rust, but doing .len() == 0 is not
    // idiomatic xd - `is_empty` is.
    //
    // int                  GetNumTables( void ) const;
    pub fn is_empty(&self) -> bool {
        self.tables.is_empty()
    }

    // TODO: add support for change list (m_pChangeList) and rollbacks
    // NOTE: might need those for fast seeks
    // void EnableRollback( bool bState );
    // void RestoreTick( int tick );
}
