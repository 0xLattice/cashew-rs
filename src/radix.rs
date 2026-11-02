//! Immutable compressed radix dictionaries. Child commitments preserve sparse subtrees.
use crate::{ContentSource, Error, Header, SerializedVolume};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry<T> {
    pub key: String,
    pub value: T,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(serialize = "V:Serialize", deserialize = "V:Deserialize<'de>"))]
pub struct RadixNode<V> {
    pub prefix: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<V>,
    #[serde(with = "children_wire")]
    pub children: Vec<Entry<Header<RadixNode<V>>>>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound = "")]
pub struct MerkleDictionary<V> {
    pub count: usize,
    #[serde(with = "children_wire")]
    pub children: Vec<Entry<Header<RadixNode<V>>>>,
}
impl<V> Default for MerkleDictionary<V> {
    fn default() -> Self {
        Self {
            count: 0,
            children: vec![],
        }
    }
}
fn first(s: &str) -> Result<&str, Error> {
    s.graphemes(true)
        .next()
        .ok_or_else(|| Error::InvalidKey("empty key".into()))
}
fn eq(a: &str, b: &str) -> bool {
    a.nfc().eq(b.nfc())
}
fn common(a: &str, b: &str) -> (usize, usize) {
    a.graphemes(true)
        .zip(b.graphemes(true))
        .take_while(|(a, b)| eq(a, b))
        .fold((0, 0), |(a, b), (x, y)| (a + x.len(), b + y.len()))
}
fn strip<'a>(key: &'a str, prefix: &str) -> Option<&'a str> {
    let (a, b) = common(prefix, key);
    if a == prefix.len() {
        Some(&key[b..])
    } else {
        None
    }
}
fn loaded<V: Serialize>(h: &Header<RadixNode<V>>) -> Result<&RadixNode<V>, Error> {
    loaded_with_keys(h, &())
}
fn loaded_with_keys<'a, V: Serialize>(
    h: &'a Header<RadixNode<V>>,
    keys: &dyn crate::KeyProvider,
) -> Result<&'a RadixNode<V>, Error> {
    let key = h
        .encryption_info
        .as_ref()
        .and_then(|i| keys.key(&i.key_hash));
    h.bytes(key.as_ref())?;
    h.node
        .as_deref()
        .ok_or_else(|| Error::MissingContent(h.raw_cid.clone()))
}
fn rewrite<V: Serialize>(
    old: &Header<RadixNode<V>>,
    node: RadixNode<V>,
    keys: &dyn crate::KeyProvider,
) -> Result<Header<RadixNode<V>>, Error> {
    if let Some(info) = &old.encryption_info {
        Header::encrypted(node, &keys.key(&info.key_hash).ok_or(Error::Encryption)?)
    } else {
        Header::new(node)
    }
}
fn sort<V>(children: &mut [Entry<Header<RadixNode<V>>>]) {
    children.sort_by(|a, b| a.key.nfc().cmp(b.key.nfc()))
}
#[derive(Clone, Debug)]
pub enum Transform<V> {
    Insert(V),
    Update(V),
    Delete,
}
impl<V: Clone + Serialize> MerkleDictionary<V> {
    pub fn get(&self, key: &str) -> Result<Option<&V>, Error> {
        self.get_with_keys(key, &())
    }
    pub fn get_with_keys(
        &self,
        key: &str,
        keys: &dyn crate::KeyProvider,
    ) -> Result<Option<&V>, Error> {
        if key.is_empty() {
            return Ok(None);
        }
        match self
            .children
            .iter()
            .find(|e| eq(&e.key, first(key).unwrap()))
        {
            Some(e) => get_node(loaded_with_keys(&e.value, keys)?, key, keys),
            None => Ok(None),
        }
    }
    pub fn inserting(&self, key: &str, value: V) -> Result<Self, Error> {
        self.apply(key, Transform::Insert(value))
    }
    pub fn updating(&self, key: &str, value: V) -> Result<Self, Error> {
        self.apply(key, Transform::Update(value))
    }
    pub fn deleting(&self, key: &str) -> Result<Self, Error> {
        self.apply(key, Transform::Delete)
    }
    pub fn apply(&self, key: &str, op: Transform<V>) -> Result<Self, Error> {
        self.apply_with_keys(key, op, &())
    }
    pub fn apply_with_keys(
        &self,
        key: &str,
        op: Transform<V>,
        keys: &dyn crate::KeyProvider,
    ) -> Result<Self, Error> {
        let leading = first(key)?;
        let mut next = self.clone();
        let position = next.children.iter().position(|e| eq(&e.key, leading));
        match position {
            Some(i) => match mutate(
                loaded_with_keys(&next.children[i].value, keys)?,
                key,
                &op,
                keys,
            )? {
                Some(node) => {
                    next.children[i].value = rewrite(&next.children[i].value, node, keys)?
                }
                None => {
                    next.children.remove(i);
                }
            },
            None => match &op {
                Transform::Insert(value) => next.children.push(Entry {
                    key: leading.into(),
                    value: Header::new(RadixNode {
                        prefix: key.into(),
                        value: Some(value.clone()),
                        children: vec![],
                    })?,
                }),
                _ => return Err(Error::InvalidKey(key.into())),
            },
        }
        match op {
            Transform::Insert(_) => {
                next.count = next
                    .count
                    .checked_add(1)
                    .ok_or_else(|| Error::InvalidKey("count overflow".into()))?
            }
            Transform::Delete => {
                next.count = next
                    .count
                    .checked_sub(1)
                    .ok_or_else(|| Error::InvalidKey("count underflow".into()))?
            }
            _ => {}
        }
        sort(&mut next.children);
        Ok(next)
    }
    /// Batch operations are transactional: failed operations leave the original untouched.
    pub fn transform(&self, ops: &BTreeMap<String, Transform<V>>) -> Result<Self, Error> {
        let mut result = self.clone();
        for (k, v) in ops {
            result = result.apply(k, v.clone())?;
        }
        Ok(result)
    }
    pub fn entries(&self) -> Result<BTreeMap<String, V>, Error> {
        let mut out = BTreeMap::new();
        for e in &self.children {
            collect(loaded(&e.value)?, "", &mut out)?
        }
        Ok(out)
    }
    pub fn sorted_entries(
        &self,
        after: Option<&str>,
        limit: usize,
    ) -> Result<Vec<(String, V)>, Error> {
        fn before(prefix: &str, after: Option<&str>) -> bool {
            after.is_some_and(|a| {
                let p: String = prefix.nfc().collect();
                let a: String = a.nfc().collect();
                p < a && !a.starts_with(&p)
            })
        }
        fn walk<V: Clone + Serialize>(
            children: &[Entry<Header<RadixNode<V>>>],
            prefix: &str,
            after: Option<&str>,
            limit: usize,
            out: &mut Vec<(String, V)>,
        ) -> Result<(), Error> {
            let mut sorted: Vec<_> = children.iter().collect();
            sorted.sort_by(|a, b| a.key.nfc().cmp(b.key.nfc()));
            for e in sorted {
                if out.len() >= limit {
                    break;
                }
                if before(&(prefix.to_owned() + &e.key), after) {
                    continue;
                }
                let node = loaded(&e.value)?;
                let path = prefix.to_owned() + &node.prefix;
                if let Some(value) = node
                    .value
                    .as_ref()
                    .filter(|_| after.is_none_or(|a| path.nfc().cmp(a.nfc()).is_gt()))
                {
                    out.push((path.clone(), value.clone()));
                }
                walk(&node.children, &path, after, limit, out)?;
            }
            Ok(())
        }
        let mut entries = Vec::new();
        walk(&self.children, "", after, limit, &mut entries)?;
        Ok(entries)
    }
    /// Serialize this boundary, excluding any application-owned leaf headers.
    pub fn serialize_volume(&self) -> Result<SerializedVolume, Error> {
        let root = Header::new(self.clone())?;
        let mut entries = BTreeMap::from([(root.raw_cid.clone(), root.bytes(None)?)]);
        for child in &self.children {
            store_node(&child.value, &mut entries)?
        }
        Ok(SerializedVolume::new(root.raw_cid, entries))
    }
    /// Membership/nonmembership sparse proof. Root and every decisive radix node remain loaded.
    pub fn proof(&self, key: &str) -> Result<Self, Error> {
        let mut proof = self.clone();
        for entry in &mut proof.children {
            entry.value = if !key.is_empty() && eq(&entry.key, first(key)?) {
                proof_node(&entry.value, key)?
            } else {
                entry.value.removing_node()
            };
        }
        Ok(proof)
    }
}
fn get_node<'a, V: Serialize>(
    node: &'a RadixNode<V>,
    key: &str,
    keys: &dyn crate::KeyProvider,
) -> Result<Option<&'a V>, Error> {
    let Some(remaining) = strip(key, &node.prefix) else {
        return Ok(None);
    };
    if remaining.is_empty() {
        return Ok(node.value.as_ref());
    }
    match node
        .children
        .iter()
        .find(|e| eq(&e.key, first(remaining).unwrap()))
    {
        Some(e) => get_node(loaded_with_keys(&e.value, keys)?, remaining, keys),
        None => Ok(None),
    }
}
fn mutate<V: Clone + Serialize>(
    node: &RadixNode<V>,
    key: &str,
    op: &Transform<V>,
    keys: &dyn crate::KeyProvider,
) -> Result<Option<RadixNode<V>>, Error> {
    let (shared, key_shared) = common(&node.prefix, key);
    if shared < node.prefix.len() {
        let Transform::Insert(value) = op else {
            return Err(Error::InvalidKey(key.into()));
        };
        let mut previous = node.clone();
        previous.prefix = previous.prefix[shared..].into();
        let child = Entry {
            key: first(&previous.prefix)?.into(),
            value: Header::new(previous)?,
        };
        let mut parent = RadixNode {
            prefix: key[..key_shared].into(),
            value: None,
            children: vec![child],
        };
        if key_shared == key.len() {
            parent.value = Some(value.clone())
        } else {
            let remaining = &key[key_shared..];
            parent.children.push(Entry {
                key: first(remaining)?.into(),
                value: Header::new(RadixNode {
                    prefix: remaining.into(),
                    value: Some(value.clone()),
                    children: vec![],
                })?,
            });
        }
        sort(&mut parent.children);
        return Ok(Some(parent));
    }
    let mut next = node.clone();
    let remaining = &key[key_shared..];
    if remaining.is_empty() {
        match op {
            Transform::Insert(v) => {
                if next.value.is_some() {
                    return Err(Error::InvalidKey(key.into()));
                }
                next.value = Some(v.clone())
            }
            Transform::Update(v) => {
                if next.value.is_none() {
                    return Err(Error::InvalidKey(key.into()));
                }
                next.value = Some(v.clone())
            }
            Transform::Delete => {
                if next.value.is_none() {
                    return Err(Error::InvalidKey(key.into()));
                }
                next.value = None
            }
        }
    } else {
        let leading = first(remaining)?;
        if let Some(i) = next.children.iter().position(|e| eq(&e.key, leading)) {
            match mutate(
                loaded_with_keys(&next.children[i].value, keys)?,
                remaining,
                op,
                keys,
            )? {
                Some(n) => next.children[i].value = rewrite(&next.children[i].value, n, keys)?,
                None => {
                    next.children.remove(i);
                }
            }
        } else {
            let Transform::Insert(value) = op else {
                return Err(Error::InvalidKey(key.into()));
            };
            next.children.push(Entry {
                key: leading.into(),
                value: Header::new(RadixNode {
                    prefix: remaining.into(),
                    value: Some(value.clone()),
                    children: vec![],
                })?,
            });
        }
    }
    if next.value.is_none() {
        if next.children.is_empty() {
            return Ok(None);
        }
        if next.children.len() == 1 {
            let child = loaded_with_keys(&next.children[0].value, keys)?;
            return Ok(Some(RadixNode {
                prefix: next.prefix + &child.prefix,
                value: child.value.clone(),
                children: child.children.clone(),
            }));
        }
    }
    sort(&mut next.children);
    Ok(Some(next))
}
fn collect<V: Clone + Serialize>(
    node: &RadixNode<V>,
    prefix: &str,
    out: &mut BTreeMap<String, V>,
) -> Result<(), Error> {
    let key = prefix.to_string() + &node.prefix;
    if let Some(v) = &node.value {
        out.insert(key.clone(), v.clone());
    }
    for child in &node.children {
        collect(loaded(&child.value)?, &key, out)?
    }
    Ok(())
}
fn store_node<V: Serialize>(
    h: &Header<RadixNode<V>>,
    out: &mut BTreeMap<String, Vec<u8>>,
) -> Result<(), Error> {
    out.insert(h.raw_cid.clone(), h.bytes(None)?);
    for e in &loaded(h)?.children {
        store_node(&e.value, out)?
    }
    Ok(())
}
fn proof_node<V: Clone + Serialize>(
    h: &Header<RadixNode<V>>,
    key: &str,
) -> Result<Header<RadixNode<V>>, Error> {
    let mut node = loaded(h)?.clone();
    let remaining = strip(key, &node.prefix).unwrap_or("");
    for entry in &mut node.children {
        entry.value = if !remaining.is_empty() && eq(&entry.key, first(remaining)?) {
            proof_node(&entry.value, remaining)?
        } else {
            entry.value.removing_node()
        };
    }
    let mut result = h.clone();
    result.node = Some(Arc::new(node));
    Ok(result)
}
impl<V: Clone + Serialize + DeserializeOwned> MerkleDictionary<V> {
    pub fn resolve_key(&self, key: &str, source: &impl ContentSource) -> Result<Self, Error> {
        let mut out = self.clone();
        if key.is_empty() {
            return Ok(out);
        }
        for e in &mut out.children {
            if eq(&e.key, first(key)?) {
                e.value = resolve_node(&e.value, Some(key), source)?
            }
        }
        Ok(out)
    }
    pub fn resolve_all(&self, source: &impl ContentSource) -> Result<Self, Error> {
        let mut out = self.clone();
        for e in &mut out.children {
            e.value = resolve_node(&e.value, None, source)?
        }
        Ok(out)
    }
}
fn resolve_node<V: Clone + Serialize + DeserializeOwned>(
    h: &Header<RadixNode<V>>,
    key: Option<&str>,
    source: &impl ContentSource,
) -> Result<Header<RadixNode<V>>, Error> {
    let mut out = h.resolve(source, None)?;
    let mut node = loaded(&out)?.clone();
    let remaining = key.and_then(|k| strip(k, &node.prefix));
    for e in &mut node.children {
        if key.is_none()
            || remaining.is_some_and(|r| !r.is_empty() && first(r).is_ok_and(|c| eq(c, &e.key)))
        {
            e.value = resolve_node(&e.value, remaining, source)?
        }
    }
    out.node = Some(Arc::new(node));
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_insert_and_delete() {
        let entries = [("cat", 1), ("car", 2), ("cab", 3), ("c", 4), ("dog", 5)];
        let mut a = MerkleDictionary::default();
        for (k, v) in entries {
            a = a.inserting(k, v).unwrap()
        }
        let mut b = MerkleDictionary::default();
        for (k, v) in entries.into_iter().rev() {
            b = b.inserting(k, v).unwrap()
        }
        assert_eq!(
            Header::new(a.clone()).unwrap().raw_cid,
            Header::new(b.clone()).unwrap().raw_cid
        );
        assert_eq!(a.count, 5);
        assert!(a.inserting("car", 9).is_err());
        assert!(a.deleting("ca").is_err());
        a = a
            .deleting("car")
            .unwrap()
            .deleting("cab")
            .unwrap()
            .deleting("c")
            .unwrap();
        let expected = MerkleDictionary::default()
            .inserting("cat", 1)
            .unwrap()
            .inserting("dog", 5)
            .unwrap();
        assert_eq!(
            Header::new(a).unwrap().raw_cid,
            Header::new(expected).unwrap().raw_cid
        );
    }
    #[test]
    fn sparse_proof_and_resolve() {
        let full = MerkleDictionary::default()
            .inserting("cat", 1)
            .unwrap()
            .inserting("car", 2)
            .unwrap()
            .inserting("dog", 3)
            .unwrap();
        let volume = full.serialize_volume().unwrap();
        volume.validate().unwrap();
        let proof = full.proof("cat").unwrap();
        assert_eq!(proof.get("cat").unwrap(), Some(&1));
        assert!(proof.get("dog").is_err());
        assert_eq!(
            Header::new(full.clone()).unwrap().raw_cid,
            Header::new(proof.clone()).unwrap().raw_cid
        );
        let resolved = proof.resolve_key("dog", &volume.entries).unwrap();
        assert_eq!(resolved.get("dog").unwrap(), Some(&3));
        let absent = full.proof("can").unwrap();
        assert!(absent.get("can").unwrap().is_none());
        assert!(full.inserting("", 0).is_err());
    }
}

type Children<V> = Vec<Entry<Header<RadixNode<V>>>>;
mod children_wire {
    use super::*;
    pub fn serialize<V, S: serde::Serializer>(
        v: &[Entry<Header<RadixNode<V>>>],
        s: S,
    ) -> Result<S::Ok, S::Error> {
        let mut sorted: Vec<_> = v.iter().collect();
        sorted.sort_by(|a, b| a.key.nfc().cmp(b.key.nfc()));
        sorted.serialize(s)
    }
    pub fn deserialize<'de, V, D: serde::Deserializer<'de>>(d: D) -> Result<Children<V>, D::Error> {
        let entries = Vec::<Entry<Header<RadixNode<V>>>>::deserialize(d)?;
        let mut result: Vec<Entry<Header<RadixNode<V>>>> = Vec::new();
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

impl<V: Clone + Serialize + DeserializeOwned> MerkleDictionary<V> {
    /// Produce a sparse write witness including every sibling needed for canonical collapse.
    /// Missing-node faults during a dry-run reveal the exact authenticated frontier needed.
    pub fn prove_transforms(
        &self,
        ops: &BTreeMap<String, Transform<V>>,
        source: &impl ContentSource,
    ) -> Result<Self, Error> {
        let mut witness = self.clone();
        for e in &mut witness.children {
            e.value = e.value.removing_node()
        }
        loop {
            match witness.transform(ops) {
                Ok(_) => return Ok(witness),
                Err(Error::MissingContent(cid)) => {
                    let mut found = false;
                    for e in &mut witness.children {
                        found |= hydrate_cid(&mut e.value, &cid, source)?
                    }
                    if !found {
                        return Err(Error::MissingContent(cid));
                    }
                }
                Err(e) => return Err(e),
            }
        }
    }
}
fn hydrate_cid<V: Clone + Serialize + DeserializeOwned>(
    h: &mut Header<RadixNode<V>>,
    cid: &str,
    source: &impl ContentSource,
) -> Result<bool, Error> {
    if h.raw_cid == cid && h.node.is_none() {
        *h = h.resolve(source, None)?;
        return Ok(true);
    }
    if let Some(node) = &h.node {
        let mut n = node.as_ref().clone();
        let mut found = false;
        for e in &mut n.children {
            found |= hydrate_cid(&mut e.value, cid, source)?
        }
        if found {
            h.node = Some(Arc::new(n))
        }
        return Ok(found);
    }
    Ok(false)
}
#[cfg(test)]
mod write_witness_tests {
    use super::*;
    #[test]
    fn batched_deletion_collapse_witness() {
        let t = MerkleDictionary::default()
            .inserting("ab", 1)
            .unwrap()
            .inserting("ac", 2)
            .unwrap()
            .inserting("ad", 3)
            .unwrap()
            .inserting("unrelated", 4)
            .unwrap();
        let volume = t.serialize_volume().unwrap();
        let ops = BTreeMap::from([
            ("ac".into(), Transform::Delete),
            ("ad".into(), Transform::Delete),
        ]);
        let proof = t.prove_transforms(&ops, &volume.entries).unwrap();
        assert!(proof.get("unrelated").is_err());
        assert_eq!(
            Header::new(proof.clone()).unwrap().raw_cid,
            Header::new(t.clone()).unwrap().raw_cid
        );
        assert_eq!(
            Header::new(proof.transform(&ops).unwrap()).unwrap().raw_cid,
            Header::new(t.transform(&ops).unwrap()).unwrap().raw_cid
        );
    }
}

impl<V: Clone + Serialize> MerkleDictionary<V> {
    /// Export available witness bytes. This sparse map is deliberately not a SerializedVolume.
    pub fn sparse_entries(&self) -> Result<BTreeMap<String, Vec<u8>>, Error> {
        fn collect<V: Serialize>(
            h: &Header<RadixNode<V>>,
            out: &mut BTreeMap<String, Vec<u8>>,
        ) -> Result<(), Error> {
            if let Some(node) = &h.node {
                out.insert(h.raw_cid.clone(), h.bytes(None)?);
                for e in &node.children {
                    collect(&e.value, out)?
                }
            }
            Ok(())
        }
        let root = Header::new(self.clone())?;
        let mut out = BTreeMap::from([(root.raw_cid.clone(), root.bytes(None)?)]);
        for e in &self.children {
            collect(&e.value, &mut out)?
        }
        Ok(out)
    }
}
impl<V: Clone + Serialize + DeserializeOwned> MerkleDictionary<V> {
    /// Authenticate a supplied sparse witness against a trusted root and read a key.
    pub fn verify_read(
        root: &str,
        key: &str,
        entries: &BTreeMap<String, Vec<u8>>,
    ) -> Result<Option<V>, Error> {
        for (cid, bytes) in entries {
            crate::validate_entry(cid, bytes)?
        }
        let header = Header::<Self>::from_cid(root)?.resolve(entries, None)?;
        let dictionary = header
            .node
            .as_deref()
            .ok_or_else(|| Error::MissingContent(root.into()))?
            .resolve_key(key, entries)?;
        Ok(dictionary.get(key)?.cloned())
    }
}

impl<V: Clone + Serialize> MerkleDictionary<V> {
    pub fn transform_with_keys(
        &self,
        ops: &BTreeMap<String, Transform<V>>,
        keys: &dyn crate::KeyProvider,
    ) -> Result<Self, Error> {
        let mut result = self.clone();
        for (k, v) in ops {
            result = result.apply_with_keys(k, v.clone(), keys)?
        }
        Ok(result)
    }
    /// Encrypt radix structure selected by full key prefixes. Longer prefix overrides win.
    /// Leaf Header values are independently encrypted through `map_values_with_keys`.
    pub fn encrypt_prefixes(
        &self,
        plan: &BTreeMap<String, [u8; 32]>,
        keys: &dyn crate::KeyProvider,
    ) -> Result<Self, Error> {
        fn walk<V: Clone + Serialize>(
            h: &Header<RadixNode<V>>,
            prefix: &str,
            plan: &BTreeMap<String, [u8; 32]>,
            keys: &dyn crate::KeyProvider,
        ) -> Result<Header<RadixNode<V>>, Error> {
            let mut node = loaded_with_keys(h, keys)?.clone();
            let path = prefix.to_string() + &node.prefix;
            let selected = plan
                .iter()
                .filter(|(p, _)| strip(&path, p).is_some())
                .max_by_key(|(p, _)| p.graphemes(true).count())
                .map(|(_, k)| k);
            let mut changed = false;
            for e in &mut node.children {
                let old = e.value.raw_cid.clone();
                e.value = walk(&e.value, &path, plan, keys)?;
                changed |= old != e.value.raw_cid;
            }
            if let Some(key) = selected {
                Header::encrypted(node, key)
            } else if changed {
                rewrite(h, node, keys)
            } else {
                Ok(h.clone())
            }
        }
        let mut out = self.clone();
        for e in &mut out.children {
            e.value = walk(&e.value, "", plan, keys)?
        }
        Ok(out)
    }
    /// Transform nested leaf values while retaining encryption on every rewritten radix node.
    pub fn map_values_with_keys(
        &self,
        keys: &dyn crate::KeyProvider,
        mut transform: impl FnMut(&str, &V) -> Result<V, Error>,
    ) -> Result<Self, Error> {
        fn walk<V: Clone + Serialize, F: FnMut(&str, &V) -> Result<V, Error>>(
            h: &Header<RadixNode<V>>,
            prefix: &str,
            keys: &dyn crate::KeyProvider,
            f: &mut F,
        ) -> Result<Header<RadixNode<V>>, Error> {
            let mut node = loaded_with_keys(h, keys)?.clone();
            let path = prefix.to_string() + &node.prefix;
            if let Some(v) = &node.value {
                node.value = Some(f(&path, v)?)
            }
            for e in &mut node.children {
                e.value = walk(&e.value, &path, keys, f)?
            }
            if crate::cbor::encode(&node)? == crate::cbor::encode(loaded_with_keys(h, keys)?)? {
                let mut out = h.clone();
                out.node = Some(Arc::new(node));
                Ok(out)
            } else {
                rewrite(h, node, keys)
            }
        }
        let mut out = self.clone();
        for e in &mut out.children {
            e.value = walk(&e.value, "", keys, &mut transform)?
        }
        Ok(out)
    }
}
impl<V: Clone + Serialize + DeserializeOwned> MerkleDictionary<V> {
    pub fn resolve_all_with_keys(
        &self,
        source: &impl ContentSource,
        keys: &dyn crate::KeyProvider,
    ) -> Result<Self, Error> {
        fn walk<V: Clone + Serialize + DeserializeOwned>(
            h: &Header<RadixNode<V>>,
            source: &impl ContentSource,
            keys: &dyn crate::KeyProvider,
        ) -> Result<Header<RadixNode<V>>, Error> {
            let key = h
                .encryption_info
                .as_ref()
                .and_then(|i| keys.key(&i.key_hash));
            let mut out = h.resolve(source, key.as_ref())?;
            let mut node = loaded_with_keys(&out, keys)?.clone();
            for e in &mut node.children {
                e.value = walk(&e.value, source, keys)?
            }
            out.node = Some(Arc::new(node));
            Ok(out)
        }
        let mut out = self.clone();
        for e in &mut out.children {
            e.value = walk(&e.value, source, keys)?
        }
        Ok(out)
    }
}
#[cfg(test)]
mod encrypted_transform_tests {
    use super::*;
    #[test]
    fn mutations_preserve_encrypted_lineage_and_require_keys() {
        let plain = MerkleDictionary::default()
            .inserting("cat", 1u64)
            .unwrap()
            .inserting("car", 2)
            .unwrap()
            .inserting("dog", 3)
            .unwrap();
        let key = [7; 32];
        let hash = crate::EncryptionInfo::new(&key, &[0; 12]).key_hash;
        let keys = BTreeMap::from([(hash, key)]);
        let encrypted = plain
            .encrypt_prefixes(&BTreeMap::from([("c".into(), key)]), &())
            .unwrap();
        assert!(encrypted.children[0].value.encryption_info.is_some());
        assert!(encrypted.children[1].value.encryption_info.is_none());
        assert!(encrypted.get("cat").is_err());
        assert_eq!(encrypted.get_with_keys("cat", &keys).unwrap(), Some(&1));
        assert!(encrypted.updating("cat", 9).is_err());
        let changed = encrypted
            .apply_with_keys("cat", Transform::Update(9), &keys)
            .unwrap();
        assert!(changed.children[0].value.encryption_info.is_some());
        assert_eq!(changed.get_with_keys("cat", &keys).unwrap(), Some(&9));
        let h = crate::Volume::new(changed.clone()).unwrap();
        let stored = crate::volume::serialize_recursive(&h, &keys).unwrap();
        let bytes: BTreeMap<_, _> = stored.into_iter().flat_map(|v| v.entries).collect();
        let lazy: MerkleDictionary<u64> =
            crate::cbor::decode(&crate::cbor::encode(&changed).unwrap()).unwrap();
        assert_eq!(
            lazy.resolve_all_with_keys(&bytes, &keys)
                .unwrap()
                .get_with_keys("cat", &keys)
                .unwrap(),
            Some(&9)
        );
    }
}

impl<V: Clone + Serialize> MerkleDictionary<V> {
    /// Replace only materialized values, proving that the authenticated wire representation is unchanged.
    /// Missing branches remain missing; this is intended for hydrating nested header pointers.
    pub fn map_values_preserving_commitments(
        &self,
        keys: &dyn crate::KeyProvider,
        mut f: impl FnMut(&str, &V) -> Result<V, Error>,
    ) -> Result<Self, Error> {
        fn walk<V: Clone + Serialize, F: FnMut(&str, &V) -> Result<V, Error>>(
            h: &Header<RadixNode<V>>,
            prefix: &str,
            keys: &dyn crate::KeyProvider,
            f: &mut F,
        ) -> Result<Header<RadixNode<V>>, Error> {
            if h.node.is_none() {
                return Ok(h.clone());
            }
            let mut node = loaded_with_keys(h, keys)?.clone();
            let path = prefix.to_owned() + &node.prefix;
            if let Some(v) = &node.value {
                node.value = Some(f(&path, v)?)
            }
            for e in &mut node.children {
                e.value = walk(&e.value, &path, keys, f)?
            }
            let mut out = h.clone();
            out.node = Some(Arc::new(node));
            loaded_with_keys(&out, keys)?;
            Ok(out)
        }
        let mut out = self.clone();
        for e in &mut out.children {
            e.value = walk(&e.value, "", keys, &mut f)?
        }
        Ok(out)
    }
}
impl<V: Clone + Serialize + DeserializeOwned> MerkleDictionary<V> {
    pub fn resolve_key_with_keys(
        &self,
        key: &str,
        source: &impl ContentSource,
        keys: &dyn crate::KeyProvider,
    ) -> Result<Self, Error> {
        fn walk<V: Clone + Serialize + DeserializeOwned>(
            h: &Header<RadixNode<V>>,
            path: &str,
            source: &impl ContentSource,
            keys: &dyn crate::KeyProvider,
        ) -> Result<Header<RadixNode<V>>, Error> {
            let key = h
                .encryption_info
                .as_ref()
                .and_then(|i| keys.key(&i.key_hash));
            let mut out = h.resolve(source, key.as_ref())?;
            let mut node = loaded_with_keys(&out, keys)?.clone();
            if let Some(rest) = strip(path, &node.prefix) {
                if !rest.is_empty() {
                    for e in &mut node.children {
                        if eq(&e.key, first(rest)?) {
                            e.value = walk(&e.value, rest, source, keys)?
                        }
                    }
                }
            }
            out.node = Some(Arc::new(node));
            Ok(out)
        }
        let mut out = self.clone();
        if key.is_empty() {
            return Ok(out);
        }
        for e in &mut out.children {
            if eq(&e.key, first(key)?) {
                e.value = walk(&e.value, key, source, keys)?
            }
        }
        Ok(out)
    }
}
impl<V: Clone + Serialize + DeserializeOwned> MerkleDictionary<V> {
    /// Hydrate only the sorted cursor window. Nested header values remain lazy.
    pub fn resolve_range_with_keys(
        &self,
        after: Option<&str>,
        limit: usize,
        source: &impl ContentSource,
        keys: &dyn crate::KeyProvider,
    ) -> Result<Self, Error> {
        fn before(prefix: &str, after: Option<&str>) -> bool {
            after.is_some_and(|a| {
                let p: String = prefix.nfc().collect();
                let a: String = a.nfc().collect();
                p < a && !a.starts_with(&p)
            })
        }
        fn walk<V: Clone + Serialize + DeserializeOwned>(
            h: &Header<RadixNode<V>>,
            prefix: &str,
            after: Option<&str>,
            left: &mut usize,
            source: &impl ContentSource,
            keys: &dyn crate::KeyProvider,
        ) -> Result<Header<RadixNode<V>>, Error> {
            if *left == 0 {
                return Ok(h.clone());
            }
            let key = h
                .encryption_info
                .as_ref()
                .and_then(|i| keys.key(&i.key_hash));
            let mut out = h.resolve(source, key.as_ref())?;
            let mut node = loaded_with_keys(&out, keys)?.clone();
            let path = prefix.to_owned() + &node.prefix;
            if node.value.is_some()
                && after
                    .is_none_or(|a| path.nfc().collect::<String>() > a.nfc().collect::<String>())
            {
                *left -= 1;
            }
            sort(&mut node.children);
            for e in &mut node.children {
                if *left == 0 {
                    break;
                }
                if !before(&(path.clone() + &e.key), after) {
                    e.value = walk(&e.value, &path, after, left, source, keys)?
                }
            }
            out.node = Some(Arc::new(node));
            Ok(out)
        }
        let mut out = self.clone();
        let mut left = limit;
        sort(&mut out.children);
        for e in &mut out.children {
            if left == 0 {
                break;
            }
            if !before(&e.key, after) {
                e.value = walk(&e.value, "", after, &mut left, source, keys)?
            }
        }
        Ok(out)
    }
    /// Include authenticated collapse siblings for a keyed batch without exposing unrelated branches.
    pub fn prove_transforms_with_keys(
        &self,
        ops: &BTreeMap<String, Transform<V>>,
        source: &impl ContentSource,
        keys: &dyn crate::KeyProvider,
    ) -> Result<Self, Error> {
        fn hydrate<V: Clone + Serialize + DeserializeOwned>(
            h: &mut Header<RadixNode<V>>,
            cid: &str,
            source: &impl ContentSource,
            keys: &dyn crate::KeyProvider,
        ) -> Result<bool, Error> {
            if h.raw_cid == cid && h.node.is_none() {
                let key = h
                    .encryption_info
                    .as_ref()
                    .and_then(|i| keys.key(&i.key_hash));
                *h = h.resolve(source, key.as_ref())?;
                return Ok(true);
            }
            if h.node.is_none() {
                return Ok(false);
            }
            let mut node = loaded_with_keys(h, keys)?.clone();
            let mut found = false;
            for e in &mut node.children {
                found |= hydrate(&mut e.value, cid, source, keys)?
            }
            if found {
                h.node = Some(Arc::new(node))
            }
            Ok(found)
        }
        let mut out = self.clone();
        for e in &mut out.children {
            e.value = e.value.removing_node()
        }
        loop {
            match out.transform_with_keys(ops, keys) {
                Ok(_) => return Ok(out),
                Err(Error::MissingContent(cid)) => {
                    let mut found = false;
                    for e in &mut out.children {
                        found |= hydrate(&mut e.value, &cid, source, keys)?
                    }
                    if !found {
                        return Err(Error::MissingContent(cid));
                    }
                }
                Err(e) => return Err(e),
            }
        }
    }
}
impl<V: Clone + Serialize> MerkleDictionary<V> {
    pub fn entries_with_keys(
        &self,
        keys: &dyn crate::KeyProvider,
    ) -> Result<BTreeMap<String, V>, Error> {
        fn collect<V: Clone + Serialize>(
            h: &Header<RadixNode<V>>,
            prefix: &str,
            keys: &dyn crate::KeyProvider,
            out: &mut BTreeMap<String, V>,
        ) -> Result<(), Error> {
            let node = loaded_with_keys(h, keys)?;
            let path = prefix.to_owned() + &node.prefix;
            if let Some(v) = &node.value {
                out.insert(path.clone(), v.clone());
            }
            for e in &node.children {
                collect(&e.value, &path, keys, out)?
            }
            Ok(())
        }
        let mut out = BTreeMap::new();
        for e in &self.children {
            collect(&e.value, "", keys, &mut out)?
        }
        Ok(out)
    }
}
