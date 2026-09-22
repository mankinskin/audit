# [AOH][Impl] Agent Identity — Persona Store, LRU Assignment, Trait Injection

## Purpose

Manage the pool of reusable agent personas and handle assignment to sessions. Per ADR-8, agents are identified by nature-vocabulary personas (e.g., "Basalt", "Coral", "Zephyr") drawn from a configurable pool in `config/personas.toml`. The persona store tracks which personas are currently assigned, performs LRU-based selection for new sessions, and supports same-persona revival for continuing work.

This ticket was identified as a gap during reconciliation (`02412b9a`): the persona store design was completed in `d45826cd` but no implementation ticket existed.

## Component Boundaries

### In scope
- **`PersonaStore`**: loads persona pool from `personas.toml`, tracks assignment state
- **LRU assignment**: assigns least-recently-used persona to new sessions; avoids re-assigning currently active personas
- **Same-persona revival**: when reviving a session, re-assigns the same persona that was used previously
- **Trait injection**: configures per-persona git identity (`user.name`, `user.email`) on the agent's worktree
- **Persona metadata**: each persona has a display name, short code, and optional trait dimensions (from design `d45826cd`)
- **Assignment record**: tracks `(persona_id, session_id, assigned_at, released_at)` for audit trail
- **Pool exhaustion handling**: when all personas are assigned, either queue the session or assign a numbered fallback

### Out of scope
- Persona behavior/personality injection into prompts (future feature)
- Git worktree creation (owned by sandbox manager `51471c3e`)
- Session lifecycle management (owned by assignment runner `a8632357`)

## Key Data Types

```rust
/// The persona store — manages persona pool and assignments.
struct PersonaStore {
    personas: Vec<Persona>,
    assignments: Vec<Assignment>,
    lru_order: VecDeque<PersonaId>,
}

/// A single persona definition from personas.toml.
struct Persona {
    id: PersonaId,
    name: String,                   // e.g., "Basalt"
    short_code: String,             // e.g., "bas"
    email: String,                  // e.g., "basalt@aoh.local"
    traits: Option<PersonaTraits>,  // optional 4-dimension trait matrix
}

/// Trait dimensions (from design d45826cd).
struct PersonaTraits {
    approach: f32,      // methodical ↔ exploratory
    verbosity: f32,     // terse ↔ detailed
    risk: f32,          // conservative ↔ bold
    focus: f32,         // depth-first ↔ breadth-first
}

/// Assignment record for audit trail.
struct Assignment {
    persona_id: PersonaId,
    session_id: SessionId,
    ticket_id: TicketId,
    assigned_at: DateTime<Utc>,
    released_at: Option<DateTime<Utc>>,
}

/// Assignment result.
enum AssignResult {
    Assigned(Persona),
    Revived(Persona),               // same persona re-assigned for revival
    Exhausted,                      // all personas currently active
}
```

## Design Decisions Mapped from ADRs

| ADR | Implication |
|---|---|
| ADR-8 (Agent identity) | Nature-vocabulary personas, LRU assignment, same-persona revival |
| ADR-11 (Branch naming) | Persona short_code used in branch names: `aoh/{short_code}/{ticket-slug}` |
| ADR-14 (Session archive) | Persona ID recorded in `session-archive.toml` for traceability |
| `d45826cd` (Persona store design) | Full schema, 39 nature names, 4-dimension trait matrix, worktree-local git config |

## Dependencies

- Design input: `d45826cd` (persona store design — done)
- Blocks: `6e6b8cf6` (orchestrator core — uses PersonaStore)

## Acceptance Criteria

- [ ] `PersonaStore` loads persona pool from `personas.toml` and validates entries
- [ ] LRU assignment selects the least-recently-used available persona
- [ ] Currently assigned personas are excluded from the available pool
- [ ] Same-persona revival re-assigns the original persona when reviving a session
- [ ] Git identity injection sets `user.name` and `user.email` on the worktree config
- [ ] Pool exhaustion returns `AssignResult::Exhausted` rather than panicking
- [ ] Assignment records are maintained for audit trail queries
- [ ] `personas.toml` schema validation rejects malformed entries with clear errors
- [ ] Unit tests cover: LRU ordering, concurrent assignment exclusion, revival re-assignment, pool exhaustion, and git config injection