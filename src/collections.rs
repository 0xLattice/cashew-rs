use crate::{Error, MerkleDictionary};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(transparent, bound = "")]
pub struct MerkleArray<V>(pub MerkleDictionary<V>);
impl<V> Default for MerkleArray<V> {
    fn default() -> Self {
        Self(MerkleDictionary::default())
    }
}
impl<V: Clone + Serialize> MerkleArray<V> {
    pub fn binary_key(index: usize) -> String {
        format!("{index:0256b}")
    }
    pub fn get(&self, index: usize) -> Result<Option<&V>, Error> {
        if index >= self.0.count {
            Ok(None)
        } else {
            self.0.get(&Self::binary_key(index))
        }
    }
    pub fn append(&self, value: V) -> Result<Self, Error> {
        Ok(Self(
            self.0.inserting(&Self::binary_key(self.0.count), value)?,
        ))
    }
    pub fn append_contents(&self, other: &Self) -> Result<Self, Error> {
        let mut result = self.clone();
        for i in 0..other.0.count {
            result = result.append(
                other
                    .get(i)?
                    .ok_or_else(|| Error::MissingContent(Self::binary_key(i)))?
                    .clone(),
            )?
        }
        Ok(result)
    }
    pub fn updating_at(&self, index: usize, value: V) -> Result<Self, Error> {
        if index >= self.0.count {
            return Err(Error::InvalidKey(index.to_string()));
        }
        Ok(Self(self.0.updating(&Self::binary_key(index), value)?))
    }
    /// Delete by moving the last element into the removed slot, as Swift does.
    pub fn deleting_at(&self, index: usize) -> Result<Self, Error> {
        if index >= self.0.count {
            return Err(Error::InvalidKey(index.to_string()));
        }
        let last = self.0.count - 1;
        let mut out = self.clone();
        if index != last {
            let v = self
                .get(last)?
                .ok_or_else(|| Error::MissingContent(Self::binary_key(last)))?
                .clone();
            out = out.updating_at(index, v)?;
        }
        Ok(Self(out.0.deleting(&Self::binary_key(last))?))
    }
    pub fn range_paths(
        range: std::ops::Range<usize>,
        inner: Option<crate::ResolutionStrategy>,
    ) -> crate::ArrayTrie<crate::ResolutionStrategy> {
        let mut out = crate::ArrayTrie::new();
        out.value = Some(crate::ResolutionStrategy::Range {
            after: range.start.checked_sub(1).map(Self::binary_key),
            limit: range.len(),
        });
        if let Some(strategy) = inner {
            for i in range {
                out.set(&[Self::binary_key(i)], strategy.clone());
            }
        }
        out
    }
    pub fn range_transforms(
        range: std::ops::Range<usize>,
        transform: crate::Transform<String>,
    ) -> crate::ArrayTrie<crate::Transform<String>> {
        let mut out = crate::ArrayTrie::new();
        for i in range {
            out.set(&[Self::binary_key(i)], transform.clone());
        }
        out
    }
    pub fn nested_range_transforms(
        range: std::ops::Range<usize>,
        inner: crate::ArrayTrie<crate::Transform<String>>,
    ) -> crate::ArrayTrie<crate::Transform<String>> {
        crate::ArrayTrie {
            value: None,
            children: range
                .map(|i| (Self::binary_key(i), inner.clone()))
                .collect(),
        }
    }
    pub fn first(&self) -> Result<Option<&V>, Error> {
        self.get(0)
    }
    pub fn last(&self) -> Result<Option<&V>, Error> {
        self.get(self.0.count.saturating_sub(1))
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MerkleSet(pub MerkleDictionary<String>);
impl MerkleSet {
    pub fn insert(&self, member: &str) -> Result<Self, Error> {
        Ok(Self(self.0.inserting(member, String::new())?))
    }
    pub fn remove(&self, member: &str) -> Result<Self, Error> {
        Ok(Self(self.0.deleting(member)?))
    }
    pub fn contains(&self, member: &str) -> Result<bool, Error> {
        Ok(self.0.get(member)?.is_some())
    }
    pub fn members(&self) -> Result<std::collections::BTreeSet<String>, Error> {
        Ok(self.0.entries()?.into_keys().collect())
    }
    pub fn union(&self, other: &Self) -> Result<Self, Error> {
        let mut out = self.clone();
        for m in other.members()? {
            if !out.contains(&m)? {
                out = out.insert(&m)?
            }
        }
        Ok(out)
    }
    pub fn intersection(&self, other: &Self) -> Result<Self, Error> {
        let mut out = Self::default();
        for m in self.members()? {
            if other.contains(&m)? {
                out = out.insert(&m)?
            }
        }
        Ok(out)
    }
    pub fn subtracting(&self, other: &Self) -> Result<Self, Error> {
        let mut out = self.clone();
        for m in other.members()? {
            if out.contains(&m)? {
                out = out.remove(&m)?
            }
        }
        Ok(out)
    }
    pub fn symmetric_difference(&self, other: &Self) -> Result<Self, Error> {
        self.subtracting(other)?.union(&other.subtracting(self)?)
    }
}
/// Swift FlatDictionary encodes directly as a map, without an entries wrapper.
pub type FlatDictionary<H> = std::collections::BTreeMap<String, H>;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn array_and_set() {
        let a = MerkleArray::default().append(3).unwrap().append(4).unwrap();
        assert_eq!(a.last().unwrap(), Some(&4));
        assert_eq!(a.get(2).unwrap(), None);
        assert_eq!(MerkleArray::<i32>::binary_key(1).len(), 256);
        let a = MerkleSet::default()
            .insert("a")
            .unwrap()
            .insert("b")
            .unwrap();
        let b = MerkleSet::default()
            .insert("b")
            .unwrap()
            .insert("c")
            .unwrap();
        assert_eq!(a.union(&b).unwrap().0.count, 3);
        assert_eq!(
            a.intersection(&b).unwrap().members().unwrap(),
            ["b".into()].into()
        );
        assert_eq!(
            a.symmetric_difference(&b).unwrap().members().unwrap(),
            ["a".into(), "c".into()].into()
        );
    }
}
