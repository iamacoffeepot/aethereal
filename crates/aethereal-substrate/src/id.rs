//! The function that computes an actor's id (ADR-0008).

use aethereal_data::{ActorId, ActorPath, Fnv, PathSegment};

/// The id of the root `segment` names.
#[must_use]
pub fn root_id(segment: &PathSegment) -> ActorId {
    ActorId::new(fold(Fnv::new().finish(), segment_hash(segment)))
}

/// The id of the child `segment` names beneath the actor `parent` stands for.
#[must_use]
pub fn child_id(parent: ActorId, segment: &PathSegment) -> ActorId {
    ActorId::new(fold(parent.bits(), segment_hash(segment)))
}

/// The id of the actor `path` names: every segment folded in turn, from the root.
#[must_use]
pub fn path_id(path: &ActorPath) -> ActorId {
    let basis = Fnv::new().finish();

    ActorId::new(path.segments().fold(basis, |above, segment| fold(above, segment_hash(&segment))))
}

/// A segment's hash: FNV-1a over the segment as the path spells it.
fn segment_hash(segment: &PathSegment) -> u64 {
    Fnv::new().bytes(segment.to_string().as_bytes()).finish()
}

/// FNV-1a continued from `above` over the eight bytes of `hash`, least significant first.
fn fold(above: u64, hash: u64) -> u64 {
    Fnv::resume(above).bytes(&hash.to_le_bytes()).finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aethereal_data::KindName;

    const WINDOW: KindName = KindName::new("aether.window");
    const SURFACE: KindName = KindName::new("aether.window.surface");

    #[test]
    fn pinned_id_matches() {
        // Catches a changed fold order, a segment hashed as something other than its text, and a wrong byte order.
        // Tripwire: the id of `aether.window/aether.window.surface:left`, computed from ADR-0008's text by a
        // separate program: each segment's text hashed apart, then folded from the basis, eight bytes little-endian.
        let left = ActorPath::new("aether.window/aether.window.surface:left");
        assert_eq!(path_id(&left).bits(), 0xd07d_f09d_723f_63c2);
    }

    #[test]
    fn child_id_equals_path_id() {
        // Catches the step from a parent's id and the walk from the root disagreeing, with a key and without one.
        let window = PathSegment::new(WINDOW, &());
        for surface in [PathSegment::new(SURFACE, &()), PathSegment::new(SURFACE, &7u64)] {
            let Ok(path) = ActorPath::from_root(&window).child(&surface) else {
                panic!("a second segment fits");
            };
            assert_eq!(child_id(root_id(&window), &surface), path_id(&path));
        }
    }
}
