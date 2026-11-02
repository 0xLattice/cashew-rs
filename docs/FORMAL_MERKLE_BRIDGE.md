# Executable Cashew / Lean Merkle batch bridge

Run from this repository:

```sh
python3 tools/formal-merkle-check.py
```

Requires the sibling pinned `lattice-formal` checkout and its Lean toolchain
(`leanprover/lean4:v4.34.1`), plus Cargo. The runner verifies the formal checkout
is at `a9f4c2879b6565ae11c17d35efe14475c5830978` with no tracked changes, builds
its unchanged Merkle proof library, exports current production observations,
and runs generated Lean checks. Outputs, logs, source hashes and the report
are under `target/formal-merkle/`. No tracked formal source is edited.

At invocation the runner atomically replaces any previous report with
`passed: false`, before pin checks or builds. It sets `passed: true` only after
all positive and negative checks succeed. A failed or interrupted run therefore
cannot retain an older success report. The final report records Cargo, rustc
and Lean versions, Cargo.lock, all Cashew and local ArrayTrie production source
hashes, bridge inputs, and all pinned formal Lean model hashes. It rechecks
these inputs and the clean formal revision at completion; concurrent source
changes invalidate the run. This detects ordinary edits between snapshots,
not adversarial transient changes restored before the final snapshot.

## What executes

`examples/formal_merkle_batch.rs` calls actual production
`MerkleDictionary::transform`, `prove_transforms`, partial-trie `transform`,
`get`, `serialize_volume`, `Header::new/bytes`, and
`VolumeMerkleDictionary::transform`. It records concrete root and node fields,
loaded nodes versus CID stubs, counts, reads, outcomes, canonical bytes and
actual CIDs. It implements no replacement transform. The original root is
checked unchanged after each attempt. Ordinary and Volume batch outputs must
have the same CID or the exporter fails.

`tools/formal/MerkleBridge.lean` imports unchanged
`LatticeProofs.MerkleBatchProofs` and `MerkleWitness`. It executes the model's
`Root.batch`, reverse-order `Root.applyAll`, `Root.prove`, `Root.lookup/toMap`
and digest definitions on the exported production trie fields. The bridge
only implements structural equality, finite encoding-table lookup and
observation comparison.

The corpus has **385 cases**, of which **125 transform successfully**. It
covers 32 initial maps and 12 distinct batches per map, plus the valueless
branch deletion distinction below. Operations exercise ancestor/descendant
writes, prefix splits, divergent insertion, collapse siblings, all-deletions,
missing updates/deletes, existing-key insertion, empty-key refusal, no-op
batches, zero values and `u64::MAX`. Actual sparse witnesses and fully stubbed
roots are tested separately. The checker compares exact full and partial
result structure/counts, not just a handful of map reads.

Generated Lean obligations prove the upstream `Root.Canon` predicate for
all **385 input roots and 125 successful output roots**, and `Batch.Nodup`
for all 385 batches. These are concrete checked obligations, not a proof that
all Rust-produced roots are canonical. Model batch results must equal actual
Rust results and the model's reverse sequential run. Model-generated and
Rust-generated batch witnesses must preserve the input commitment, and their
successful transforms must yield the same final commitment as the full trie.
Fully stubbed transformations must match production success/failure and
result structure.

## Encoding boundary

The model explicitly abstracts DAG-CBOR and CID text (`Merkle.lean`, `Enc`,
`Trie.digest`, `Root.digest`). The bridge uses a finite table from abstract
`Enc` fields to **actual production CIDs**, exported with each node's bytes.
The Python runner independently recomputes each SHA-256 multihash and canonical
CIDv1 DAG-CBOR base32 string from those bytes, and rejects two distinct abstract
encodings mapping to one observed CID. Every model encoding reached during
comparison must exist in this table; a missing lookup is an explicit failed
check, never an accepted fallback digest.

This is a finite observed encoding relation. It is not a Lean proof of the
Rust serializer, SHA-256, CID parser, byte-to-fields decoder, or collision
resistance. The exporter is a trusted test adapter for projecting Rust structs
into model fields; its source hash and the hashes of production radix,
volume-radix, header and CBOR code are recorded. The theorem `batch_sound`
requires its stated no-collision hypothesis for the relevant encodings; the
finite table check does not establish that assumption for arbitrary adversarial
inputs or unseen encodings.

Keys are **visible ASCII strings**, mapped byte-for-byte to `List Nat` in
Lean. Byte, scalar and grapheme order agree in this restricted domain. Values
are plain **u64 integers**, represented exactly as Lean Nat. Batches are Rust
`BTreeMap<String, Transform<u64>>`, so each key has one operation. Root counts
are production usize values represented as Lean Int; generated canonicality
obligations check the counts equal actual model value cardinality.

This restriction deliberately avoids inventing a refinement for Unicode NFC
and grapheme segmentation, nested multi-segment path transforms, polymorphic
Header leaf values, encrypted metadata/ciphertexts, nonstandard CID codecs, or
Volume boundary acquisition. The batch model itself excludes nested writes
(`MerkleBatch.lean` opening assumptions). Those require separate representation
and refinement work.

## API and theorem distinctions

Production Rust currently implements a batch as transactional sequential
immutable writes. The imported Lean `Root.batch` models the recursive one-pass
Swift implementation. `batch_eq_sequential` in `MerkleBatchProofs.lean` states
when they agree: a canonical trie, a strict key order, unique batch keys and a
permutation of that batch. This bridge verifies those concrete assumptions and
compares the two actual algorithms over the finite corpus; it does not claim
algorithmic or performance identity.

`prove_transforms` in Rust both obtains a witness and checks that transformation
will succeed. Lean `Root.prove` can return a witness for deletion at a valueless
branch even though `Root.batch` then refuses. The named
`delete-valueless-branch` case explicitly checks that the model has a witness,
the actual Rust proof API refuses, and both transforms fail. The bridge does
not require proof-construction outcome equivalence on failed batches. On
successful batches both witnesses must suffice. This follows the actual
`proof_scope` statement rather than the stronger informal phrase “exists
exactly when the batch succeeds.” Witness materialization shape need not equal
between the two proof builders; their commitments and transform results must.

## Negative controls and limits

Six deliberately altered observations are rejected by the same Lean checker:
wrong output count, wrong output CID, changed value, altered proof stub,
false claimed success, and an omitted required encoding-table entry. Each
negative-control Lean file asserts rejection; syntax or elaboration failure
is a harness failure, not a successful negative test. An additional changed
CBOR-byte control must fail independent CID verification.

The report is finite conformance evidence. It does not prove all production
Rust execution refines Lean, does not establish whole-platform consensus or
network safety, and does not validate arbitrary persisted malformed tries.
Production source changes invalidate the recorded hashes until the runner is
rerun. The existing Swift differential and native Cashew tests remain separate
checks rather than being replaced by this bridge.
