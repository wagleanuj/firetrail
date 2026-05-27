# M1 scenario suite

**Epic:** `firetrail-939`
**Wave:** 5 (after all other M1 epics)
**Depends on:** every other M1 epic
**Depended on by:** the M1 release gate

---

## Purpose

End-to-end black-box tests that prove the M1 promise. Scenarios run the actual
`firetrail` binary against a fixture workspace and assert observable state.
Failure of any M1 scenario blocks the M1 release gate.

These are Layer 4 tests in the test harness (see ADR-0016). They are slow
relative to unit and property tests but catch integration bugs the lower layers
miss.

---

## Scenario file location

```
tests/scenarios/
├── m1-happy-path.scenario
├── m1-ac-enforcement.scenario
├── m1-conflict-resolution.scenario
├── m1-index-rebuild.scenario
└── m1-clean-clone.scenario
```

Format: YAML (see `docs/components/ft-testkit.md` for the schema).

---

## Required scenarios

### `m1-happy-path.scenario`

Walks through the full M1 user journey for one engineer.

Steps:

1. `firetrail init` in a fresh git repo.
2. `firetrail epic create "Improve checkout reliability"` — capture epic ID.
3. `firetrail task create "Add Redis pool alert" --epic ${epic_id}` — capture
   task A.
4. `firetrail task create "Add retry budget" --epic ${epic_id}` — capture
   task B.
5. `firetrail dep add ${task_b} ${task_a} --type blocked-by` — B blocked by A.
6. `firetrail criteria add ${task_a} "Alert fires when Redis pool usage exceeds 85% for 5 minutes"`
7. `firetrail criteria add ${task_a} "Alert includes service, environment, pool name"`
8. `firetrail ready --json` — assert only task A is ready, not task B.
9. `firetrail claim ${task_a}` — succeeds.
10. `firetrail close ${task_a}` — fails with exit 1 (ACs incomplete).
11. `firetrail criteria check ${task_a} 1` and `firetrail criteria check ${task_a} 2`.
12. `firetrail close ${task_a}` — succeeds.
13. `firetrail ready --json` — assert task B is now ready (A is closed).
14. `firetrail board --json` — assert task A is in DONE, task B is in TODO.
15. `firetrail graph ${epic_id}` — assert the tree shows the epic with both
    tasks, marking A closed and B open.

Acceptance: all 15 steps succeed without manual intervention; total runtime
under 5 seconds.

---

### `m1-ac-enforcement.scenario`

Covers the acceptance-criteria enforcement edges.

Steps:

1. Init, create epic, create task.
2. Add 3 ACs.
3. Attempt close — fails with exit 1; stderr names the 3 incomplete ACs.
4. Check 2 of 3 ACs.
5. Attempt close — fails again; stderr names the 1 remaining incomplete AC.
6. Check the third AC.
7. Close — succeeds.
8. Create another task with 2 ACs.
9. Force-close without `--reason` — fails (reason is mandatory with `--force`).
10. Force-close with `--reason "Tracked externally in Jira PAY-123"` — succeeds.
11. `firetrail show <forced-id>` — output contains the force-close reason in
    the history entry.

Acceptance: every assertion passes; total runtime under 3 seconds.

---

### `m1-conflict-resolution.scenario`

Exercises the JSON merge driver for record conflicts.

Setup: workspace with one epic on `main`.

Steps:

1. Create branch `feature-a` from `main`.
2. On `feature-a`: create task A under the epic with title "Task A".
3. Switch to `main`. Create branch `feature-b` from `main`.
4. On `feature-b`: create task B under the same epic with title "Task B".
5. Merge `feature-a` into `main`.
6. Merge `feature-b` into `main`. The merge driver runs on the epic's JSON
   (`child_ids` array conflicts) and produces a clean merged record listing
   both tasks.
7. Assert `main` contains both task A and task B.
8. Assert the epic's `child_ids` array contains both IDs in deterministic
   order (sorted by ID).
9. `firetrail show <epic_id>` lists both tasks.

Acceptance: merge driver produces correct output without manual intervention;
no `<<<<<<<` markers left in any record file; total runtime under 5 seconds.

Note: The merge driver itself is M4 (`ft-pr`). For M1, this scenario can be
deferred behind a feature flag (`--features m4-merge-driver`) and pass with
an "intentionally pending" marker. The scenario file exists to lock the
expected behavior; M4 turns it on.

---

### `m1-index-rebuild.scenario`

Verifies the index is truly a derived cache.

Steps:

1. Init, create epic, 3 tasks with deps.
2. Run `firetrail list --json` — capture the result.
3. `rm .firetrail/index.db`.
4. Run `firetrail list --json` again — first command after deletion triggers a
   rebuild; output matches the captured result exactly.
5. `firetrail doctor` reports `OK` for index integrity.
6. Manually corrupt the index (write garbage to `.firetrail/index.db`).
7. `firetrail list --json` — should detect corruption and rebuild, or fail
   cleanly with a clear "run firetrail index rebuild" message.
8. After `firetrail index rebuild`, `firetrail list` returns the captured
   result.

Acceptance: all steps succeed; no data loss; total runtime under 3 seconds.

---

### `m1-clean-clone.scenario`

A new engineer's first experience.

Setup: workspace A with init done, epic + 5 tasks + ACs created.

Steps:

1. Clone workspace A's git repo to workspace B (no `.firetrail/index.db` or
   `.firetrail/cache/` — those are gitignored).
2. In workspace B, run `firetrail list` as the first command.
3. The command succeeds without needing a manual `init` or `rebuild`. The
   index is built lazily on first read.
4. The output matches workspace A's `firetrail list` output for the same
   filter.
5. `firetrail doctor` in workspace B reports `OK`.

Acceptance: zero manual setup steps; total runtime under 3 seconds (most of
which is the index build).

---

## Verifier subagent brief

The verifier for E-M1-10 reads `docs/ROADMAP.md` §M1 success criteria and
independently writes three additional scenarios that should also pass. The
author's scenarios above are not shown to the verifier.

Verifier's three scenarios are added to `tests/scenarios/` alongside the
author's and must also pass for the M1 gate.

---

## Failure handling

When a scenario fails, the runner:

1. Prints which step failed and why (assertion mismatch with expected vs
   actual).
2. Dumps the workspace state (file tree + key record contents) to stderr.
3. Exits with code 1.

CI archives the workspace dump on failure for post-mortem.

---

## Performance

Total runtime of the M1 scenario suite (5 scenarios + verifier's 3) must stay
under 30 seconds (per `docs/ROADMAP.md` §M1 success criteria). Scenarios run
in parallel via `cargo nextest`; individual scenarios stay under 5 seconds.

If a scenario exceeds 5 seconds locally, it is a regression and gets a bd
issue filed.

---

## References

- `docs/ROADMAP.md` §M1 — success criteria these scenarios validate
- `docs/components/ft-testkit.md` — scenario file format and runner
- ADR-0016 — Build approach (Layer 4 tests)
