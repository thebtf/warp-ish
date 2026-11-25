# Updating the Warp base

Use the accepted fork commit, its recorded upstream base, and one pinned target from the official Warp remote. Record full commit SHAs and the previous accepted Windows ZIP's SHA256. Resolve the target ref to a commit after fetching. Record the upstream ref or release provenance separately.

## Rehearsal

Open a managed candidate through `nvmd-ai-client task start <task-id> --base <accepted-fork-branch>`. In that checkout, confirm that the task branch points to the pinned accepted fork, the tree is clean, and the old base is an ancestor of both the accepted fork and the target. Keep the published branch, release tag and live profiles out of the rehearsal.

```powershell
git status --porcelain
git merge-base --is-ancestor $oldUpstream $acceptedFork
git merge-base --is-ancestor $oldUpstream $targetUpstream
git log --oneline --merges "${oldUpstream}..${acceptedFork}"
git rebase --onto $targetUpstream $oldUpstream
```

Check each exit code. The plain rebase assumes a linear fork range. If the merge log is nonempty, inspect merge-only changes and choose a topology-preserving replay when those changes matter. If rebase stops, classify the affected hunks:

- **Removed platform** — preserve product removals and check files newly added under excluded components.
- **Retained terminal** — take upstream terminal behavior and reapply only the local edits required by the fork contract. Check Windows shell startup, layout and persistence when their interfaces change.
- **Additive MCP and other fork features** — preserve the feature and adapt its integration to upstream API and lifecycle changes. Keep the control endpoint profile-scoped and opt-in.

Stage resolved files and use `git rebase --continue` from the disposable copy. Use `git rebase --abort` there if the assumed base or replay is wrong. Record unexpected drops, deliberate substitutions and unresolved dependencies.

After rebase, pin the new HEAD and tree and compare both intervals.

```powershell
$candidateFork = git rev-parse HEAD
git rev-parse 'HEAD^{tree}'
git range-diff "${oldUpstream}..${acceptedFork}" "${targetUpstream}..${candidateFork}"
git diff --name-status --diff-filter=ACMR $oldUpstream $targetUpstream
git diff --stat $targetUpstream $candidateFork
git diff --check $acceptedFork $candidateFork
python script/warp-ish/maintain.py audit --repo .
```

Inspect the upstream additions and changed entry points even after a conflict-free rebase. Cover dependency and feature edges, startup registrations, settings/actions and packaged services around the affected surfaces. The targeted audit supplements this review. Update the policy pin and observed records from the target tree. Review new or changed inventory before allowing it. Require scoped removals and drift checks to pass before compiling. The limited source audit reports full terminal-only verification separately. Keep the pins, comparison, results and any conflicts under the root project's run records. A rehearsal can stop here with a reproducible candidate. Promotion requires the update checks below.

The audit can exit with code 2 when the scoped checks pass. Check `scoped_removal.status` and `checks.scope_and_drift.status` in the JSON report. The `terminal_only` field tracks broader verification separately.

In WARP-010, the official master target `e3525c726d885830a66474fd468ac8014dbf2ec8` was eleven upstream commits ahead of `75f8808a1c94ee54840f517cfff54595589c32a5`. The first fork commit had 48 modify/delete and eight content conflicts. All nine later fork patches matched their original counterparts. Inspecting new upstream paths found five files for excluded platform features that had arrived without conflicts. After resolving those changes, the only product-tree delta from the accepted fork was an upstream Arch Linux packaging edit.

The WARP-010 rehearsal used one RC4 Windows build and the accepted RC3 archive as its runtime baseline. In a disposable profile, upgrading to RC4 preserved two tabs, one split, three PowerShell working directories, settings and the three seeded history entries. RC3 restored the pre-upgrade snapshot and omitted the RC4-only history entry. Neither restart replayed the seed commands. The packaged MCP adapter returned its usage examples and passed the addressed peer lifecycle. The rehearsal exercised snapshot restoration directly on the disposable profile.

## Update acceptance

1. Use the reviewed, committed candidate. Hydrate the required LFS objects and run affected fast checks first. Build and package once on Windows x64 through the candidate's `script/warp-ish/package-windows.ps1 -Version <new-candidate-version>`. Bind its manifest and SHA256 to the exact tested archive. Follow [ACCEPTANCE.md](ACCEPTANCE.md) for the changes in scope.
2. In a disposable profile, establish settings, shell configuration, layout, pane CWD, command history and any saved blocks supported by the old package. Back up the old state consistently with the app stopped, including SQLite/WAL as applicable. Start the new archive against that profile and check fresh usable shells, state parity and no command replay. Exercise the local MCP peer workflow where kept or affected. Keep the live profile and its selector outside the fixture.
3. Run the old archive against the restored pre-update snapshot. Check state parity and verify that a command added only after the update is absent. Keep the previous package and verified snapshot as rollback inputs.
4. Promote the exact candidate only after acceptance. Keep the prior release, the new archive, checksums/manifests and evidence. Remove spent task-owned build targets after their last use and verify cleanup. Close the managed copy through the normal task workflow. Publish only when separately requested.
