# Standard — Git workflow

| Field | Value |
| --- | --- |
| Trigger | Starting or resuming development; creating, merging or retiring a branch/worktree |
| Owns | Primary checkout, task isolation, merge completion and cleanup authority |

## Ownership

[AGENTS.md](../../AGENTS.md#working-discipline) owns the default: the primary checkout stays on
`main`, and development happens in task worktrees. A task branch and its worktree are temporary
resources owned by the task's primary agent; delegates remain read-only. Merging starts retirement;
cleanup completes it.
Git refs, registered worktrees and the PR are the inventory; do not maintain a second registry.

Permission to commit, publish or merge still follows AGENTS.md. An authorized merge includes
syncing the primary checkout and retiring that task's verified merged worktree and local/remote
branches. Do not ask separately for each cleanup step. A request to prepare changes or open a PR
does not authorize merging. A local-only merge authorizes local retirement; upstream sync and
remote deletion require authorization for upstream integration. Preserve a branch if the user
explicitly asks to keep it.

Rejected: treating worktree removal and branch deletion as optional follow-up chores, which left
merged branches for the user to rediscover; and reusing merged branches for new tasks, which makes
old PR evidence ambiguous.

## Start or resume

1. Inspect `git worktree list --porcelain`, branch tips and working-tree status. Resolve the primary
   checkout from Git, even when starting inside a linked worktree; resolve `.worktrees/` against
   that primary path.
2. Resume an existing task in its own worktree. For new work, create a fresh task branch and
   `.worktrees/<name>/` under the primary checkout before editing, including small docs/CI fixes.
   Use explicit working directories for edits and checks. Do not switch the primary checkout to
   the task branch or borrow another task's checkout.
3. Base new work on current `main`. When upstream access is authorized, fetch/prune and
   fast-forward a clean primary `main` first; otherwise use local `main` and report that freshness
   was not checked. Never reset, stash, discard changes or create a merge commit to make the
   primary checkout appear synchronized. If it is dirty or divergent, preserve it and create the
   task worktree from the verified base without modifying that checkout.

From the primary checkout, for a new task:

```console
git worktree add .worktrees/<task> -b <type>/<task> main
```

[Quality gates §Parallel checkouts](./quality-gates.md#parallel-checkouts) owns independent build
directories. Worktree storage is temporary: keep durable evidence in tracked task files and
runtime state outside the worktree. Paused or unmerged tasks retain their branch and worktree.

## Merge and retire

The task owner completes this sequence in the same workflow as the authorized merge. On resuming
a task merged by somebody else, verify and finish its retirement before opening new work.

1. Use the intended PR and base, verify required checks for the head being merged, and bind the
   merge to that head (`gh pr merge <number> --match-head-commit <head> ...`). A queued merge is
   still pending; wait for confirmed `MERGED` state before cleanup.
2. Fetch the base. Verify the PR merged into `main`, its merge commit is reachable from current
   `origin/main`, and each branch being deleted still points to the PR's merged head. Inspect
   local and remote tips separately: local unpushed commits and later remote commits both block
   deletion of the affected branch. A remote already deleted by GitHub is fine. For a direct
   local merge, prove the entire task tip is an ancestor of `main`. Retain an unrecorded local
   squash for review rather than inferring safety from tree equality alone. Retire a remote branch
   only after its result is also in the upstream `main`.
3. Stop this task's owned processes and establish exclusive ownership for retirement. Inspect
   `git -C <path> status --porcelain=v1 --untracked-files=all --ignored=matching`.
   Disposable build output such as `target/` can go; preserve private runtime state, exports and
   other valuable files outside the worktree first. Leave dirty, locked or actively used worktrees
   intact and report why retirement is blocked. Never remove another task's resources.
4. From outside the retiring worktree, recheck that the primary checkout is on `main` and
   `git status --porcelain=v1 --untracked-files=all` is empty. Fast-forward it to `origin/main`
   (or verify it already contains an authorized local merge).
5. Delete any remaining task branch on the remote using an explicit expected-tip lease, so a
   concurrent push cannot be erased. If the lease fails, retain the local branch and worktree.
   For local-only retirement, leave the remote intact and report it.
6. Recheck the local tip and worktree status, then use `git worktree remove <path>` and delete the
   verified local branch. Do not use `rm -rf` or force worktree removal. `git branch -D <branch>`
   is permitted only with exclusive task ownership and the merge proof above when squash/rebase
   history prevents `-d`. Prune remote-tracking refs when upstream access is authorized and verify
   the task branch and worktree are gone. Missing resources are already complete, not errors to
   repair or recreate. If a step fails, preserve what remains and report the unfinished step.

Remote deletion, with placeholders filled from the verified PR and current ref:

```console
git push --force-with-lease=refs/heads/<branch>:<verified-head> origin :refs/heads/<branch>
git fetch origin --prune
```

Do not infer merge safety from age, a missing upstream, or a successful `git branch -d`: that
command may check the branch's upstream rather than `main`. Squash merges also make
`git branch --merged` insufficient. Use the PR record and tip checks above.
[Git branch semantics](https://git-scm.com/docs/git-branch) and
[explicit push leases](https://git-scm.com/docs/git-push) define these checks.

## Handoff

For pending work, report the branch/worktree and pending review, commit or CI step. For merged
work, report the merge, primary sync, and completed cleanup together; identify any retained
resource and the concrete reason. Cleanup of the owning task is the default, not a periodic scan
that deletes every apparently stale branch.

GitHub's [automatic head-branch deletion](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-the-automatic-deletion-of-branches)
can reduce remote leftovers if enabled by the repository owner; it does not remove local branches
or registered worktrees. This standard governs agent
behavior; it is not hook enforcement or a background cleanup service.
