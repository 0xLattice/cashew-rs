# lattice-cashew

Rust implementation of 0xLattice Cashew content-addressed data structures. This is an active, **incomplete** platform port, not a full-parity release. Source provenance is in `UPSTREAM`; original MIT license is retained.

## Implemented

- Canonical Swift DAG-CBOR serde codec: length-first string map keys, f64 floats, strict re-encoding, unique keys, collection/depth limits, base64 Data adapter, integer overflow rejection.
- Canonical CID creation and entry/volume integrity validation; SHA2, SHA3, Blake and identity hashes. Unsupported multihashes fail closed.
- Typed lazy Header, encrypted Header, authenticated AES-256-GCM combined format and key metadata.
- Persistent compressed MerkleDictionary insertion, update, deletion, batch transforms and canonical collapse; Swift Unicode grapheme equivalence preserves original wire spelling.
- Sparse read proofs, sparse write witnesses with required collapse siblings, targeted and complete radix resolution.
- MerkleArray (256-bit binary index keys), MerkleSet algebra, generic FlatDictionary wire shape, VolumeMerkleDictionary/VolumeRadixNode with independent internal boundaries.
- Explicit typed DAG link/Volume boundaries: ordinary descendants are complete, nested volumes are independent, missing ordinary content fails rather than silently producing an incomplete boundary.
- Async batched source, overlay/composite sources, verified caching, per-wave fetch coalescing, breadth-first radix loading.
- Query tokenizer/parser/plan compiler, nested Header/Volume/dictionary/array execution, scalar and heterogeneous diffs.
- Checked PlatformNode/ErasedHeader traversal with explicit named children, safe reconstruction, sync/async nested resolution and proofs, bounded range loading, encrypted mutations.
- Selected storage plans preserving internal VolumeRadix boundaries; retryable concurrent boundary deduplication sessions; authenticated postorder owned-subtree edge multiplicities.

## Usage

```rust
use lattice_cashew::{Header, MerkleDictionary};
let tree = MerkleDictionary::default().inserting("hello", 42u64)?;
let root = Header::new(tree.clone())?;
let volume = tree.serialize_volume()?;
volume.validate()?;
# Ok::<(), lattice_cashew::Error>(())
```

Swift `Data` fields must use `#[serde(with = "lattice_cashew::cbor::base64_bytes")]`.
Header serializes `rawCID` and optional `encryptionInfo`; loaded nodes never enter parent bytes.
Use `volume::serialize_boundary` for graphs whose values themselves carry headers; `MerkleDictionary::serialize_volume` covers the radix structure only.

`source::ContentSource` is the asynchronous batched interface. The root-exported `ContentSource` is the synchronous single-entry adapter used by the initial typed resolver. Applications should use the async interface for network sources.

## Verification

`cargo test` and `cargo clippy --all-targets -- -D warnings`.
`python3 tools/regenerate-fixtures.py` builds the pinned sibling Swift checkout and regenerates `tests/swift.tsv`. The 71 checked outputs cover 33 dictionary encodings, 33 root CIDs, integer limits, f64, a Data-bearing record, and AES-GCM bytes. Unicode decomposed/composed characters are included. These vectors do not establish parity for every generic Node type.

`tests/formal_properties.rs` translates the concrete batch examples and canonical/sparse obligations from 0xLattice/lattice-formal at a9f4c2879b6565ae11c17d35efe14475c5830978. Tests exercise independent operations in both orders, collapse, root preservation, unknown-versus-absent reads, and duplicate-child rejection. The Lean proofs are abstract-model evidence, **not a Rust refinement proof**.

## Remaining parity requirements

- Full Swift differential corpus for heterogeneous paths, sparse nested proofs and selected storage plans. Implemented surfaces are tested but not a refinement proof.
- Automatic source-aware lazy query dispatch (explicit async path hydration is available).
- Exact Swift encryption selector conventions: Rust's explicit root strategy encrypts the current Header and recursive propagation follows semantic paths. Swift separates encryptSelf and uses empty segments / compressed prefix overrides. Wire AES-GCM itself is differential-tested.
- Broader legacy codec differential coverage. Like pinned Swift, Header codec changes the CID label while node serialization remains DAG-CBOR.
- Full Swift source test corpus differential testing, malformed-data fuzzing and asynchronous cancellation/failure tests.
- Performance/allocation and thread-safety parity. No remote repository published yet.
