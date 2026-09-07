import os
from pathlib import Path
import subprocess
import sys
from trial_support import CARGO

processes=[]
for arg in sys.argv[1:]:
    root=Path(arg)
    env=os.environ.copy();env['KACHE_EVENT_ROOT']=str(root)
    processes.append(subprocess.Popen([str(CARGO),'build','--offline','--locked','--jobs','3'],cwd=root,env=env))
codes=[p.wait(timeout=60) for p in processes]
if any(codes): raise SystemExit(str(codes))
