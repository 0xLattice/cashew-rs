//! Cashew uses length-first map keys and always encodes floating point as f64.
use crate::Error;
use ciborium::value::Value;
pub use ciborium::value::Value as CborValue;
use serde::{de::DeserializeOwned, Serialize};

pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, Error> {
    let value = Value::serialized(value).map_err(|e| Error::Cbor(e.to_string()))?;
    encode_value(&value)
}
pub fn decode<T: DeserializeOwned + Serialize>(bytes: &[u8]) -> Result<T, Error> {
    let value = decode_value(bytes)?;
    let decoded: T = value
        .deserialized()
        .map_err(|e| Error::Cbor(e.to_string()))?;
    if encode(&decoded)? != bytes {
        return Err(Error::NonCanonical);
    }
    Ok(decoded)
}
pub fn decode_value(bytes: &[u8]) -> Result<Value, Error> {
    let mut input = bytes;
    let value: Value = ciborium::de::from_reader_with_recursion_limit(&mut input, 64)
        .map_err(|e| Error::Cbor(e.to_string()))?;
    if !input.is_empty() || encode_value(&value)? != bytes {
        return Err(Error::NonCanonical);
    }
    Ok(value)
}
pub fn encode_value(value: &Value) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    write(value, &mut out, 0)?;
    Ok(out)
}
fn head(n: u64, major: u8, out: &mut Vec<u8>) {
    let tag = major << 5;
    if n < 24 {
        out.push(tag | n as u8)
    } else if n <= u8::MAX as u64 {
        out.extend([tag | 24, n as u8])
    } else if n <= u16::MAX as u64 {
        out.push(tag | 25);
        out.extend((n as u16).to_be_bytes())
    } else if n <= u32::MAX as u64 {
        out.push(tag | 26);
        out.extend((n as u32).to_be_bytes())
    } else {
        out.push(tag | 27);
        out.extend(n.to_be_bytes())
    }
}
fn write(v: &Value, o: &mut Vec<u8>, depth: usize) -> Result<(), Error> {
    if depth >= 64 {
        return Err(Error::Limit);
    }
    match v {
        Value::Null => o.push(0xf6),
        Value::Bool(v) => o.push(if *v { 0xf5 } else { 0xf4 }),
        Value::Integer(v) => {
            let n = i128::from(*v);
            if n >= 0 {
                head(u64::try_from(n).map_err(|_| Error::Unsupported)?, 0, o)
            } else {
                head(u64::try_from(-1 - n).map_err(|_| Error::Unsupported)?, 1, o)
            }
        }
        Value::Float(v) => {
            o.push(0xfb);
            o.extend(v.to_bits().to_be_bytes())
        }
        Value::Bytes(v) => {
            head(v.len() as u64, 2, o);
            o.extend(v)
        }
        Value::Text(v) => {
            head(v.len() as u64, 3, o);
            o.extend(v.as_bytes())
        }
        Value::Array(v) => {
            if v.len() > 65536 {
                return Err(Error::Limit);
            }
            head(v.len() as u64, 4, o);
            for v in v {
                write(v, o, depth + 1)?
            }
        }
        Value::Map(v) => {
            if v.len() > 65536 {
                return Err(Error::Limit);
            }
            let mut entries = Vec::new();
            for (k, v) in v {
                let Value::Text(k) = k else {
                    return Err(Error::Unsupported);
                };
                entries.push((k, v));
            }
            entries.sort_by(|(a, _), (b, _)| {
                a.len().cmp(&b.len()).then(a.as_bytes().cmp(b.as_bytes()))
            });
            if entries.windows(2).any(|w| w[0].0 == w[1].0) {
                return Err(Error::NonCanonical);
            }
            head(entries.len() as u64, 5, o);
            for (k, v) in entries {
                head(k.len() as u64, 3, o);
                o.extend(k.as_bytes());
                write(v, o, depth + 1)?
            }
        }
        _ => return Err(Error::Unsupported),
    };
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_rules() {
        assert_eq!(
            encode(&1.5f64).unwrap(),
            vec![0xfb, 0x3f, 0xf8, 0, 0, 0, 0, 0, 0]
        );
        assert!(decode_value(&[0x18, 0x01]).is_err());
        assert!(decode_value(&[0xf9, 0x3e, 0x00]).is_err());
        assert!(decode_value(&[0xa2, 0x61, b'a', 1, 0x61, b'a', 2]).is_err());
        assert!(decode_value(&[0, 0]).is_err());
        assert!(decode_value(&[0xf7]).is_err());
    }
    #[test]
    fn map_order() {
        let value = Value::Map(vec![
            (Value::Text("aa".into()), Value::Null),
            (Value::Text("b".into()), Value::Bool(true)),
        ]);
        let bytes = encode_value(&value).unwrap();
        assert_eq!(bytes, vec![0xa2, 0x61, b'b', 0xf5, 0x62, b'a', b'a', 0xf6]);
        assert!(decode_value(&bytes).is_ok());
    }
}

pub use decode as from_slice;
/// Convenient serde aliases.
pub use encode as to_vec;
/// Swift Foundation.Data wire compatibility: base64 text, not a CBOR byte string.
pub mod base64_bytes {
    use base64::{engine::general_purpose::STANDARD, Engine};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    pub fn serialize<S: Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
        STANDARD.encode(v).serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        STANDARD
            .decode(String::deserialize(d)?)
            .map_err(serde::de::Error::custom)
    }
}
