//! Which actions of an `execute_binary` program the daemon performs.
//!
//! A program's actions are returned to the client as a list (`actions`), and
//! the response has no place to say that one was not carried out, so an action
//! the daemon cannot perform is refused with the whole program, before any
//! action runs, the way an undeclared capability is. Pure, so the daemon (a
//! binary) and the tests share one definition.
//!
//! Performed in every build: `navigate` (outbound fetch, needs `network`) and
//! `store_knowledge` (needs `storage`). `search` is performed only with the
//! `research` feature, which broadcasts it to cluster peers. `query_knowledge`
//! and `reason` are performed in no build: the daemon only logged them.

use spine_wasm::WasmAction;

/// The name of `action` when this daemon does not perform it, `None` when it
/// does. `research` is whether the daemon was built with the `research`
/// feature.
pub fn unsupported_action(action: &WasmAction, research: bool) -> Option<&'static str> {
    match action {
        WasmAction::Navigate(_) | WasmAction::StoreKnowledge { .. } => None,
        WasmAction::Search(_) => (!research).then_some("search"),
        WasmAction::QueryKnowledge { .. } => Some("query_knowledge"),
        WasmAction::Reason(_) => Some("reason"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigate_and_store_knowledge_are_always_performed() {
        let navigate = WasmAction::Navigate("http://example.test".into());
        let store = WasmAction::StoreKnowledge {
            key: "k".into(),
            value: serde_json::json!(1),
            tags: Vec::new(),
        };
        for research in [false, true] {
            assert_eq!(unsupported_action(&navigate, research), None);
            assert_eq!(unsupported_action(&store, research), None);
        }
    }

    #[test]
    fn search_is_performed_only_in_a_research_build() {
        let search = WasmAction::Search("q".into());
        assert_eq!(unsupported_action(&search, false), Some("search"));
        assert_eq!(unsupported_action(&search, true), None);
    }

    #[test]
    fn query_knowledge_and_reason_are_performed_in_no_build() {
        let query = WasmAction::QueryKnowledge {
            query: "q".into(),
            tags: Vec::new(),
            limit: 1,
        };
        let reason = WasmAction::Reason("why".into());
        for research in [false, true] {
            assert_eq!(
                unsupported_action(&query, research),
                Some("query_knowledge")
            );
            assert_eq!(unsupported_action(&reason, research), Some("reason"));
        }
    }
}
