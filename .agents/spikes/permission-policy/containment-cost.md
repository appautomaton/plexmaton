# What confinement costs per command

| Field | Value |
| --- | --- |
| Read when | Deciding whether a per-command wrapper is affordable, or writing a profile's paths |
| Status | macOS measured on one host; Linux and Windows unmeasured |
| Basis | [seatbelt-cost](./seatbelt-cost.py), 60 runs per arm, against the write-fence profile [seatbelt-probe](./seatbelt-probe.py) proves correct (13/13 on 2026-09-19) |

The spike deferred containment partly because nobody knew what it cost, here or in any of the five
compared sources. On macOS it is now known.

| Command | Bare | Confined | Added |
| --- | --- | --- | --- |
| `true` | 3.7 ms | 9.6 ms | 5.9 ms (2.6x) |
| one write | 4.2 ms | 10.7 ms | 6.5 ms (2.6x) |
| three-stage pipeline | 6.9 ms | 12.8 ms | 5.9 ms (1.8x) |

It is a constant, not a proportion: roughly six milliseconds per invocation whatever the command
does. The multiple falls as the command does more, and a command taking a second pays under one
percent. Cost is therefore not a reason to defer containment on macOS. It says nothing about Linux,
Windows, or the throughput of a wrapped toolchain running thousands of short processes.

## The constraint that only running it exposes

A `subpath` parameter must be a **resolved** path. The macOS temporary root arrives as `/var/...`
while the kernel matches `/private/var/...`. The unresolved form is accepted as a valid profile and
grants nothing: a write fence that looks applied, reports no error, and denies the roots it was told
to allow. Any writable root reaching a profile — a user's workspace included — is resolved first.

This is the shape of failure [the spike](./README.md) already warns about in other systems: a
boundary that looks enforced and is not. Here it costs one `Path.resolve()`.
