import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
from fixture_trial import create,verify,LIB
from trial_support import ROOT,CARGO,RUSTC,TOOL,environment,execute,start_daemon,stop_group

OPTIONS={'KACHE_PRESERVE_INCREMENTAL':'1','KACHE_CACHE_EXECUTABLES':'0','CACHE_SIGNAL':'same'}

left=create('concurrent-left');right=create('concurrent-right')
for root,marker in [(left,'left'),(right,'right')]:
    (root/'Cargo.toml').write_text((root/'Cargo.toml').read_text()+'\n[profile.dev.package.portable]\nincremental = false\n')
    (root/'src/lib.rs').write_text(LIB.replace('"one"','"'+marker+'"'))
    (root/'portable/src/lib.rs').write_text('pub fn answer() -> u32 { 42 }\n')
execute('concurrent-different-code',[sys.executable,str(Path(__file__).with_name('concurrent_builds.py')),str(left),str(right)],ROOT,cache=True,extra_env=OPTIONS,instrument=False)
verify(left,'concurrent-left',marker='left',portable='42')
verify(right,'concurrent-right',marker='right',portable='42')

root=create('cancel')
(root/'Cargo.toml').write_text((root/'Cargo.toml').read_text()+'\n[profile.dev.package.portable]\nincremental = false\n')
proxy=ROOT/'compiler-proxy/rustc';proxy.parent.mkdir()
proxy.write_text('#!'+sys.executable+'\n'+'''import os,sys
from pathlib import Path
args=sys.argv[1:]
if os.environ.get("PLEXMATON_BLOCK_CRATE")==os.environ.get("CARGO_PKG_NAME") and any(a.startswith("--emit=") and "link" in a for a in args) and not any(a.startswith("--print") for a in args):
    Path(os.environ["PLEXMATON_READY"]).write_text(str(os.getpid()))
    with open(os.environ["PLEXMATON_GATE"],"rb",buffering=0) as gate: gate.read(1)
os.execv('''+repr(str(RUSTC))+''', ['''+repr(str(RUSTC))+''', *args])
''')
proxy.chmod(0o755)
ready=ROOT/'cancel.ready'; gate=ROOT/'cancel.fifo';os.mkfifo(gate)
env=environment(True);env.update(OPTIONS,RUSTC=str(proxy),KACHE_NAMESPACE='cancel-fixture',KACHE_EVENT_ROOT=str(root),PLEXMATON_BLOCK_CRATE='portable',PLEXMATON_READY=str(ready),PLEXMATON_GATE=str(gate))
daemon=None;process=None
with (ROOT/'logs/cancel-daemon.txt').open('w') as daemon_log, (ROOT/'logs/cancel-build.txt').open('w') as log:
    try:
        daemon=start_daemon(env,daemon_log)
        process=subprocess.Popen([str(CARGO),'build','--offline','--locked','--jobs','3'],cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
        deadline=time.monotonic()+30
        while not ready.exists():
            if process.poll() is not None: raise RuntimeError('Build exited before cancellation barrier')
            if time.monotonic()>deadline: raise TimeoutError('Cancellation barrier')
            time.sleep(0.02)
        blocked_pid=int(ready.read_text())
        assert os.getpgid(blocked_pid)==process.pid
        stop_group(process)
        cancelled_code=process.returncode
        print('cancelled compiler at owned barrier:',blocked_pid,'cargo exit:',cancelled_code,flush=True)
        subprocess.run([str(TOOL),'daemon','stop'],env=environment(True),check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        daemon.wait(timeout=15)
    finally:
        if process:stop_group(process)
        if daemon:stop_group(daemon)
        gate.unlink(missing_ok=True)
recovery=execute('cancel-recovery',[str(CARGO),'build','--offline','--locked','--jobs','3'],root,cache=True,extra_env={**OPTIONS,'RUSTC':str(proxy),'KACHE_NAMESPACE':'cancel-fixture'},instrument=False)
verify(root,'cancel')
(ROOT/'concurrency-cancellation.json').write_text(json.dumps({'concurrent_different_code':'passed','cancelled_at_compile_barrier':True,'cancelled_exit':cancelled_code,'recovery':'passed','recovery_seconds':recovery['seconds']},indent=2)+'\n')
