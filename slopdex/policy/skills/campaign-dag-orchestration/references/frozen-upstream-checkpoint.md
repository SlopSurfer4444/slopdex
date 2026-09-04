# Frozen Upstream Checkpoint

Use this procedure only when an authorized campaign explicitly binds a frozen
base and a later upstream reconciliation checkpoint.

Hold the base stable while source slices are accepted. At the authorized
terminal checkpoint:

1. select and pin one target upstream snapshot;
2. map exact source, test, and semantic overlap;
3. remove local glue only where the target demonstrably owns the contract;
4. transplant and integrate the remaining accepted behavior;
5. obtain independent exact-final-bytes review; and
6. run the domain's terminal verification and, when required, one terminal
   build gate rather than one build per node.

A failed terminal build may be repeated only after an evidence-backed
correction. The single build gate is an integration cadence, not a ban on
honest repair. This procedure grants no fetch, mutation, build, install, push,
publication, or release authority.
