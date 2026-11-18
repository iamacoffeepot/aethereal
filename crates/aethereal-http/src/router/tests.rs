use super::*;
use crate::Method;
use aethereal_data::Plain;

/// A trie whose routes each hold their own pattern.
type Patterns = Trie<&'static str>;

/// A route a test adds: its method and pattern, and how it is added, with its captures type.
#[derive(Clone, Copy)]
struct Add {
    method: Method,
    pattern: &'static str,
    route: fn(Patterns, Method, &'static str) -> Result<Patterns, Unroutable>,
}

impl Add {
    fn to(self, trie: Patterns) -> Result<Patterns, Unroutable> {
        (self.route)(trie, self.method, self.pattern)
    }
}

/// A route for `method` on `pattern`, its captures read as `C`, holding its pattern.
fn route<C: Captures>(method: Method, pattern: &'static str) -> Add {
    Add { method, pattern, route: |trie, method, pattern| trie.route::<C>(method, pattern, pattern) }
}

/// The trie of `routes`, added in their order.
fn build(routes: &[Add]) -> Result<Patterns, Unroutable> {
    routes.iter().try_fold(Patterns::default(), |trie, route| route.to(trie))
}

/// The route found on `pattern`, its captures matching `slices`.
fn found<'p>(pattern: &'static &'static str, slices: Vec<&'p str>) -> Routed<Found<'static, 'p, &'static str>> {
    Routed::Found(Found { route: pattern, slices })
}

/// A repository, by name.
#[derive(Debug, PartialEq, Eq, Plain)]
struct Repo {
    repo: String,
}

impl Captures for Repo {
    const NAMES: &'static [&'static str] = &["repo"];

    fn read(slices: &[&str]) -> Option<Self> {
        let [repo] = slices else {
            return None;
        };

        Some(Self { repo: (*repo).to_owned() })
    }
}

/// An item, by number.
#[derive(Debug, PartialEq, Eq, Plain)]
struct Item {
    id: u32,
}

impl Captures for Item {
    const NAMES: &'static [&'static str] = &["id"];

    fn read(slices: &[&str]) -> Option<Self> {
        let [id] = slices else {
            return None;
        };

        Some(Self { id: id.parse().ok()? })
    }
}

/// A repository's blob, by number.
#[derive(Debug, PartialEq, Eq, Plain)]
struct Blob {
    repo: String,
    id: u32,
}

impl Captures for Blob {
    const NAMES: &'static [&'static str] = &["repo", "id"];

    fn read(slices: &[&str]) -> Option<Self> {
        let [repo, id] = slices else {
            return None;
        };

        Some(Self { repo: (*repo).to_owned(), id: id.parse().ok()? })
    }
}

#[test]
fn a_literal_route_matches_its_path_alone() -> Result<(), Unroutable> {
    // Catches a pattern matched as a prefix of the path, or the path as a prefix of the pattern.
    let trie = build(&[route::<()>(Method::Post, "/mcp")])?;

    assert_eq!(trie.find(Method::Post, "/mcp"), found(&"/mcp", vec![]));
    assert_eq!(trie.find(Method::Post, "/mcp/tools"), Routed::NotFound);
    assert_eq!(trie.find(Method::Post, "/mc"), Routed::NotFound);
    assert_eq!(trie.find(Method::Post, "/"), Routed::NotFound);

    Ok(())
}

#[test]
fn a_capture_matches_one_whole_segment_that_is_not_empty_and_hands_its_slice_of_the_path() -> Result<(), Unroutable> {
    // Catches a capture that takes more than one segment, or an empty one; and slices taken from the wrong segments,
    // or out of their pattern's order.
    let trie =
        build(&[route::<Repo>(Method::Get, "/git/{repo}/info/refs"), route::<Blob>(Method::Get, "/{repo}/{id}")])?;

    assert_eq!(trie.find(Method::Get, "/git/aether/info/refs"), found(&"/git/{repo}/info/refs", vec!["aether"]));
    assert_eq!(trie.find(Method::Get, "/aether/7"), found(&"/{repo}/{id}", vec!["aether", "7"]));
    assert_eq!(trie.find(Method::Get, "/git/a/b/info/refs"), Routed::NotFound);
    assert_eq!(trie.find(Method::Get, "/git//info/refs"), Routed::NotFound);
    assert_eq!(trie.find(Method::Get, "/aether/"), Routed::NotFound);

    Ok(())
}

#[test]
fn a_path_that_routes_match_for_other_methods_only_is_unallowed_naming_them() -> Result<(), Unroutable> {
    // Catches `Allow` taken from one branch of the walk alone (the literal `new`, or the capture beside it), in the
    // order the routes were given; and a path no route matches, or one routes match with more segments, answered
    // `405` rather than `404`.
    let trie = build(&[
        route::<()>(Method::Post, "/items/new"),
        route::<Item>(Method::Get, "/items/{id}"),
        route::<Item>(Method::Delete, "/items/{id}"),
    ])?;

    assert_eq!(
        trie.find(Method::Put, "/items/new"),
        Routed::Unallowed(vec![Method::Delete, Method::Get, Method::Post])
    );
    assert_eq!(trie.find(Method::Post, "/items/7"), Routed::Unallowed(vec![Method::Delete, Method::Get]));
    assert_eq!(trie.find(Method::Get, "/items/new"), found(&"/items/{id}", vec!["new"]));
    assert_eq!(trie.find(Method::Get, "/items"), Routed::NotFound);
    assert_eq!(trie.find(Method::Put, "/items/7/edit"), Routed::NotFound);

    Ok(())
}

#[test]
fn a_path_is_matched_as_sent_never_normalised() -> Result<(), Unroutable> {
    // Catches `//` collapsed, a trailing `/` dropped, or percent-encoding decoded, any of which routes a path its
    // pattern does not name; and a capture handed its slice decoded.
    let trie = build(&[
        route::<()>(Method::Get, "/a/b"),
        route::<()>(Method::Get, "/files/a%20b"),
        route::<Repo>(Method::Get, "/git/{repo}"),
    ])?;

    for path in ["/a//b", "//a/b", "/a/b/", "/files/a b", "/files/a%20B", "/git/a/b"] {
        assert_eq!(trie.find(Method::Get, path), Routed::NotFound, "{path}");
    }
    assert_eq!(trie.find(Method::Get, "/files/a%20b"), found(&"/files/a%20b", vec![]));
    assert_eq!(trie.find(Method::Get, "/git/a%2Fb"), found(&"/git/{repo}", vec!["a%2Fb"]));

    Ok(())
}

#[test]
fn a_pattern_that_is_not_slash_and_then_whole_segments_is_malformed() {
    // Catches a pattern without its leading `/`, a capture sharing its segment with text, a capture with no name or a
    // brace in its name, a stray brace, and a name captured twice, any of them routed.
    for pattern in ["", "mcp", "/v{n}", "/{n}x", "/{}", "/{a{b}}", "/a}", "/{id}/{id}"] {
        let refused = build(&[route::<()>(Method::Get, pattern)]).err();
        assert_eq!(refused, Some(Unroutable::Malformed { pattern: pattern.to_owned() }), "{pattern}");
    }
}

#[test]
fn a_pattern_whose_names_are_not_its_captures_types_is_uncaptured() {
    // Catches captures checked by their count alone, or not in order, so a slice is read as another name's; and a
    // captures type that reads names the pattern does not capture.
    for refused in [
        route::<Repo>(Method::Get, "/git/{name}"),
        route::<()>(Method::Get, "/git/{repo}"),
        route::<Repo>(Method::Get, "/git"),
        route::<Blob>(Method::Get, "/{id}/{repo}"),
    ] {
        let pattern = refused.pattern.to_owned();
        assert_eq!(build(&[refused]).err(), Some(Unroutable::Uncaptured { pattern }));
    }
}

#[test]
fn routes_that_match_one_path_for_one_method_overlap_in_either_order() {
    // Catches a literal and a capture at one depth taken as distinct, as a trie keyed by segment alone takes them;
    // two captures with other names taken as distinct; one pattern given twice kept as two routes; routes that cross,
    // each with a literal where the other has a capture; and a refusal that depends on which route came first.
    for (first, second) in [
        (route::<()>(Method::Get, "/items/new"), route::<Item>(Method::Get, "/items/{id}")),
        (route::<Repo>(Method::Get, "/git/{repo}"), route::<Item>(Method::Get, "/git/{id}")),
        (route::<()>(Method::Post, "/mcp"), route::<()>(Method::Post, "/mcp")),
        (route::<Repo>(Method::Get, "/{repo}/refs"), route::<Item>(Method::Get, "/git/{id}")),
    ] {
        let overlaps = |pattern: Add, other: Add| Unroutable::Overlaps {
            pattern: pattern.pattern.to_owned(),
            other: other.pattern.to_owned(),
        };

        assert_eq!(build(&[first, second]).err(), Some(overlaps(second, first)), "{}", second.pattern);
        assert_eq!(build(&[second, first]).err(), Some(overlaps(first, second)), "{}", first.pattern);
    }
}

/// Routes that share no path for any one method.
fn apart() -> [Add; 6] {
    [
        route::<()>(Method::Get, "/items/new"),
        route::<Item>(Method::Post, "/items/{id}"),
        route::<Item>(Method::Get, "/items/{id}/edit"),
        route::<()>(Method::Get, "/items/old"),
        route::<()>(Method::Get, "/"),
        route::<Item>(Method::Get, "/{id}"),
    ]
}

#[test]
fn routes_that_share_no_path_for_one_method_are_all_kept() -> Result<(), Unroutable> {
    // Catches an overlap found where there is none: one pattern for two methods, patterns of different lengths, two
    // literals at one depth, and an empty segment against a capture, which takes no empty segment.
    let trie = build(&apart())?;

    assert_eq!(trie.find(Method::Get, "/items/new"), found(&"/items/new", vec![]));
    assert_eq!(trie.find(Method::Post, "/items/new"), found(&"/items/{id}", vec!["new"]));
    assert_eq!(trie.find(Method::Get, "/items/7/edit"), found(&"/items/{id}/edit", vec!["7"]));
    assert_eq!(trie.find(Method::Get, "/"), found(&"/", vec![]));
    assert_eq!(trie.find(Method::Get, "/items"), found(&"/{id}", vec!["items"]));

    Ok(())
}

#[test]
fn the_order_routes_are_given_in_changes_no_answer() -> Result<(), Unroutable> {
    // Catches a walk that stops at the first route it reaches, or `Allow` in the order its routes were given, so the
    // order a router was built in decides a request's answer.
    let routes = apart();
    let reversed: Vec<Add> = routes.iter().rev().copied().collect();
    let probes = [
        (Method::Get, "/items/new"),
        (Method::Post, "/items/new"),
        (Method::Put, "/items/new"),
        (Method::Get, "/items/7/edit"),
        (Method::Get, "/"),
        (Method::Get, "/items"),
        (Method::Delete, "/nothing/here"),
    ];
    let given = build(&routes)?;

    for shift in 0..routes.len() {
        for list in [&routes[..], &reversed[..]] {
            let mut order = list.to_vec();
            order.rotate_left(shift);
            let trie = build(&order)?;

            for (method, path) in probes {
                assert_eq!(trie.find(method, path), given.find(method, path), "{path}");
            }
        }
    }

    Ok(())
}
