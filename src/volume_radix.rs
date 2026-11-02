//! Radix collections whose every internal edge is an independent Volume boundary.
use crate::radix::Entry;
use crate::{
    ContentSource, DagHeader, DagNode, Error, Header, MerkleDictionary, RadixNode, Transform,
    Volume,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(serialize = "V:Serialize", deserialize = "V:Deserialize<'de>"))]
pub struct VolumeRadixNode<V> {
    pub prefix: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<V>,
    #[serde(with = "children_wire")]
    pub children: Vec<Entry<Volume<VolumeRadixNode<V>>>>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound = "")]
pub struct VolumeMerkleDictionary<V> {
    pub count: usize,
    #[serde(with = "children_wire")]
    pub children: Vec<Entry<Volume<VolumeRadixNode<V>>>>,
}
impl<V> Default for VolumeMerkleDictionary<V> {
    fn default() -> Self {
        Self {
            count: 0,
            children: vec![],
        }
    }
}
fn ordinary_header<V: Clone>(h: &Volume<VolumeRadixNode<V>>) -> Header<RadixNode<V>> {
    Header {
        raw_cid: h.0.raw_cid.clone(),
        encryption_info: h.0.encryption_info.clone(),
        node: h.0.node.as_ref().map(|n| {
            Arc::new(RadixNode {
                prefix: n.prefix.clone(),
                value: n.value.clone(),
                children: n
                    .children
                    .iter()
                    .map(|e| Entry {
                        key: e.key.clone(),
                        value: ordinary_header(&e.value),
                    })
                    .collect(),
            })
        }),
    }
}
fn volume_header<V: Clone>(h: &Header<RadixNode<V>>) -> Volume<VolumeRadixNode<V>> {
    Volume(Header {
        raw_cid: h.raw_cid.clone(),
        encryption_info: h.encryption_info.clone(),
        node: h.node.as_ref().map(|n| {
            Arc::new(VolumeRadixNode {
                prefix: n.prefix.clone(),
                value: n.value.clone(),
                children: n
                    .children
                    .iter()
                    .map(|e| Entry {
                        key: e.key.clone(),
                        value: volume_header(&e.value),
                    })
                    .collect(),
            })
        }),
    })
}
impl<V: Clone> VolumeMerkleDictionary<V> {
    pub fn to_ordinary(&self) -> MerkleDictionary<V> {
        MerkleDictionary {
            count: self.count,
            children: self
                .children
                .iter()
                .map(|e| Entry {
                    key: e.key.clone(),
                    value: ordinary_header(&e.value),
                })
                .collect(),
        }
    }
    pub fn from_ordinary(t: &MerkleDictionary<V>) -> Self {
        Self {
            count: t.count,
            children: t
                .children
                .iter()
                .map(|e| Entry {
                    key: e.key.clone(),
                    value: volume_header(&e.value),
                })
                .collect(),
        }
    }
}
fn eq(a: &str, b: &str) -> bool {
    a.nfc().eq(b.nfc())
}
fn strip<'a>(key: &'a str, prefix: &str) -> Option<&'a str> {
    let mut offset = 0;
    let mut keys = key.graphemes(true);
    for p in prefix.graphemes(true) {
        let k = keys.next()?;
        if !eq(p, k) {
            return None;
        }
        offset += k.len();
    }
    Some(&key[offset..])
}
fn get_node<'a, V: Serialize>(
    h: &'a Volume<VolumeRadixNode<V>>,
    key: &str,
) -> Result<Option<&'a V>, Error> {
    h.bytes(None)?;
    let node =
        h.0.node
            .as_deref()
            .ok_or_else(|| Error::MissingContent(h.0.raw_cid.clone()))?;
    let Some(rest) = strip(key, &node.prefix) else {
        return Ok(None);
    };
    if rest.is_empty() {
        return Ok(node.value.as_ref());
    }
    let first = rest.graphemes(true).next().unwrap();
    if let Some(e) = node.children.iter().find(|e| eq(&e.key, first)) {
        get_node(&e.value, rest)
    } else {
        Ok(None)
    }
}
impl<V: Serialize> VolumeMerkleDictionary<V> {
    pub fn get(&self, key: &str) -> Result<Option<&V>, Error> {
        let Some(first) = key.graphemes(true).next() else {
            return Ok(None);
        };
        if let Some(e) = self.children.iter().find(|e| eq(&e.key, first)) {
            get_node(&e.value, key)
        } else {
            Ok(None)
        }
    }
}
impl<V: Clone + Serialize> VolumeMerkleDictionary<V> {
    pub fn inserting(&self, key: &str, value: V) -> Result<Self, Error> {
        Ok(Self::from_ordinary(
            &self.to_ordinary().inserting(key, value)?,
        ))
    }
    pub fn updating(&self, key: &str, value: V) -> Result<Self, Error> {
        Ok(Self::from_ordinary(
            &self.to_ordinary().updating(key, value)?,
        ))
    }
    pub fn deleting(&self, key: &str) -> Result<Self, Error> {
        Ok(Self::from_ordinary(&self.to_ordinary().deleting(key)?))
    }
    pub fn transform(&self, ops: &BTreeMap<String, Transform<V>>) -> Result<Self, Error> {
        Ok(Self::from_ordinary(&self.to_ordinary().transform(ops)?))
    }
    pub fn entries(&self) -> Result<BTreeMap<String, V>, Error> {
        self.to_ordinary().entries()
    }
    pub fn sorted_entries(
        &self,
        after: Option<&str>,
        limit: usize,
    ) -> Result<Vec<(String, V)>, Error> {
        self.to_ordinary().sorted_entries(after, limit)
    }
    pub fn proof(&self, key: &str) -> Result<Self, Error> {
        Ok(Self::from_ordinary(&self.to_ordinary().proof(key)?))
    }
    pub fn sparse_entries(&self) -> Result<BTreeMap<String, Vec<u8>>, Error> {
        self.to_ordinary().sparse_entries()
    }
}
impl<V: Clone + Serialize + DeserializeOwned> VolumeMerkleDictionary<V> {
    /// Hydrate only the ordered cursor window, preserving Volume boundaries.
    pub fn resolve_range_with_keys(
        &self,
        after: Option<&str>,
        limit: usize,
        source: &impl ContentSource,
        keys: &dyn crate::KeyProvider,
    ) -> Result<Self, Error> {
        Ok(Self::from_ordinary(
            &self
                .to_ordinary()
                .resolve_range_with_keys(after, limit, source, keys)?,
        ))
    }
    pub fn bounded_keys_and_values(
        &self,
        after: Option<&str>,
        limit: usize,
        source: &impl ContentSource,
    ) -> Result<Vec<(String, V)>, Error> {
        self.resolve_range_with_keys(after, limit, source, &())?
            .sorted_entries(after, limit)
    }
    pub fn resolve_key(&self, key: &str, source: &impl ContentSource) -> Result<Self, Error> {
        Ok(Self::from_ordinary(
            &self.to_ordinary().resolve_key(key, source)?,
        ))
    }
    pub fn resolve_all(&self, source: &impl ContentSource) -> Result<Self, Error> {
        Ok(Self::from_ordinary(
            &self.to_ordinary().resolve_all(source)?,
        ))
    }
    pub fn prove_transforms(
        &self,
        ops: &BTreeMap<String, Transform<V>>,
        source: &impl ContentSource,
    ) -> Result<Self, Error> {
        Ok(Self::from_ordinary(
            &self.to_ordinary().prove_transforms(ops, source)?,
        ))
    }
}
impl<V: DagNode> DagNode for VolumeRadixNode<V> {
    fn links(&self) -> Vec<&dyn DagHeader> {
        let mut out: Vec<&dyn DagHeader> = self
            .children
            .iter()
            .map(|e| &e.value as &dyn DagHeader)
            .collect();
        if let Some(v) = &self.value {
            out.extend(v.links())
        }
        out
    }
}
impl<V: DagNode> DagNode for VolumeMerkleDictionary<V> {
    fn links(&self) -> Vec<&dyn DagHeader> {
        self.children
            .iter()
            .map(|e| &e.value as &dyn DagHeader)
            .collect()
    }
}
type Children<V> = Vec<Entry<Volume<VolumeRadixNode<V>>>>;
mod children_wire {
    use super::*;
    pub fn serialize<V, S: serde::Serializer>(
        v: &[Entry<Volume<VolumeRadixNode<V>>>],
        s: S,
    ) -> Result<S::Ok, S::Error> {
        let mut sorted: Vec<_> = v.iter().collect();
        sorted.sort_by(|a, b| a.key.nfc().cmp(b.key.nfc()));
        sorted.serialize(s)
    }
    pub fn deserialize<'de, V, D: serde::Deserializer<'de>>(d: D) -> Result<Children<V>, D::Error> {
        let entries = Children::<V>::deserialize(d)?;
        let mut result: Children<V> = vec![];
        for mut e in entries {
            let Some(first) = e.key.graphemes(true).next() else {
                continue;
            };
            let key = first.to_string();
            if result.iter().any(|e| eq(&e.key, &key)) {
                return Err(serde::de::Error::custom("duplicate child key"));
            }
            e.key = key;
            result.push(e)
        }
        Ok(result)
    }
}

impl<V: crate::query::QueryValue> VolumeMerkleDictionary<V> {
    pub fn execute(
        &self,
        plan: &crate::query::Plan,
    ) -> Result<(Self, crate::query::QueryResult), Error> {
        let (node, result) = self.to_ordinary().execute(plan)?;
        Ok((Self::from_ordinary(&node), result))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_page_does_not_fetch_later_volume_branches() {
        let tree = VolumeMerkleDictionary::default()
            .inserting("a", 1u64)
            .unwrap()
            .inserting("b", 2)
            .unwrap()
            .inserting("z", 3)
            .unwrap();
        let root = Volume::new(tree.clone()).unwrap();
        let mut entries: BTreeMap<String, Vec<u8>> = crate::volume::serialize_recursive(&root, &())
            .unwrap()
            .into_iter()
            .flat_map(|v| v.entries)
            .collect();
        let z = tree
            .children
            .iter()
            .find(|e| e.key == "z")
            .unwrap()
            .value
            .0
            .raw_cid
            .clone();
        entries.remove(&z);
        let lazy: VolumeMerkleDictionary<u64> =
            crate::cbor::decode(&crate::cbor::encode(&tree).unwrap()).unwrap();
        assert_eq!(
            lazy.bounded_keys_and_values(None, 1, &entries).unwrap(),
            vec![("a".into(), 1)]
        );
        assert_eq!(
            lazy.bounded_keys_and_values(Some("a"), 1, &entries)
                .unwrap(),
            vec![("b".into(), 2)]
        );
        assert!(lazy
            .bounded_keys_and_values(Some("b"), 1, &entries)
            .is_err());
        assert!(lazy
            .bounded_keys_and_values(None, 0, &BTreeMap::<String, Vec<u8>>::new())
            .unwrap()
            .is_empty());
    }
    #[test]
    fn every_edge_is_independent_and_wire_equal() {
        let ordinary = MerkleDictionary::default()
            .inserting("cat", 1u64)
            .unwrap()
            .inserting("car", 2)
            .unwrap()
            .inserting("dog", 3)
            .unwrap();
        let tree = VolumeMerkleDictionary::from_ordinary(&ordinary);
        assert_eq!(
            crate::cbor::encode(&tree).unwrap(),
            crate::cbor::encode(&ordinary).unwrap()
        );
        let root = Volume::new(tree.clone()).unwrap();
        let volumes = crate::volume::serialize_recursive(&root, &()).unwrap();
        assert!(volumes.len() > 3);
        assert!(volumes.iter().all(|v| v.entries.len() == 1));
        let entries: BTreeMap<String, Vec<u8>> =
            volumes.into_iter().flat_map(|v| v.entries).collect();
        let lazy: VolumeMerkleDictionary<u64> =
            crate::cbor::decode(&crate::cbor::encode(&tree).unwrap()).unwrap();
        assert_eq!(
            lazy.resolve_all(&entries).unwrap().get("cat").unwrap(),
            Some(&1)
        );
        let after = tree.deleting("car").unwrap();
        assert_eq!(
            Header::new(after.clone()).unwrap().raw_cid,
            Header::new(ordinary.deleting("car").unwrap())
                .unwrap()
                .raw_cid
        );
        assert!(
            crate::volume::serialize_recursive(&Volume::new(after).unwrap(), &())
                .unwrap()
                .iter()
                .all(|v| v.entries.len() == 1)
        );
    }
}
