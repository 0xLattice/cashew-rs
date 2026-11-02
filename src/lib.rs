//! Content-addressed data, canonical Swift-compatible DAG-CBOR and volume boundaries.
use cid::Cid;
use multihash_codetable::{Code, MultihashDigest};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub mod cbor;
pub mod diff;
pub mod header;
pub mod platform;
pub mod query;
pub mod radix;
pub mod source;
pub use lattice_array_trie::ArrayTrie;
pub use platform::{
    EncryptionStrategy, ErasedHeader, NamedHeaders, PlatformNode, PlatformValue, ProofKind,
    ResolutionStrategy, StorageStrategy,
};
pub mod volume;
pub mod volume_radix;
pub use volume::{DagHeader, DagNode, KeyProvider, Volume};
pub use volume_radix::{VolumeMerkleDictionary, VolumeRadixNode};
pub mod collections;
pub use collections::{FlatDictionary, MerkleArray, MerkleSet};
pub use header::{ContentSource, EncryptionInfo, Header};
pub use radix::{MerkleDictionary, RadixNode, Transform};

#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    #[error("header type mismatch for property: {0}")]
    TypeMismatch(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("source error: {0}")]
    Source(String),
    #[error("invalid CID: {0}")]
    InvalidCid(String),
    #[error("content address mismatch: {0}")]
    ContentAddressMismatch(String),
    #[error("volume root is empty")]
    EmptyRoot,
    #[error("missing root entry: {0}")]
    MissingRoot(String),
    #[error("invalid CBOR: {0}")]
    Cbor(String),
    #[error("noncanonical CBOR")]
    NonCanonical,
    #[error("CBOR depth or collection limit exceeded")]
    Limit,
    #[error("unsupported CBOR value")]
    Unsupported,
    #[error("encryption authentication failed")]
    Encryption,
    #[error("invalid key: {0}")]
    InvalidKey(String),
    #[error("missing content: {0}")]
    MissingContent(String),
}

pub fn canonical_cid(raw: &str) -> Result<String, Error> {
    Ok(raw
        .parse::<Cid>()
        .map_err(|_| Error::InvalidCid(raw.into()))?
        .to_string())
}
pub fn cid_for_bytes(codec: u64, data: &[u8]) -> String {
    Cid::new_v1(codec, Code::Sha2_256.digest(data)).to_string()
}
pub fn validate_entry(raw_cid: &str, data: &[u8]) -> Result<(), Error> {
    let cid: Cid = raw_cid
        .parse()
        .map_err(|_| Error::InvalidCid(raw_cid.into()))?;
    if cid.to_string() != raw_cid || cid.hash().size() == 0 {
        return Err(Error::InvalidCid(raw_cid.into()));
    }
    let expected = cid.hash().digest();
    let actual = if cid.hash().code() == 0 {
        data.to_vec()
    } else {
        let code =
            Code::try_from(cid.hash().code()).map_err(|_| Error::InvalidCid(raw_cid.into()))?;
        let hash = code.digest(data);
        if expected.len() > hash.digest().len() {
            return Err(Error::InvalidCid(raw_cid.into()));
        }
        hash.digest()[..expected.len()].to_vec()
    };
    if actual != expected {
        return Err(Error::ContentAddressMismatch(raw_cid.into()));
    }
    Ok(())
}
/// Typed Cashew DAG references require full SHA-256, unlike generic broker CAS entries.
pub fn verify_content_address(raw_cid: &str, data: &[u8]) -> Result<(), Error> {
    let cid: cid::Cid = raw_cid
        .parse()
        .map_err(|_| Error::InvalidCid(raw_cid.into()))?;
    if cid.hash().code() != 0x12 || cid.hash().size() != 32 {
        return Err(Error::InvalidCid(raw_cid.into()));
    }
    validate_entry(raw_cid, data)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SerializedVolume {
    pub root: String,
    pub entries: BTreeMap<String, Vec<u8>>,
}
impl SerializedVolume {
    pub fn new(root: impl Into<String>, entries: BTreeMap<String, Vec<u8>>) -> Self {
        Self {
            root: root.into(),
            entries,
        }
    }
    pub fn validate(&self) -> Result<(), Error> {
        if self.root.is_empty() {
            return Err(Error::EmptyRoot);
        }
        if !self.entries.contains_key(&self.root) {
            return Err(Error::MissingRoot(self.root.clone()));
        }
        for (cid, bytes) in &self.entries {
            validate_entry(cid, bytes)?;
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn content_addresses() {
        let data = b"hello";
        let cid = cid_for_bytes(0x71, data);
        validate_entry(&cid, data).unwrap();
        assert!(validate_entry(&cid, b"bye").is_err());
        assert!(validate_entry(&cid.to_uppercase(), data).is_err());
        SerializedVolume::new(cid.clone(), BTreeMap::from([(cid, data.to_vec())]))
            .validate()
            .unwrap();
    }
    #[test]
    fn boundary_validation() {
        assert!(SerializedVolume::new("", BTreeMap::new())
            .validate()
            .is_err());
        assert!(
            SerializedVolume::new(cid_for_bytes(0x71, b"x"), BTreeMap::new())
                .validate()
                .is_err()
        );
    }
}
