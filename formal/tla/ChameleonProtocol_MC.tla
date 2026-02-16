---- MODULE ChameleonProtocol_MC ----
\* Model checking configuration for ChameleonProtocol.tla
\*
\* Run with TLC:
\*   tlc ChameleonProtocol_MC.tla -workers auto

EXTENDS ChameleonProtocol

\* --- Finite Constants ---
MC_MaxMessages == 4
MC_MaxMorphSeed == 3

\* --- Constraint (bound state space) ---
StateConstraint ==
    /\ messageCount <= MC_MaxMessages
    /\ Len(channel_AB) <= 2
    /\ Len(channel_BA) <= 2
    /\ Cardinality(morphRequests) <= 2

\* --- Invariants to check ---
\* 1. EpochMonotonicity
\* 2. SynchronizedEvolution (critical safety property)
\* 3. BoundedMessages
\* 4. WellFormedChannel
\* 5. KeyEvolved

\* --- Temporal properties to check ---
\* 1. EventualDelivery

====
