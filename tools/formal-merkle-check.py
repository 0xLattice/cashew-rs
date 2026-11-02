#!/usr/bin/env python3
"""Execute unchanged Lean Merkle models on production Cashew observations."""
import base64,copy,datetime,hashlib,json,pathlib,subprocess,sys
repo=pathlib.Path(__file__).resolve().parents[1];formal=repo.parent/'lattice-formal';lean=formal/'lean';out=repo/'target/formal-merkle';out.mkdir(parents=True,exist_ok=True)
report_path=out/'report.json'
started=datetime.datetime.now(datetime.timezone.utc).isoformat()
def write_report(value):
 temporary=out/'report.json.tmp'
 temporary.write_text(json.dumps(value,indent=2)+'\n')
 temporary.replace(report_path)
# Invalidate the preceding success before checking pins, building, or exporting.
write_report({'passed':False,'started_at':started,'status':'in progress or interrupted'})
def digest_file(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def snapshot():
 paths=[repo/'Cargo.toml',repo/'Cargo.lock',pathlib.Path(__file__).resolve(),repo/'tools/formal/MerkleBridge.lean',repo/'examples/formal_merkle_batch.rs']
 paths+=list((repo/'src').rglob('*.rs'))
 dependency=repo.parent/'array-trie-rs'
 paths+=[dependency/'Cargo.toml']+list((dependency/'src').rglob('*.rs'))
 paths+=list((lean/'LatticeProofs').rglob('*.lean'))
 paths+=[p for p in [lean/'lean-toolchain',lean/'lakefile.toml',lean/'lakefile.lean',lean/'lake-manifest.json'] if p.exists()]
 return {str(p.relative_to(repo.parent)):digest_file(p) for p in sorted(set(paths))}
inputs=snapshot()
pin='a9f4c2879b6565ae11c17d35efe14475c5830978'
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=formal,text=True).strip()==pin
assert not subprocess.check_output(['git','status','--porcelain','--untracked-files=no'],cwd=formal,text=True).strip()
observation=out/'observations.json'
with observation.open('w') as f:subprocess.run(['cargo','run','--quiet','--locked','--example','formal_merkle_batch'],cwd=repo,stdout=f,check=True)
data=json.loads(observation.read_text())
def verify_encodings(rows):
 seen={}
 for row in rows:
  raw=bytes(row['bytes']);cid='b'+base64.b32encode(bytes([1,0x71,0x12,0x20])+hashlib.sha256(raw).digest()).decode().lower().rstrip('=')
  assert cid==row['cid'],'production CBOR bytes do not match SHA256 DAG-CBOR CID'
  if cid in seen:assert seen[cid]==row['enc'],'finite encoding collision'
  seen[cid]=row['enc']
verify_encodings(data['encodings'])
def string(s):return json.dumps(s)
def chars(xs):return '['+','.join(map(str,xs))+']'
def val(v):return 'none' if v is None else f'(some {v})'
def kids(xs):
 r='.nil'
 for k,t in reversed(xs):r=f'(.cons {k} {trie(t)} {r})'
 return r
def trie(t):
 if 'stub' in t:return f'(.stub {string(t["stub"])})'
 p,v,ks=t['node'];return f'(.node {chars(p)} {val(v)} {kids(ks)})'
def root(r):return f'⟨{r["count"]},{kids(r["kids"])}⟩'
def option(r):return 'none' if r is None else f'(some ({root(r)} : Rt))'
def enc(e):
 if 'node' in e:
  p,v,ks=e['node'];return f'(.node {chars(p)} {val(v)} {links(ks)})'
 n,ks=e['root'];return f'(.root {n} {links(ks)})'
def links(xs):return '['+','.join(f'({k},{string(cid)})' for k,cid in xs)+']'
def writes(xs):return '['+','.join(f'({chars(o[0])},.{o[1]}'+(f' {o[2]}' if len(o)>2 else '')+')' for o in xs)+']'
base=(repo/'tools/formal/MerkleBridge.lean').read_text()+'\nopen CashewBridge\n'
table='def table : List (En × String) := ['+','.join(f'({enc(r["enc"])},{string(r["cid"])})' for r in data['encodings'])+']\n'
def case_def(c,name):return f'''def {name} : Case := {{
 before := {root(c['before'])}
 beforeCID := {string(c['beforeCID'])}
 writes := {writes(c['ops'])}
 after := {option(c['after'])}
 afterCID := {'none' if c['afterCID'] is None else '(some '+string(c['afterCID'])+')'}
 proof := {option(c['proof'])}
 partialAfter := {option(c['partialAfter'])}
 stubbed := {root(c['stubbed'])}
 stubbedAfter := {option(c['stubbedAfter'])}
 reads := [{','.join('('+chars(q['key'])+','+val(q['value'])+')' for q in c['queries'])}]
}}
'''
canon='Root.Canon,Kids.Canon,Trie.Canon,Trie.HeadIs,Kids.keys,Kids.size,Trie.size,Kids.length'
body=base+table
for i,c in enumerate(data['cases']):
 name=f'case{i}'
 body+='-- '+c['name']+'\n'+case_def(c,name)+f'#guard check table {name}\n'
 body+=f'example : {name}.before.Canon := by simp [{name},{canon}]\n'
 body+=f'example : {name}.writes.Nodup := by simp [{name},Batch.Nodup]\n'
 if c['name']=='delete-valueless-branch':body+=f'#guard (Root.prove (digest table) {name}.writes {name}.before).isSome && {name}.proof.isNone && {name}.after.isNone\n'
 if c['after'] is not None:body+=f'example : ({root(c["after"])} : Rt).Canon := by simp [{canon}]\n'
positive=out/'Positive.lean';positive.write_text(body)
subprocess.run(['lake','build','LatticeProofs.MerkleBatchProofs'],cwd=lean,check=True,stdout=subprocess.DEVNULL)
result=subprocess.run(['lake','env','lean',str(positive)],cwd=lean,text=True,capture_output=True)
(out/'positive.log').write_text(result.stdout+result.stderr)
if result.returncode:print(result.stdout+result.stderr);raise SystemExit(result.returncode)
negatives={}
# Every mutation must be rejected by the same Lean checker, not a syntax error.
good=next(c for c in data['cases'] if c['after'] and c['after']['count']>0 and c['partialAfter'] and c['proof']['kids'])
for name in ['count','post-cid','value','proof-stub','false-success']:
 c=copy.deepcopy(good)
 if name=='count':c['after']['count']+=1
 elif name=='post-cid':c['afterCID']='bwrong'
 elif name=='value':
  def change(t):
   if 'node' not in t:return False
   if t['node'][1] is not None:t['node'][1]+=1;return True
   return any(change(v) for _,v in t['node'][2])
  assert any(change(v) for _,v in c['after']['kids'])
 elif name=='proof-stub':
  assert c['proof']['kids'];c['proof']['kids'][0][1]={'stub':'bwrong'}
 else:
  c=copy.deepcopy(next(c for c in data['cases'] if c['after'] is None));c['after']=copy.deepcopy(c['before']);c['afterCID']=c['beforeCID']
 path=out/(name+'.lean');path.write_text(base+table+case_def(c,'negative')+'#guard !(check table negative)\n')
 result=subprocess.run(['lake','env','lean',str(path)],cwd=lean,text=True,capture_output=True);(out/(name+'.log')).write_text(result.stdout+result.stderr);assert result.returncode==0,(name,result.stdout,result.stderr);negatives[name]='rejected by Lean check'
# Omitting a required encoding must fail closed rather than use fallback H.
missing=[r for r in data['encodings'] if r['cid']!=good['beforeCID']]
missing_table='def table : List (En × String) := ['+','.join(f'({enc(r["enc"])},{string(r["cid"])})' for r in missing)+']\n'
path=out/'encoding-omission.lean';path.write_text(base+missing_table+case_def(good,'negative')+'#guard !(check table negative)\n')
result=subprocess.run(['lake','env','lean',str(path)],cwd=lean,text=True,capture_output=True);assert result.returncode==0,(result.stdout,result.stderr);negatives['encoding-omission']='rejected by Lean check'
# Independently ensure altered canonical bytes cannot be accepted as a CID row.
rows=copy.deepcopy(data['encodings']);rows[0]['bytes'][0]^=1
try:verify_encodings(rows)
except AssertionError:negatives['cbor-bytes']='rejected by SHA256 CID check'
else:raise AssertionError('corrupt bytes passed')
assert snapshot()==inputs,'source or lockfile changed during bridge run'
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=formal,text=True).strip()==pin
assert not subprocess.check_output(['git','status','--porcelain','--untracked-files=no'],cwd=formal,text=True).strip()
report={'passed':True,'started_at':started,'completed_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'inputs_sha256':inputs,'cargo_lock_sha256':digest_file(repo/'Cargo.lock'),'rustc_version':subprocess.check_output(['rustc','--version','--verbose'],cwd=repo,text=True).strip(),'cargo_version':subprocess.check_output(['cargo','--version'],cwd=repo,text=True).strip(),'formal_revision':pin,'lean_version':subprocess.check_output(['lake','env','lean','--version'],cwd=lean,text=True).strip(),'cases':len(data['cases']),'successful':sum(c['after'] is not None for c in data['cases']),'encoding_rows':len(data['encodings']),'negative_controls':negatives,'observations_sha256':hashlib.sha256(observation.read_bytes()).hexdigest(),'lean_checker_sha256':hashlib.sha256((repo/'tools/formal/MerkleBridge.lean').read_bytes()).hexdigest(),'exporter_sha256':hashlib.sha256((repo/'examples/formal_merkle_batch.rs').read_bytes()).hexdigest(),'production_sha256':{n:hashlib.sha256((repo/'src'/n).read_bytes()).hexdigest() for n in ['radix.rs','volume_radix.rs','cbor.rs','header.rs']}}
write_report(report);print(json.dumps({k:v for k,v in report.items() if k!='inputs_sha256'},indent=2))
