use strum::AsRefStr;
use strum::Display;
use strum::EnumString;

/// Status attached to a directional thread-spawn edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, AsRefStr, Display, EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum DirectionalThreadSpawnEdgeStatus {
    Open,
    Closed,
}

/// Outcome of closing one exact Open directional thread-spawn edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadSpawnEdgeCloseOutcome {
    /// The exact parent/child edge transitioned from Open to Closed.
    NewlyClosed,
    /// The exact parent/child edge was already Closed.
    AlreadyExactClosed,
    /// The child edge is missing or belongs to a different parent.
    MismatchOrMissing,
}
