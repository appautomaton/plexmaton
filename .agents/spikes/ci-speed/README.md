# CI speed

| Field | Value |
| --- | --- |
| Read when | Changing CI scheduling or cache policy |
| Question | Can independent checks overlap Rust verification without reducing coverage? |
| Status | Parallel lanes implemented; PR checks own GitHub validation |

## Baseline

Seven successful warm-cache runs sampled on 2026-09-07 had a median job time of **229 s**
(range 204–262 s). The [latest main run](https://github.com/appautomaton/plexmaton/actions/runs/34078042431)
at `78c6e9c` took 206 s. These runs span different revisions; they are observational timings.

| Stage | Warm-run median |
| --- | ---: |
| Rust compilation and tests | 73 s |
| Python script tests | 43 s |
| Permissions smoke | 41 s |
| Rust setup and cache | 17 s |
| Clippy | 15 s |

All seven runs restored the same roughly 247 MB dependency cache. CPU utilization was not logged.
Use GitHub results to assess speedups; local workstation timings do not establish CI gains.

## Change and validation

The [workflow](../../../.github/workflows/ci.yml) runs static/supply-chain/Python checks alongside
Rust/PTY verification. The Rust job retains its `verify` identity and target cache; the checks job
omits target artifacts from its cache. Both execute on macOS. A final result-only Ubuntu job keeps
the `macOS Apple Silicon` check name and requires both lanes to succeed.

All 15 original validation commands are retained. Local structural validation also checked all
16 success/failure/cancellation/skip combinations for the aggregate gate. Use the PR checks for
GitHub results and elapsed time. The tradeoff is another macOS runner, so assess total runner
usage as well as PR feedback time.

```console
gh run view 34078042431 --json headSha,jobs,url
```
