# ft-core — record types, schema, identity, hash chain

**Epic:** `firetrail-kss`
**Wave:** 1
**Depends on:** workspace skeleton (`firetrail-bnp`)
**Depended on by:** ft-storage, ft-identity, ft-index, ft-cli, and every later crate

---

## Purpose

`ft-core` defines the canonical types that every other crate consumes. Record kinds,
field schemas, identity, IDs, the hash-chain fields, the relation enum, the trust
state machine, the scope routing fields. Serde for canonical JSON serialization.
Validation for malformed input.

`ft-core` has no I/O. It is types and pure functions.

---

## Public API

### Identifiers

```rust
/// A canonical record identifier. Stored as the full content-derived hash (ADR-0015).
/// Display logic uses adaptive prefix length; the wire format is always the full hash.
pub struct RecordId(String);

impl RecordId {
    /// Mint a new RecordId from a record's creation context.
    /// Uses a 128-bit nonce + identity + type + millisecond timestamp, SHA-256.
    pub fn mint(kind: RecordKind, identity: &Identity) -> Self;

    /// Construct from an existing string (validation only; no derivation).
    pub fn from_string(s: impl Into<String>) -> Result<Self, CoreError>;

    pub fn as_str(&self) -> &str;
    pub fn short(&self, len: usize) -> &str;
}

/// Canonical identity reference. M1 form: a single string carrying the resolved email.
/// M5 extends with kind, status, capabilities (ADR-0008).
pub struct Identity(String);

impl Identity {
    pub fn new(s: impl Into<String>) -> Result<Self, CoreError>;
    pub fn as_str(&self) -> &str;
}
```

### Record kinds

```rust
/// Every record type Firetrail supports. The ticket kinds (epic/task/subtask/bug)
/// landed in M1; the memory kinds (incident/finding/runbook/decision/gotcha/memory)
/// are authorable as of M2.
pub enum RecordKind {
    Epic,
    Task,
    Subtask,
    Bug,
    Incident,
    Finding,
    Runbook,
    Decision,
    Gotcha,
    Memory,
}
```

### The record envelope

```rust
/// Common fields shared by every record. Type-specific fields live in the variant.
pub struct RecordEnvelope {
    pub id: RecordId,
    pub kind: RecordKind,
    pub title: String,
    pub status: Status,
    pub priority: Priority,
    pub owner: Option<Identity>,
    pub created_by: Identity,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,

    /// Scope routing (ADR-0004). M1 ships these fields plumbed through but does not
    /// enforce multi-scope semantics. ft-scope (Wave 3) consumes them.
    pub owning_scope: Option<String>,
    pub affected_scopes: Vec<String>,
    pub applies_to: Vec<String>,

    /// Hash chain (ADR-0017). M1 sets these to deterministic values; ft-history
    /// (M2) computes the chain transitions.
    pub state_hash: String,
    pub prev_state_hash: Option<String>,

    /// Free-form labels.
    pub labels: Vec<Label>,

    /// Per-PR compaction entries (ADR-0003). M1 ships an empty Vec; ft-history (M2)
    /// populates and compacts.
    pub history: Vec<HistoryEntry>,

    /// Origin flag (ADR-0013). M1 sets human; M3 wires bot/imported from invocation
    /// context.
    pub origin: Origin,
}

pub struct Record {
    pub envelope: RecordEnvelope,
    pub body: RecordBody,
}

/// Variant carrying type-specific fields.
pub enum RecordBody {
    Epic(Epic),
    Task(Task),
    Subtask(Subtask),
    Bug(Bug),
    // Memory-kind variants — authorable via `RecordBuilder` as of M2.
    Incident(Incident),
    Finding(Finding),
    Runbook(Runbook),
    Decision(Decision),
    Gotcha(Gotcha),
    Memory(Memory),
}
```

### Ticket bodies (M1)

```rust
pub struct Epic {
    pub description: String,
    pub child_ids: Vec<RecordId>,  // denormalized for fast reads
}

pub struct Task {
    pub description: String,
    pub parent_epic: Option<RecordId>,
    pub acceptance_criteria: Vec<AcceptanceCriterion>,
    pub evidence: Vec<Evidence>,
    pub claim: Option<Claim>,
}

pub struct Subtask {
    pub description: String,
    pub parent_task: RecordId,
    pub acceptance_criteria: Vec<AcceptanceCriterion>,
    pub evidence: Vec<Evidence>,
    pub claim: Option<Claim>,
}

pub struct Bug {
    pub description: String,
    pub service: Option<String>,
    pub severity: Option<String>,
    pub acceptance_criteria: Vec<AcceptanceCriterion>,
    pub evidence: Vec<Evidence>,
    pub claim: Option<Claim>,
}
```

### Memory-kind bodies (M2)

Finalized as of M2: these bodies round-trip cleanly **and** are authorable via
`RecordBuilder` (`.incident(..)`, `.finding(..)`, `.runbook(..)`, `.decision(..)`,
`.gotcha(..)`, `.memory(..)`). The `ft-ops` create surface
(`create_incident`, `create_finding`, `create_runbook`, `create_decision`,
`create_gotcha`, `create_memory`, plus the polymorphic `capture`) and the
matching `firetrail <kind> create` CLI commands write them. All optional fields
carry `#[serde(default)]`, so older payloads that omit them deserialize without
loss. Memory records are immutable once written, so every authorable field must
be supplied at create time.

```rust
pub struct Incident {
    pub summary: String,
    pub severity: Severity,                 // defaults to Sev3
    pub started_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub services_affected: Vec<String>,
    pub root_cause: Option<String>,
    pub findings: Vec<RecordId>,
    pub runbooks_invoked: Vec<RecordId>,
    pub risk_class: Option<RiskClass>,
    pub trust: TrustState,
}

pub struct Finding {
    pub summary: String,
    pub details: String,
    pub incident: Option<RecordId>,
    pub risk_class: Option<RiskClass>,
    pub affected_paths: Vec<String>,
    pub superseded_by: Option<RecordId>,
    pub trust: TrustState,
}

pub struct RunbookStep {
    pub description: String,
    pub command: Option<String>,
    pub expected_outcome: String,
}

pub struct Runbook {
    pub title: String,
    pub summary: String,
    pub steps: Vec<RunbookStep>,
    pub applies_to: Vec<String>,
    pub risk_class: Option<RiskClass>,
    pub trust: TrustState,
}

pub struct Decision {
    pub title: String,
    pub context: String,
    pub decision: String,
    pub consequences: String,
    pub alternatives_considered: Vec<String>,
    pub status: DecisionStatus,             // proposed | accepted | superseded | deprecated
    pub superseded_by: Option<RecordId>,
    pub risk_class: Option<RiskClass>,
    pub trust: TrustState,
}

pub struct Gotcha {
    pub summary: String,
    pub details: String,
    pub affected_paths: Vec<String>,
    pub risk_class: Option<RiskClass>,
    pub trust: TrustState,
}

pub struct Memory {
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
    pub related: Vec<RecordId>,
    pub risk_class: Option<RiskClass>,
    pub trust: TrustState,
}
```

### Status

```rust
pub enum Status {
    Open,
    Ready,
    InProgress,
    Review,
    Blocked,
    Closed,
    Deferred,
    Archived,
}
```

### Priority

```rust
/// 0 = critical, 4 = backlog. Mirrors bd's priority scheme.
pub enum Priority { P0, P1, P2, P3, P4 }
```

### Origin

```rust
pub enum Origin { Human, Agent, Imported }
```

### Acceptance criteria

```rust
pub struct AcceptanceCriterion {
    pub id: String,                  // local to the record; "ac-01", "ac-02", ...
    pub text: String,
    pub status: AcStatus,
    pub evidence_url: Option<String>,
    pub checked_by: Option<Identity>,
    pub checked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub proposed: bool,              // ADR-0013: agent-proposed needs human confirm
}

pub enum AcStatus { Unchecked, Checked }
```

### Evidence

```rust
pub struct Evidence {
    pub id: String,
    pub kind: EvidenceKind,
    pub url: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub created_by: Identity,
    pub commit_sha: Option<String>,
    pub symbol_name: Option<String>,
    pub content_hash: Option<String>,
}

pub enum EvidenceKind {
    IncidentReport,
    PullRequest,
    Commit,
    Dashboard,
    LogQuery,
    TestResult,
    JiraTicket,
    ConfluencePage,
    ManualNote,
}
```

### Claim

```rust
pub struct Claim {
    pub claimed_by: Identity,
    pub claimed_at: DateTime<Utc>,
    pub claim_source: String,
    pub claim_expires_at: DateTime<Utc>,  // mandatory (ADR-0008)
}
```

### Relations

```rust
pub struct Relation {
    pub from: RecordId,
    pub to: RecordId,
    pub kind: RelationKind,
    pub created_at: DateTime<Utc>,
    pub created_by: Identity,
}

pub enum RelationKind {
    Blocks,
    BlockedBy,
    ParentOf,
    ChildOf,
    RelatedTo,
    Duplicates,
    Supersedes,
    DiscoveredDuring,
    FollowUpFrom,
    FixedBy,
    CausedBy,
    MitigatedBy,
    DocumentedIn,
    ImplementedBy,
    RegressedBy,
    Affects,
    OwnedBy,
}
```

M1 writable subset: Blocks/BlockedBy, ParentOf/ChildOf, RelatedTo, Duplicates,
Supersedes. The rest declared for forward compatibility.

### Labels

```rust
pub struct Label {
    pub key: String,
    pub value: String,
}
```

### History entry (declared at M1; populated by ft-history in M2)

```rust
pub struct HistoryEntry {
    pub merged_via_pr: Option<u64>,
    pub timestamp: DateTime<Utc>,
    pub primary_actor: Identity,
    pub contributors: Vec<Identity>,
    pub ops_summary: Vec<String>,
    pub ops_count: u32,
    pub from_hash: String,
    pub to_hash: String,
}
```

### Trust state (declared at M1; enforced by ft-trust in M2)

```rust
pub enum TrustState {
    Draft,
    Reviewed,
    Verified,
    Stale,
    Deprecated,
    Archived,
    Superseded,
    Rejected,
    Redacted,
}

pub enum RiskClass {
    Security,
    Availability,
    DataLoss,
    Compliance,
    Performance,
    Correctness,
}
```

### Errors

```rust
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("invalid record id: {0}")]
    InvalidId(String),

    #[error("invalid identity: {0}")]
    InvalidIdentity(String),

    #[error("schema validation failed: {0}")]
    SchemaValidation(String),

    #[error("serde failure: {0}")]
    Serde(#[from] serde_json::Error),
}
```

---

## Internal design

### Canonical JSON serialization

Field order is fixed. `serde` derives produce deterministic output. The
`canonical_json` helper sorts maps and arrays per ADR-0017 conventions so
`state_hash` is reproducible.

### `state_hash` computation

Concatenates the canonical JSON of the record envelope and body **excluding**
`state_hash` and `prev_state_hash`. SHA-256. Hex lowercase.

At M1, every record carries a `state_hash` set by this function on every write.
`prev_state_hash` is `None` for new records and remains `None` through M1 — the
chain is wired by ft-history at M2.

### `RecordId::mint`

```
nonce      = 128-bit cryptographically random
identity   = identity.as_str()
kind       = serde_json::to_string(&kind)
timestamp  = chrono::Utc::now().timestamp_millis().to_string()

material   = format!("{nonce}|{identity}|{kind}|{timestamp}")
hash       = sha256(material)
RecordId   = format!("{kind_prefix}-{hex(hash)}")
```

`kind_prefix` is `TASK`, `EPIC`, `BUG`, `SUB`, `INC`, `FIND`, `RUN`, `DEC`,
`GOTCHA`, `MEM`. Lowercase in stored paths (ADR-0015); uppercase in display
where conventional.

### Validation

Every record builds through a `RecordBuilder` that validates as it constructs.
JSON Schema is generated from the Rust types via `schemars`. The schema is
exported to `docs/schema/firetrail-record-v1.json` so external tools (PR review
bots, importers) can validate independently.

---

## Acceptance

1. All record kinds round-trip through `serde_json::to_string` → `from_str`
   producing identical structs (property test over `proptest::arbitrary`).
2. `RecordId::mint` produces 16M+ unique IDs in a stress test (no collisions on
   any reasonable run; collision check is on the full hash, not a prefix).
3. Canonical JSON is byte-stable across runs for the same record.
4. `state_hash` is deterministic and excludes `state_hash` and `prev_state_hash`
   from its own input (test: alter only `state_hash`, verify recomputed hash is
   unchanged).
5. `RecordBuilder` rejects invalid records: missing title, invalid identity,
   acceptance criterion text empty, etc.
6. Display of `RecordId::short(n)` is exactly `n` hex characters (n >= 6).
7. JSON Schema generated by `schemars` validates every test fixture record
   produced by `ft-testkit` factories.
8. Memory-kind variants serialize and deserialize but the M1 `RecordBuilder`
   rejects creating them with a clear "writable from M2" error.

---

## Testing requirements

- Unit tests for every public method.
- Property tests for:
  - Record roundtrip
  - `RecordId::mint` uniqueness
  - Canonical JSON stability
  - Hash computation determinism
- Integration tests via `ft-testkit` once that crate is ready (E-M1-02).
- Doc tests on every public function with at least one runnable example.

---

## Out of scope (deferred to later milestones)

- History chain transitions (`ft-history` in M2).
- Trust state machine enforcement (`ft-trust` in M2).
- Scope routing logic (`ft-scope` in M5).
- Identity registry, capabilities, on-behalf-of (`ft-identity` registry features in M5).
- Memory-kind writable bodies (`finding create`, etc. in M2).

---

## References

- ADR-0002 — JSON-in-Git storage
- ADR-0003 — PR-time history compaction
- ADR-0004 — Multi-scope records
- ADR-0008 — Identity registry
- ADR-0013 — Trust model
- ADR-0015 — Hash-based IDs
- ADR-0017 — Audit chain integrity
