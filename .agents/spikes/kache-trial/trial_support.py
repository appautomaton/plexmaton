import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import threading
import time

ROOT = Path(os.environ["PLEXMATON_KACHE_TRIAL"]).resolve()
WORKTREE = Path(os.environ["PLEXMATON_WORKTREE"]).resolve()
if not ROOT.is_relative_to(Path("/private/tmp")):
    raise RuntimeError("Use an owned system-temporary directory for this macOS trial")
TOOL = Path(os.environ.get("PLEXMATON_KACHE_TOOL", str(ROOT / "tool/kache")))
CACHE = ROOT / ("metered-cache" if TOOL.name == "kache-metered" else "cache")
RUNTIME = ROOT / ("metered-runtime" if TOOL.name == "kache-metered" else "runtime")
CARGO = Path(subprocess.check_output(["rustup", "which", "cargo"], cwd=WORKTREE, text=True).strip())
RUSTC = CARGO.with_name("rustc")
CLANG = Path(subprocess.check_output(["xcrun", "--find", "clang"], text=True).strip())
SDK = subprocess.check_output(["xcrun", "--show-sdk-path"], text=True).strip()

def environment(cache=False, collector=None):
    keep = ("PATH", "HOME", "USER", "LOGNAME", "LANG", "LC_ALL", "SHELL", "TMPDIR", "RUSTUP_HOME", "CARGO_HOME", "PLEXMATON_KACHE_TRIAL", "PLEXMATON_WORKTREE", "PLEXMATON_PRODUCER", "PLEXMATON_KACHE_TOOL")
    env = {k: os.environ[k] for k in keep if k in os.environ}
    env.update(RUSTC=str(RUSTC), CARGO_INCREMENTAL="1", CARGO_NET_OFFLINE="true",
               CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER=str(CLANG), SDKROOT=SDK,
               CC=str(CLANG), CXX=str(CLANG.with_name("clang++")), HOST_CC=str(CLANG), HOST_CXX=str(CLANG.with_name("clang++")),
               KACHE_CONFIG=str(ROOT/"kache.toml"), KACHE_CACHE_DIR=str(CACHE),
               KACHE_RUNTIME_DIR=str(RUNTIME), KACHE_LOCAL_ONLY="1", KACHE_MAX_SIZE="2GiB", KACHE_LOG_FILE="off")
    if cache:
        env["RUSTC_WRAPPER"] = str(TOOL)
    if collector:
        env.update(DYLD_INSERT_LIBRARIES=str(ROOT/"io-account.dylib"), PLEXMATON_IO_SOCKET=str(collector.path))
    return env

class Collector:
    def __init__(self, label):
        (ROOT/"io").mkdir(exist_ok=True)
        self.path = ROOT/"io"/(label+".sock")
        self.socket = socket.socket(socket.AF_UNIX,socket.SOCK_DGRAM)
        self.socket.bind(str(self.path))
        self.socket.settimeout(0.1)
        self.events = []
        self.errors = []
        self.closed = False
        self.condition = threading.Condition()
        self.thread = threading.Thread(target=self.receive)
        self.thread.start()

    def receive(self):
        while not self.closed:
            try:
                data = self.socket.recv(4096)
            except socket.timeout:
                continue
            except OSError:
                return
            try:
                event=json.loads(data)
            except Exception as exc:
                self.errors.append(str(exc))
                continue
            with self.condition:
                self.events.append(event)
                self.condition.notify_all()

    def wait_end(self,pid,timeout=5):
        with self.condition:
            return self.condition.wait_for(lambda:any(e["event"]=="end" and e["pid"]==pid for e in self.events),timeout)

    def close(self):
        self.closed=True
        self.thread.join(timeout=2)
        self.socket.close()
        self.path.unlink(missing_ok=True)

    def summary(self):
        groups={}
        for e in self.events:
            key=(e["pid"],e["start"])
            groups.setdefault(key,[]).append(e)
        finished=[]; unfinished=[]
        for key,events in groups.items():
            ends=[e for e in events if e["event"]=="end"]
            starts=[e for e in events if e["event"]=="start"]
            if len(starts)!=1 or len(ends)!=1 or any(e["rc"]!=0 for e in events):
                unfinished.append(events[-1]); continue
            end=ends[-1]
            start=starts[0]
            finished.append(dict(pid=key[0],name=end["name"],package=end.get("package",""),
                disk_write=max(0,end["disk_write"]-start["disk_write"]),
                disk_read=max(0,end["disk_read"]-start["disk_read"]),rc=end["rc"]))
        return dict(process_disk_write_bytes=sum(e["disk_write"] for e in finished),
                    process_disk_read_bytes=sum(e["disk_read"] for e in finished),
                    completed_processes=len(finished),unfinished=unfinished,errors=self.errors,processes=finished)

def stop_group(process):
    if process.poll() is None:
        os.killpg(process.pid,signal.SIGTERM)
        try: process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid,signal.SIGKILL); process.wait(timeout=5)

def start_daemon(env,log):
    process=subprocess.Popen([str(TOOL),"daemon","run"],env=env,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
    deadline=time.monotonic()+15
    while time.monotonic()<deadline:
        if process.poll() is not None:
            raise RuntimeError("Kache daemon exited before readiness")
        endpoint=RUNTIME/"daemon.sock"
        if endpoint.exists():
            probe=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM)
            try:
                probe.connect(str(endpoint)); return process
            except OSError: pass
            finally: probe.close()
        time.sleep(0.02)
    stop_group(process)
    raise TimeoutError("Kache daemon readiness")

def execute(label,command,cwd,cache=False,expected=0,extra_env=None,instrument=True):
    logs=ROOT/"logs"; logs.mkdir(exist_ok=True)
    collector=Collector(label)
    env=environment(cache,collector if instrument else None)
    if cache:
        env["KACHE_EVENT_ROOT"]=str(cwd)
    env.update(extra_env or {})
    daemon=None; process=None; daemon_log=None
    try:
        if cache:
            daemon_log=(logs/(label+"-daemon.log")).open("w")
            daemon=start_daemon(env,daemon_log)
        print(label+": running",flush=True)
        started=time.monotonic()
        with (logs/(label+".out")).open("w") as out, (logs/(label+".err")).open("w") as err:
            process=subprocess.Popen(command,cwd=cwd,env=env,stdout=out,stderr=err,start_new_session=True)
            code=process.wait(timeout=300)
        elapsed=time.monotonic()-started
        root_end=collector.wait_end(process.pid) if instrument else None
        if daemon:
            subprocess.run([str(TOOL),"daemon","stop"],env=environment(cache),stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=15,check=True)
            daemon.wait(timeout=15)
            daemon_end=collector.wait_end(daemon.pid) if instrument else None
        else: daemon_end=True if instrument else None
        result=dict(label=label,seconds=round(elapsed,3),exit_code=code,instrumented=instrument,root_end=root_end,daemon_end=daemon_end,**collector.summary())
        if not instrument:
            result["process_disk_write_bytes"]=None
            result["process_disk_read_bytes"]=None
        (logs/(label+"-io.json")).write_text(json.dumps(result,indent=2)+"\n")
        (logs/(label+"-events.json")).write_text(json.dumps(collector.events,indent=2)+"\n")
        print(json.dumps({k:v for k,v in result.items() if k not in ("processes","unfinished")}),flush=True)
        if code != expected: raise RuntimeError(f"{label} returned {code}; expected {expected}")
        return result
    finally:
        if process: stop_group(process)
        if daemon: stop_group(daemon)
        if daemon_log: daemon_log.close()
        collector.close()

if __name__=="__main__":
    for label,mode in [("calibration-sync","sync"),("calibration-nosync","nosync")]:
        execute(label,[str(ROOT/"io-calibration"),mode,str(ROOT/(label+".data"))],ROOT)
    execute("calibration-clone",[str(ROOT/"io-calibration"),"clone",str(ROOT/"calibration-sync.data"),str(ROOT/"calibration-clone.data")],ROOT)
