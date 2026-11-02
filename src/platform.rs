//! Checked heterogeneous node traversal analogous to Swift Node's named properties.
use crate::{
    ArrayTrie, ContentSource, DagHeader, DagNode, Error, Header, KeyProvider, Transform, Volume,
};
use serde::de::DeserializeOwned;
use std::{any::Any, collections::BTreeMap, sync::Arc};
pub type NamedHeaders = BTreeMap<String, Box<dyn ErasedHeader>>;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolutionStrategy {
    Targeted,
    Recursive,
    List,
    Range { after: Option<String>, limit: usize },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EncryptionStrategy {
    Targeted([u8; 32]),
    List([u8; 32]),
    Recursive([u8; 32]),
}
impl EncryptionStrategy {
    pub fn key(&self) -> &[u8; 32] {
        match self {
            Self::Targeted(k) | Self::List(k) | Self::Recursive(k) => k,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageStrategy {
    Targeted,
    Recursive,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProofKind {
    Insertion,
    Mutation,
    Deletion,
    Existence,
}
pub trait ErasedHeader: DagHeader + Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn clone_header(&self) -> Box<dyn ErasedHeader>;
    fn is_materialized(&self) -> bool;
    fn without_node(&self) -> Box<dyn ErasedHeader>;
    fn encryption_info(&self) -> Option<&crate::EncryptionInfo>;
    fn named_children(&self, keys: &dyn KeyProvider) -> Result<NamedHeaders, Error>;
    fn resolve_paths(
        &self,
        plan: &ArrayTrie<ResolutionStrategy>,
        source: &dyn ContentSource,
        keys: &dyn KeyProvider,
    ) -> Result<Box<dyn ErasedHeader>, Error>;
    fn transform_paths(
        &self,
        plan: &ArrayTrie<Transform<String>>,
        keys: &dyn KeyProvider,
    ) -> Result<Option<Box<dyn ErasedHeader>>, Error>;
    fn encrypt_paths(
        &self,
        plan: &ArrayTrie<EncryptionStrategy>,
        keys: &dyn KeyProvider,
    ) -> Result<Box<dyn ErasedHeader>, Error>;
    fn storage_paths(
        &self,
        plan: &ArrayTrie<StorageStrategy>,
        keys: &dyn KeyProvider,
    ) -> Result<Vec<crate::SerializedVolume>, Error>;
    fn proof_paths(
        &self,
        plan: &ArrayTrie<ProofKind>,
        source: &dyn ContentSource,
        keys: &dyn KeyProvider,
    ) -> Result<Box<dyn ErasedHeader>, Error>;
}
impl Clone for Box<dyn ErasedHeader> {
    fn clone(&self) -> Self {
        self.clone_header()
    }
}
/// Implement child enumeration and checked rebuilding for a structured node.
/// Leaf nodes can use the defaults; no unchecked downcasts or silently ignored children occur.
pub trait PlatformNode: DagNode + Clone + DeserializeOwned + Send + Sync + 'static {
    fn named_children(&self) -> Result<NamedHeaders, Error> {
        Ok(BTreeMap::new())
    }
    fn named_children_with_keys(&self, _keys: &dyn KeyProvider) -> Result<NamedHeaders, Error> {
        self.named_children()
    }
    fn with_named_children(&self, children: NamedHeaders) -> Result<Self, Error> {
        if children.is_empty() {
            Ok(self.clone())
        } else {
            Err(Error::TypeMismatch("leaf has no children".into()))
        }
    }
    fn resolve_paths(
        &self,
        plan: &ArrayTrie<ResolutionStrategy>,
        source: &dyn ContentSource,
        keys: &dyn KeyProvider,
    ) -> Result<Self, Error> {
        let mut children = self.named_children()?;
        for (name, child) in &mut children {
            let selected = if matches!(plan.value, Some(ResolutionStrategy::Recursive)) {
                let mut p = ArrayTrie::new();
                p.value = Some(ResolutionStrategy::Recursive);
                Some(p)
            } else {
                plan.children.get(name).cloned()
            };
            if let Some(p) = selected {
                *child = child.resolve_paths(&p, source, keys)?;
            }
        }
        self.with_named_children(children)
    }
    fn transform_paths(
        &self,
        plan: &ArrayTrie<Transform<String>>,
        keys: &dyn KeyProvider,
    ) -> Result<Self, Error> {
        let node = if let Some(Transform::Update(json)) = &plan.value {
            serde_json::from_str(json).map_err(|e| Error::Cbor(e.to_string()))?
        } else {
            self.clone()
        };
        let mut children = node.named_children()?;
        for (name, p) in &plan.children {
            let child = children
                .get(name)
                .ok_or_else(|| Error::InvalidKey(name.clone()))?;
            let new = child
                .transform_paths(p, keys)?
                .ok_or_else(|| Error::InvalidKey(format!("cannot delete fixed property {name}")))?;
            children.insert(name.clone(), new);
        }
        node.with_named_children(children)
    }
    fn encrypt_paths(
        &self,
        plan: &ArrayTrie<EncryptionStrategy>,
        keys: &dyn KeyProvider,
    ) -> Result<Self, Error> {
        let mut children = self.named_children()?;
        for (name, child) in &mut children {
            let selected = plan
                .children
                .get(name)
                .cloned()
                .or_else(|| match &plan.value {
                    Some(EncryptionStrategy::Recursive(k)) => Some(ArrayTrie {
                        value: Some(EncryptionStrategy::Recursive(*k)),
                        children: BTreeMap::new(),
                    }),
                    Some(EncryptionStrategy::List(k)) => Some(ArrayTrie {
                        value: Some(EncryptionStrategy::Targeted(*k)),
                        children: BTreeMap::new(),
                    }),
                    _ => None,
                });
            if let Some(p) = selected {
                *child = child.encrypt_paths(&p, keys)?;
            }
        }
        self.with_named_children(children)
    }
    fn storage_paths(
        &self,
        plan: &ArrayTrie<StorageStrategy>,
        keys: &dyn KeyProvider,
    ) -> Result<Vec<crate::SerializedVolume>, Error> {
        let children = self.named_children()?;
        let mut out = vec![];
        for (name, p) in &plan.children {
            if let Some(h) = children.get(name) {
                out.extend(h.storage_paths(p, keys)?);
            }
        }
        Ok(out)
    }
    fn proof_paths(
        &self,
        plan: &ArrayTrie<ProofKind>,
        source: &dyn ContentSource,
        keys: &dyn KeyProvider,
    ) -> Result<Self, Error> {
        let mut children = self.named_children()?;
        for (name, p) in &plan.children {
            if !children.contains_key(name)
                && p.all_values()
                    .iter()
                    .any(|k| matches!(k, ProofKind::Mutation | ProofKind::Deletion))
            {
                return Err(Error::InvalidKey(name.clone()));
            }
        }
        for (name, child) in &mut children {
            if let Some(p) = plan.children.get(name) {
                if p.value == Some(ProofKind::Insertion) {
                    return Err(Error::InvalidKey(format!("existing property {name}")));
                }
                *child = child.proof_paths(p, source, keys)?
            } else {
                *child = child.without_node()
            }
        }
        self.with_named_children(children)
    }
}
macro_rules! leaf {($($t:ty),*)=>{$(impl PlatformNode for $t{})*}}
leaf!(
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
struct SourceRef<'a>(&'a dyn ContentSource);
impl ContentSource for SourceRef<'_> {
    fn fetch(&self, cid: &str) -> Result<Vec<u8>, Error> {
        self.0.fetch(cid)
    }
}
fn materialized<'a, T: PlatformNode>(
    h: &'a Header<T>,
    keys: &dyn KeyProvider,
) -> Result<&'a T, Error> {
    let key = h
        .encryption_info
        .as_ref()
        .and_then(|i| keys.key(&i.key_hash));
    h.bytes(key.as_ref())?;
    h.node
        .as_deref()
        .ok_or_else(|| Error::MissingContent(h.raw_cid.clone()))
}
fn wrap<T: PlatformNode>(h: Header<T>, volume: bool) -> Box<dyn ErasedHeader> {
    if volume {
        Box::new(Volume(h))
    } else {
        Box::new(h)
    }
}
fn replacing<T: PlatformNode>(
    old: &Header<T>,
    node: T,
    keys: &dyn KeyProvider,
) -> Result<Header<T>, Error> {
    if let Some(info) = &old.encryption_info {
        Header::encrypted(node, &keys.key(&info.key_hash).ok_or(Error::Encryption)?)
    } else {
        Header::new(node)
    }
}
fn resolve<T: PlatformNode>(
    h: &Header<T>,
    volume: bool,
    plan: &ArrayTrie<ResolutionStrategy>,
    source: &dyn ContentSource,
    keys: &dyn KeyProvider,
) -> Result<Box<dyn ErasedHeader>, Error> {
    if plan.is_empty() {
        return Ok(wrap(h.clone(), volume));
    }
    let key = h
        .encryption_info
        .as_ref()
        .and_then(|i| keys.key(&i.key_hash));
    let mut h = h.resolve(&SourceRef(source), key.as_ref())?;
    let node = materialized(&h, keys)?.resolve_paths(plan, source, keys)?;
    h.node = Some(Arc::new(node));
    materialized(&h, keys)?;
    Ok(wrap(h, volume))
}
fn transform<T: PlatformNode>(
    h: &Header<T>,
    volume: bool,
    plan: &ArrayTrie<Transform<String>>,
    keys: &dyn KeyProvider,
) -> Result<Option<Box<dyn ErasedHeader>>, Error> {
    if plan.is_empty() {
        return Ok(Some(wrap(h.clone(), volume)));
    }
    let node = materialized(h, keys)?.transform_paths(plan, keys)?;
    Ok(Some(wrap(replacing(h, node, keys)?, volume)))
}
fn encrypt<T: PlatformNode>(
    h: &Header<T>,
    volume: bool,
    plan: &ArrayTrie<EncryptionStrategy>,
    keys: &dyn KeyProvider,
) -> Result<Box<dyn ErasedHeader>, Error> {
    if plan.is_empty() {
        return Ok(wrap(h.clone(), volume));
    }
    let node = materialized(h, keys)?.encrypt_paths(plan, keys)?;
    let h = if let Some(strategy) = &plan.value {
        Header::encrypted(node, strategy.key())?
    } else {
        replacing(h, node, keys)?
    };
    Ok(wrap(h, volume))
}
fn storage<T: PlatformNode>(
    h: &Header<T>,
    volume: bool,
    p: &ArrayTrie<StorageStrategy>,
    keys: &dyn KeyProvider,
) -> Result<Vec<crate::SerializedVolume>, Error> {
    if p.value == Some(StorageStrategy::Recursive) {
        let mut out = crate::volume::serialize_recursive(h, keys)?;
        if !volume {
            out.remove(0);
        }
        return Ok(out);
    }
    let mut out = if volume {
        vec![crate::volume::serialize_boundary(h, keys)?]
    } else {
        vec![]
    };
    out.extend(materialized(h, keys)?.storage_paths(p, keys)?);
    Ok(out)
}
fn proof<T: PlatformNode>(
    h: &Header<T>,
    volume: bool,
    plan: &ArrayTrie<ProofKind>,
    source: &dyn ContentSource,
    keys: &dyn KeyProvider,
) -> Result<Box<dyn ErasedHeader>, Error> {
    if plan.is_empty() {
        return Ok(wrap(h.clone(), volume));
    }
    let key = h
        .encryption_info
        .as_ref()
        .and_then(|i| keys.key(&i.key_hash));
    let mut h = h.resolve(&SourceRef(source), key.as_ref())?;
    let node = materialized(&h, keys)?.proof_paths(plan, source, keys)?;
    h.node = Some(Arc::new(node));
    materialized(&h, keys)?;
    Ok(wrap(h, volume))
}
impl<T: PlatformNode> ErasedHeader for Header<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn clone_header(&self) -> Box<dyn ErasedHeader> {
        Box::new(self.clone())
    }
    fn is_materialized(&self) -> bool {
        self.node.is_some()
    }
    fn without_node(&self) -> Box<dyn ErasedHeader> {
        Box::new(self.removing_node())
    }
    fn encryption_info(&self) -> Option<&crate::EncryptionInfo> {
        self.encryption_info.as_ref()
    }
    fn named_children(&self, keys: &dyn KeyProvider) -> Result<NamedHeaders, Error> {
        materialized(self, keys)?.named_children_with_keys(keys)
    }
    fn resolve_paths(
        &self,
        p: &ArrayTrie<ResolutionStrategy>,
        s: &dyn ContentSource,
        k: &dyn KeyProvider,
    ) -> Result<Box<dyn ErasedHeader>, Error> {
        resolve(self, false, p, s, k)
    }
    fn transform_paths(
        &self,
        p: &ArrayTrie<Transform<String>>,
        k: &dyn KeyProvider,
    ) -> Result<Option<Box<dyn ErasedHeader>>, Error> {
        transform(self, false, p, k)
    }
    fn encrypt_paths(
        &self,
        p: &ArrayTrie<EncryptionStrategy>,
        k: &dyn KeyProvider,
    ) -> Result<Box<dyn ErasedHeader>, Error> {
        encrypt(self, false, p, k)
    }
    fn storage_paths(
        &self,
        p: &ArrayTrie<StorageStrategy>,
        k: &dyn KeyProvider,
    ) -> Result<Vec<crate::SerializedVolume>, Error> {
        storage(self, false, p, k)
    }
    fn proof_paths(
        &self,
        p: &ArrayTrie<ProofKind>,
        s: &dyn ContentSource,
        k: &dyn KeyProvider,
    ) -> Result<Box<dyn ErasedHeader>, Error> {
        proof(self, false, p, s, k)
    }
}
impl<T: PlatformNode> ErasedHeader for Volume<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn clone_header(&self) -> Box<dyn ErasedHeader> {
        Box::new(self.clone())
    }
    fn is_materialized(&self) -> bool {
        self.0.node.is_some()
    }
    fn without_node(&self) -> Box<dyn ErasedHeader> {
        Box::new(self.removing_node())
    }
    fn encryption_info(&self) -> Option<&crate::EncryptionInfo> {
        self.0.encryption_info.as_ref()
    }
    fn named_children(&self, k: &dyn KeyProvider) -> Result<NamedHeaders, Error> {
        materialized(&self.0, k)?.named_children_with_keys(k)
    }
    fn resolve_paths(
        &self,
        p: &ArrayTrie<ResolutionStrategy>,
        s: &dyn ContentSource,
        k: &dyn KeyProvider,
    ) -> Result<Box<dyn ErasedHeader>, Error> {
        resolve(&self.0, true, p, s, k)
    }
    fn transform_paths(
        &self,
        p: &ArrayTrie<Transform<String>>,
        k: &dyn KeyProvider,
    ) -> Result<Option<Box<dyn ErasedHeader>>, Error> {
        transform(&self.0, true, p, k)
    }
    fn encrypt_paths(
        &self,
        p: &ArrayTrie<EncryptionStrategy>,
        k: &dyn KeyProvider,
    ) -> Result<Box<dyn ErasedHeader>, Error> {
        encrypt(&self.0, true, p, k)
    }
    fn storage_paths(
        &self,
        p: &ArrayTrie<StorageStrategy>,
        k: &dyn KeyProvider,
    ) -> Result<Vec<crate::SerializedVolume>, Error> {
        storage(&self.0, true, p, k)
    }
    fn proof_paths(
        &self,
        p: &ArrayTrie<ProofKind>,
        s: &dyn ContentSource,
        k: &dyn KeyProvider,
    ) -> Result<Box<dyn ErasedHeader>, Error> {
        proof(&self.0, true, p, s, k)
    }
}
pub fn expect_header<T: PlatformNode>(h: &dyn ErasedHeader) -> Result<Header<T>, Error> {
    h.as_any()
        .downcast_ref::<Header<T>>()
        .cloned()
        .ok_or_else(|| Error::TypeMismatch(std::any::type_name::<Header<T>>().into()))
}
pub fn expect_volume<T: PlatformNode>(h: &dyn ErasedHeader) -> Result<Volume<T>, Error> {
    h.as_any()
        .downcast_ref::<Volume<T>>()
        .cloned()
        .ok_or_else(|| Error::TypeMismatch(std::any::type_name::<Volume<T>>().into()))
}

/// Explicit value extension point: scalar values have no named link; typed links do.
pub trait PlatformValue:
    DagNode + Clone + DeserializeOwned + Send + Sync + std::str::FromStr + ToString + 'static
{
    fn erased(&self) -> Option<Box<dyn ErasedHeader>> {
        None
    }
    fn from_erased(_header: &dyn ErasedHeader) -> Result<Self, Error> {
        Err(Error::TypeMismatch("scalar is not a header".into()))
    }
}
macro_rules! values {($($t:ty),*)=>{$(impl PlatformValue for $t{})*}}
values!(String, bool, u8, u16, u32, u64, i8, i16, i32, i64, f32, f64);
impl<T: PlatformNode> PlatformValue for Header<T> {
    fn erased(&self) -> Option<Box<dyn ErasedHeader>> {
        Some(Box::new(self.clone()))
    }
    fn from_erased(h: &dyn ErasedHeader) -> Result<Self, Error> {
        expect_header(h)
    }
}
impl<T: PlatformNode> PlatformValue for Volume<T> {
    fn erased(&self) -> Option<Box<dyn ErasedHeader>> {
        Some(Box::new(self.clone()))
    }
    fn from_erased(h: &dyn ErasedHeader) -> Result<Self, Error> {
        expect_volume(h)
    }
}
impl<V: PlatformValue> PlatformNode for BTreeMap<String, V> {
    fn named_children(&self) -> Result<NamedHeaders, Error> {
        Ok(self
            .iter()
            .filter_map(|(k, v)| v.erased().map(|h| (k.clone(), h)))
            .collect())
    }
    fn with_named_children(&self, children: NamedHeaders) -> Result<Self, Error> {
        let mut out = self.clone();
        for (k, h) in children {
            if !out.contains_key(&k) {
                return Err(Error::InvalidKey(k));
            }
            out.insert(k, V::from_erased(h.as_ref())?);
        }
        Ok(out)
    }
}
impl<V: PlatformValue> PlatformNode for crate::MerkleDictionary<V> {
    fn named_children(&self) -> Result<NamedHeaders, Error> {
        Ok(self
            .entries()?
            .into_iter()
            .filter_map(|(k, v)| v.erased().map(|h| (k, h)))
            .collect())
    }
    fn named_children_with_keys(&self, keys: &dyn KeyProvider) -> Result<NamedHeaders, Error> {
        Ok(self
            .entries_with_keys(keys)?
            .into_iter()
            .filter_map(|(k, v)| v.erased().map(|h| (k, h)))
            .collect())
    }
    fn with_named_children(&self, children: NamedHeaders) -> Result<Self, Error> {
        self.map_values_with_keys(&(), |k, v| match children.get(k) {
            Some(h) => V::from_erased(h.as_ref()),
            None => Ok(v.clone()),
        })
    }
    fn resolve_paths(
        &self,
        plan: &ArrayTrie<ResolutionStrategy>,
        source: &dyn ContentSource,
        keys: &dyn KeyProvider,
    ) -> Result<Self, Error> {
        let mut out = self.clone();
        match &plan.value {
            Some(ResolutionStrategy::Recursive | ResolutionStrategy::List) => {
                out = out.resolve_all_with_keys(&SourceRef(source), keys)?
            }
            Some(ResolutionStrategy::Range { after, limit }) => {
                out =
                    out.resolve_range_with_keys(after.as_deref(), *limit, &SourceRef(source), keys)?
            }
            _ => {}
        }
        for k in plan.children.keys() {
            out = out.resolve_key_with_keys(k, &SourceRef(source), keys)?;
        }
        out.map_values_preserving_commitments(keys, |name, v| {
            let Some(h) = v.erased() else {
                return Ok(v.clone());
            };
            let p = plan.children.get(name).cloned().or_else(|| {
                matches!(plan.value, Some(ResolutionStrategy::Recursive)).then(|| ArrayTrie {
                    value: Some(ResolutionStrategy::Recursive),
                    children: BTreeMap::new(),
                })
            });
            match p {
                Some(p) => V::from_erased(h.resolve_paths(&p, source, keys)?.as_ref()),
                None => Ok(v.clone()),
            }
        })
    }
    fn transform_paths(
        &self,
        plan: &ArrayTrie<Transform<String>>,
        keys: &dyn KeyProvider,
    ) -> Result<Self, Error> {
        let mut out = self.clone();
        for (name, p) in &plan.children {
            if let Some(op) = &p.value {
                let parse = |s: &String| {
                    s.parse::<V>()
                        .map_err(|_| Error::TypeMismatch(format!("invalid value for {name}")))
                };
                let op = match op {
                    Transform::Insert(v) => Transform::Insert(parse(v)?),
                    Transform::Update(v) => Transform::Update(parse(v)?),
                    Transform::Delete => Transform::Delete,
                };
                out = out.apply_with_keys(name, op, keys)?;
            }
            if !p.children.is_empty() {
                let v = out
                    .get_with_keys(name, keys)?
                    .ok_or_else(|| Error::InvalidKey(name.clone()))?;
                let h = v
                    .erased()
                    .ok_or_else(|| Error::TypeMismatch("nested scalar transform".into()))?;
                let nested = ArrayTrie {
                    value: None,
                    children: p.children.clone(),
                };
                let h = h
                    .transform_paths(&nested, keys)?
                    .ok_or_else(|| Error::InvalidKey(name.clone()))?;
                out = out.apply_with_keys(
                    name,
                    Transform::Update(V::from_erased(h.as_ref())?),
                    keys,
                )?;
            }
        }
        Ok(out)
    }
    fn storage_paths(
        &self,
        p: &ArrayTrie<StorageStrategy>,
        keys: &dyn KeyProvider,
    ) -> Result<Vec<crate::SerializedVolume>, Error> {
        let mut out = vec![];
        for (name, p) in &p.children {
            if let Some(v) = self.get_with_keys(name, keys)? {
                if let Some(h) = v.erased() {
                    out.extend(h.storage_paths(p, keys)?);
                }
            }
        }
        Ok(out)
    }
    fn proof_paths(
        &self,
        plan: &ArrayTrie<ProofKind>,
        source: &dyn ContentSource,
        keys: &dyn KeyProvider,
    ) -> Result<Self, Error> {
        let deletes = plan
            .children
            .iter()
            .filter(|(_, p)| p.value == Some(ProofKind::Deletion))
            .map(|(k, _)| (k.clone(), Transform::Delete))
            .collect();
        let mut out = self.prove_transforms_with_keys(&deletes, &SourceRef(source), keys)?;
        for (name, p) in &plan.children {
            if name.is_empty() {
                return Err(Error::InvalidKey(name.clone()));
            }
            out = out.resolve_key_with_keys(name, &SourceRef(source), keys)?;
            let present = out.get_with_keys(name, keys)?.is_some();
            if (present && p.value == Some(ProofKind::Insertion))
                || (!present
                    && p.all_values()
                        .iter()
                        .any(|v| matches!(v, ProofKind::Mutation | ProofKind::Deletion)))
            {
                return Err(Error::InvalidKey(name.clone()));
            }
        }
        out.map_values_preserving_commitments(keys, |name, v| {
            let Some(h) = v.erased() else {
                return Ok(v.clone());
            };
            if let Some(p) = plan.children.get(name) {
                if !p.children.is_empty() {
                    return V::from_erased(h.proof_paths(p, source, keys)?.as_ref());
                }
            }
            V::from_erased(h.without_node().as_ref())
        })
    }
    fn encrypt_paths(
        &self,
        plan: &ArrayTrie<EncryptionStrategy>,
        keys: &dyn KeyProvider,
    ) -> Result<Self, Error> {
        let out = self.map_values_with_keys(keys, |name, v| {
            let Some(h) = v.erased() else {
                return Ok(v.clone());
            };
            let p = plan
                .children
                .get(name)
                .cloned()
                .or_else(|| match &plan.value {
                    Some(EncryptionStrategy::Recursive(k)) => Some(ArrayTrie {
                        value: Some(EncryptionStrategy::Recursive(*k)),
                        children: BTreeMap::new(),
                    }),
                    Some(EncryptionStrategy::List(k)) => Some(ArrayTrie {
                        value: Some(EncryptionStrategy::Targeted(*k)),
                        children: BTreeMap::new(),
                    }),
                    _ => None,
                });
            match p {
                Some(p) => V::from_erased(h.encrypt_paths(&p, keys)?.as_ref()),
                None => Ok(v.clone()),
            }
        })?;
        if let Some(EncryptionStrategy::Recursive(k)) = &plan.value {
            out.encrypt_prefixes(&BTreeMap::from([("".into(), *k)]), keys)
        } else {
            Ok(out)
        }
    }
}
impl<V: PlatformValue> PlatformNode for crate::VolumeMerkleDictionary<V> {
    fn named_children(&self) -> Result<NamedHeaders, Error> {
        self.to_ordinary().named_children()
    }
    fn named_children_with_keys(&self, keys: &dyn KeyProvider) -> Result<NamedHeaders, Error> {
        self.to_ordinary().named_children_with_keys(keys)
    }
    fn with_named_children(&self, c: NamedHeaders) -> Result<Self, Error> {
        Ok(Self::from_ordinary(
            &self.to_ordinary().with_named_children(c)?,
        ))
    }
    fn resolve_paths(
        &self,
        p: &ArrayTrie<ResolutionStrategy>,
        s: &dyn ContentSource,
        k: &dyn KeyProvider,
    ) -> Result<Self, Error> {
        Ok(Self::from_ordinary(&PlatformNode::resolve_paths(
            &self.to_ordinary(),
            p,
            s,
            k,
        )?))
    }
    fn transform_paths(
        &self,
        p: &ArrayTrie<Transform<String>>,
        k: &dyn KeyProvider,
    ) -> Result<Self, Error> {
        Ok(Self::from_ordinary(&PlatformNode::transform_paths(
            &self.to_ordinary(),
            p,
            k,
        )?))
    }
    fn storage_paths(
        &self,
        p: &ArrayTrie<StorageStrategy>,
        keys: &dyn KeyProvider,
    ) -> Result<Vec<crate::SerializedVolume>, Error> {
        fn walk<V: PlatformValue>(
            h: &Volume<crate::VolumeRadixNode<V>>,
            prefix: &str,
            p: &ArrayTrie<StorageStrategy>,
            keys: &dyn KeyProvider,
            out: &mut Vec<crate::SerializedVolume>,
        ) -> Result<(), Error> {
            use unicode_normalization::UnicodeNormalization;
            let key = h
                .encryption_info
                .as_ref()
                .and_then(|i| keys.key(&i.key_hash));
            h.bytes(key.as_ref())?;
            let n = h
                .node
                .as_ref()
                .ok_or_else(|| Error::MissingContent(h.raw_cid.clone()))?;
            let path = prefix.to_owned() + &n.prefix;
            out.push(crate::volume::serialize_boundary(h, keys)?);
            if let Some(v) = &n.value {
                if let Some(plan) = p.children.get(&path) {
                    if let Some(h) = v.erased() {
                        out.extend(h.storage_paths(plan, keys)?);
                    }
                }
            }
            for e in &n.children {
                let next = path.clone() + &e.key;
                let next: String = next.nfc().collect();
                if p.children
                    .keys()
                    .any(|k| k.nfc().collect::<String>().starts_with(&next))
                {
                    walk(&e.value, &path, p, keys, out)?;
                }
            }
            Ok(())
        }
        use unicode_normalization::UnicodeNormalization;
        let mut out = vec![];
        for e in &self.children {
            let leading: String = e.key.nfc().collect();
            if p.children
                .keys()
                .any(|k| k.nfc().collect::<String>().starts_with(&leading))
            {
                walk(&e.value, "", p, keys, &mut out)?
            }
        }
        Ok(out)
    }
    fn proof_paths(
        &self,
        p: &ArrayTrie<ProofKind>,
        s: &dyn ContentSource,
        k: &dyn KeyProvider,
    ) -> Result<Self, Error> {
        Ok(Self::from_ordinary(&PlatformNode::proof_paths(
            &self.to_ordinary(),
            p,
            s,
            k,
        )?))
    }
    fn encrypt_paths(
        &self,
        p: &ArrayTrie<EncryptionStrategy>,
        k: &dyn KeyProvider,
    ) -> Result<Self, Error> {
        Ok(Self::from_ordinary(&PlatformNode::encrypt_paths(
            &self.to_ordinary(),
            p,
            k,
        )?))
    }
}
impl<V: PlatformValue> PlatformNode for crate::MerkleArray<V> {
    fn named_children(&self) -> Result<NamedHeaders, Error> {
        self.0.named_children()
    }
    fn named_children_with_keys(&self, keys: &dyn KeyProvider) -> Result<NamedHeaders, Error> {
        self.0.named_children_with_keys(keys)
    }
    fn with_named_children(&self, c: NamedHeaders) -> Result<Self, Error> {
        Ok(Self(self.0.with_named_children(c)?))
    }
    fn resolve_paths(
        &self,
        p: &ArrayTrie<ResolutionStrategy>,
        s: &dyn ContentSource,
        k: &dyn KeyProvider,
    ) -> Result<Self, Error> {
        Ok(Self(PlatformNode::resolve_paths(&self.0, p, s, k)?))
    }
    fn transform_paths(
        &self,
        p: &ArrayTrie<Transform<String>>,
        k: &dyn KeyProvider,
    ) -> Result<Self, Error> {
        Ok(Self(PlatformNode::transform_paths(&self.0, p, k)?))
    }
    fn encrypt_paths(
        &self,
        p: &ArrayTrie<EncryptionStrategy>,
        k: &dyn KeyProvider,
    ) -> Result<Self, Error> {
        Ok(Self(PlatformNode::encrypt_paths(&self.0, p, k)?))
    }
    fn storage_paths(
        &self,
        p: &ArrayTrie<StorageStrategy>,
        k: &dyn KeyProvider,
    ) -> Result<Vec<crate::SerializedVolume>, Error> {
        self.0.storage_paths(p, k)
    }
    fn proof_paths(
        &self,
        p: &ArrayTrie<ProofKind>,
        s: &dyn ContentSource,
        k: &dyn KeyProvider,
    ) -> Result<Self, Error> {
        Ok(Self(PlatformNode::proof_paths(&self.0, p, s, k)?))
    }
}
impl PlatformNode for crate::MerkleSet {
    fn resolve_paths(
        &self,
        p: &ArrayTrie<ResolutionStrategy>,
        s: &dyn ContentSource,
        k: &dyn KeyProvider,
    ) -> Result<Self, Error> {
        Ok(Self(PlatformNode::resolve_paths(&self.0, p, s, k)?))
    }
    fn transform_paths(
        &self,
        p: &ArrayTrie<Transform<String>>,
        k: &dyn KeyProvider,
    ) -> Result<Self, Error> {
        Ok(Self(PlatformNode::transform_paths(&self.0, p, k)?))
    }
    fn encrypt_paths(
        &self,
        p: &ArrayTrie<EncryptionStrategy>,
        k: &dyn KeyProvider,
    ) -> Result<Self, Error> {
        Ok(Self(PlatformNode::encrypt_paths(&self.0, p, k)?))
    }
    fn proof_paths(
        &self,
        p: &ArrayTrie<ProofKind>,
        s: &dyn ContentSource,
        k: &dyn KeyProvider,
    ) -> Result<Self, Error> {
        Ok(Self(PlatformNode::proof_paths(&self.0, p, s, k)?))
    }
}

impl<T: PlatformNode> Header<T> {
    /// Drive the checked synchronous traversal from an asynchronous fetcher. Each
    /// missing CID is fetched once; successful hydration retains the original root.
    pub async fn resolve_paths_async(
        &self,
        plan: &ArrayTrie<ResolutionStrategy>,
        source: &dyn crate::source::Fetcher,
        keys: &dyn KeyProvider,
    ) -> Result<Self, Error> {
        let mut cache = BTreeMap::new();
        loop {
            match ErasedHeader::resolve_paths(self, plan, &cache, keys) {
                Ok(h) => return expect_header(h.as_ref()),
                Err(Error::MissingContent(cid)) => {
                    if cache.contains_key(&cid) {
                        return Err(Error::MissingContent(cid));
                    }
                    let bytes = source.fetch_one(&cid).await?;
                    crate::verify_content_address(&cid, &bytes)?;
                    cache.insert(cid, bytes);
                }
                Err(e) => return Err(e),
            }
        }
    }
    pub async fn proof_paths_async(
        &self,
        plan: &ArrayTrie<ProofKind>,
        source: &dyn crate::source::Fetcher,
        keys: &dyn KeyProvider,
    ) -> Result<Self, Error> {
        let mut cache = BTreeMap::new();
        loop {
            match ErasedHeader::proof_paths(self, plan, &cache, keys) {
                Ok(h) => return expect_header(h.as_ref()),
                Err(Error::MissingContent(cid)) => {
                    if cache.contains_key(&cid) {
                        return Err(Error::MissingContent(cid));
                    }
                    let bytes = source.fetch_one(&cid).await?;
                    crate::verify_content_address(&cid, &bytes)?;
                    cache.insert(cid, bytes);
                }
                Err(e) => return Err(e),
            }
        }
    }
}
impl<T: PlatformNode> Volume<T> {
    pub async fn resolve_paths_async(
        &self,
        p: &ArrayTrie<ResolutionStrategy>,
        s: &dyn crate::source::Fetcher,
        k: &dyn KeyProvider,
    ) -> Result<Self, Error> {
        Ok(Self(self.0.resolve_paths_async(p, s, k).await?))
    }
    pub async fn proof_paths_async(
        &self,
        p: &ArrayTrie<ProofKind>,
        s: &dyn crate::source::Fetcher,
        k: &dyn KeyProvider,
    ) -> Result<Self, Error> {
        Ok(Self(self.0.proof_paths_async(p, s, k).await?))
    }
}
