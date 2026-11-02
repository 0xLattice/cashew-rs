//! Asynchronous batch sources, verified caching, and request coalescing.
use crate::{cbor, verify_content_address, ContentSource as SyncSource, Error, Header};
use async_trait::async_trait;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use tokio::sync::{oneshot, Mutex};
#[async_trait]
pub trait ContentSource: Send + Sync {
    async fn fetch(&self, cids: BTreeSet<String>) -> BTreeMap<String, Vec<u8>>;
}
#[async_trait]
pub trait Fetcher: Send + Sync {
    async fn fetch_one(&self, cid: &str) -> Result<Vec<u8>, Error>;
}
#[async_trait]
pub trait Storer: Send + Sync {
    async fn store_entries(&self, entries: BTreeMap<String, Vec<u8>>) -> Result<(), Error>;
}
#[async_trait]
impl ContentSource for BTreeMap<String, Vec<u8>> {
    async fn fetch(&self, cids: BTreeSet<String>) -> BTreeMap<String, Vec<u8>> {
        cids.into_iter()
            .filter_map(|c| self.get(&c).cloned().map(|v| (c, v)))
            .collect()
    }
}
#[async_trait]
impl Fetcher for BTreeMap<String, Vec<u8>> {
    async fn fetch_one(&self, cid: &str) -> Result<Vec<u8>, Error> {
        SyncSource::fetch(self, cid)
    }
}
pub struct OverlaySource {
    pub entries: BTreeMap<String, Vec<u8>>,
    pub fallback: Arc<dyn ContentSource>,
}
#[async_trait]
impl ContentSource for OverlaySource {
    async fn fetch(&self, cids: BTreeSet<String>) -> BTreeMap<String, Vec<u8>> {
        let mut found = BTreeMap::new();
        let mut missing = BTreeSet::new();
        for cid in cids {
            if let Some(bytes) = self.entries.get(&cid) {
                found.insert(cid, bytes.clone());
            } else {
                missing.insert(cid);
            }
        }
        if !missing.is_empty() {
            for (k, v) in self.fallback.fetch(missing.clone()).await {
                if missing.contains(&k) {
                    found.entry(k).or_insert(v);
                }
            }
        }
        found
    }
}
pub struct CompositeSource {
    pub sources: Vec<Arc<dyn ContentSource>>,
}
#[async_trait]
impl ContentSource for CompositeSource {
    async fn fetch(&self, mut cids: BTreeSet<String>) -> BTreeMap<String, Vec<u8>> {
        let mut found = BTreeMap::new();
        for source in &self.sources {
            if cids.is_empty() {
                break;
            }
            for (k, v) in source.fetch(cids.clone()).await {
                if cids.remove(&k) {
                    found.insert(k, v);
                }
            }
        }
        found
    }
}
pub struct FetcherSource {
    pub fetcher: Arc<dyn Fetcher>,
}
#[async_trait]
impl ContentSource for FetcherSource {
    async fn fetch(&self, cids: BTreeSet<String>) -> BTreeMap<String, Vec<u8>> {
        let mut out = BTreeMap::new();
        for c in cids {
            if let Ok(v) = self.fetcher.fetch_one(&c).await {
                out.insert(c, v);
            }
        }
        out
    }
}
pub struct CachingFetcher {
    pub fetcher: Arc<dyn Fetcher>,
    pub storer: Arc<dyn Storer>,
}
#[async_trait]
impl Fetcher for CachingFetcher {
    async fn fetch_one(&self, cid: &str) -> Result<Vec<u8>, Error> {
        let bytes = self.fetcher.fetch_one(cid).await?;
        verify_content_address(cid, &bytes)?;
        self.storer
            .store_entries(BTreeMap::from([(cid.into(), bytes.clone())]))
            .await?;
        Ok(bytes)
    }
}
type Reply = oneshot::Sender<Result<Vec<u8>, Error>>;
struct Pending {
    waiters: BTreeMap<String, Vec<Reply>>,
    scheduled: bool,
}
#[derive(Clone)]
pub struct CoalescingFetcher {
    source: Arc<dyn ContentSource>,
    pending: Arc<Mutex<Pending>>,
}
impl CoalescingFetcher {
    pub fn new(source: Arc<dyn ContentSource>) -> Self {
        Self {
            source,
            pending: Arc::new(Mutex::new(Pending {
                waiters: BTreeMap::new(),
                scheduled: false,
            })),
        }
    }
}
#[async_trait]
impl Fetcher for CoalescingFetcher {
    async fn fetch_one(&self, cid: &str) -> Result<Vec<u8>, Error> {
        let (tx, rx) = oneshot::channel();
        let mut pending = self.pending.lock().await;
        pending.waiters.entry(cid.into()).or_default().push(tx);
        if !pending.scheduled {
            pending.scheduled = true;
            let state = self.pending.clone();
            let source = self.source.clone();
            tokio::spawn(async move {
                tokio::task::yield_now().await;
                let waiters = {
                    let mut p = state.lock().await;
                    p.scheduled = false;
                    std::mem::take(&mut p.waiters)
                };
                let results = source.fetch(waiters.keys().cloned().collect()).await;
                for (cid, list) in waiters {
                    let value = results.get(&cid).cloned().ok_or(Error::MissingContent(cid));
                    for reply in list {
                        let _ = reply.send(value.clone());
                    }
                }
            });
        }
        drop(pending);
        rx.await.map_err(|_| Error::MissingContent(cid.into()))?
    }
}
impl<T: serde::Serialize + serde::de::DeserializeOwned> Header<T> {
    pub async fn resolve_async(
        &self,
        source: &impl Fetcher,
        key: Option<&[u8; 32]>,
    ) -> Result<Self, Error> {
        let bytes = source.fetch_one(&self.raw_cid).await?;
        self.removing_node()
            .resolve(&BTreeMap::from([(self.raw_cid.clone(), bytes)]), key)
    }
}
/// Verify every batch entry before decoding or caching any untrusted bytes.
pub async fn verified_batch(
    source: &dyn ContentSource,
    cids: BTreeSet<String>,
) -> Result<BTreeMap<String, Vec<u8>>, Error> {
    let bytes = source.fetch(cids.clone()).await;
    let mut out = BTreeMap::new();
    for cid in cids {
        let b = bytes
            .get(&cid)
            .ok_or_else(|| Error::MissingContent(cid.clone()))?;
        verify_content_address(&cid, b)?;
        out.insert(cid, b.clone());
    }
    Ok(out)
}
/// Load the radix frontier breadth-first, using one batch request per level.
pub async fn resolve_dictionary<V: Clone + serde::Serialize + serde::de::DeserializeOwned>(
    dictionary: &crate::MerkleDictionary<V>,
    source: &dyn ContentSource,
) -> Result<crate::MerkleDictionary<V>, Error> {
    let mut frontier: BTreeSet<String> = dictionary
        .children
        .iter()
        .map(|e| e.value.raw_cid.clone())
        .collect();
    let mut loaded = BTreeMap::new();
    while !frontier.is_empty() {
        let batch = verified_batch(source, frontier).await?;
        let mut next = BTreeSet::new();
        for (cid, bytes) in batch {
            let node: crate::RadixNode<V> = cbor::decode(&bytes)?;
            for e in node.children {
                if !loaded.contains_key(&e.value.raw_cid) {
                    next.insert(e.value.raw_cid);
                }
            }
            loaded.insert(cid, bytes);
        }
        frontier = next
            .into_iter()
            .filter(|c| !loaded.contains_key(c))
            .collect();
    }
    dictionary.resolve_all(&loaded)
}
#[async_trait]
pub trait VolumeStorer: Send + Sync {
    async fn store_volume(&self, volume: crate::SerializedVolume) -> Result<(), Error>;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn batch_coalesces_and_not_found() {
        struct Count {
            calls: std::sync::atomic::AtomicUsize,
            data: BTreeMap<String, Vec<u8>>,
        }
        #[async_trait]
        impl ContentSource for Count {
            async fn fetch(&self, c: BTreeSet<String>) -> BTreeMap<String, Vec<u8>> {
                self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                ContentSource::fetch(&self.data, c).await
            }
        }
        let source = Arc::new(Count {
            calls: 0.into(),
            data: BTreeMap::from([("a".into(), vec![1]), ("b".into(), vec![2])]),
        });
        let fetcher = CoalescingFetcher::new(source.clone());
        let (a, b, c) = tokio::join!(
            fetcher.fetch_one("a"),
            fetcher.fetch_one("b"),
            fetcher.fetch_one("missing")
        );
        assert_eq!(a.unwrap(), vec![1]);
        assert_eq!(b.unwrap(), vec![2]);
        assert!(c.is_err());
        assert_eq!(source.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}

/// Per-operation deduplication of successful boundary writes. Writes continue if
/// one waiter is cancelled; failed boundaries are retryable on a later call.
#[derive(Clone)]
pub struct VolumeStorageSession {
    storer: Arc<dyn VolumeStorer>,
    states: Arc<Mutex<BTreeMap<String, VolumeStoreState>>>,
}
#[derive(Clone)]
enum VolumeStoreState {
    Stored,
    InFlight(tokio::sync::watch::Receiver<Option<Result<(), Error>>>),
}
impl VolumeStorageSession {
    pub fn new(storer: Arc<dyn VolumeStorer>) -> Self {
        Self {
            storer,
            states: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }
    pub async fn is_stored(&self, root: &str) -> bool {
        matches!(
            self.states.lock().await.get(root),
            Some(VolumeStoreState::Stored)
        )
    }
}
#[async_trait]
impl VolumeStorer for VolumeStorageSession {
    async fn store_volume(&self, volume: crate::SerializedVolume) -> Result<(), Error> {
        volume.validate()?;
        let mut states = self.states.lock().await;
        let mut receiver = match states.get(&volume.root) {
            Some(VolumeStoreState::Stored) => return Ok(()),
            Some(VolumeStoreState::InFlight(rx)) => rx.clone(),
            None => {
                let (tx, rx) = tokio::sync::watch::channel(None);
                states.insert(volume.root.clone(), VolumeStoreState::InFlight(rx.clone()));
                let session = self.clone();
                tokio::spawn(async move {
                    let root = volume.root.clone();
                    let result = session.storer.store_volume(volume).await;
                    let mut states = session.states.lock().await;
                    if result.is_ok() {
                        states.insert(root, VolumeStoreState::Stored);
                    } else {
                        states.remove(&root);
                    }
                    tx.send_replace(Some(result));
                });
                rx
            }
        };
        drop(states);
        loop {
            if let Some(result) = receiver.borrow().clone() {
                return result;
            }
            receiver
                .changed()
                .await
                .map_err(|_| Error::Storage("boundary writer terminated".into()))?;
        }
    }
}
