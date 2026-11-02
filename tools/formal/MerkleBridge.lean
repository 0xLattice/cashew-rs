import LatticeProofs.MerkleBatchProofs
import LatticeProofs.MerkleWitness
open Lattice.Merkle
set_option linter.unusedSimpArgs false
set_option maxRecDepth 20000
set_option maxHeartbeats 8000000
namespace CashewBridge
abbrev Rt := Root Nat Nat String
abbrev Tr := Trie Nat Nat String
abbrev Ks := Kids Nat Nat String
abbrev En := Enc Nat Nat String
mutual
def trieEq : Tr → Tr → Bool
 | .stub a, .stub b => a == b
 | .node p v ks, .node q w ls => p == q && v == w && kidsEq ks ls
 | _, _ => false
def kidsEq : Ks → Ks → Bool
 | .nil, .nil => true
 | .cons c t ks, .cons d u ls => c == d && trieEq t u && kidsEq ks ls
 | _, _ => false
end
def rootEq (a b : Rt) : Bool := a.count == b.count && kidsEq a.kids b.kids
def optionalRootEq : Option Rt → Option Rt → Bool
 | none, none => true
 | some a, some b => rootEq a b
 | _, _ => false
def encEq : En → En → Bool
 | .node p v ks, .node q w ls => p == q && v == w && ks == ls
 | .root n ks, .root m ls => n == m && ks == ls
 | _, _ => false
/-- Missing encodings never silently acquire an arbitrary valid digest. -/
def digest (table : List (En × String)) (e : En) : String :=
 ((table.find? fun row => encEq row.1 e).map (·.2)).getD "MISSING_ENCODING"
mutual
def knownTrie (h : En → String) : Tr → Bool
 | .stub _ => true
 | .node p v ks => knownKids h ks && h (.node p v (ks.digests h)) != "MISSING_ENCODING"
def knownKids (h : En → String) : Ks → Bool
 | .nil => true
 | .cons _ t rest => knownTrie h t && knownKids h rest
end
def knownRoot (h : En → String) (r : Rt) : Bool :=
 knownKids h r.kids && r.digest h != "MISSING_ENCODING"
def agreesDigest (h : En → String) (a b : Option Rt) : Bool :=
 (a.map (·.digest h)) == (b.map (·.digest h))
structure Case where
 before : Rt
 beforeCID : String
 writes : Batch Nat Nat
 after : Option Rt
 afterCID : Option String
 proof : Option Rt
 partialAfter : Option Rt
 stubbed : Rt
 stubbedAfter : Option Rt
 reads : List (List Nat × Option Nat)
/-- All transforms and witness construction below are the imported upstream
model; the bridge only compares observations and checks finite table coverage. -/
def check (table : List (En × String)) (c : Case) : Bool := Id.run do
 let h := digest table
 let actual := c.before.batch c.writes
 let modelProof := Root.prove h c.writes c.before
 let proofAfter := c.proof.bind (·.batch c.writes)
 return knownRoot h c.before && c.before.digest h == c.beforeCID &&
   optionalRootEq actual c.after &&
   optionalRootEq (c.before.applyAll c.writes.reverse) c.after &&
   c.after.all (knownRoot h) && c.after.map (·.digest h) == c.afterCID &&
   c.after.all (fun r => c.reads.all fun (k,v) => r.toMap k == v) &&
   (c.after.isSome == c.proof.isSome) &&
   c.proof.all (fun p => knownRoot h p && p.digest h == c.beforeCID) &&
   optionalRootEq proofAfter c.partialAfter &&
   c.partialAfter.all (knownRoot h) && agreesDigest h c.partialAfter c.after &&
   modelProof.all (fun p => knownRoot h p && p.digest h == c.beforeCID) &&
   (modelProof.bind (·.batch c.writes)).all (knownRoot h) &&
   agreesDigest h (modelProof.bind (·.batch c.writes)) c.after &&
   knownRoot h c.stubbed && c.stubbed.digest h == c.beforeCID &&
   optionalRootEq (c.stubbed.batch c.writes) c.stubbedAfter
end CashewBridge
