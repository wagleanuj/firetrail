# ADR-0014: Imported records land in a quarantine index by default

## Status

Accepted — 2026-05-26

## Context

Firetrail imports existing knowledge: historical markdown incident reports, Confluence pages, Jira tickets, ADRs, runbooks. A team typically has thousands of these, accumulated over years, varying widely in quality.

If imports land directly in the canonical record store and the canonical vector index, two problems appear immediately:

1. **Search signal collapses.** A team that imports 4,000 historical postmortems sees 4,000 mediocre matches drown out every high-quality finding. Engineers stop trusting `firetrail similar`. The vector index becomes useless.

2. **Trust laundering at scale.** Imported records carry `origin: imported`. Without a separate gate they would still appear in `prime` output if they reached `reviewed` status — but they were never reviewed in the Firetrail sense. The original markdown went through some review years ago, but that review was about a different question and is not transferable.

The original requirements (FR-037–FR-045) describe import dry-run, mark-as-imported, and review-required flags. Those mechanics are correct. What is missing is a hard separation between the imported corpus and the canonical corpus until promotion happens.

## Decision

Imported records land in a quarantine index, separate from the canonical vector index. They are excluded from `firetrail search`, `firetrail similar`, and `firetrail prime` results by default. They become canonical only via explicit promotion or via accumulated inbound references.

### Quarantine index

A second sqlite-vec table inside the same SQLite database (`embeddings_quarantine`) holds vectors for imported records. The records themselves still live as JSON files in the normal record tree, with a label distinguishing them:

```json
{
  "id": "INC-imported-2023-redis-incident",
  "type": "incident",
  "labels": ["source:import", "status:triage"],
  "origin": "imported",
  "imported_at": "2026-05-26",
  "imported_from": "docs/incidents/2023-08-21-redis.md",
  ...
}
```

### Default exclusion

`firetrail search`, `firetrail similar`, `firetrail prime`, and the default vector queries operate only on the canonical index. The quarantine index is queried explicitly:

```
firetrail search "redis pool" --include-quarantine
firetrail similar INC-2481 --include-quarantine
```

The flag is conspicuous; results from quarantine are labeled `[quarantined]` in output.

### Promotion to canonical

A quarantined record becomes canonical via:

1. **Explicit human promotion.** `firetrail promote-import <id>` runs the standard review workflow and, on approval, moves the record's vectors into the canonical index. The record's `origin` stays `imported` (ADR-0013) — promotion changes status, not origin.

2. **Automatic promotion on inbound references.** A quarantined record that accumulates N (default 3) inbound references from canonical records becomes a candidate for auto-promotion. `firetrail promote-import --auto` runs the promotion workflow against all candidates. Still requires a human to approve the batch.

3. **Re-validation.** A team can run `firetrail import --refresh <source>` to re-process source markdown after editing. The re-processed records replace the quarantined ones at the same IDs.

### Bulk import caps

Single import operations are capped at a configurable record count (default 500). Larger imports must be batched and require the `--i-understand-quality-impact` flag, which writes an audit record naming the operator and source.

### Quality reporting at import time

`firetrail import` emits a quality report:

```
Imported 487 records from docs/incidents/

Confidence breakdown:
  high:   42  (clean structure, all sections detected)
  medium: 213 (most sections detected)
  low:    232 (sparse structure, fields missing)

Quarantine candidates flagged as low-quality: 232
Suggested action: review high-confidence records first.

Run `firetrail promote-import --interactive` to walk records by confidence.
```

### Lifecycle of quarantined records

Quarantined records that never get promoted within a configurable window (default one year) are auto-archived. They remain in the repo as files but disappear from `firetrail memory list` by default. Archived imports can be revisited later but do not weigh on the active index.

## Consequences

Positive:

- A bulk import cannot poison search. The canonical index stays small and high-quality.
- High-value historical knowledge gets promoted intentionally, with review, into the canonical corpus.
- Low-value or duplicative imports stay out of agent prime output.
- The vector index is sized to the number of canonical records, not to the total historical archive. Search stays fast.
- Imports become a *resource* rather than a *firehose* — a team can mine their archive over time without overwhelming the active workspace.

Negative:

- Imported knowledge is invisible to engineers until promoted. A team that imports thoroughly but never promotes has wasted the import. Mitigated by inbound-reference auto-candidacy and by the interactive promotion flow.
- Two indexes to maintain. Acceptable — same SQLite database, separate table, same code path with a query flag.
- Promotion is human work. Cannot be fully automated without re-introducing the quality problem.

## Alternatives considered

**Direct import into canonical index with `status: needs-review`.** Tried in the original spec. Disqualified by the search-poisoning failure mode — `needs-review` records still match queries.

**Single index with weighted ranking that demotes imports.** Possible but fragile. Ranking weights are easy to tune wrong and hard to debug. Hard separation is cleaner.

**Reject imports entirely.** Loses the value of historical incident archives. Disqualified.

**Per-import promotion required for every record.** Too much human work for a thousand-record import. The hybrid (explicit + auto-candidate via inbound references) splits the load reasonably.

## References

- ADR-0013: Trust model (origin flag, draft hygiene)
- ADR-0007: Embeddings (the index lives in the same SQLite database)
