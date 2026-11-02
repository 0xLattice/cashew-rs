# Cashew source coverage

Status is per source unit, not a claim that all generic instantiations match Swift. Every partial row remains an acceptance requirement.

| Swift source | Status | Rust evidence / remaining work |
|---|---|---|
| `Core/DagCBOR.swift` | implemented/tested | src/cbor.rs; 71 Swift fixture suite includes canonical serialization |
| `Core/EncryptionHelper.swift` | implemented/tested | src/header.rs; deterministic AES-256-GCM fixture |
| `Core/EncryptionInfo.swift` | implemented/tested | src/header.rs; deterministic AES-256-GCM fixture |
| `Core/Header+OwnedSubtree.swift` | partial | src/{cbor,header,volume}.rs; src/platform.rs adds checked heterogeneous traits and owned-subtree traversal; full Swift generic corpus remains |
| `Core/Header.swift` | partial | src/{cbor,header,volume}.rs; src/platform.rs adds checked heterogeneous traits and owned-subtree traversal; full Swift generic corpus remains |
| `Core/HeaderImpl.swift` | partial | src/{cbor,header,volume}.rs; src/platform.rs adds checked heterogeneous traits and owned-subtree traversal; full Swift generic corpus remains |
| `Core/MulticodecExtensions.swift` | partial | src/{cbor,header,volume}.rs; src/platform.rs adds checked heterogeneous traits and owned-subtree traversal; full Swift generic corpus remains |
| `Core/Node.swift` | partial | src/{cbor,header,volume}.rs; src/platform.rs adds checked heterogeneous traits and owned-subtree traversal; full Swift generic corpus remains |
| `Core/Scalar.swift` | partial | src/{cbor,header,volume}.rs; src/platform.rs adds checked heterogeneous traits and owned-subtree traversal; full Swift generic corpus remains |
| `Core/SendableBox.swift` | partial | src/{cbor,header,volume}.rs; src/platform.rs adds checked heterogeneous traits and owned-subtree traversal; full Swift generic corpus remains |
| `Core/Volume.swift` | partial | src/{cbor,header,volume}.rs; src/platform.rs adds checked heterogeneous traits and owned-subtree traversal; full Swift generic corpus remains |
| `DataErrors.swift` | partial | errors represented in src/lib.rs; full error taxonomy differs |
| `DecodingErrors.swift` | partial | errors represented in src/lib.rs; full error taxonomy differs |
| `Diff/CashewDiff.swift` | partial | src/diff.rs; scalar dictionary and materialized heterogeneous header diffs implemented; broader sparse diff corpus remains |
| `Diff/Header+diff.swift` | partial | src/diff.rs; scalar dictionary and materialized heterogeneous header diffs implemented; broader sparse diff corpus remains |
| `Diff/MerkleDictionary+diff.swift` | partial | src/diff.rs; scalar dictionary and materialized heterogeneous header diffs implemented; broader sparse diff corpus remains |
| `Diff/Node+diff.swift` | partial | src/diff.rs; scalar dictionary and materialized heterogeneous header diffs implemented; broader sparse diff corpus remains |
| `Encryption/EncryptionStrategy.swift` | partial | src/header.rs; wire encryption and explicit Rust path strategies implemented; Swift empty-segment/prefix selector equivalence remains |
| `Encryption/Header+encrypt.swift` | partial | src/header.rs; wire encryption and explicit Rust path strategies implemented; Swift empty-segment/prefix selector equivalence remains |
| `Encryption/MerkleDictionary+encrypt.swift` | partial | src/header.rs; wire encryption and explicit Rust path strategies implemented; Swift empty-segment/prefix selector equivalence remains |
| `Encryption/Node+encrypt.swift` | partial | src/header.rs; wire encryption and explicit Rust path strategies implemented; Swift empty-segment/prefix selector equivalence remains |
| `Encryption/RadixHeader+encrypt.swift` | partial | src/header.rs; wire encryption and explicit Rust path strategies implemented; Swift empty-segment/prefix selector equivalence remains |
| `Encryption/RadixNode+encrypt.swift` | partial | src/header.rs; wire encryption and explicit Rust path strategies implemented; Swift empty-segment/prefix selector equivalence remains |
| `Fetcher/CachingFetcher.swift` | partial | src/{source,volume}.rs; batching, complete boundaries, selected storage planner and retry/dedup sessions implemented; full Swift corpus remains |
| `Fetcher/CoalescingFetcher.swift` | partial | src/{source,volume}.rs; batching, complete boundaries, selected storage planner and retry/dedup sessions implemented; full Swift corpus remains |
| `Fetcher/ContentSource.swift` | partial | src/{source,volume}.rs; batching, complete boundaries, selected storage planner and retry/dedup sessions implemented; full Swift corpus remains |
| `Fetcher/Fetcher.swift` | partial | src/{source,volume}.rs; batching, complete boundaries, selected storage planner and retry/dedup sessions implemented; full Swift corpus remains |
| `Fetcher/Header+store.swift` | partial | src/{source,volume}.rs; batching, complete boundaries, selected storage planner and retry/dedup sessions implemented; full Swift corpus remains |
| `Fetcher/KeyProvider.swift` | partial | src/{source,volume}.rs; batching, complete boundaries, selected storage planner and retry/dedup sessions implemented; full Swift corpus remains |
| `Fetcher/KeyProvidingFetcher.swift` | partial | src/{source,volume}.rs; batching, complete boundaries, selected storage planner and retry/dedup sessions implemented; full Swift corpus remains |
| `Fetcher/MerkleDictionary+store.swift` | partial | src/{source,volume}.rs; batching, complete boundaries, selected storage planner and retry/dedup sessions implemented; full Swift corpus remains |
| `Fetcher/Node+store.swift` | partial | src/{source,volume}.rs; batching, complete boundaries, selected storage planner and retry/dedup sessions implemented; full Swift corpus remains |
| `Fetcher/RadixNode+store.swift` | partial | src/{source,volume}.rs; batching, complete boundaries, selected storage planner and retry/dedup sessions implemented; full Swift corpus remains |
| `Fetcher/StorageStrategy.swift` | partial | src/{source,volume}.rs; batching, complete boundaries, selected storage planner and retry/dedup sessions implemented; full Swift corpus remains |
| `Fetcher/Storer.swift` | partial | src/{source,volume}.rs; batching, complete boundaries, selected storage planner and retry/dedup sessions implemented; full Swift corpus remains |
| `Fetcher/Volume+store.swift` | partial | src/{source,volume}.rs; batching, complete boundaries, selected storage planner and retry/dedup sessions implemented; full Swift corpus remains |
| `Fetcher/VolumeStorer.swift` | partial | SerializedVolume + async VolumeStorer; retry/dedup storage session implemented and tested; cancellation stress corpus remains |
| `LosslessStringConvertible+data.swift` | partial | errors represented in src/lib.rs; full error taxonomy differs |
| `MerkleDataStructures/FlatDictionary.swift` | partial | src/{radix,collections}.rs; VolumeRadix variants implemented in src/volume_radix.rs; broader fixtures remain |
| `MerkleDataStructures/MerkleArray.swift` | partial | src/{radix,collections}.rs; VolumeRadix variants implemented in src/volume_radix.rs; broader fixtures remain |
| `MerkleDataStructures/MerkleArrayImpl.swift` | partial | src/{radix,collections}.rs; VolumeRadix variants implemented in src/volume_radix.rs; broader fixtures remain |
| `MerkleDataStructures/MerkleDictionary.swift` | partial | src/{radix,collections}.rs; VolumeRadix variants implemented in src/volume_radix.rs; broader fixtures remain |
| `MerkleDataStructures/MerkleDictionaryImpl.swift` | partial | src/{radix,collections}.rs; VolumeRadix variants implemented in src/volume_radix.rs; broader fixtures remain |
| `MerkleDataStructures/MerkleSet.swift` | partial | src/{radix,collections}.rs; VolumeRadix variants implemented in src/volume_radix.rs; broader fixtures remain |
| `MerkleDataStructures/MerkleSetImpl.swift` | partial | src/{radix,collections}.rs; VolumeRadix variants implemented in src/volume_radix.rs; broader fixtures remain |
| `MerkleDataStructures/RadixHeader.swift` | partial | src/{radix,collections}.rs; VolumeRadix variants implemented in src/volume_radix.rs; broader fixtures remain |
| `MerkleDataStructures/RadixHeaderImpl.swift` | partial | src/{radix,collections}.rs; VolumeRadix variants implemented in src/volume_radix.rs; broader fixtures remain |
| `MerkleDataStructures/RadixNode.swift` | partial | src/{radix,collections}.rs; VolumeRadix variants implemented in src/volume_radix.rs; broader fixtures remain |
| `MerkleDataStructures/RadixNodeImpl.swift` | partial | src/{radix,collections}.rs; VolumeRadix variants implemented in src/volume_radix.rs; broader fixtures remain |
| `MerkleDataStructures/SortedEntry.swift` | partial | src/{radix,collections}.rs; VolumeRadix variants implemented in src/volume_radix.rs; broader fixtures remain |
| `MerkleDataStructures/VolumeMerkleDictionary.swift` | partial | src/{radix,collections}.rs; VolumeRadix variants implemented in src/volume_radix.rs; broader fixtures remain |
| `MerkleDataStructures/VolumeMerkleDictionaryImpl.swift` | partial | src/{radix,collections}.rs; VolumeRadix variants implemented in src/volume_radix.rs; broader fixtures remain |
| `MerkleDataStructures/VolumeRadixHeader.swift` | partial | src/{radix,collections}.rs; VolumeRadix variants implemented in src/volume_radix.rs; broader fixtures remain |
| `MerkleDataStructures/VolumeRadixHeaderImpl.swift` | partial | src/{radix,collections}.rs; VolumeRadix variants implemented in src/volume_radix.rs; broader fixtures remain |
| `MerkleDataStructures/VolumeRadixNodeImpl.swift` | partial | src/{radix,collections}.rs; VolumeRadix variants implemented in src/volume_radix.rs; broader fixtures remain |
| `Proofs/Header+proofs.swift` | partial | src/radix.rs; read/write witnesses, keyed collapse siblings and declarative nested selectors implemented; Swift strategy differential corpus remains |
| `Proofs/MerkleDictionary+proofs.swift` | partial | src/radix.rs; read/write witnesses, keyed collapse siblings and declarative nested selectors implemented; Swift strategy differential corpus remains |
| `Proofs/Node+proofs.swift` | partial | src/radix.rs; read/write witnesses, keyed collapse siblings and declarative nested selectors implemented; Swift strategy differential corpus remains |
| `Proofs/ProofErrors.swift` | partial | src/radix.rs; read/write witnesses, keyed collapse siblings and declarative nested selectors implemented; Swift strategy differential corpus remains |
| `Proofs/RadixHeader+proofs.swift` | partial | src/radix.rs; read/write witnesses, keyed collapse siblings and declarative nested selectors implemented; Swift strategy differential corpus remains |
| `Proofs/RadixNode+proofs.swift` | partial | src/radix.rs; read/write witnesses, keyed collapse siblings and declarative nested selectors implemented; Swift strategy differential corpus remains |
| `Proofs/SparseMerkleProof.swift` | partial | src/radix.rs; read/write witnesses, keyed collapse siblings and declarative nested selectors implemented; Swift strategy differential corpus remains |
| `Query/CashewExecutor.swift` | partial | src/query.rs; parser/compiler and scalar/nested Header/Volume/array execution implemented; automatic lazy query loading remains |
| `Query/CashewExpression.swift` | partial | src/query.rs; parser/compiler and scalar/nested Header/Volume/array execution implemented; automatic lazy query loading remains |
| `Query/CashewParser.swift` | partial | src/query.rs; parser/compiler and scalar/nested Header/Volume/array execution implemented; automatic lazy query loading remains |
| `Query/CashewPlan.swift` | partial | src/query.rs; parser/compiler and scalar/nested Header/Volume/array execution implemented; automatic lazy query loading remains |
| `Query/CashewQueryable.swift` | partial | src/query.rs; parser/compiler and scalar/nested Header/Volume/array execution implemented; automatic lazy query loading remains |
| `Resolver/Header+resolve.swift` | partial | src/{header,radix,source}.rs; targeted/list/recursive/range and checked heterogeneous paths implemented, sync and async; full Swift selector corpus remains |
| `Resolver/MerkleArray+resolve.swift` | partial | src/{header,radix,source}.rs; targeted/list/recursive/range and checked heterogeneous paths implemented, sync and async; full Swift selector corpus remains |
| `Resolver/MerkleDictionary+resolve.swift` | partial | src/{header,radix,source}.rs; targeted/list/recursive/range and checked heterogeneous paths implemented, sync and async; full Swift selector corpus remains |
| `Resolver/Node+resolve.swift` | partial | src/{header,radix,source}.rs; targeted/list/recursive/range and checked heterogeneous paths implemented, sync and async; full Swift selector corpus remains |
| `Resolver/RadixHeader+resolve.swift` | partial | src/{header,radix,source}.rs; targeted/list/recursive/range and checked heterogeneous paths implemented, sync and async; full Swift selector corpus remains |
| `Resolver/RadixNode+resolve.swift` | partial | src/{header,radix,source}.rs; targeted/list/recursive/range and checked heterogeneous paths implemented, sync and async; full Swift selector corpus remains |
| `Resolver/ResolutionErrors.swift` | partial | src/{header,radix,source}.rs; targeted/list/recursive/range and checked heterogeneous paths implemented, sync and async; full Swift selector corpus remains |
| `Resolver/ResolutionStrategy.swift` | partial | src/{header,radix,source}.rs; targeted/list/recursive/range and checked heterogeneous paths implemented, sync and async; full Swift selector corpus remains |
| `Resolver/Scalar+resolve.swift` | partial | src/{header,radix,source}.rs; targeted/list/recursive/range and checked heterogeneous paths implemented, sync and async; full Swift selector corpus remains |
| `Transform/Header+transform.swift` | partial | src/radix.rs; dictionary, heterogeneous and keyed encrypted transforms implemented; full Swift nested differential corpus remains |
| `Transform/MerkleArray+transform.swift` | partial | src/radix.rs; dictionary, heterogeneous and keyed encrypted transforms implemented; full Swift nested differential corpus remains |
| `Transform/MerkleDictionary+transform.swift` | partial | src/radix.rs; dictionary, heterogeneous and keyed encrypted transforms implemented; full Swift nested differential corpus remains |
| `Transform/Node+transform.swift` | partial | src/radix.rs; dictionary, heterogeneous and keyed encrypted transforms implemented; full Swift nested differential corpus remains |
| `Transform/RadixHeader+transform.swift` | partial | src/radix.rs; dictionary, heterogeneous and keyed encrypted transforms implemented; full Swift nested differential corpus remains |
| `Transform/RadixNode+transform.swift` | partial | src/radix.rs; dictionary, heterogeneous and keyed encrypted transforms implemented; full Swift nested differential corpus remains |
| `Transform/Transform.swift` | partial | src/radix.rs; dictionary, heterogeneous and keyed encrypted transforms implemented; full Swift nested differential corpus remains |
| `Transform/TransformErrors.swift` | partial | src/radix.rs; dictionary, heterogeneous and keyed encrypted transforms implemented; full Swift nested differential corpus remains |

## Acceptance gates still open

- All partial rows implemented or explicitly excluded by user.
- Swift tests/fixture parity for each implemented generic family and strategy.
- Full platform Node/SDK integration exercising nested volumes and sparse writes.
- Fuzzed untrusted serialization and authenticated encryption input handling.
- Async cancellation and transient-source failure correctness.
- Formal model correspondence audit; passing Lean proves the model, not Rust refinement.
