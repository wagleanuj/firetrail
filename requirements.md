# Firetrail — Repo-Native Work Graph & Incident Memory System

> **Note:** This document is the original requirements brief. The current
> design has evolved through design discussion and stress-testing.
>
> See `docs/` for the canonical current design:
>
> - `docs/ARCHITECTURE.md` — how the system fits together (entry point)
> - `docs/decisions/` — ADRs explaining what changed from this brief and why
> - `docs/components/` — per-component specifications
> - `docs/BUILD_PLAN.md` — phased implementation plan
>
> Notable departures from this brief (see ADRs for details):
> Rust instead of TypeScript. JSON files in Git instead of Dolt. No LLM
> calls from the tool itself. Identity registry instead of raw git email.
> Multi-scope records instead of singleton scope. PR-time history
> compaction. Embedded and external storage modes.

## 1. Product Summary

**Firetrail** is a repo-native task management, dependency graph, and incident memory system for engineering teams working in fast-moving monorepos.

It combines:

- **Backlog-style task management**: epics, tasks, subtasks, bugs, acceptance criteria, board views, and developer-friendly CLI workflows.
- **Beads-style work graph and memory**: dependency-aware work tracking, claims, ready-work detection, structured records, and AI-agent-readable context.
- **Incident knowledge management**: incident reports, findings, root causes, runbooks, gotchas, decisions, and reusable lessons learned.
- **Semantic search**: find similar incidents, repeated failure patterns, related tasks, historical decisions, and runbooks even when wording differs.
- **Team-safe memory workflows**: PR review, evidence requirements, trust levels, quality checks, pruning, stale memory management, and audit history.
- **Integrations**: GitHub CLI, Jira MCP, Confluence MCP, Git, PR workflows, CODEOWNERS, and existing Markdown incident reports.

Firetrail is not meant to replace Jira, GitHub Issues, Confluence, or existing postmortem files. It is meant to become the **structured, searchable memory graph close to the code**.

---

## 2. Core Positioning

### One-liner

> Firetrail is a repo-native work graph and incident memory system that helps engineering teams manage work, remember production failures, and prime humans or AI agents with relevant context before touching code.

### Short pitch

Current engineering reality:

```txt
Tasks live in Jira or GitHub.
Incidents live in Markdown.
Runbooks live in Confluence.
Context lives in Slack.
Production knowledge lives in people’s heads.
AI agents forget everything after a few sessions.
```

Firetrail turns that into:

```txt
A structured, versioned, searchable work and memory graph inside the repo.
```

### Product tagline ideas

> Find the last time production caught fire — and what actually put it out.

> Repo-native task management with incident memory.

> A work graph for humans and AI agents that remembers why the code is the way it is.

---

## 3. Goals

Firetrail should help teams:

1. Manage epics, tasks, subtasks, bugs, incidents, findings, decisions, runbooks, and memories in one graph.
2. Track dependencies between pieces of work.
3. Identify unblocked “ready” work.
4. Allow teammates and AI agents to claim work safely.
5. Store acceptance criteria natively for every task.
6. Import existing Markdown incident reports.
7. Extract structured memory from historical incident reports.
8. Search incident history semantically.
9. Prime AI agents with task-specific and incident-specific context.
10. Capture reusable findings during work.
11. Review memory changes through PRs.
12. Prevent low-quality or stale memory from poisoning future work.
13. Prune, archive, deprecate, merge, redact, or supersede memories over time.
14. Integrate with GitHub, Jira, Confluence, Git, and monorepo workflows.
15. Remain safe for parallel teammate usage and constant branch merges.

---

## 4. Non-Goals

Firetrail should **not** initially try to:

1. Replace Jira as the official company planning/status tool.
2. Replace GitHub PRs or code review.
3. Replace Confluence as a long-form documentation system.
4. Replace incident management tools like PagerDuty or Opsgenie.
5. Become a giant enterprise dashboard before the CLI and data model are solid.
6. Treat AI-generated memories as trusted truth without human review.
7. Store secrets, customer data, logs with sensitive data, or raw production dumps.
8. Automatically hard-delete memory without review.
9. Depend entirely on semantic search while ignoring structured metadata.
10. Make Markdown the only source of truth if Dolt-backed structured storage is available.

---

## 5. Mental Model

```txt
Git
= version control for code and Firetrail changes.

Dolt
= versioned, mergeable database for structured work/memory records.

Firetrail
= CLI + workflow + schema + search layer on top of the graph.

Markdown
= detailed human-readable incident reports, generated exports, and existing docs.

Vector index
= rebuildable semantic search layer.

GitHub/Jira/Confluence
= external systems Firetrail can sync/link/import from.

AI agents
= consumers and contributors, but not trusted reviewers by default.
```

---

## 6. Core Concepts

### 6.1 Work Graph

Firetrail stores all work and knowledge as records in a graph.

Primary record types:

```txt
epic
task
subtask
bug
incident
finding
runbook
decision
gotcha
memory
doc
```

Example:

```txt
EPIC-12 Improve checkout reliability
├─ TASK-882 Add Redis pool saturation alert
├─ TASK-883 Add retry budget to checkout worker
├─ INC-2481 Checkout latency after Redis deploy
│  ├─ FIND-901 Redis pool exhaustion appears before CPU alarms
│  └─ RUN-311 Inspect Redis pool saturation
└─ DEC-44 Checkout retries must use bounded backoff
```

### 6.2 Source of Truth

Recommended hierarchy:

```txt
Dolt/Firetrail records
= structured source of truth for tasks, memory, links, dependencies, status.

Markdown incident reports
= long-form historical evidence and human-readable postmortems.

Vector DB
= search index only; rebuildable from Firetrail records and Markdown.

Git
= transport, review, history, and merge path.

External systems
= linked/synced but not the only canonical source for repo-local context.

---

## 7. Functional Requirements

## 7.1 Initialization & Setup

### FR-001: Initialize Firetrail in a repository

Firetrail shall provide:

```bash
firetrail init
```

This command should:

- Detect whether the current directory is a Git repository.
- Create required Firetrail folders.
- Initialize local storage.
- Create default configuration.
- Optionally generate or update `AGENTS.md`.
- Optionally add Firetrail patterns to `.gitignore` or `.firetrailignore`.

Example structure:

```txt
.firetrail/
  config.yml
  dolt/
  indexes/
    vectors/
  exports/
    epics/
    tasks/
    incidents/
    findings/
    runbooks/
  reports/

.firetrailignore
AGENTS.md
```

### FR-002: Setup wizard

Firetrail shall provide:

```bash
firetrail setup
```

This command should detect:

```txt
Git repository
GitHub CLI availability
Jira MCP availability
Confluence MCP availability
Existing incident docs
Existing backlog docs
Existing ADRs
Existing runbooks
Existing AGENTS.md
Existing CLAUDE.md
Existing Cursor rules
CODEOWNERS
```

It should ask the user how Firetrail should operate:

```txt
1. Local only
2. GitHub sync
3. Jira sync
4. Confluence sync
5. Hybrid
```

### FR-003: Setup integrations

Firetrail shall provide:

```bash
firetrail setup integrations
firetrail setup integrations github
firetrail setup integrations jira
firetrail setup integrations confluence
```

The setup flow should determine whether the project uses:

- GitHub CLI.
- Jira MCP.
- Confluence MCP.
- GitHub Issues.
- GitHub Pull Requests.
- Confluence incident/runbook pages.
- Jira epics/stories/incidents.

### FR-004: Health check

Firetrail shall provide:

```bash
firetrail doctor
```

This should verify:

- Firetrail config validity.
- Dolt/database accessibility.
- Vector index status.
- Git status.
- Integration availability.
- Schema migration status.
- Missing required files.
- Broken links.
- Unindexed imported docs.

---

## 7.2 Work Item Management

### FR-005: Create epics

Firetrail shall support creating epics.

```bash
firetrail epic create "Improve checkout reliability"
```

An epic may contain:

- Tasks.
- Bugs.
- Subtasks.
- Incidents.
- Findings.
- Decisions.
- Runbooks.
- Related external tickets.

### FR-006: Create tasks

Firetrail shall support creating tasks.

```bash
firetrail task create "Add Redis pool saturation alert" --epic EPIC-12
```

A task shall support:

- Title.
- Description.
- Status.
- Priority.
- Owner.
- Labels.
- Parent epic.
- Dependencies.
- Acceptance criteria.
- Implementation notes.
- Related incidents.
- Related findings.
- Related runbooks.
- Linked PRs.
- Linked external issues.
- Memory captured while working.

### FR-007: Create subtasks

Firetrail shall support subtasks.

```bash
firetrail subtask create "Add alert threshold config" --parent TASK-882
```

### FR-008: Create bugs

Firetrail shall support bugs.

```bash
firetrail bug create "Checkout timeout after deploy" --service checkout-api
```

### FR-009: Update work items

Firetrail shall support updates:

```bash
firetrail task update TASK-882 --status in-progress
firetrail task update TASK-882 --owner anuj
firetrail task update TASK-882 --priority high
```

### FR-010: Close work items

Firetrail shall support closing work:

```bash
firetrail close TASK-882
```

Closing should validate:

- Acceptance criteria completion.
- Required evidence.
- Linked PR or implementation note.
- Required memory updates for incident-related tasks.
- No unresolved blockers.

Force close may be allowed:

```bash
firetrail close TASK-882 --force --reason "Tracked externally in Jira"
```

Force-close must record an audit reason.

---

## 7.3 Acceptance Criteria

### FR-011: Native acceptance criteria

Firetrail shall treat acceptance criteria as first-class data, not just freeform Markdown.

Each acceptance criterion should have:

```txt
id
record_id
text
status
evidence_url
checked_by
checked_at
created_at
updated_at
```

### FR-012: Add criteria

```bash
firetrail criteria add TASK-882 "Alert fires when Redis pool usage exceeds 85% for 5 minutes"
```

### FR-013: List criteria

```bash
firetrail criteria list TASK-882
```

Example output:

```txt
TASK-882 Add Redis pool saturation alert

Acceptance Criteria:
[ ] Alert fires when Redis pool usage exceeds 85% for 5 minutes
[ ] Alert includes service, environment, pool name, and saturation percentage
[ ] Runbook is linked from alert description
[ ] Validation evidence is attached
```

### FR-014: Check and uncheck criteria

```bash
firetrail criteria check TASK-882 1
firetrail criteria uncheck TASK-882 1
```

### FR-015: Evidence for criteria

Firetrail shall allow adding evidence to criteria:

```bash
firetrail criteria evidence TASK-882 1 --url https://github.com/org/repo/pull/8892
```

Evidence types may include:

- PR.
- Commit.
- Test run.
- Dashboard.
- Log query.
- Incident report.
- Confluence page.
- Manual validation note.

### FR-016: Criteria templates

Firetrail shall support templates.

```bash
firetrail task create "Add retry budget" --template backend
```

Example backend template:

```txt
[ ] API behavior is implemented
[ ] Unit tests are added or updated
[ ] Integration path is validated
[ ] Logging/metrics are updated if needed
[ ] Error handling is covered
[ ] Relevant docs or memory are updated
```

Example incident follow-up template:

```txt
[ ] Root cause is linked
[ ] Fix is implemented
[ ] Regression test or monitor exists
[ ] Runbook is updated
[ ] Related incident is linked
[ ] Prevention action is documented
```

### FR-017: Generate acceptance criteria

Firetrail may provide an AI-assisted generation command:

```bash
firetrail criteria generate TASK-882
```

It should generate draft criteria from:

- Task title.
- Description.
- Parent epic.
- Related incident.
- Related findings.
- Service/component.
- Existing runbooks.
- Existing decisions.

Generated criteria must be reviewed before being saved.

### FR-018: Acceptance criteria enforcement

Firetrail shall prevent task closure if required criteria are incomplete unless force-close is used.

---

## 7.4 Dependency Graph

### FR-019: Add dependencies

Firetrail shall support explicit dependency types.

```bash
firetrail dep add TASK-102 TASK-98 --type blocked-by
```

### FR-020: Supported relationship types

Firetrail shall support at least:

```txt
blocks
blocked-by
parent-of
child-of
related-to
duplicates
supersedes
discovered-during
follow-up-from
fixed-by
caused-by
mitigated-by
documented-in
implemented-by
regressed-by
affects
owned-by
```

### FR-021: View dependency graph

```bash
firetrail graph TASK-882
firetrail graph EPIC-12
firetrail graph --service checkout-api
firetrail graph --incident INC-2481
```

Example:

```txt
EPIC-12 Improve checkout reliability
├─ TASK-882 Add Redis pool saturation alert [ready]
├─ TASK-883 Add retry budget [blocked by TASK-882]
├─ INC-2481 Checkout latency after Redis deploy
│  ├─ FIND-901 Redis pool exhaustion
│  └─ RUN-311 Inspect Redis pool saturation
└─ DEC-44 Bounded retry policy
```

### FR-022: Ready work detection

Firetrail shall provide:

```bash
firetrail ready
```

It should return only work that:

- Is not closed.
- Has no unresolved blockers.
- Is not already claimed, unless requested.
- Meets required dependency conditions.

### FR-023: Claim work

Firetrail shall allow teammates or agents to claim work.

```bash
firetrail claim TASK-882
firetrail unclaim TASK-882
```

Claim metadata:

```txt
claimed_by
claimed_at
claim_source
claim_expires_at optional
```

### FR-024: Detect conflicting claims

Firetrail shall warn when multiple users or agents attempt to work on the same item.

---

## 7.5 Board & Backlog Views

### FR-025: Board view

Firetrail shall provide:

```bash
firetrail board
```

Example:

```txt
TODO                  IN PROGRESS             REVIEW                 DONE
────────────────────────────────────────────────────────────────────────────
TASK-44 Redis alert   TASK-45 Retry budget    TASK-39 Fix auth       TASK-33 Add logs
TASK-50 Runbook       INC-2481 Checkout SEV2
```

### FR-026: Board filters

```bash
firetrail board --type task
firetrail board --type incident
firetrail board --service checkout-api
firetrail board --owner anuj
firetrail board --epic EPIC-12
```

### FR-027: List views

Firetrail shall support:

```bash
firetrail task list
firetrail epic list
firetrail incident list
firetrail memory list
firetrail finding list
```

### FR-028: Searchable backlog

Firetrail shall support searching backlog items by:

- Title.
- Description.
- Labels.
- Type.
- Status.
- Owner.
- Service.
- Component.
- External ticket.
- Semantic similarity.

---

## 7.6 Incident Management

### FR-029: Create incidents

```bash
firetrail incident create "INC-2481 checkout latency after Redis deploy"
```

Incident fields:

```txt
id
external_incident_id
title
summary
severity
status
service
component
environment
started_at
resolved_at
owner
commander
source_refs
body
```

### FR-030: Link incident to tasks

```bash
firetrail link TASK-882 INC-2481 --type follow-up-from
```

### FR-031: Create findings

```bash
firetrail finding create "Redis pool exhaustion appears before CPU alarms" --incident INC-2481
```

Finding fields:

```txt
service
component
symptom
pattern
confidence
status
evidence_refs
related_incidents
```

### FR-032: Create runbooks

```bash
firetrail runbook create "Inspect Redis pool saturation" --service checkout-api --component redis
```

### FR-033: Create gotchas

```bash
firetrail gotcha create "Redis CPU alarm may not fire during client pool saturation"
```

### FR-034: Create decisions

```bash
firetrail decision create "Checkout retries must use bounded backoff"
```

### FR-035: Incident capture section

Firetrail should support adding a “Firetrail Capture” section to Markdown incident reports.

Example:

```md
## Firetrail Capture

- Incident: INC-2481
- Findings:
  - FIND-901 Redis pool exhaustion appears before CPU alarms
- Runbooks:
  - RUN-311 Inspect Redis pool saturation
- Follow-up Tasks:
  - TASK-882 Add Redis pool saturation alert
```

### FR-036: Incident-to-memory conversion

Firetrail shall convert incident sections into structured records:

```txt
Symptoms -> incident fields and searchable chunks
Root Cause -> finding
Resolution -> runbook/finding
Action Items -> tasks
Lessons Learned -> memory/gotcha/decision
```

---

## 7.7 Import Existing Markdown Incident Reports

### FR-037: Import command

Firetrail shall provide:

```bash
firetrail import incidents ./docs/incidents
```

### FR-038: Recursive import

```bash
firetrail import incidents ./docs/incidents --recursive
```

### FR-039: Dry run by default or supported strongly

```bash
firetrail import incidents ./docs/incidents --dry-run
```

Example dry-run output:

```txt
Found 2,418 Markdown incident reports

Detected:
- 381 SEV1/SEV2 reports
- 912 reports with Root Cause sections
- 604 reports with Action Items
- 337 reports with Runbook-like steps
- 1,129 reports missing service metadata

Would create:
- 2,418 incident records
- 1,936 findings
- 604 runbooks
- 2,981 follow-up tasks
- 5,422 semantic chunks

No changes written.
```

### FR-040: Apply import

```bash
firetrail import incidents ./docs/incidents --apply
```

### FR-041: Import modes

Firetrail shall support:

#### Index-only mode

```bash
firetrail import incidents ./docs/incidents --index-only
```

Creates searchable memory without modifying source files.

#### Linked mode

```bash
firetrail import incidents ./docs/incidents --write-frontmatter
```

Adds Firetrail metadata to Markdown files.

Example:

```yaml
---
firetrail:
  incident: INC-2481
  findings:
    - FIND-901
  runbooks:
    - RUN-311
  imported_at: 2026-05-26
---
```

#### Curated mode

```bash
firetrail import incidents ./docs/incidents \
  --severity sev1,sev2 \
  --since 2024-01-01 \
  --services checkout-api,payment-api \
  --needs-review
```

### FR-042: Extract structured fields

Firetrail shall attempt to extract:

```txt
incident ID
title
date
severity
service
component
environment
symptoms
root cause
resolution
impact
action items
owners
links
related PRs
related dashboards
related logs
lessons learned
```

### FR-043: Create records from imports

From one incident report, Firetrail may create:

```txt
incident
finding
runbook
task
decision
gotcha
memory
```

### FR-044: Mark imported records

Imported records should be tagged:

```txt
imported
needs-review
source:markdown
```

### FR-045: Import quality report

Firetrail shall produce a report showing:

- Successfully imported files.
- Skipped files.
- Low-confidence extraction.
- Missing metadata.
- Duplicate candidates.
- Suggested manual review items.

---

## 7.8 Import Other Knowledge Sources

### FR-046: Import Backlog-style Markdown

```bash
firetrail import backlog ./backlog
```

### FR-047: Import ADRs

```bash
firetrail import adrs ./docs/adr
```

### FR-048: Import runbooks

```bash
firetrail import runbooks ./docs/runbooks
```

### FR-049: Import Confluence pages

```bash
firetrail import confluence --space ENGINEERING
firetrail confluence import-page <page-id>
```

### FR-050: Import Jira issues

```bash
firetrail jira import PAY-1234
```

---

## 7.9 Memory Capture

### FR-051: Capture command

Firetrail shall provide:

```bash
firetrail capture
```

Interactive flow:

```txt
What did you learn?
> Redis pool saturation happens before Redis CPU alarm fires.

Type?
> finding

Related to?
> INC-2481, TASK-882

Service?
> checkout-api

Component?
> redis

Should this be critical memory?
> no, reference only

Create follow-up task?
> yes
```

### FR-052: Non-interactive capture

```bash
firetrail memory create \
  "Redis pool exhaustion appears before CPU alarms in checkout-api" \
  --type finding \
  --source INC-2481 \
  --source PR-8892 \
  --service checkout-api \
  --component redis
```

### FR-053: Memory starts as draft

New memory shall default to:

```txt
status: draft
trust: low
confidence: low
```

Unless created by an approved import/review process.

### FR-054: Memory scopes

Memory should be scoped by:

```txt
service
component
environment
repository
package
file path
team
incident
task
epic
```

### FR-055: Memory types

Supported memory-like types:

```txt
finding
runbook
decision
gotcha
memory
doc
```

### FR-056: Critical vs reference memory

Firetrail shall distinguish:

```txt
critical
= loaded automatically during prime/startup when relevant.

reference
= searchable but not loaded by default.
```

Critical memory shall require stricter review.

---

## 7.10 Trust, Review, and Safety

### FR-057: Memory trust states

Firetrail shall support:

```txt
draft
reviewed
verified
stale
deprecated
archived
superseded
rejected
redacted
```

### FR-058: Confidence levels

Firetrail shall support:

```txt
low
medium
high
```

### FR-059: Evidence requirements

For incident/finding/runbook/critical memory, Firetrail should require evidence:

```txt
incident report
PR
commit
dashboard
log query
test result
Jira ticket
Confluence page
manual validation note
owner approval
```

### FR-060: Review memory

```bash
firetrail memory review FIND-901 --approve
firetrail memory review FIND-901 --reject "Too vague"
firetrail memory promote FIND-901 --verified
```

### FR-061: Require review for critical memory

```bash
firetrail memory promote FIND-901 --critical
```

Promotion should require:

- Evidence.
- Scope.
- Reviewer.
- Owner/team approval.
- No stale/deprecated conflicts.

### FR-062: Memory quality checklist

A good memory should be:

- Specific.
- Evidence-backed.
- Reusable.
- Scoped.
- Current.
- Linked to source.
- Free of secrets/customer data.
- Not a duplicate.
- Not an opinion pretending to be fact.

### FR-063: Memory quality score

Firetrail may compute a memory quality score.

Example:

```txt
Memory Quality Score: 86/100

+ Has linked incident
+ Has linked PR
+ Has service/component labels
+ Has clear symptom and fix
+ Has confidence level
+ Has reviewer
- Missing dashboard/log evidence
```

### FR-064: Memory linting

```bash
firetrail lint memory
```

Checks:

- Too vague.
- Missing service/component.
- Missing source.
- Missing owner.
- Missing confidence.
- Missing evidence.
- Possible duplicate.
- Secret-like content.
- Critical memory without approval.
- Forbidden absolute wording like “always” or “never” without evidence.
- Deprecated memory referenced by active work.
- Runbook references deleted file path.

### FR-065: Secret scanning

Firetrail shall scan memory content and imported docs for possible secrets.

It should detect:

- API keys.
- Tokens.
- Password-like values.
- Private keys.
- Customer identifiers if configured.
- Sensitive URLs if configured.

### FR-066: Redaction

```bash
firetrail memory redact MEM-123 --reason "Contained token-like string"
```

Redaction should preserve audit history while removing sensitive content from normal display/search.

---

## 7.11 PR Review Workflow

### FR-067: Memory diff

```bash
firetrail diff --memory
firetrail diff main...HEAD
```

Should show:

- New memories.
- Updated memories.
- Deprecated memories.
- Merged memories.
- Redacted memories.
- Linked tasks.
- Linked incidents.
- Acceptance criteria changes.
- Risk flags.

### FR-068: PR check

```bash
firetrail check pr
```

It should verify:

- PR is linked to a task/incident where required.
- Closed tasks have complete acceptance criteria.
- New memories have evidence.
- Critical memory has reviewer approval.
- Incident follow-ups link to incidents.
- Runbook updates have validation evidence.
- No deprecated memory is newly referenced.
- No likely secret is included.
- Duplicates are detected.
- Relevant CODEOWNERS are included.

Example output:

```txt
Firetrail PR Check

Linked task: TASK-882
Linked incident: INC-2481

Acceptance Criteria:
[x] Alert fires at 85% Redis pool saturation
[x] Runbook linked
[ ] Validation evidence attached

Memory Review:
[ ] FIND-901 has evidence
[x] RUN-311 links to incident
[!] MEM-421 marked critical but has no reviewer

Result:
FAIL
```

### FR-069: CI integration

Firetrail should support CI usage.

Example GitHub Action:

```yaml
name: Firetrail Check

on:
  pull_request:

jobs:
  firetrail:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install Firetrail
        run: npm install -g firetrail
      - name: Check Firetrail memory and task quality
        run: firetrail check pr --strict
```

### FR-070: PR comment summary

Firetrail may generate a PR summary:

```md
## Firetrail Review

### Work
- TASK-882: Add Redis pool saturation alert

### Acceptance Criteria
- [x] Alert fires after 5 minutes over 85%
- [x] Includes service/env/pool name
- [ ] Validation evidence linked

### New Memory
#### FIND-901
Claim: Redis pool exhaustion can show up as checkout latency before Redis CPU alarms fire.
Evidence: INC-2481, PR-8892
Scope: checkout-api, prod, Redis
Status: draft -> awaiting review
```

### FR-071: CODEOWNERS support

Firetrail shall use `CODEOWNERS` when available to require relevant owners for memory or task changes.

Example:

```txt
apps/checkout/** @checkout-team
infra/redis/** @platform-team
```

A Redis-related checkout memory may require:

```txt
@checkout-team
@platform-team
```

---

## 7.12 Semantic Search

### FR-072: Semantic search command

```bash
firetrail search "checkout timeout after redis deploy"
```

Search shall return:

- Similar incidents.
- Relevant findings.
- Related runbooks.
- Decisions.
- Tasks.
- Gotchas.
- Source documents.

Example output:

```txt
Similar incident memories:

1. FIND-901 — Redis pool exhaustion caused checkout p95 latency
   Score: 0.91
   Service: checkout-api
   Source: docs/incidents/2026-05-21-checkout-api-latency.md

2. FIND-778 — Retry storm amplified downstream timeout
   Score: 0.84
   Service: checkout-api

3. RUN-311 — Inspect Redis pool saturation
   Score: 0.81
   Type: runbook
```

### FR-073: Similarity search

```bash
firetrail similar INC-2481
firetrail similar FIND-901
firetrail similar TASK-882
```

### FR-074: Hybrid search

Firetrail shall support hybrid search:

```txt
keyword search
+ semantic similarity
+ metadata filters
+ trust/confidence weighting
+ recency/relevance weighting
```

Suggested ranking:

```txt
final_score =
  semantic_similarity * 0.65
  + keyword_match * 0.25
  + recency/relevance * 0.10
```

Trust weighting should prefer:

```txt
verified > reviewed > draft
current > stale
same service > nearby service
exact error match > vague semantic match
```

### FR-075: Search filters

```bash
firetrail search "timeout after deploy" \
  --service checkout-api \
  --component redis \
  --type finding,runbook,incident \
  --status reviewed,verified
```

### FR-076: Include draft/archived flags

```bash
firetrail search "redis latency" --include-drafts
firetrail search "redis latency" --include-archived
```

### FR-077: Chunking strategy

Firetrail shall not embed entire large Markdown files as one chunk.

It should chunk by:

```txt
summary
symptoms
root cause
resolution
action items
lessons learned
runbook steps
error snippets
decision rationale
```

### FR-078: Vector index is rebuildable

Vector search index shall be treated as derived data.

```bash
firetrail index rebuild
firetrail index refresh --changed-only
```

---

## 7.13 Agent Priming

### FR-079: Prime command

```bash
firetrail prime
```

### FR-080: Prime by task

```bash
firetrail prime --task TASK-882
```

### FR-081: Prime by incident

```bash
firetrail prime --incident INC-2481
```

### FR-082: Prime by query

```bash
firetrail prime --query "checkout latency after redis deploy"
```

### FR-083: Prime by files

```bash
firetrail prime --files apps/checkout/src/cache.ts
```

### FR-084: Prime output formats

```bash
firetrail prime --task TASK-882 --format markdown
firetrail prime --task TASK-882 --format json
```

Example Markdown output:

```md
# Firetrail Context Prime

## Current Work
TASK-882: Add Redis pool saturation alert

## Acceptance Criteria
- [ ] Alert fires when Redis pool usage exceeds 85% for 5 minutes
- [ ] Alert includes service, environment, pool name, and saturation percentage
- [ ] Runbook is linked from alert description
- [ ] Validation evidence is attached

## Related Incidents
- INC-2481: Checkout latency after Redis deploy
- INC-2190: Redis timeout during Black Friday traffic

## Relevant Findings
- FIND-901: Redis pool exhaustion appears before CPU alarms
- FIND-778: Retry storms amplify checkout latency

## Runbooks
- RUN-311: Inspect Redis pool saturation
- RUN-208: Roll back checkout worker concurrency safely

## Decisions
- DEC-44: Checkout retry policy must use bounded backoff

## Suggested Files
- apps/checkout/src/cache.ts
- apps/checkout/src/retry-policy.ts
- infra/alerts/checkout.yml
```

### FR-085: Agent memory safety

By default, prime should include only:

```txt
verified
reviewed
critical
```

It should exclude:

```txt
rejected
deprecated
low-confidence draft
archived
redacted
superseded
```

Unless explicitly requested.

```bash
firetrail prime --include-drafts
```

---

## 7.14 Memory Pruning

### FR-086: Prune command

```bash
firetrail memory prune
```

### FR-087: Dry-run pruning

```bash
firetrail memory prune --dry-run
```

### FR-088: Interactive pruning

```bash
firetrail memory prune --interactive
```

### FR-089: Prune modes

```bash
firetrail memory prune --stale
firetrail memory prune --duplicates
firetrail memory prune --low-confidence
firetrail memory prune --older-than 12mo
firetrail memory prune --service checkout-api
```

### FR-090: Soft-pruning actions

Firetrail shall support:

```txt
deprecate
archive
merge
supersede
redact
reject
delete
```

Hard delete should not be the default.

### FR-091: Deprecate memory

```bash
firetrail memory deprecate FIND-778 \
  --reason "Checkout retry system was replaced in 2026"
```

### FR-092: Archive memory

```bash
firetrail memory archive FIND-441 \
  --reason "Old incident pattern no longer relevant"
```

### FR-093: Supersede memory

```bash
firetrail memory supersede FIND-778 FIND-901 \
  --reason "Newer Redis pool finding is more accurate"
```

### FR-094: Merge memories

```bash
firetrail memory merge FIND-112 FIND-341 --into FIND-901
```

Merge should preserve:

- Strongest wording.
- Evidence links.
- Related incidents.
- Related PRs.
- Aliases.
- Audit history.

Old records become `superseded`.

### FR-095: Semantic duplicate detection

```bash
firetrail memory duplicates
```

Example:

```txt
Possible duplicate cluster:

FIND-901
"Redis pool exhaustion appears before CPU alarms in checkout-api"

FIND-112
"Checkout latency can happen when Redis connections are exhausted"

FIND-341
"Redis client pool saturation caused p95 spike"

Suggested action:
Merge into FIND-901 and keep aliases.
```

### FR-096: Stale memory detection

```bash
firetrail memory stale
firetrail memory review-due
```

### FR-097: Prune config

Example:

```yaml
memory:
  pruning:
    enabled: true
    defaultAction: archive
    reviewRequired: true
    staleAfterDays: 180
    archiveAfterDays: 365
    pruneDraftsAfterDays: 30
    pruneLowConfidenceAfterDays: 90
    neverPruneLabels:
      - critical
      - compliance
      - security
    requireEvidenceForReviewed: true
```

### FR-098: Protected memory

Firetrail shall protect:

```txt
critical
security
compliance
verified
owner-protected
```

from automatic pruning.

---

## 7.15 GitHub Integration

### FR-099: Detect GitHub CLI

Firetrail setup shall detect `gh`.

### FR-100: Link GitHub issues

```bash
firetrail github link TASK-882 --issue 1234
```

### FR-101: Create GitHub issue from Firetrail task

```bash
firetrail github create-issue TASK-882
```

### FR-102: Link PR

```bash
firetrail github link TASK-882 --pr 8892
```

### FR-103: Sync GitHub

```bash
firetrail github sync
```

### FR-104: Read PR metadata

Firetrail should read:

- PR title.
- PR description.
- Changed files.
- Linked issues.
- Reviewers.
- Status checks.
- Labels.
- Author.

### FR-105: PR memory requirement detection

Firetrail should warn when a PR touches incident-related or high-risk files without:

- Linked task.
- Linked incident.
- Memory update.
- Acceptance criteria.
- Runbook update.

---

## 7.16 Jira Integration

### FR-106: Jira MCP setup

```bash
firetrail setup integrations jira
```

### FR-107: Jira import

```bash
firetrail jira import PAY-1234
```

### FR-108: Jira link

```bash
firetrail jira link TASK-882 PAY-1234
```

### FR-109: Jira sync

```bash
firetrail jira sync TASK-882
```

### FR-110: Jira create

```bash
firetrail jira create TASK-882
```

### FR-111: Jira mapping

Suggested mapping:

```txt
Jira Epic     -> Firetrail epic
Jira Story    -> Firetrail task
Jira Bug      -> Firetrail bug
Jira Incident -> Firetrail incident
Jira Subtask  -> Firetrail subtask
```

---

## 7.17 Confluence Integration

### FR-112: Confluence MCP setup

```bash
firetrail setup integrations confluence
```

### FR-113: Import Confluence space

```bash
firetrail confluence import --space INCIDENTS
```

### FR-114: Import Confluence page

```bash
firetrail confluence import-page <page-id>
```

### FR-115: Publish runbook

```bash
firetrail confluence publish RUN-311
```

### FR-116: Sync runbook

```bash
firetrail confluence sync-runbook RUN-311
```

### FR-117: Link Confluence pages

Firetrail shall link imported/published Confluence pages to:

- Incidents.
- Runbooks.
- Decisions.
- Findings.
- Tasks.

---

## 7.18 Markdown Export

### FR-118: Export records to Markdown

Firetrail shall support generated Markdown exports for review and readability.

```bash
firetrail export markdown
```

Possible output:

```txt
.firetrail/exports/
  epics/
  tasks/
  incidents/
  findings/
  runbooks/
```

### FR-119: Human-readable task export

Example:

```md
# TASK-882 Add Redis pool saturation alert

Status: In Progress
Owner: Anuj
Epic: EPIC-12 Improve checkout reliability
Service: checkout-api
Component: redis

## Description

Add Redis pool saturation monitoring.

## Acceptance Criteria

- [ ] Alert fires when Redis pool usage exceeds 85% for 5 minutes
- [ ] Alert includes service, environment, pool name, and saturation percentage
- [ ] Runbook is linked from alert description
- [ ] Validation evidence is attached

## Related

- Incident: INC-2481
- Finding: FIND-901
- Runbook: RUN-311
```

### FR-120: Import from generated Markdown

If supported, Firetrail must handle conflicts carefully and warn when Markdown is stale compared to Dolt records.

---

## 8. Data Model

## 8.1 Records Table

```sql
records
- id
- type
- title
- body
- status
- priority
- owner
- created_by
- created_at
- updated_at
- closed_at
- source
- external_id
- trust_status
- confidence
```

## 8.2 Labels Table

```sql
labels
- id
- record_id
- key
- value
```

Examples:

```txt
service:checkout-api
component:redis
env:prod
severity:sev2
symptom:latency
critical:true
```

## 8.3 Relations Table

```sql
relations
- id
- from_record_id
- to_record_id
- relation_type
- created_at
- created_by
```

## 8.4 Acceptance Criteria Table

```sql
acceptance_criteria
- id
- record_id
- text
- status
- evidence_url
- checked_by
- checked_at
- created_at
- updated_at
```

## 8.5 Evidence Table

```sql
evidence
- id
- record_id
- evidence_type
- url
- description
- created_at
- created_by
```

Evidence types:

```txt
incident_report
pull_request
commit
dashboard
log_query
test_result
jira_ticket
confluence_page
manual_note
```

## 8.6 Claims Table

```sql
claims
- id
- record_id
- claimed_by
- claimed_at
- claim_source
- claim_expires_at
- released_at
```

## 8.7 Memory Review Table

```sql
memory_reviews
- id
- record_id
- reviewer
- decision
- reason
- reviewed_at
```

## 8.8 Chunks Table

```sql
chunks
- id
- record_id
- source_file
- chunk_type
- text
- hash
- created_at
- updated_at
```

## 8.9 Embedding Index Metadata

```sql
embedding_index
- id
- chunk_id
- vector_store_id
- embedding_model
- text_hash
- indexed_at
```

## 8.10 Audit Log

```sql
audit_log
- id
- actor
- action
- record_id
- before
- after
- reason
- created_at
```

---

## 9. CLI Command Catalog

## 9.1 Setup

```bash
firetrail init
firetrail setup
firetrail setup integrations
firetrail doctor
```

## 9.2 Work Graph

```bash
firetrail epic create
firetrail task create
firetrail subtask create
firetrail bug create
firetrail update
firetrail close
firetrail claim
firetrail unclaim
firetrail ready
firetrail board
firetrail graph
firetrail link
firetrail dep add
firetrail dep remove
```

## 9.3 Acceptance Criteria

```bash
firetrail criteria add
firetrail criteria list
firetrail criteria check
firetrail criteria uncheck
firetrail criteria evidence
firetrail criteria generate
firetrail criteria validate
```

## 9.4 Incident Memory

```bash
firetrail incident create
firetrail finding create
firetrail runbook create
firetrail decision create
firetrail gotcha create
firetrail memory create
firetrail capture
```

## 9.5 Import

```bash
firetrail import incidents
firetrail import backlog
firetrail import adrs
firetrail import runbooks
firetrail import confluence
firetrail import jira
```

## 9.6 Search & Context

```bash
firetrail search
firetrail similar
firetrail prime
firetrail index rebuild
firetrail index refresh
```

## 9.7 Review & Quality

```bash
firetrail diff
firetrail diff --memory
firetrail check
firetrail check pr
firetrail lint memory
firetrail review
```

## 9.8 Memory Lifecycle

```bash
firetrail memory list
firetrail memory review
firetrail memory promote
firetrail memory deprecate
firetrail memory archive
firetrail memory supersede
firetrail memory merge
firetrail memory duplicates
firetrail memory stale
firetrail memory review-due
firetrail memory prune
firetrail memory redact
```

## 9.9 Integrations

```bash
firetrail github sync
firetrail github link
firetrail github create-issue

firetrail jira sync
firetrail jira link
firetrail jira create
firetrail jira import

firetrail confluence import
firetrail confluence import-page
firetrail confluence publish
firetrail confluence sync-runbook
```

---

## 10. Non-Functional Requirements

## 10.1 Reliability

### NFR-001

Firetrail shall avoid data loss during merges, imports, and sync operations.

### NFR-002

All destructive operations shall support dry-run or confirmation.

### NFR-003

Hard delete shall not be the default for memory records.

### NFR-004

Vector indexes shall be rebuildable from canonical records and source docs.

### NFR-005

Schema migrations shall be reversible or safely forward-compatible where possible.

---

## 10.2 Performance

### NFR-006

Common CLI commands should return quickly in large monorepos.

Targets:

```txt
firetrail ready: < 2 seconds for normal repos
firetrail show: < 1 second
firetrail search: < 3 seconds for local index
firetrail prime: < 5 seconds for typical task context
```

### NFR-007

Importing thousands of Markdown incident files should support batching and resume.

### NFR-008

Index refresh should support changed-only mode.

```bash
firetrail index refresh --changed-only
```

### NFR-009

Semantic search should support metadata filtering before or during vector retrieval.

---

## 10.3 Scalability

### NFR-010

Firetrail shall support repositories with:

```txt
thousands of incident reports
thousands of tasks
many concurrent branches
many teammates
many AI-agent sessions
```

### NFR-011

Firetrail shall support monorepo service/component mapping.

### NFR-012

Firetrail shall support partial indexing by path, service, or time range.

---

## 10.4 Security

### NFR-013

Firetrail shall not intentionally store secrets.

### NFR-014

Firetrail shall scan memory/imported content for secret-like strings.

### NFR-015

Firetrail shall support redaction.

### NFR-016

Firetrail shall avoid sending sensitive repository content to external services unless explicitly configured.

### NFR-017

Firetrail shall provide local-only mode.

### NFR-018

Firetrail shall allow configuration of ignored paths and sensitive paths.

Example:

```yaml
security:
  ignoredPaths:
    - .env
    - secrets/
    - customer-data/
  scanForSecrets: true
  allowExternalEmbeddings: false
```

---

## 10.5 Privacy

### NFR-019

Firetrail shall avoid storing customer-identifying information by default.

### NFR-020

Firetrail shall allow teams to configure redaction patterns.

### NFR-021

Firetrail shall distinguish public, internal, confidential, security, and compliance records if configured.

---

## 10.6 Auditability

### NFR-022

All memory lifecycle changes shall be auditable.

Audit events include:

- Create.
- Update.
- Review.
- Verify.
- Deprecate.
- Archive.
- Merge.
- Supersede.
- Redact.
- Delete.
- Import.
- Sync.

### NFR-023

PR review should show Firetrail diffs clearly.

### NFR-024

Force-close and force-prune actions must record reasons.

---

## 10.7 Usability

### NFR-025

CLI should be simple enough for engineers during incidents.

### NFR-026

Commands should have clear error messages.

### NFR-027

Firetrail should support both interactive and non-interactive usage.

### NFR-028

All important commands should support JSON output for AI agents and automation.

Example:

```bash
firetrail ready --json
firetrail prime --task TASK-882 --format json
```

### NFR-029

The tool should be useful without a web UI.

---

## 10.8 AI-Agent Compatibility

### NFR-030

Firetrail shall provide machine-readable outputs.

### NFR-031

Firetrail shall generate compact context packs.

### NFR-032

Firetrail shall avoid including untrusted draft memory in agent prime output by default.

### NFR-033

Firetrail shall support `AGENTS.md` workflow generation.

### NFR-034

Firetrail shall include enough metadata for agents to cite source records and avoid unsupported claims.

---

## 10.9 Merge Safety

### NFR-035

Firetrail shall support branch-based parallel work.

### NFR-036

Storage should be merge-friendly.

### NFR-037

Conflicts should be explainable at the record/field level where possible.

### NFR-038

Concurrent task/memory edits should not silently overwrite each other.

---

## 10.10 Maintainability

### NFR-039

The codebase should be modular:

```txt
storage
cli
graph
importers
search
integrations
review
pruning
agent-prime
```

### NFR-040

Importers should be plugin-like.

### NFR-041

Integrations should be optional.

### NFR-042

The vector index should be replaceable.

---

## 11. Configuration

Example `.firetrail/config.yml`:

```yaml
workspace:
  name: checkout-platform
  mode: monorepo

storage:
  engine: dolt
  path: .firetrail/dolt

search:
  semantic: true
  vectorStore: lancedb
  indexPath: .firetrail/index
  includeDraftsByDefault: false

memory:
  defaultStatus: draft
  requireEvidenceForReviewed: true
  criticalRequiresReview: true
  pruning:
    enabled: true
    defaultAction: archive
    reviewRequired: true
    staleAfterDays: 180
    archiveAfterDays: 365
    pruneDraftsAfterDays: 30
    pruneLowConfidenceAfterDays: 90
    neverPruneLabels:
      - critical
      - compliance
      - security

integrations:
  github:
    enabled: true
    cli: gh
    syncIssues: true
    syncPRs: true

  jira:
    enabled: true
    mcp: true
    projectKey: PAY

  confluence:
    enabled: true
    mcp: true
    spaces:
      - ENGINEERING
      - INCIDENTS

incidentImport:
  paths:
    - docs/incidents
    - postmortems
  frontmatter: true
  createFindings: true
  createRunbooks: true
  createFollowUpTasks: true

security:
  scanForSecrets: true
  allowExternalEmbeddings: false
  ignoredPaths:
    - .env
    - secrets/
    - customer-data/
```

---

## 12. Recommended AGENTS.md Section

```md
## Firetrail Workflow

This repository uses Firetrail for task management, dependency tracking, incident memory, and AI-agent context.

Before starting work:

1. Run `firetrail ready` to find unblocked work.
2. Claim the task with `firetrail claim <id>`.
3. Run `firetrail prime --task <id>`.
4. Read related incidents, findings, decisions, runbooks, and acceptance criteria.

During work:

1. Link new findings with `firetrail capture`.
2. Link touched PRs/files to the task.
3. Add dependencies if new blockers are discovered.
4. Update acceptance criteria as implementation details become clearer.

Before finishing:

1. Run `firetrail check`.
2. Ensure the task has linked PR, tests, and evidence.
3. Ensure all acceptance criteria are complete.
4. Capture useful findings, runbooks, decisions, or gotchas.
5. Do not close the task until Firetrail checks pass.

Memory rules:

- New memories start as draft.
- Do not mark memory as critical without review.
- Do not include secrets, customer data, or raw sensitive logs.
- Prefer specific, evidence-backed, scoped memories.
- Deprecate or supersede stale memories instead of deleting them.
```

---

## 13. Example End-to-End Workflow

### Scenario

A new production incident occurs:

```txt
Checkout API latency after Redis deploy.
```

### Step 1: Create incident

```bash
firetrail incident create "INC-2481 checkout latency after Redis deploy" \
  --service checkout-api \
  --component redis \
  --severity sev2 \
  --env prod
```

### Step 2: Search memory

```bash
firetrail search "checkout latency after redis deploy"
```

### Step 3: Prime context

```bash
firetrail prime --incident INC-2481
```

### Step 4: Capture finding

```bash
firetrail finding create "Redis pool exhaustion appears before CPU alarms" \
  --incident INC-2481 \
  --service checkout-api \
  --component redis \
  --source docs/incidents/2026-05-21-checkout-api-latency.md
```

### Step 5: Create follow-up task

```bash
firetrail task create "Add Redis pool saturation alert" \
  --epic EPIC-12 \
  --service checkout-api \
  --component redis
```

### Step 6: Add acceptance criteria

```bash
firetrail criteria add TASK-882 "Alert fires when Redis pool usage exceeds 85% for 5 minutes"
firetrail criteria add TASK-882 "Alert includes service, environment, pool name, and saturation percentage"
firetrail criteria add TASK-882 "Runbook is linked from alert description"
firetrail criteria add TASK-882 "Validation evidence is attached"
```

### Step 7: Link graph

```bash
firetrail link TASK-882 INC-2481 --type follow-up-from
firetrail link FIND-901 INC-2481 --type discovered-during
```

### Step 8: Work and PR

```bash
firetrail claim TASK-882
firetrail prime --task TASK-882 > .firetrail/context.md
```

### Step 9: Check before merge

```bash
firetrail check pr
```

### Step 10: Review memory

```bash
firetrail memory review FIND-901 --approve
firetrail memory promote FIND-901 --verified
```

---

## 14. MVP Scope

### MVP 1: Local Work Graph

Required:

- `init`
- `task create`
- `epic create`
- `subtask create`
- `dep add`
- `ready`
- `claim`
- `close`
- `board`
- `graph`
- native acceptance criteria
- simple Markdown export

### MVP 2: Incident Memory

Required:

- `incident create`
- `finding create`
- `runbook create`
- `memory create`
- `capture`
- link incidents to tasks
- trust statuses
- evidence refs
- memory diff

### MVP 3: Import & Search

Required:

- `import incidents`
- dry-run import
- index-only mode
- create findings/runbooks/tasks from Markdown sections
- semantic search
- similar search
- prime command

### MVP 4: PR Safety

Required:

- `check pr`
- memory linting
- acceptance criteria enforcement
- critical memory review
- CODEOWNERS support
- GitHub CLI integration

### MVP 5: Integrations

Required:

- GitHub issue/PR sync
- Jira MCP link/import/sync
- Confluence MCP import/publish/sync

### MVP 6: Memory Hygiene

Required:

- stale detection
- duplicate detection
- pruning
- archive/deprecate/supersede
- merge
- redaction

---

## 15. Risks & Mitigations

### Risk: Firetrail becomes another documentation graveyard

Mitigation:

- Keep structured records small.
- Require evidence.
- Add pruning.
- Add review states.
- Use search/prime to prove usefulness.

### Risk: Bad memories poison future agent work

Mitigation:

- Draft by default.
- Verified/reviewed only in prime.
- Evidence requirements.
- PR review.
- Critical memory approval.

### Risk: Importing thousands of Markdown files creates noise

Mitigation:

- Dry-run.
- Index-only first.
- Curated import.
- Tag imported records as `needs-review`.
- Prioritize SEV1/SEV2 and repeated incidents.

### Risk: Semantic search returns plausible but wrong results

Mitigation:

- Combine semantic with structured filters.
- Show confidence/trust/source.
- Prefer verified/reviewed records.
- Include citations/links to source records.

### Risk: Merge conflicts in monorepo

Mitigation:

- Use versioned structured storage.
- Keep exports generated.
- Use record-level conflict handling.
- Use PR checks.

### Risk: Sensitive data leaks into memory

Mitigation:

- Secret scanning.
- Redaction.
- Ignored paths.
- Local-only mode.
- Configurable external embedding policy.

---

## 16. Open Questions

1. Should Dolt be embedded directly or accessed as a local service?
2. Should Markdown exports be committed by default?
3. Should the vector store be LanceDB, Chroma, SQLite vector extension, or Postgres/pgvector?
4. Should external embeddings be allowed, or should local embeddings be default?
5. Should Firetrail support a web UI in v1 or remain CLI-first?
6. Should tasks sync bidirectionally with Jira/GitHub, or should Firetrail remain repo-local with links?
7. How strict should PR checks be by default?
8. Should critical memories require CODEOWNER approval?
9. Should imported Markdown files be modified with frontmatter, or should mapping live only inside Firetrail?
10. Should Firetrail support hosted/shared team mode later?

---

## 17. Suggested Initial Tech Direction

Given the likely user/developer profile, a good initial implementation could be:

```txt
Language: TypeScript
CLI: Node.js + Commander or oclif
Storage: Dolt-backed structured DB
Markdown parsing: unified/remark
Frontmatter: gray-matter
Embeddings: configurable provider
Local vector DB: LanceDB or Chroma
Schema validation: zod
Git integration: simple-git or shell git
GitHub integration: gh CLI wrapper
Jira/Confluence: MCP adapters
Testing: Vitest
Package manager: pnpm
```

Project structure:

```txt
firetrail/
  apps/
    cli/
  packages/
    core/
    storage/
    graph/
    importers/
    search/
    integrations/
    review/
    pruning/
    agent-prime/
  docs/
  examples/
```

---

## 18. Final Definition

Firetrail is:

```txt
A repo-native work graph and incident memory system.

It lets teams manage epics/tasks like Backlog.md,
coordinate dependencies like Beads,
store structured knowledge in a versioned database,
import old incident reports,
sync with GitHub/Jira/Confluence,
search similar failures semantically,
prime humans and AI agents with relevant context,
and review/prune memories safely through PR workflows.
```

The system should help answer:

```txt
What should I work on next?
What blocks this?
What are the acceptance criteria?
Has this incident happened before?
What fixed it last time?
What did we learn?
Can I trust this memory?
Is this memory stale?
What context should an AI agent know before touching this code?
```

That is the core product.
