//! Attenuation and the scope containment rule.

mod common;

use common::*;
use spine_authority::{
    is_attenuation_of, Action, Broadening, Effect, Grant, LimitKind, Limits, ResourceName,
    ResourcePattern, ScopeError,
};
use spine_name::SpineUri;

// ---------------------------------------------------------------------------
// scope containment
// ---------------------------------------------------------------------------

#[test]
fn a_prefix_contains_by_segment_never_by_raw_string() {
    let k = key(1);
    let data = prefix(&k, "/data");
    assert!(data.contains(&name(&k, "/data")));
    assert!(data.contains(&name(&k, "/data/x")));
    assert!(data.contains(&name(&k, "/data/x/y")));
    // A raw string prefix would accept these two.
    assert!(!data.contains(&name(&k, "/database")));
    assert!(!data.contains(&name(&k, "/data-old/x")));
    assert!(!data.contains(&name(&k, "/")));
    assert!(!data.contains(&name(&k, "/dat")));
    // Another owner with the same path.
    assert!(!data.contains(&name(&key(2), "/data/x")));
}

#[test]
fn an_exact_pattern_contains_only_its_own_resource() {
    let k = key(1);
    let one = exact(&k, "/data/x");
    assert!(one.contains(&name(&k, "/data/x")));
    assert!(!one.contains(&name(&k, "/data/x/y")));
    assert!(!one.contains(&name(&k, "/data")));
    assert!(!one.contains(&name(&k, "/data/xy")));
}

#[test]
fn the_root_prefix_is_everything_the_owner_has_and_nothing_else() {
    let k = key(1);
    let all = prefix(&k, "/");
    assert!(all.path.is_empty());
    assert!(all.contains(&name(&k, "/")));
    assert!(all.contains(&name(&k, "/a/b/c")));
    assert!(!all.contains(&name(&key(2), "/a")));
    let root_exact = exact(&k, "/");
    assert!(root_exact.contains(&name(&k, "/")));
    assert!(!root_exact.contains(&name(&k, "/a")));
}

#[test]
fn pattern_coverage_is_a_partial_order() {
    let k = key(1);
    let all = prefix(&k, "/");
    let data = prefix(&k, "/data");
    let reports = prefix(&k, "/data/reports");
    let q3 = exact(&k, "/data/reports/q3");
    let database = prefix(&k, "/database");
    let other = prefix(&key(2), "/data");

    // Reflexive.
    for p in [&all, &data, &reports, &q3] {
        assert!(p.covered_by(p));
    }
    // Narrower is covered by wider.
    assert!(data.covered_by(&all));
    assert!(reports.covered_by(&data));
    assert!(q3.covered_by(&reports));
    assert!(q3.covered_by(&data));
    assert!(q3.covered_by(&all));
    // Wider is not covered by narrower.
    assert!(!all.covered_by(&data));
    assert!(!data.covered_by(&reports));
    assert!(!reports.covered_by(&q3));
    // A prefix is never covered by an exact pattern, even the same path.
    assert!(exact(&k, "/data").covered_by(&data));
    assert!(!data.covered_by(&exact(&k, "/data")));
    // Siblings and other owners.
    assert!(!database.covered_by(&data));
    assert!(!data.covered_by(&database));
    assert!(!other.covered_by(&data));
    assert!(!data.covered_by(&other));
    // Two different exact patterns.
    assert!(!exact(&k, "/data/a").covered_by(&exact(&k, "/data/b")));
}

#[test]
fn resource_names_come_from_did_names_only() {
    let k = key(1);
    let ok = name(&k, "/data/x");
    assert_eq!(ok.owner, public(&k));
    assert_eq!(ok.path, vec!["data".to_string(), "x".to_string()]);
    assert_eq!(ok.to_uri(), uri(&k, "/data/x"));
    assert_eq!(ResourceName::from_uri(&ok.to_uri()).unwrap(), ok);
    assert_eq!(name(&k, "/").path, Vec::<String>::new());
    assert_eq!(name(&k, "/").to_uri(), uri(&k, "/"));

    let blob = SpineUri::blob([1u8; 32]);
    assert_eq!(ResourceName::from_uri(&blob), Err(ScopeError::NotDid));
    let cap = SpineUri::capability("web.search");
    assert_eq!(ResourceName::from_uri(&cap), Err(ScopeError::NotDid));
    let host = SpineUri::parse("spine://host:example.com/data").unwrap();
    assert_eq!(ResourceName::from_uri(&host), Err(ScopeError::NotDid));

    let with_query = uri(&k, "/data").with_query("a", "b");
    assert_eq!(
        ResourceName::from_uri(&with_query),
        Err(ScopeError::QueryOrFragment)
    );
    let with_fragment = uri(&k, "/data").with_fragment("top");
    assert_eq!(
        ResourcePattern::exact(&with_fragment),
        Err(ScopeError::QueryOrFragment)
    );
    let trailing = uri(&k, "/data/");
    assert_eq!(
        ResourceName::from_uri(&trailing),
        Err(ScopeError::TrailingSlash)
    );
}

#[test]
fn segments_are_checked() {
    let k = public(&key(1));
    assert!(ResourceName::new(k, vec!["a".to_string(), "b".to_string()]).is_ok());
    for bad in ["", ".", "..", "a/b", "a\nb", "a\u{0}b"] {
        assert_eq!(
            ResourceName::new(k, vec![bad.to_string()]),
            Err(ScopeError::BadSegment),
            "{bad:?}"
        );
    }
    assert_eq!(
        ResourceName::new(k, vec!["s".repeat(129)]),
        Err(ScopeError::SegmentTooLong)
    );
    assert!(ResourceName::new(k, vec!["s".repeat(128)]).is_ok());
    let deep: Vec<String> = (0..17).map(|i| i.to_string()).collect();
    assert_eq!(ResourceName::new(k, deep.clone()), Err(ScopeError::TooDeep));
    assert!(ResourceName::new(k, deep[..16].to_vec()).is_ok());
    // A `..` in a name is resolved by the name layer before it gets here, so
    // it can never climb out of a prefix.
    let climbing = uri(&key(1), "/data/../etc");
    assert_eq!(
        ResourceName::from_uri(&climbing).unwrap().path,
        vec!["etc".to_string()]
    );
}

// ---------------------------------------------------------------------------
// attenuation
// ---------------------------------------------------------------------------

fn standard() -> (World, Vec<Grant>) {
    let w = World::new();
    let grants = w.grants();
    (w, grants)
}

#[test]
fn the_standard_links_are_attenuations() {
    let (_, grants) = standard();
    assert_eq!(is_attenuation_of(&grants[1], &grants[0]), Ok(()));
    assert_eq!(is_attenuation_of(&grants[2], &grants[1]), Ok(()));
    // Not symmetric.
    assert!(is_attenuation_of(&grants[0], &grants[1]).is_err());
}

#[test]
fn a_child_equal_to_its_parent_in_everything_but_delegable_is_allowed() {
    let mut w = World::new();
    let mut child = w.bodies[0].clone();
    child.iss = w.bodies[0].aud;
    child.delegable = 2;
    child.nonce = [0x42u8; 16];
    w.bodies[1] = child;
    let grants = w.grants();
    assert_eq!(is_attenuation_of(&grants[1], &grants[0]), Ok(()));
}

#[test]
fn each_broadening_is_named() {
    // (what to change in the child, what is reported)
    let cases: Vec<(&str, Broadening)> = vec![
        ("audience", Broadening::Audience),
        ("resource wider", Broadening::Resource),
        ("resource other owner", Broadening::Resource),
        ("resource sibling", Broadening::Resource),
        ("resource exact to prefix", Broadening::Resource),
        ("action", Broadening::Action),
        ("effect", Broadening::Effect),
        ("delegable equal", Broadening::Delegable),
        ("delegable higher", Broadening::Delegable),
        ("nbf", Broadening::NotBefore),
        ("exp", Broadening::Expiry),
        ("calls raised", Broadening::LimitRaised(LimitKind::Calls)),
        ("cost raised", Broadening::LimitRaised(LimitKind::Cost)),
        ("bytes raised", Broadening::LimitRaised(LimitKind::Bytes)),
        ("rate raised", Broadening::LimitRaised(LimitKind::Rate)),
        ("calls removed", Broadening::LimitRemoved(LimitKind::Calls)),
        ("cost removed", Broadening::LimitRemoved(LimitKind::Cost)),
        ("bytes removed", Broadening::LimitRemoved(LimitKind::Bytes)),
        ("rate removed", Broadening::LimitRemoved(LimitKind::Rate)),
        ("caveat dropped", Broadening::CaveatDropped),
    ];
    for (label, expected) in cases {
        let mut w = World::new();
        // The parent is the middle grant: prefix /data/reports, read+invoke,
        // reversible, delegable 2, window 1000..9000, all four limits set.
        // The child starts as a valid narrowing of it.
        let mut child = w.bodies[2].clone();
        child.iss = w.bodies[1].aud;
        match label {
            "audience" => child.iss = public(&key(50)),
            "resource wider" => child.res = vec![prefix(&w.keys[0], "/data")],
            "resource other owner" => child.res = vec![exact(&w.keys[1], "/data/reports/q3")],
            "resource sibling" => child.res = vec![exact(&w.keys[0], "/data/reportsX/q3")],
            "resource exact to prefix" => {
                // Parent holds an exact pattern; the child asks for a prefix.
                w.bodies[1].res = vec![exact(&w.keys[0], "/data/reports/q3")];
                child.res = vec![prefix(&w.keys[0], "/data/reports/q3")];
            }
            "action" => child.act = vec![Action::Publish],
            "effect" => child.effect = Effect::Irreversible,
            "delegable equal" => child.delegable = 2,
            "delegable higher" => child.delegable = 5,
            "nbf" => child.nbf = 999,
            "exp" => child.exp = 9_001,
            "calls raised" => child.limits.calls = Some(51),
            "cost raised" => child.limits.cost = Some(501),
            "bytes raised" => child.limits.bytes = Some(500_001),
            "rate raised" => child.limits.rate = Some(6),
            "calls removed" => child.limits.calls = None,
            "cost removed" => child.limits.cost = None,
            "bytes removed" => child.limits.bytes = None,
            "rate removed" => child.limits.rate = None,
            "caveat dropped" => w.bodies[1].caveats = vec![caveat("only-in-parent")],
            _ => panic!("unknown case {label}"),
        }
        w.bodies[2] = child;
        let grants = w.grants();
        assert_eq!(
            is_attenuation_of(&grants[2], &grants[1]),
            Err(expected),
            "{label}"
        );
    }
}

#[test]
fn a_child_may_add_caveats_and_must_repeat_the_parents() {
    let mut w = World::new();
    w.bodies[1].caveats = vec![caveat("a")];
    // Repeats and adds.
    w.bodies[2].caveats = vec![caveat("a"), caveat("b")];
    let grants = w.grants();
    assert_eq!(is_attenuation_of(&grants[2], &grants[1]), Ok(()));
    // A caveat with the same name but another argument is not the same caveat.
    let mut w = World::new();
    w.bodies[1].caveats = vec![caveat("a")];
    let mut changed = caveat("a");
    changed.arg = vec![9];
    w.bodies[2].caveats = vec![changed];
    let grants = w.grants();
    assert_eq!(
        is_attenuation_of(&grants[2], &grants[1]),
        Err(Broadening::CaveatDropped)
    );
}

#[test]
fn a_parent_with_no_limit_lets_the_child_choose_any() {
    let mut w = World::new();
    w.bodies[1].limits = Limits::default();
    w.bodies[2].limits = Limits {
        calls: Some(1_000_000),
        cost: None,
        bytes: Some(1),
        rate: None,
    };
    let grants = w.grants();
    assert_eq!(is_attenuation_of(&grants[2], &grants[1]), Ok(()));
}

#[test]
fn the_first_broadening_in_rule_order_is_the_one_reported() {
    // Wrong audience and a wider resource: the audience rule comes first.
    let mut w = World::new();
    w.bodies[2].iss = public(&key(50));
    w.bodies[2].res = vec![prefix(&w.keys[0], "/")];
    let stranger = key(50);
    let grants = sign_chain(&w.bodies, &[&w.keys[0], &w.keys[1], &stranger]);
    assert_eq!(
        is_attenuation_of(&grants[2], &grants[1]),
        Err(Broadening::Audience)
    );
}
