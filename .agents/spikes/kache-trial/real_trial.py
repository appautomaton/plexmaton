import json
import os
from pathlib import Path
import subprocess
from trial_support import ROOT,CARGO,TOOL,environment,execute

WORKTREE=Path(os.environ['PLEXMATON_WORKTREE']).resolve()
PRODUCER=Path(os.environ['PLEXMATON_PRODUCER']).resolve()
if WORKTREE == PRODUCER:
    raise RuntimeError('Producer and consumer must be distinct worktrees')
REVISIONS={}
for root in (WORKTREE, PRODUCER):
    top=Path(subprocess.check_output(['git','rev-parse','--show-toplevel'],cwd=root,text=True).strip()).resolve()
    if top != root:
        raise RuntimeError('Expected a worktree root: '+str(root))
    REVISIONS[str(root)]=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()
    changed=subprocess.check_output(['git','status','--porcelain','--untracked-files=all','--','Cargo.toml','Cargo.lock','rust-toolchain.toml','.cargo','crates'],cwd=root)
    if changed:
        raise RuntimeError('Build inputs must be clean: '+str(root))
if len(set(REVISIONS.values())) != 1:
    raise RuntimeError('Producer and consumer must have the same HEAD')
METERED=TOOL.name=='kache-metered'
MODE='meter' if METERED else 'official'
OPTIONS={'KACHE_PRESERVE_INCREMENTAL':'1','KACHE_CACHE_EXECUTABLES':'0'}
SOURCE=WORKTREE/'crates/plexmaton-tui/src/theme.rs'
ORIGINAL=SOURCE.read_bytes()
NEEDLE=b'    pub fn style(&self, role: Role) -> Style {'
assert ORIGINAL.count(NEEDLE)==1
assert not subprocess.check_output(['git','diff','HEAD','--',str(SOURCE)],cwd=WORKTREE)
RESULTS=[]

def run(label,worktree,target,cache):
    if target.parent != ROOT/'targets': raise RuntimeError('Unexpected trial target')
    command=[str(CARGO),'test','-p','plexmaton-tui','--lib','--no-run','--offline','--locked','--jobs','6','--target-dir',str(target),'--message-format=json-render-diagnostics']
    result=execute(label,command,worktree,cache=cache,extra_env=OPTIONS if cache else {},instrument=METERED)
    if METERED and (not result['root_end'] or not result['daemon_end'] or result['unfinished'] or result['errors']):
        raise RuntimeError('Incomplete measurement coverage')
    result['target_bytes']=int(subprocess.check_output(['du','-sk',str(target)],text=True).split()[0])*1024
    result['source_sha256']=__import__('hashlib').sha256(SOURCE.read_bytes()).hexdigest()
    result['revisions']=REVISIONS
    if cache:
        report=ROOT/'logs'/(label+'-cache.json')
        subprocess.run([str(TOOL),'report','--format','json','--last-build','--root',str(worktree),'--output',str(report)],env=environment(True),check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        details=json.loads(report.read_text())
        result['cache_summary']=details['summary']; result['cache_storage']=details['storage'];result['cache_bypass']=details['bypass']
    RESULTS.append(result)
    (ROOT/(MODE+'-real-results.json')).write_text(json.dumps(RESULTS,indent=2)+'\n')
    return result

def edited_runs(prefix,target,cache):
    try:
        for index,attribute in enumerate([b'#[inline(never)]',b'#[inline]',b'#[inline(always)]'],1):
            SOURCE.write_bytes(ORIGINAL.replace(NEEDLE,b'    '+attribute+b'\n'+NEEDLE))
            run(prefix+'-edit'+str(index),WORKTREE,target,cache)
    finally:
        SOURCE.write_bytes(ORIGINAL)

if __name__=='__main__':
    baseline=ROOT/'targets'/(MODE+'-plain')
    producer=ROOT/'targets'/(MODE+'-producer')
    consumer=ROOT/'targets'/(MODE+'-consumer')
    for p in (baseline,producer,consumer):
        if p.exists(): raise SystemExit('Refusing existing trial target: '+str(p))
    try:
        run(MODE+'-plain-cold',WORKTREE,baseline,False)
        edited_runs(MODE+'-plain',baseline,False)
        run(MODE+'-producer-cold',PRODUCER,producer,True)
        run(MODE+'-consumer-reuse',WORKTREE,consumer,True)
        edited_runs(MODE+'-consumer',consumer,True)
        run(MODE+'-consumer-restored',WORKTREE,consumer,True)
    finally:
        SOURCE.write_bytes(ORIGINAL)
