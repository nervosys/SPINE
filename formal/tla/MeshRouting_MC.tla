--------------------------- MODULE MeshRouting_MC -----------------------------
\* Model checking configuration for MeshRouting.tla
\*
\* Run with: tlc MeshRouting_MC.tla -workers auto
\*
\* Uses small constants for bounded model checking.

EXTENDS MeshRouting

\* --- Model Values ---

\* 3 nodes: sufficient to exercise multi-hop routing, gossip, and loops
MC_Nodes == {"A", "B", "C"}

\* Small TTL for feasible state space
MC_MaxTTL == 3

\* Bound total messages to keep state space tractable
MC_MaxMessages == 2

=============================================================================
\* Overrides for model checking:
\*   Nodes       <- MC_Nodes
\*   MaxTTL      <- MC_MaxTTL
\*   MaxMessages <- MC_MaxMessages
\*
\* Invariants to check:
\*   TTLMonotonicity
\*   DeduplicationInvariant
\*   PathBounded
\*   NoRoutingLoops
\*   ValidRoutes
\*   SelfRouteZero
\*   DeliveryCorrectness
\*
\* Temporal properties (optional, slower):
\*   EventualResolution
\*   GossipConvergence
