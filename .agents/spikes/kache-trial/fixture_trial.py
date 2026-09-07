import json
from pathlib import Path
import subprocess
from trial_support import ROOT,CARGO,TOOL,environment,execute

FIXTURES=ROOT/"fixtures"
MANIFEST='''[package]
name = "kache_signal"
version = "0.0.0"
edition = "2024"
[dependencies]
portable = { path = "portable" }
[profile.dev]
debug = "line-tables-only"
'''
LIB='''pub fn marker() -> &'static str { "one" }
pub fn origin() -> &'static str { env!("CARGO_MANIFEST_DIR") }
pub fn embedded() -> &'static str { include_str!("../data.txt") }
pub fn flag() -> &'static str { env!("CACHE_SIGNAL") }
'''
MAIN='''fn main() {
    println!("{}", kache_signal::marker());
    println!("{}", kache_signal::origin());
    println!("{}", kache_signal::embedded());
    println!("{}", kache_signal::flag());
    println!("{}", std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("runtime.txt")).unwrap());
    println!("{}", portable::answer());
}
'''

def create(name):
    root=FIXTURES/name
    root.mkdir(parents=True,exist_ok=False)
    (root/"src").mkdir()
    (root/"portable/src").mkdir(parents=True)
    (root/"Cargo.toml").write_text(MANIFEST)
    (root/"Cargo.lock").write_text('version = 4\n\n[[package]]\nname = "kache_signal"\nversion = "0.0.0"\ndependencies = ["portable"]\n\n[[package]]\nname = "portable"\nversion = "0.0.0"\n')
    (root/"src/lib.rs").write_text(LIB)
    (root/"src/main.rs").write_text(MAIN)
    (root/"data.txt").write_text("embedded-one")
    (root/"runtime.txt").write_text("runtime-"+name)
    (root/"portable/Cargo.toml").write_text('[package]\nname = "portable"\nversion = "0.0.0"\nedition = "2024"\n')
    (root/"portable/src/lib.rs").write_text('pub fn answer() -> u32 { 41 }\n')
    return root

def build(label,root,flag="same",expected=0):
    result=execute(label,[str(CARGO),"build","--offline","--locked","--jobs","6","--message-format=json-render-diagnostics"],root,cache=True,expected=expected,extra_env={"CACHE_SIGNAL":flag},instrument=TOOL.name=="kache-metered")
    report=ROOT/"logs"/(label+"-cache.json")
    subprocess.run([str(TOOL),"report","--format","json","--last-build","--root",str(root),"--output",str(report)],env=environment(True),check=True,stdout=subprocess.DEVNULL)
    return result

def verify(root,name,marker="one",embedded="embedded-one",flag="same",portable="41"):
    actual=subprocess.check_output([str(root/"target/debug/kache_signal")],cwd=root,env=environment(),text=True).splitlines()
    expected=[marker,str(root),embedded,flag,"runtime-"+name,portable]
    if actual != expected: raise AssertionError({"actual":actual,"expected":expected})
    print("correct:",name,marker,embedded,flag,portable,flush=True)

if __name__=="__main__":
    a=create("a"); b=create("b")
    results=[]
    results.append(build("fixture-a-cold",a)); verify(a,"a")
    results.append(build("fixture-b-reuse",b)); verify(b,"b")
    a.rename(FIXTURES/"retired-a")
    verify(b,"b")
    (b/"src/lib.rs").write_text(LIB.replace('"one"','"two"'))
    results.append(build("fixture-b-source",b)); verify(b,"b",marker="two")
    (b/"data.txt").write_text("embedded-two")
    results.append(build("fixture-b-include",b)); verify(b,"b",marker="two",embedded="embedded-two")
    results.append(build("fixture-b-env",b,flag="changed")); verify(b,"b",marker="two",embedded="embedded-two",flag="changed")
    (b/"src/lib.rs").write_text("this is invalid Rust\n")
    results.append(build("fixture-b-failure",b,flag="changed",expected=101))
    (b/"src/lib.rs").write_text(LIB.replace('"one"','"recovered"'))
    results.append(build("fixture-b-recovery",b,flag="changed")); verify(b,"b",marker="recovered",embedded="embedded-two",flag="changed")
    (ROOT/"fixture-results.json").write_text(json.dumps(results,indent=2)+"\n")
