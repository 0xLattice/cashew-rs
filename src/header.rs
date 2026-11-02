use crate::{canonical_cid, cbor, cid_for_bytes, verify_content_address, Error};
use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};
use base64::{engine::general_purpose::STANDARD, Engine};
use multihash_codetable::{Code, MultihashDigest};
use rand::RngCore;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptionInfo {
    #[serde(rename = "keyHash")]
    pub key_hash: String,
    pub iv: String,
}
impl EncryptionInfo {
    pub fn new(key: &[u8; 32], nonce: &[u8; 12]) -> Self {
        Self {
            key_hash: STANDARD.encode(Code::Sha2_256.digest(key).digest()),
            iv: STANDARD.encode(nonce),
        }
    }
}
/// Implementations must return the requested raw self-authenticating bytes.
pub trait ContentSource {
    fn fetch(&self, cid: &str) -> Result<Vec<u8>, Error>;
}
impl ContentSource for BTreeMap<String, Vec<u8>> {
    fn fetch(&self, cid: &str) -> Result<Vec<u8>, Error> {
        self.get(cid)
            .cloned()
            .ok_or_else(|| Error::MissingContent(cid.into()))
    }
}
/// A header's serialized representation contains only its CID and encryption metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound = "")]
pub struct Header<T> {
    #[serde(rename = "rawCID", deserialize_with = "deserialize_cid")]
    pub raw_cid: String,
    #[serde(
        rename = "encryptionInfo",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub encryption_info: Option<EncryptionInfo>,
    #[serde(skip)]
    pub node: Option<Arc<T>>,
}
impl<T> Header<T> {
    pub fn from_cid(cid: impl Into<String>) -> Result<Self, Error> {
        Ok(Self {
            raw_cid: canonical_cid(&cid.into())?,
            encryption_info: None,
            node: None,
        })
    }
    pub fn removing_node(&self) -> Self {
        Self {
            raw_cid: self.raw_cid.clone(),
            encryption_info: self.encryption_info.clone(),
            node: None,
        }
    }
}
impl<T: Serialize> Header<T> {
    pub fn new(node: T) -> Result<Self, Error> {
        Self::with_codec(node, 0x71)
    }
    pub fn with_codec(node: T, codec: u64) -> Result<Self, Error> {
        let raw_cid = cid_for_bytes(codec, &cbor::encode(&node)?);
        Ok(Self {
            raw_cid,
            encryption_info: None,
            node: Some(Arc::new(node)),
        })
    }
    pub fn encrypted(node: T, key: &[u8; 32]) -> Result<Self, Error> {
        let mut nonce = [0; 12];
        rand::thread_rng().fill_bytes(&mut nonce);
        Self::encrypted_with_nonce(node, key, &nonce)
    }
    pub fn encrypted_with_nonce(node: T, key: &[u8; 32], nonce: &[u8; 12]) -> Result<Self, Error> {
        let encrypted = encrypt(&cbor::encode(&node)?, key, nonce)?;
        Ok(Self {
            raw_cid: cid_for_bytes(0x71, &encrypted),
            encryption_info: Some(EncryptionInfo::new(key, nonce)),
            node: Some(Arc::new(node)),
        })
    }
    pub fn bytes(&self, key: Option<&[u8; 32]>) -> Result<Vec<u8>, Error> {
        let node = self
            .node
            .as_ref()
            .ok_or_else(|| Error::MissingContent(self.raw_cid.clone()))?;
        let plain = cbor::encode(node.as_ref())?;
        let bytes = if let Some(info) = &self.encryption_info {
            let key = key.ok_or(Error::Encryption)?;
            let nonce: [u8; 12] = STANDARD
                .decode(&info.iv)
                .map_err(|_| Error::Encryption)?
                .try_into()
                .map_err(|_| Error::Encryption)?;
            if EncryptionInfo::new(key, &nonce) != *info {
                return Err(Error::Encryption);
            }
            encrypt(&plain, key, &nonce)?
        } else {
            plain
        };
        verify_content_address(&self.raw_cid, &bytes)?;
        Ok(bytes)
    }
}
impl<T: Serialize + DeserializeOwned> Header<T> {
    pub fn resolve(
        &self,
        source: &impl ContentSource,
        key: Option<&[u8; 32]>,
    ) -> Result<Self, Error> {
        if self.node.is_some() {
            self.bytes(key)?;
            return Ok(Self {
                raw_cid: self.raw_cid.clone(),
                encryption_info: self.encryption_info.clone(),
                node: self.node.clone(),
            });
        }
        let mut bytes = source.fetch(&self.raw_cid)?;
        verify_content_address(&self.raw_cid, &bytes)?;
        if let Some(info) = &self.encryption_info {
            let key = key.ok_or(Error::Encryption)?;
            if bytes.len() < 28 {
                return Err(Error::Encryption);
            }
            let nonce: [u8; 12] = bytes[..12].try_into().map_err(|_| Error::Encryption)?;
            if EncryptionInfo::new(key, &nonce) != *info {
                return Err(Error::Encryption);
            }
            bytes = decrypt(&bytes, key)?;
        }
        let node = cbor::decode(&bytes)?;
        Ok(Self {
            raw_cid: self.raw_cid.clone(),
            encryption_info: self.encryption_info.clone(),
            node: Some(Arc::new(node)),
        })
    }
}
pub fn encrypt(bytes: &[u8], key: &[u8; 32], nonce: &[u8; 12]) -> Result<Vec<u8>, Error> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| Error::Encryption)?;
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(nonce), bytes)
        .map_err(|_| Error::Encryption)?;
    let mut combined = nonce.to_vec();
    combined.extend(ciphertext);
    Ok(combined)
}
pub fn decrypt(bytes: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, Error> {
    if bytes.len() < 28 {
        return Err(Error::Encryption);
    }
    Aes256Gcm::new_from_slice(key)
        .map_err(|_| Error::Encryption)?
        .decrypt(Nonce::from_slice(&bytes[..12]), &bytes[12..])
        .map_err(|_| Error::Encryption)
}
fn deserialize_cid<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let raw = String::deserialize(d)?;
    Ok(canonical_cid(&raw).unwrap_or(raw))
}

impl<T> std::fmt::Display for Header<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(info) = &self.encryption_info {
            write!(f, "enc:{}:{}:{}", info.key_hash, info.iv, self.raw_cid)
        } else {
            f.write_str(&self.raw_cid)
        }
    }
}
impl<T> std::str::FromStr for Header<T> {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Error> {
        if let Some(rest) = s.strip_prefix("enc:") {
            let parts: Vec<_> = rest.splitn(3, ':').collect();
            if parts.len() != 3 || parts.iter().any(|s| s.is_empty()) {
                return Err(Error::InvalidCid(s.into()));
            }
            Ok(Self {
                raw_cid: canonical_cid(parts[2]).unwrap_or_else(|_| parts[2].into()),
                encryption_info: Some(EncryptionInfo {
                    key_hash: parts[0].into(),
                    iv: parts[1].into(),
                }),
                node: None,
            })
        } else {
            Ok(Self {
                raw_cid: canonical_cid(s).unwrap_or_else(|_| s.into()),
                encryption_info: None,
                node: None,
            })
        }
    }
}
impl<T> PartialEq for Header<T> {
    fn eq(&self, other: &Self) -> bool {
        self.raw_cid == other.raw_cid && self.encryption_info == other.encryption_info
    }
}
impl<T> Eq for Header<T> {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plaintext_roundtrip() {
        let h = Header::new("hello".to_string()).unwrap();
        let source = BTreeMap::from([(h.raw_cid.clone(), h.bytes(None).unwrap())]);
        let bytes = cbor::encode(&h).unwrap();
        let lazy: Header<String> = cbor::decode(&bytes).unwrap();
        assert!(lazy.node.is_none());
        assert_eq!(
            lazy.resolve(&source, None).unwrap().node.as_deref(),
            Some(&"hello".to_string())
        );
    }
    #[test]
    fn encryption_authentication() {
        let key = [7; 32];
        let h = Header::encrypted_with_nonce("secret".to_string(), &key, &[8; 12]).unwrap();
        let bytes = h.bytes(Some(&key)).unwrap();
        assert_eq!(&bytes[..12], &[8; 12]);
        let source = BTreeMap::from([(h.raw_cid.clone(), bytes)]);
        assert!(h.removing_node().resolve(&source, Some(&[6; 32])).is_err());
        assert_eq!(
            h.removing_node()
                .resolve(&source, Some(&key))
                .unwrap()
                .node
                .as_deref(),
            Some(&"secret".to_string())
        );
    }
}
