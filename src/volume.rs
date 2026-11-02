//! Explicit typed DAG links define complete retention boundaries.
use crate::{Error, Header, MerkleArray, MerkleDictionary, MerkleSet, RadixNode, SerializedVolume};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
/// A node enumerates precisely the headers included in its serialized content.
/// Leaf structs can implement this with the empty default.
pub trait DagNode: Serialize {
    fn links(&self) -> Vec<&dyn DagHeader> {
        vec![]
    }
}
/// Object-safe view of a typed header, used only during traversal.
pub trait DagHeader {
    fn cid(&self) -> &str;
    fn is_volume(&self) -> bool {
        false
    }
    fn serialized(&self, keys: &dyn KeyProvider) -> Result<Vec<u8>, Error>;
    fn links(&self) -> Result<Vec<&dyn DagHeader>, Error>;
}
pub trait KeyProvider {
    fn key(&self, key_hash: &str) -> Option<[u8; 32]>;
}
impl KeyProvider for () {
    fn key(&self, _: &str) -> Option<[u8; 32]> {
        None
    }
}
impl KeyProvider for BTreeMap<String, [u8; 32]> {
    fn key(&self, h: &str) -> Option<[u8; 32]> {
        self.get(h).copied()
    }
}
impl<T: DagNode> DagHeader for Header<T> {
    fn cid(&self) -> &str {
        &self.raw_cid
    }
    fn serialized(&self, keys: &dyn KeyProvider) -> Result<Vec<u8>, Error> {
        let key = self
            .encryption_info
            .as_ref()
            .and_then(|info| keys.key(&info.key_hash));
        self.bytes(key.as_ref())
    }
    fn links(&self) -> Result<Vec<&dyn DagHeader>, Error> {
        Ok(self
            .node
            .as_deref()
            .ok_or_else(|| Error::MissingContent(self.raw_cid.clone()))?
            .links())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent, bound = "")]
pub struct Volume<T>(pub Header<T>);
impl<T: Serialize> Volume<T> {
    pub fn new(node: T) -> Result<Self, Error> {
        Ok(Self(Header::new(node)?))
    }
}

impl<T> Volume<T> {
    pub fn from_cid(cid: impl Into<String>) -> Result<Self, Error> {
        Ok(Self(Header::from_cid(cid)?))
    }
    pub fn removing_node(&self) -> Self {
        Self(self.0.removing_node())
    }
}
impl<T: Serialize> Volume<T> {
    pub fn encrypted(node: T, key: &[u8; 32]) -> Result<Self, Error> {
        Ok(Self(Header::encrypted(node, key)?))
    }
    pub fn bytes(&self, key: Option<&[u8; 32]>) -> Result<Vec<u8>, Error> {
        self.0.bytes(key)
    }
}
impl<T: Serialize + serde::de::DeserializeOwned> Volume<T> {
    pub fn resolve(
        &self,
        source: &impl crate::ContentSource,
        key: Option<&[u8; 32]>,
    ) -> Result<Self, Error> {
        Ok(Self(self.0.resolve(source, key)?))
    }
    pub async fn resolve_async(
        &self,
        source: &impl crate::source::Fetcher,
        key: Option<&[u8; 32]>,
    ) -> Result<Self, Error> {
        Ok(Self(self.0.resolve_async(source, key).await?))
    }
}
impl<T> std::ops::Deref for Volume<T> {
    type Target = Header<T>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl<T> std::fmt::Display for Volume<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl<T> std::str::FromStr for Volume<T> {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Error> {
        Ok(Self(s.parse()?))
    }
}
impl<T> PartialEq for Volume<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl<T> Eq for Volume<T> {}

impl<T: DagNode> DagHeader for Volume<T> {
    fn cid(&self) -> &str {
        &self.0.raw_cid
    }
    fn is_volume(&self) -> bool {
        true
    }
    fn serialized(&self, keys: &dyn KeyProvider) -> Result<Vec<u8>, Error> {
        self.0.serialized(keys)
    }
    fn links(&self) -> Result<Vec<&dyn DagHeader>, Error> {
        DagHeader::links(&self.0)
    }
}
impl<T: DagNode> DagNode for Header<T> {
    fn links(&self) -> Vec<&dyn DagHeader> {
        vec![self]
    }
}
impl<T: DagNode> DagNode for Volume<T> {
    fn links(&self) -> Vec<&dyn DagHeader> {
        vec![self]
    }
}
macro_rules! scalar {($($t:ty),*)=>{$(impl DagNode for $t {})*}}
scalar!(
    String,
    bool,
    u8,
    u16,
    u32,
    u64,
    i8,
    i16,
    i32,
    i64,
    f32,
    f64,
    ()
);
impl<T: DagNode> DagNode for RadixNode<T> {
    fn links(&self) -> Vec<&dyn DagHeader> {
        let mut links: Vec<&dyn DagHeader> = self
            .children
            .iter()
            .map(|e| &e.value as &dyn DagHeader)
            .collect();
        if let Some(v) = &self.value {
            links.extend(v.links())
        }
        links
    }
}
impl<T: DagNode> DagNode for MerkleDictionary<T> {
    fn links(&self) -> Vec<&dyn DagHeader> {
        self.children
            .iter()
            .map(|e| &e.value as &dyn DagHeader)
            .collect()
    }
}
impl<T: DagNode> DagNode for MerkleArray<T> {
    fn links(&self) -> Vec<&dyn DagHeader> {
        self.0.links()
    }
}
impl DagNode for MerkleSet {
    fn links(&self) -> Vec<&dyn DagHeader> {
        self.0.links()
    }
}
impl<T: DagNode> DagNode for BTreeMap<String, T> {
    fn links(&self) -> Vec<&dyn DagHeader> {
        self.values().flat_map(DagNode::links).collect()
    }
}
/// Complete one boundary. Nested volumes remain committed references, without their bytes.
pub fn serialize_boundary(
    root: &dyn DagHeader,
    keys: &dyn KeyProvider,
) -> Result<SerializedVolume, Error> {
    let mut entries = BTreeMap::new();
    let mut pending = vec![root];
    while let Some(h) = pending.pop() {
        if entries.contains_key(h.cid()) {
            continue;
        }
        entries.insert(h.cid().into(), h.serialized(keys)?);
        for child in h.links()? {
            if !child.is_volume() {
                pending.push(child)
            }
        }
    }
    let volume = SerializedVolume::new(root.cid(), entries);
    volume.validate()?;
    Ok(volume)
}
/// Materialize all boundaries before writing. Repeated CIDs are emitted once.
pub fn serialize_recursive(
    root: &dyn DagHeader,
    keys: &dyn KeyProvider,
) -> Result<Vec<SerializedVolume>, Error> {
    let mut volumes = vec![];
    let mut pending = vec![root];
    let mut seen = BTreeSet::new();
    while let Some(root) = pending.pop() {
        if !seen.insert(root.cid().to_owned()) {
            continue;
        }
        volumes.push(serialize_boundary(root, keys)?);
        let mut walk = vec![root];
        let mut ordinary_seen = BTreeSet::new();
        while let Some(h) = walk.pop() {
            if !ordinary_seen.insert(h.cid()) {
                continue;
            }
            for child in h.links()? {
                if child.is_volume() {
                    match child.links() {
                        Ok(_) => pending.push(child),
                        Err(Error::MissingContent(_)) => {}
                        Err(e) => return Err(e),
                    }
                } else {
                    walk.push(child)
                }
            }
        }
    }
    Ok(volumes)
}
/// Report authenticated materialized nodes in post-order and retain repeated
/// parent-to-child edges. A seeded visited set excludes previously walked roots.
pub fn walk_owned_subtree(
    root: &dyn DagHeader,
    visited: &mut BTreeSet<String>,
    keys: &dyn KeyProvider,
    visit: &mut impl FnMut(&str, &BTreeMap<String, usize>),
) -> Result<(), Error> {
    if visited.contains(root.cid()) {
        return Ok(());
    }
    match root.serialized(keys) {
        Err(Error::MissingContent(_)) => return Ok(()),
        Err(e) => return Err(e),
        Ok(_) => {}
    }
    visited.insert(root.cid().into());
    let children = root.links()?;
    let mut edges = BTreeMap::new();
    for child in children {
        *edges.entry(child.cid().to_owned()).or_insert(0) += 1;
        walk_owned_subtree(child, visited, keys, visit)?
    }
    visit(root.cid(), &edges);
    Ok(())
}
/// Store one complete boundary using the caller's retry/deduplication session.
pub async fn store_boundary(
    root: &dyn DagHeader,
    keys: &dyn KeyProvider,
    storer: &dyn crate::source::VolumeStorer,
) -> Result<(), Error> {
    storer.store_volume(serialize_boundary(root, keys)?).await
}
/// Store each materialized boundary in traversal order. Earlier writes remain
/// committed if a later boundary fails, matching Swift's nontransactional plan.
pub async fn store_recursive(
    root: &dyn DagHeader,
    keys: &dyn KeyProvider,
    storer: &dyn crate::source::VolumeStorer,
) -> Result<(), Error> {
    let mut pending = vec![root];
    let mut seen = BTreeSet::new();
    while let Some(boundary) = pending.pop() {
        if !seen.insert(boundary.cid().to_owned()) {
            continue;
        }
        store_boundary(boundary, keys, storer).await?;
        let mut walk = vec![boundary];
        let mut ordinary_seen = BTreeSet::new();
        while let Some(h) = walk.pop() {
            if !ordinary_seen.insert(h.cid().to_owned()) {
                continue;
            }
            for child in h.links()? {
                if child.is_volume() {
                    match child.links() {
                        Ok(_) => pending.push(child),
                        Err(Error::MissingContent(_)) => {}
                        Err(e) => return Err(e),
                    }
                } else {
                    walk.push(child)
                }
            }
        }
    }
    Ok(())
}

/// Plan selected boundaries without writing; root Volume is always included.
/// Shared boundary CIDs are emitted once and every included boundary is complete.
pub fn serialize_selected(
    root: &dyn crate::ErasedHeader,
    paths: &crate::ArrayTrie<crate::StorageStrategy>,
    keys: &dyn KeyProvider,
) -> Result<Vec<SerializedVolume>, Error> {
    let mut seen = BTreeSet::new();
    Ok(root
        .storage_paths(paths, keys)?
        .into_iter()
        .filter(|v| seen.insert(v.root.clone()))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Serialize)]
    struct Folder {
        ordinary: Header<String>,
        nested: Volume<String>,
    }
    impl DagNode for Folder {
        fn links(&self) -> Vec<&dyn DagHeader> {
            vec![&self.ordinary, &self.nested]
        }
    }
    #[test]
    fn nested_boundaries() {
        let leaf = Header::new("ordinary".into()).unwrap();
        let nested = Volume::new("nested".into()).unwrap();
        let inner = nested.0.raw_cid.clone();
        let outer = Volume::new(Folder {
            ordinary: leaf.clone(),
            nested,
        })
        .unwrap();
        let boundary = serialize_boundary(&outer, &()).unwrap();
        assert!(boundary.entries.contains_key(&leaf.raw_cid));
        assert!(!boundary.entries.contains_key(&inner));
        let all = serialize_recursive(&outer, &()).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[1].entries.len(), 1);
        assert!(all[1].entries.contains_key(&inner));
    }
    #[test]
    fn missing_ordinary_data_fails_complete_boundary() {
        let missing: Header<String> = Header::new("missing".into()).unwrap().removing_node();
        let folder = Volume::new(Folder {
            ordinary: missing,
            nested: Volume::new("independent".into()).unwrap(),
        })
        .unwrap();
        assert!(serialize_boundary(&folder, &()).is_err());
    }
}
