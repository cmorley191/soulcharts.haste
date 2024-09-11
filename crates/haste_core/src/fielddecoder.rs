use crate::{
    bitreader::BitReader,
    fieldvalue::FieldValue,
    flattenedserializers::FlattenedSerializerField,
    fxhash,
    quantizedfloat::{self, QuantizedFloat},
};
use dyn_clone::DynClone;
use std::{fmt::Debug, mem::MaybeUninit};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    // crate
    #[error(transparent)]
    QuantizedFloat(#[from] quantizedfloat::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

// NOTE: PropTypeFns (from csgo source code) is what you are looking for, it has all the encoders,
// decoders, proxies and all of the stuff.

#[derive(Debug)]
pub struct FieldDecodeContext {
    pub tick_interval: f32,
}

// TODO: get rid of trait objects; find a better, more efficient, way to
// "attach" decoders to fields; but note that having separate decoding functions
// and attaching function "pointers" to fields is even worse.

pub trait FieldDecode: DynClone + Debug {
    fn decode(&self, ctx: &FieldDecodeContext, br: &mut BitReader) -> Result<FieldValue>;
}

dyn_clone::clone_trait_object!(FieldDecode);

/// used during multi-phase initialization. never called.
#[derive(Debug, Clone, Default)]
pub struct InvalidDecoder;

impl FieldDecode for InvalidDecoder {
    #[cold]
    fn decode(&self, _ctx: &FieldDecodeContext, _br: &mut BitReader) -> Result<FieldValue> {
        unreachable!()
    }
}

// ----

#[derive(Debug, Clone, Default)]
pub struct I32Decoder;

impl FieldDecode for I32Decoder {
    fn decode(&self, _ctx: &FieldDecodeContext, br: &mut BitReader) -> Result<FieldValue> {
        Ok(FieldValue::I32(br.read_varint32()))
    }
}

// ----

#[derive(Debug, Clone, Default)]
pub struct I64Decoder;

impl FieldDecode for I64Decoder {
    fn decode(&self, _ctx: &FieldDecodeContext, br: &mut BitReader) -> Result<FieldValue> {
        Ok(FieldValue::I64(br.read_varint64()))
    }
}

// ----

#[derive(Debug, Clone, Default)]
pub struct U32Decoder;

impl FieldDecode for U32Decoder {
    fn decode(&self, _ctx: &FieldDecodeContext, br: &mut BitReader) -> Result<FieldValue> {
        Ok(FieldValue::U32(br.read_uvarint32()))
    }
}

// ----

#[derive(Debug, Clone, Default)]
struct InternalU64Decoder;

impl FieldDecode for InternalU64Decoder {
    fn decode(&self, _ctx: &FieldDecodeContext, br: &mut BitReader) -> Result<FieldValue> {
        Ok(FieldValue::U64(br.read_uvarint64()))
    }
}

#[derive(Debug, Clone, Default)]
struct InternalU64Fixed64Decoder;

impl FieldDecode for InternalU64Fixed64Decoder {
    fn decode(&self, _ctx: &FieldDecodeContext, br: &mut BitReader) -> Result<FieldValue> {
        let mut buf = [0u8; 8];
        br.read_bytes(&mut buf);
        Ok(FieldValue::U64(u64::from_le_bytes(buf)))
    }
}

#[derive(Debug, Clone)]
pub struct U64Decoder {
    decoder: Box<dyn FieldDecode>,
}

impl U64Decoder {
    #[inline]
    pub fn new(field: &FlattenedSerializerField) -> Self {
        if field.var_encoder_heq(fxhash::hash_bytes(b"fixed64")) {
            Self {
                decoder: Box::<InternalU64Fixed64Decoder>::default(),
            }
        } else {
            Self {
                decoder: Box::<InternalU64Decoder>::default(),
            }
        }
    }
}

impl FieldDecode for U64Decoder {
    fn decode(&self, ctx: &FieldDecodeContext, br: &mut BitReader) -> Result<FieldValue> {
        self.decoder.decode(ctx, br)
    }
}

// ----

#[derive(Debug, Clone, Default)]
pub struct BoolDecoder;

impl FieldDecode for BoolDecoder {
    fn decode(&self, _ctx: &FieldDecodeContext, br: &mut BitReader) -> Result<FieldValue> {
        Ok(FieldValue::Bool(br.read_bool()))
    }
}

// ----

#[derive(Debug, Clone, Default)]
pub struct StringDecoder;

impl FieldDecode for StringDecoder {
    fn decode(&self, _ctx: &FieldDecodeContext, br: &mut BitReader) -> Result<FieldValue> {
        // NOTE(blukai): the stuff is safe cause we can make sure here that no uninit memory is being read.
        #[allow(invalid_value)]
        let mut buf: [u8; 1024] = unsafe { MaybeUninit::uninit().assume_init() };
        let n = br.read_string(&mut buf, false);
        // TODO(blukai): should string conversion be actually checked? why not?
        Ok(FieldValue::String(Box::<str>::from(unsafe {
            std::str::from_utf8_unchecked(&buf[..n])
        })))
    }
}

// ----

trait InternalF32Decode: DynClone + Debug {
    fn decode(&self, ctx: &FieldDecodeContext, br: &mut BitReader) -> Result<f32>;
}

dyn_clone::clone_trait_object!(InternalF32Decode);

// ----

#[derive(Debug, Clone, Default)]
struct InternalF32SimulationTimeDecoder;

impl InternalF32Decode for InternalF32SimulationTimeDecoder {
    fn decode(&self, ctx: &FieldDecodeContext, br: &mut BitReader) -> Result<f32> {
        Ok(br.read_uvarint32() as f32 * ctx.tick_interval)
    }
}

#[derive(Debug, Clone, Default)]
struct InternalF32CoordDecoder;

impl InternalF32Decode for InternalF32CoordDecoder {
    fn decode(&self, _ctx: &FieldDecodeContext, br: &mut BitReader) -> Result<f32> {
        Ok(br.read_bitcoord())
    }
}

#[derive(Debug, Clone, Default)]
struct InternalF32NormalDecoder;

impl InternalF32Decode for InternalF32NormalDecoder {
    fn decode(&self, _ctx: &FieldDecodeContext, br: &mut BitReader) -> Result<f32> {
        Ok(br.read_bitnormal())
    }
}

#[derive(Debug, Clone, Default)]
struct InternalF32NoScaleDecoder;

impl InternalF32Decode for InternalF32NoScaleDecoder {
    fn decode(&self, _ctx: &FieldDecodeContext, br: &mut BitReader) -> Result<f32> {
        Ok(br.read_bitfloat())
    }
}

#[derive(Debug, Clone)]
struct InternalQuantizedFloatDecoder {
    quantized_float: QuantizedFloat,
}

impl InternalQuantizedFloatDecoder {
    #[inline]
    pub fn new(field: &FlattenedSerializerField) -> Result<Self> {
        Ok(Self {
            quantized_float: QuantizedFloat::new(
                field.bit_count.unwrap_or_default(),
                field.encode_flags.unwrap_or_default(),
                field.low_value.unwrap_or_default(),
                field.high_value.unwrap_or_default(),
            )?,
        })
    }
}

impl InternalF32Decode for InternalQuantizedFloatDecoder {
    fn decode(&self, _ctx: &FieldDecodeContext, br: &mut BitReader) -> Result<f32> {
        Ok(self.quantized_float.decode(br))
    }
}

// ----

#[derive(Debug, Clone)]
pub struct InternalF32Decoder {
    decoder: Box<dyn InternalF32Decode>,
}

impl InternalF32Decoder {
    pub fn new(field: &FlattenedSerializerField) -> Result<Self> {
        if field.var_name.hash == fxhash::hash_bytes(b"m_flSimulationTime")
            || field.var_name.hash == fxhash::hash_bytes(b"m_flAnimTime")
        {
            return Ok(Self {
                decoder: Box::<InternalF32SimulationTimeDecoder>::default(),
            });
        }

        if let Some(var_encoder) = field.var_encoder.as_ref() {
            match var_encoder.hash {
                hash if hash == fxhash::hash_bytes(b"coord") => {
                    return Ok(Self {
                        decoder: Box::<InternalF32CoordDecoder>::default(),
                    })
                }
                hash if hash == fxhash::hash_bytes(b"normal") => {
                    return Ok(Self {
                        decoder: Box::<InternalF32NormalDecoder>::default(),
                    })
                }
                _ => unimplemented!("{:?}", var_encoder),
            }
        }

        let bit_count = field.bit_count.unwrap_or_default();

        // NOTE: that would mean that something is seriously wrong - in that case yell at me
        // loudly.
        debug_assert!(bit_count >= 0 && bit_count <= 32);

        if bit_count == 0 || bit_count == 32 {
            return Ok(Self {
                decoder: Box::<InternalF32NoScaleDecoder>::default(),
            });
        }

        Ok(Self {
            decoder: Box::new(InternalQuantizedFloatDecoder::new(field)?),
        })
    }
}

impl InternalF32Decode for InternalF32Decoder {
    fn decode(&self, ctx: &FieldDecodeContext, br: &mut BitReader) -> Result<f32> {
        self.decoder.decode(ctx, br)
    }
}

#[derive(Debug, Clone)]
pub struct F32Decoder {
    decoder: Box<dyn InternalF32Decode>,
}

impl F32Decoder {
    #[inline]
    pub fn new(field: &FlattenedSerializerField) -> Result<Self> {
        Ok(Self {
            decoder: Box::new(InternalF32Decoder::new(field)?),
        })
    }
}

impl FieldDecode for F32Decoder {
    fn decode(&self, ctx: &FieldDecodeContext, br: &mut BitReader) -> Result<FieldValue> {
        self.decoder
            .decode(ctx, br)
            .map(FieldValue::F32)
            .map_err(Error::from)
    }
}

// ----

#[derive(Debug, Clone)]
struct InternalVectorDefaultDecoder {
    decoder: Box<dyn InternalF32Decode>,
}

impl FieldDecode for InternalVectorDefaultDecoder {
    fn decode(&self, ctx: &FieldDecodeContext, br: &mut BitReader) -> Result<FieldValue> {
        let vec3 = [
            self.decoder.decode(ctx, br)?,
            self.decoder.decode(ctx, br)?,
            self.decoder.decode(ctx, br)?,
        ];
        Ok(FieldValue::Vector(vec3))
    }
}

#[derive(Debug, Clone, Default)]
struct InternalVectorNormalDecoder;

impl FieldDecode for InternalVectorNormalDecoder {
    fn decode(&self, _ctx: &FieldDecodeContext, br: &mut BitReader) -> Result<FieldValue> {
        Ok(FieldValue::Vector(br.read_bitvec3normal()))
    }
}

#[derive(Debug, Clone)]
pub struct VectorDecoder {
    decoder: Box<dyn FieldDecode>,
}

impl VectorDecoder {
    #[inline]
    pub fn new(field: &FlattenedSerializerField) -> Result<Self> {
        if field.var_encoder_heq(fxhash::hash_bytes(b"normal")) {
            Ok(Self {
                decoder: Box::<InternalVectorNormalDecoder>::default(),
            })
        } else {
            Ok(Self {
                decoder: Box::new(InternalVectorDefaultDecoder {
                    decoder: Box::new(InternalF32Decoder::new(field)?),
                }),
            })
        }
    }
}

impl FieldDecode for VectorDecoder {
    fn decode(&self, ctx: &FieldDecodeContext, br: &mut BitReader) -> Result<FieldValue> {
        self.decoder.decode(ctx, br)
    }
}

// ----

#[derive(Debug, Clone)]
pub struct Vector2DDecoder {
    decoder: Box<dyn InternalF32Decode>,
}

impl Vector2DDecoder {
    #[inline]
    pub fn new(field: &FlattenedSerializerField) -> Result<Self> {
        Ok(Self {
            decoder: Box::new(InternalF32Decoder::new(field)?),
        })
    }
}

impl FieldDecode for Vector2DDecoder {
    fn decode(&self, ctx: &FieldDecodeContext, br: &mut BitReader) -> Result<FieldValue> {
        let vec2 = [self.decoder.decode(ctx, br)?, self.decoder.decode(ctx, br)?];
        Ok(FieldValue::Vector2D(vec2))
    }
}

// ----

#[derive(Debug, Clone)]
pub struct Vector4DDecoder {
    decoder: Box<dyn InternalF32Decode>,
}

impl Vector4DDecoder {
    #[inline]
    pub fn new(field: &FlattenedSerializerField) -> Result<Self> {
        Ok(Self {
            decoder: Box::new(InternalF32Decoder::new(field)?),
        })
    }
}

impl FieldDecode for Vector4DDecoder {
    fn decode(&self, ctx: &FieldDecodeContext, br: &mut BitReader) -> Result<FieldValue> {
        let vec4 = [
            self.decoder.decode(ctx, br)?,
            self.decoder.decode(ctx, br)?,
            self.decoder.decode(ctx, br)?,
            self.decoder.decode(ctx, br)?,
        ];
        Ok(FieldValue::Vector4D(vec4))
    }
}

// ----

#[derive(Debug, Clone)]
struct InternalQAnglePitchYawDecoder {
    bit_count: usize,
}

impl FieldDecode for InternalQAnglePitchYawDecoder {
    fn decode(&self, _ctx: &FieldDecodeContext, br: &mut BitReader) -> Result<FieldValue> {
        let vec3 = [
            br.read_bitangle(self.bit_count),
            br.read_bitangle(self.bit_count),
            0.0,
        ];
        Ok(FieldValue::QAngle(vec3))
    }
}

#[derive(Debug, Clone, Default)]
struct InternalQAngleNoBitCountDecoder;

impl FieldDecode for InternalQAngleNoBitCountDecoder {
    fn decode(&self, _ctx: &FieldDecodeContext, br: &mut BitReader) -> Result<FieldValue> {
        Ok(FieldValue::QAngle(br.read_bitvec3coord()))
    }
}

#[derive(Debug, Clone, Default)]
struct InternalQAnglePreciseDecoder;

impl FieldDecode for InternalQAnglePreciseDecoder {
    fn decode(&self, _ctx: &FieldDecodeContext, br: &mut BitReader) -> Result<FieldValue> {
        let mut vec3 = [0f32; 3];

        let rx = br.read_bool();
        let ry = br.read_bool();
        let rz = br.read_bool();

        if rx {
            vec3[0] = br.read_bitangle(20);
        }
        if ry {
            vec3[1] = br.read_bitangle(20);
        }
        if rz {
            vec3[2] = br.read_bitangle(20);
        }

        Ok(FieldValue::QAngle(vec3))
    }
}

#[derive(Debug, Clone)]
struct InternalQAngleBitCountDecoder {
    bit_count: usize,
}

impl FieldDecode for InternalQAngleBitCountDecoder {
    fn decode(&self, _ctx: &FieldDecodeContext, br: &mut BitReader) -> Result<FieldValue> {
        let vec3 = [
            br.read_bitangle(self.bit_count),
            br.read_bitangle(self.bit_count),
            br.read_bitangle(self.bit_count),
        ];
        Ok(FieldValue::QAngle(vec3))
    }
}

#[derive(Debug, Clone)]
pub struct QAngleDecoder {
    decoder: Box<dyn FieldDecode>,
}

impl QAngleDecoder {
    pub fn new(field: &FlattenedSerializerField) -> Self {
        let bit_count = field.bit_count.unwrap_or_default() as usize;

        if let Some(var_encoder) = field.var_encoder.as_ref() {
            match var_encoder.hash {
                hash if hash == fxhash::hash_bytes(b"qangle_pitch_yaw") => {
                    return Self {
                        decoder: Box::new(InternalQAnglePitchYawDecoder { bit_count }),
                    }
                }
                hash if hash == fxhash::hash_bytes(b"qangle_precise") => {
                    return Self {
                        decoder: Box::<InternalQAnglePreciseDecoder>::default(),
                    }
                }

                hash if hash == fxhash::hash_bytes(b"qangle") => {}
                // NOTE(blukai): naming of var encoders seem inconsistent. found this pascal cased
                // name in dota 2 replay from 2018.
                hash if hash == fxhash::hash_bytes(b"QAngle") => {}

                _ => unimplemented!("{:?}", var_encoder),
            }
        }

        if bit_count == 0 {
            return Self {
                decoder: Box::<InternalQAngleNoBitCountDecoder>::default(),
            };
        }

        Self {
            decoder: Box::new(InternalQAngleBitCountDecoder { bit_count }),
        }
    }
}

impl FieldDecode for QAngleDecoder {
    fn decode(&self, ctx: &FieldDecodeContext, br: &mut BitReader) -> Result<FieldValue> {
        self.decoder.decode(ctx, br)
    }
}
