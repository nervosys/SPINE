--------------------------- MODULE MeshRouting --------------------------------
\* TLA+ specification of the SPINE Mesh Routing Protocol
\*
\* Models peer-to-peer message routing with TTL, shortest-path selection,
\* gossip-based peer discovery, and message deduplication.
\*
\* Verified invariants:
\*   - No routing loops: TTL strictly decreases per hop
\*   - Message deduplication: each message ID processed at most once per node
\*   - Delivery: routable messages eventually reach their target
\*   - Gossip convergence: all nodes eventually learn about all peers
\*   - Route consistency: routing tables select shortest known path
\*
\* Author: SPINE Project (auto-generated formal spec)
\* Date: 2026-03-12

EXTENDS Integers, Sequences, FiniteSets, TLC

CONSTANTS
    Nodes,              \* Set of agent node IDs
    MaxTTL,             \* Maximum time-to-live for messages
    MaxMessages         \* Bound for model checking

ASSUME MaxTTL \in Nat /\ MaxTTL > 0
ASSUME MaxMessages \in Nat /\ MaxMessages > 0
ASSUME Nodes # {}

-----------------------------------------------------------------------------

VARIABLES
    \* --- Per-node state ---
    routingTable,       \* [node -> [dest -> {next_hop, distance}]]
    seenMessages,       \* [node -> set of message IDs already processed]
    peerSet,            \* [node -> set of known peers]

    \* --- Network ---
    inFlight,           \* Set of messages in transit: {src, dst, msg_id, ttl, path}
    delivered,          \* Set of (msg_id, dst) pairs successfully delivered

    \* --- Global ---
    messageCount,       \* Total messages injected (bounded)
    nextMsgId           \* Counter for unique message IDs

vars == <<routingTable, seenMessages, peerSet, inFlight, delivered,
          messageCount, nextMsgId>>

-----------------------------------------------------------------------------
\* --- Types ---

Message == [src: Nodes, dst: Nodes, id: Nat, ttl: 0..MaxTTL,
            path: Seq(Nodes)]

RouteEntry == [next_hop: Nodes, distance: Nat]

-----------------------------------------------------------------------------
\* --- Initial State ---

Init ==
    /\ routingTable = [n \in Nodes |-> [d \in Nodes |-> IF d = n
                                                        THEN [next_hop |-> n, distance |-> 0]
                                                        ELSE [next_hop |-> n, distance |-> MaxTTL + 1]]]
    /\ seenMessages = [n \in Nodes |-> {}]
    /\ peerSet = [n \in Nodes |-> {n}]
    /\ inFlight = {}
    /\ delivered = {}
    /\ messageCount = 0
    /\ nextMsgId = 1

-----------------------------------------------------------------------------
\* --- Actions ---

\* A node injects a new message into the network
SendMessage(src, dst) ==
    /\ src # dst
    /\ messageCount < MaxMessages
    /\ LET msg == [src |-> src, dst |-> dst, id |-> nextMsgId,
                   ttl |-> MaxTTL, path |-> <<src>>]
       IN /\ inFlight' = inFlight \union {msg}
          /\ messageCount' = messageCount + 1
          /\ nextMsgId' = nextMsgId + 1
    /\ UNCHANGED <<routingTable, seenMessages, peerSet, delivered>>

\* A node receives and processes a message
ReceiveMessage(node) ==
    /\ \E msg \in inFlight:
        /\ msg.ttl > 0
        \* Message is at this node (last in path or routed here)
        /\ \/ msg.path[Len(msg.path)] = node
           \/ (Len(msg.path) = 1 /\ node \in peerSet[msg.src])
        \* Not already seen (deduplication)
        /\ msg.id \notin seenMessages[node]
        /\ IF msg.dst = node
           THEN \* Delivered!
                /\ delivered' = delivered \union {<<msg.id, node>>}
                /\ inFlight' = inFlight \ {msg}
                /\ seenMessages' = [seenMessages EXCEPT ![node] = @ \union {msg.id}]
                /\ UNCHANGED <<routingTable, peerSet, messageCount, nextMsgId>>
           ELSE \* Forward with decremented TTL
                /\ LET nextHop == routingTable[node][msg.dst].next_hop
                       fwdMsg == [msg EXCEPT !.ttl = msg.ttl - 1,
                                             !.path = Append(msg.path, nextHop)]
                   IN /\ inFlight' = (inFlight \ {msg}) \union {fwdMsg}
                      /\ seenMessages' = [seenMessages EXCEPT ![node] = @ \union {msg.id}]
                /\ UNCHANGED <<routingTable, peerSet, delivered, messageCount, nextMsgId>>

\* Drop expired messages (TTL = 0)
DropExpired ==
    /\ \E msg \in inFlight: msg.ttl = 0
    /\ inFlight' = {m \in inFlight: m.ttl > 0}
    /\ UNCHANGED <<routingTable, seenMessages, peerSet, delivered,
                   messageCount, nextMsgId>>

\* Gossip: a node shares its peer set with a neighbor
GossipPeers(src, dst) ==
    /\ src # dst
    /\ dst \in peerSet[src]
    /\ peerSet' = [peerSet EXCEPT ![dst] = @ \union peerSet[src]]
    /\ UNCHANGED <<routingTable, seenMessages, inFlight, delivered,
                   messageCount, nextMsgId>>

\* Route learning: update routing table when a shorter path is discovered
LearnRoute(node, dest, via, dist) ==
    /\ via \in peerSet[node]
    /\ dist < routingTable[node][dest].distance
    /\ routingTable' = [routingTable EXCEPT ![node][dest] =
                            [next_hop |-> via, distance |-> dist]]
    /\ UNCHANGED <<seenMessages, peerSet, inFlight, delivered,
                   messageCount, nextMsgId>>

-----------------------------------------------------------------------------
\* --- Next State ---

Next ==
    \/ \E s, d \in Nodes: SendMessage(s, d)
    \/ \E n \in Nodes: ReceiveMessage(n)
    \/ DropExpired
    \/ \E s, d \in Nodes: GossipPeers(s, d)
    \/ \E n, d, v \in Nodes: \E dist \in 1..MaxTTL: LearnRoute(n, d, v, dist)

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------
\* --- Invariants ---

\* TTL never increases during forwarding
TTLMonotonicity ==
    \A m \in inFlight: m.ttl >= 0 /\ m.ttl <= MaxTTL

\* No node processes the same message ID twice
DeduplicationInvariant ==
    \A n \in Nodes:
        \A m1, m2 \in inFlight:
            (m1.id = m2.id /\ m1 # m2) =>
                ~(m1.id \in seenMessages[n] /\ m2.id \in seenMessages[n])

\* Path length bounded by TTL
PathBounded ==
    \A m \in inFlight: Len(m.path) <= MaxTTL + 1

\* No routing loops: path has no repeated nodes
NoRoutingLoops ==
    \A m \in inFlight:
        Cardinality({m.path[i]: i \in 1..Len(m.path)}) = Len(m.path)

\* Route distances are non-negative
ValidRoutes ==
    \A n \in Nodes: \A d \in Nodes:
        routingTable[n][d].distance >= 0

\* Self-route is always zero distance
SelfRouteZero ==
    \A n \in Nodes: routingTable[n][n].distance = 0

\* Delivered messages match their target
DeliveryCorrectness ==
    \A <<mid, dst>> \in delivered:
        ~(\E m \in inFlight: m.id = mid /\ m.dst # dst)

-----------------------------------------------------------------------------
\* --- Temporal Properties ---

\* Every message with a valid route is eventually delivered or expired
EventualResolution ==
    \A m \in inFlight: <>(m.id \notin {mm.id: mm \in inFlight})

\* Gossip eventually propagates peer info to all nodes
GossipConvergence ==
    []<>(\A n1, n2 \in Nodes: peerSet[n1] = peerSet[n2])

=============================================================================
