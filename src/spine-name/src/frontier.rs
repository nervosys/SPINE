//! The crawl frontier — how an agent walks the web rather than a single site.
//!
//! Everything the human web's crawlers had to reconstruct heuristically is
//! explicit here: edges are typed, so ordering is a property of the data;
//! budgets are enforced up front, so a traversal cannot run away; and dedup is
//! by routing key, so the same resource reached by two spellings is visited
//! once.
//!
//! The ordering is a deliberate blend rather than plain breadth-first. Agents
//! traverse under a token budget, so the frontier must spend early visits on
//! edges most likely to matter: depth first (stay shallow), then relation
//! priority (dependencies before documentation), then insertion order for
//! determinism. Pure BFS would exhaust the budget on a wide shallow fan-out of
//! `describes` edges before ever following the one `requires` edge that
//! actually unblocks the task.

use std::collections::{BinaryHeap, HashSet};

use crate::key::NameKey;
use crate::link::{Link, Rel};
use crate::uri::SpineUri;

/// A name queued for visiting, with the context that got it there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Visit {
    /// The name to fetch.
    pub uri: SpineUri,
    /// Hops from the seed.
    pub depth: u32,
    /// The relation of the edge that led here (`None` for a seed).
    pub via: Option<Rel>,
}

/// Heap entry ordering by (depth, rel priority, sequence), min-first.
#[derive(Debug, PartialEq, Eq)]
struct Queued {
    depth: u32,
    priority: u8,
    /// Insertion counter, making the order total and deterministic.
    seq: u64,
    visit: Visit,
}

impl Ord for Queued {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // BinaryHeap is a max-heap; reverse so the smallest key pops first.
        (other.depth, other.priority, other.seq).cmp(&(self.depth, self.priority, self.seq))
    }
}

impl PartialOrd for Queued {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Limits on a traversal.
#[derive(Debug, Clone)]
pub struct CrawlBudget {
    /// Maximum hops from any seed.
    pub max_depth: u32,
    /// Maximum names handed out across the whole traversal.
    pub max_visits: usize,
    /// Relations to follow. Empty means all.
    pub follow: Vec<Rel>,
}

impl Default for CrawlBudget {
    fn default() -> Self {
        Self {
            max_depth: 3,
            max_visits: 1_000,
            follow: Vec::new(),
        }
    }
}

impl CrawlBudget {
    pub fn with_max_depth(mut self, depth: u32) -> Self {
        self.max_depth = depth;
        self
    }

    pub fn with_max_visits(mut self, visits: usize) -> Self {
        self.max_visits = visits;
        self
    }

    /// Restrict traversal to specific relations.
    pub fn following(mut self, rels: Vec<Rel>) -> Self {
        self.follow = rels;
        self
    }

    fn allows(&self, rel: &Rel) -> bool {
        self.follow.is_empty() || self.follow.contains(rel)
    }
}

/// Why a candidate was not enqueued. Returned so a caller can report coverage
/// honestly instead of presenting a truncated crawl as a complete one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skipped {
    /// Already seen this routing key.
    Duplicate,
    /// Beyond `max_depth`.
    TooDeep,
    /// Relation excluded by the budget.
    RelationFiltered,
    /// `max_visits` already reached.
    BudgetExhausted,
}

/// A deduplicating, priority-ordered traversal queue.
#[derive(Debug)]
pub struct CrawlFrontier {
    queue: BinaryHeap<Queued>,
    /// Dedup by routing key, so `/a/b` and `/a/./b` are one resource.
    seen: HashSet<NameKey>,
    budget: CrawlBudget,
    handed_out: usize,
    counter: u64,
    skipped: Vec<(SpineUri, Skipped)>,
}

impl CrawlFrontier {
    pub fn new(budget: CrawlBudget) -> Self {
        Self {
            queue: BinaryHeap::new(),
            seen: HashSet::new(),
            budget,
            handed_out: 0,
            counter: 0,
            skipped: Vec::new(),
        }
    }

    /// Seed the traversal at depth 0.
    pub fn seed(&mut self, uri: SpineUri) -> bool {
        self.enqueue(uri, 0, None).is_ok()
    }

    /// Enqueue every link a record exposes, at `depth + 1`.
    ///
    /// Returns how many were accepted; the rest are recorded in
    /// [`CrawlFrontier::skipped`] with a reason.
    pub fn expand(&mut self, from: &SpineUri, links: &[Link], depth: u32) -> usize {
        let mut accepted = 0;
        for link in links {
            if !self.budget.allows(&link.rel) {
                self.skipped
                    .push((link.target.clone(), Skipped::RelationFiltered));
                continue;
            }
            // Resolve relative targets against the source, so a publisher can
            // emit `child` links without repeating its own authority.
            let target = match from.join(&link.target.to_string()) {
                Ok(t) => t,
                Err(_) => link.target.clone(),
            };
            if self
                .enqueue(target, depth + 1, Some(link.rel.clone()))
                .is_ok()
            {
                accepted += 1;
            }
        }
        accepted
    }

    fn enqueue(&mut self, uri: SpineUri, depth: u32, via: Option<Rel>) -> Result<(), Skipped> {
        if depth > self.budget.max_depth {
            self.skipped.push((uri, Skipped::TooDeep));
            return Err(Skipped::TooDeep);
        }
        let key = uri.key();
        if !self.seen.insert(key) {
            self.skipped.push((uri, Skipped::Duplicate));
            return Err(Skipped::Duplicate);
        }
        let priority = via.as_ref().map_or(0, |r| r.default_priority());
        self.counter += 1;
        self.queue.push(Queued {
            depth,
            priority,
            seq: self.counter,
            visit: Visit { uri, depth, via },
        });
        Ok(())
    }

    /// Take the next name to visit, or `None` when drained or over budget.
    pub fn next_visit(&mut self) -> Option<Visit> {
        if self.handed_out >= self.budget.max_visits {
            // Record the remainder so the caller can say what was left unwalked.
            while let Some(q) = self.queue.pop() {
                self.skipped.push((q.visit.uri, Skipped::BudgetExhausted));
            }
            return None;
        }
        let q = self.queue.pop()?;
        self.handed_out += 1;
        Some(q.visit)
    }

    /// Names pending.
    pub fn pending(&self) -> usize {
        self.queue.len()
    }

    /// Distinct names ever enqueued.
    pub fn seen_count(&self) -> usize {
        self.seen.len()
    }

    /// Names handed out so far.
    pub fn visited_count(&self) -> usize {
        self.handed_out
    }

    /// Candidates that were not walked, with the reason. Surfacing this is what
    /// keeps a bounded crawl from silently reading as an exhaustive one.
    pub fn skipped(&self) -> &[(SpineUri, Skipped)] {
        &self.skipped
    }

    /// Whether a name has already been enqueued.
    pub fn has_seen(&self, uri: &SpineUri) -> bool {
        self.seen.contains(&uri.key())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri(path: &str) -> SpineUri {
        SpineUri::did([1u8; 32]).with_path(path)
    }

    #[test]
    fn seeds_then_drains_in_order() {
        let mut f = CrawlFrontier::new(CrawlBudget::default());
        assert!(f.seed(uri("/a")));
        assert_eq!(f.pending(), 1);
        assert_eq!(f.next_visit().unwrap().uri, uri("/a"));
        assert!(f.next_visit().is_none());
        assert_eq!(f.visited_count(), 1);
    }

    #[test]
    fn deduplicates_by_routing_key_not_by_spelling() {
        let mut f = CrawlFrontier::new(CrawlBudget::default());
        assert!(f.seed(uri("/a/b")));
        // Same resource, different spelling and a fragment.
        assert!(!f.seed(SpineUri::did([1u8; 32]).with_path("/a/./b")));
        assert!(!f.seed(uri("/a/b").with_fragment("section")));
        assert_eq!(f.seen_count(), 1);
        assert_eq!(
            f.skipped()
                .iter()
                .filter(|(_, s)| *s == Skipped::Duplicate)
                .count(),
            2
        );
    }

    #[test]
    fn expands_links_at_the_next_depth() {
        let mut f = CrawlFrontier::new(CrawlBudget::default());
        f.seed(uri("/"));
        let root = f.next_visit().unwrap();
        let links = vec![
            Link::new(Rel::Child, uri("/a")),
            Link::new(Rel::Child, uri("/b")),
        ];
        assert_eq!(f.expand(&root.uri, &links, root.depth), 2);
        let v = f.next_visit().unwrap();
        assert_eq!(v.depth, 1);
        assert_eq!(v.via, Some(Rel::Child));
    }

    #[test]
    fn shallower_names_are_visited_before_deeper_ones() {
        let mut f = CrawlFrontier::new(CrawlBudget::default());
        f.seed(uri("/root"));
        let root = f.next_visit().unwrap();
        f.expand(&root.uri, &[Link::new(Rel::Child, uri("/d1"))], 0);
        let d1 = f.next_visit().unwrap();
        // Enqueue a depth-2 name, then another depth-1 name.
        f.expand(&d1.uri, &[Link::new(Rel::Child, uri("/d2"))], 1);
        f.expand(&root.uri, &[Link::new(Rel::Child, uri("/d1b"))], 0);
        assert_eq!(f.next_visit().unwrap().depth, 1, "depth 1 before depth 2");
        assert_eq!(f.next_visit().unwrap().depth, 2);
    }

    #[test]
    fn at_equal_depth_dependencies_precede_documentation() {
        let mut f = CrawlFrontier::new(CrawlBudget::default());
        f.seed(uri("/root"));
        let root = f.next_visit().unwrap();
        // Enqueue in the *worst* order to prove ordering is by relation.
        f.expand(
            &root.uri,
            &[
                Link::new(Rel::Describes, uri("/docs")),
                Link::new(Rel::Peer, uri("/peer")),
                Link::new(Rel::Requires, uri("/dep")),
            ],
            0,
        );
        assert_eq!(f.next_visit().unwrap().via, Some(Rel::Requires));
        assert_eq!(f.next_visit().unwrap().via, Some(Rel::Peer));
        assert_eq!(f.next_visit().unwrap().via, Some(Rel::Describes));
    }

    #[test]
    fn max_depth_bounds_the_traversal_and_is_reported() {
        let mut f = CrawlFrontier::new(CrawlBudget::default().with_max_depth(1));
        f.seed(uri("/root"));
        let root = f.next_visit().unwrap();
        f.expand(&root.uri, &[Link::new(Rel::Child, uri("/a"))], 0);
        let a = f.next_visit().unwrap();
        assert_eq!(a.depth, 1);
        // Depth 2 is refused.
        assert_eq!(f.expand(&a.uri, &[Link::new(Rel::Child, uri("/b"))], 1), 0);
        assert!(f.next_visit().is_none());
        assert!(f.skipped().iter().any(|(_, s)| *s == Skipped::TooDeep));
    }

    #[test]
    fn max_visits_stops_the_crawl_and_records_what_was_left() {
        let mut f = CrawlFrontier::new(CrawlBudget::default().with_max_visits(2));
        f.seed(uri("/a"));
        f.seed(uri("/b"));
        f.seed(uri("/c"));
        assert!(f.next_visit().is_some());
        assert!(f.next_visit().is_some());
        assert!(f.next_visit().is_none(), "budget exhausted");
        assert_eq!(
            f.skipped()
                .iter()
                .filter(|(_, s)| *s == Skipped::BudgetExhausted)
                .count(),
            1,
            "the unwalked name must be reported, not silently dropped"
        );
    }

    #[test]
    fn relation_filter_restricts_which_edges_are_followed() {
        let mut f =
            CrawlFrontier::new(CrawlBudget::default().following(vec![Rel::Child, Rel::Requires]));
        f.seed(uri("/root"));
        let root = f.next_visit().unwrap();
        let accepted = f.expand(
            &root.uri,
            &[
                Link::new(Rel::Child, uri("/keep")),
                Link::new(Rel::Describes, uri("/drop")),
                Link::new(Rel::Requires, uri("/dep")),
            ],
            0,
        );
        assert_eq!(accepted, 2);
        assert!(f
            .skipped()
            .iter()
            .any(|(u, s)| *s == Skipped::RelationFiltered && u.path() == "/drop"));
    }

    #[test]
    fn relative_link_targets_resolve_against_the_source() {
        let mut f = CrawlFrontier::new(CrawlBudget::default());
        let source = SpineUri::did([2u8; 32]).with_path("/tools/search");
        // A link whose target is a bare name under a *different* authority is
        // taken as-is; a same-authority sibling resolves relative to the source.
        f.expand(
            &source,
            &[Link::new(
                Rel::Peer,
                SpineUri::did([2u8; 32]).with_path("/tools/index"),
            )],
            0,
        );
        let v = f.next_visit().unwrap();
        assert_eq!(v.uri.path(), "/tools/index");
        assert_eq!(v.uri.public_key(), Some(&[2u8; 32]));
    }

    #[test]
    fn traversal_over_a_cycle_terminates() {
        let mut f = CrawlFrontier::new(CrawlBudget::default().with_max_depth(10));
        f.seed(uri("/a"));
        let mut visited = 0;
        while let Some(v) = f.next_visit() {
            visited += 1;
            // /a -> /b -> /a, forever, if dedup did not hold.
            let next = if v.uri.path() == "/a" { "/b" } else { "/a" };
            f.expand(&v.uri, &[Link::new(Rel::Child, uri(next))], v.depth);
            assert!(visited < 100, "cycle failed to terminate");
        }
        assert_eq!(visited, 2);
    }
}
