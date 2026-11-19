//! The room a list or a table gives back once it is emptied, past what it keeps for what fills it next.

use std::collections::HashMap;
use std::hash::{BuildHasher, Hash};

/// Empties nothing, and gives back what room `list`, an empty list, has past `room`.
pub fn trim<T>(list: &mut Vec<T>, room: usize) {
    let is_spare = list.is_empty() && list.capacity() > room;
    if is_spare {
        list.shrink_to(room);
    }
}

/// Empties nothing, and gives back what room `table`, an empty table, has past `room`.
///
/// A table keeps whole buckets, so one given back keeps the fewest that hold `room`, which is a little more room than
/// `room`: it is trimmed again each time it empties, and that frees nothing and allocates nothing.
pub fn trim_table<K: Eq + Hash, V, S: BuildHasher>(table: &mut HashMap<K, V, S>, room: usize) {
    let is_spare = table.is_empty() && table.capacity() > room;
    if is_spare {
        table.shrink_to(room);
    }
}
