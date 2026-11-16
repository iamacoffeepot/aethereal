//! What an engine is called in the chat (ADR-0057: The console is the chat): a name made from its id alone, so every
//! engine calls the engine numbered 4 the same, `mellow-snipe:4`, and a person tells engines apart by a name rather
//! than a bare number; an engine calls its own lines `local`. The id is kept in the name, so two engines whose words
//! fall alike are still told apart where an engine is named in output and tools; a transcript's tags show the words
//! alone, [`engine_words`], so the id stays out of the conversation's view.
//!
//! ```text
//! id ──> mixed ──> top 6 bits: an adjective;  the next 6: a noun ──> "adjective-noun" ──> "adjective-noun:id"
//! ```
//!
//! The words are fixed lists, and the mix a fixed function: a name is a pure function of the id, never drawn at random.
//! Changing either list, its order included, or the mix renames every engine, which every engine that shows the chat
//! must then be built with.

#[cfg(test)]
mod tests;

/// The first word of a name: 64 of them, a name's top six bits of its mixed id.
const ADJECTIVES: [&str; 64] = [
    "amber", "ashen", "bold", "brisk", "calm", "clear", "coral", "crisp", "dappled", "dewy", "dusky", "eager", "early",
    "fallow", "fleet", "frosty", "gentle", "gilded", "hazel", "hollow", "idle", "ivory", "jade", "keen", "lively",
    "lunar", "mellow", "misty", "mossy", "nimble", "noble", "ochre", "pale", "patient", "plain", "quick", "quiet",
    "rapid", "russet", "rustic", "sandy", "silent", "silver", "slate", "sleek", "snowy", "solemn", "steady", "still",
    "stormy", "sunny", "tawny", "tidal", "umber", "valiant", "velvet", "vernal", "vivid", "wary", "windy", "wintry",
    "wise", "woven", "young",
];

/// The second word of a name: 64 of them, the next six bits of its mixed id.
const NOUNS: [&str; 64] = [
    "adder", "avocet", "badger", "bittern", "bunting", "crane", "curlew", "dipper", "dunlin", "egret", "ermine",
    "falcon", "finch", "fox", "gannet", "grebe", "hare", "harrier", "hawk", "heron", "ibis", "jay", "kestrel", "kite",
    "lark", "linnet", "lynx", "magpie", "marten", "merlin", "mole", "newt", "osprey", "otter", "owl", "petrel", "pika",
    "plover", "puffin", "quail", "raven", "robin", "rook", "sable", "shrike", "siskin", "skua", "snipe", "sparrow",
    "starling", "stoat", "stork", "swallow", "swan", "teal", "tern", "thrush", "toad", "vole", "wagtail", "weasel",
    "whimbrel", "wigeon", "wren",
];

/// What the id is mixed with before it is hashed, so the engine numbered 0 is not hashed from nothing: the golden
/// ratio's 32 bits.
const SEED: u32 = 0x9e37_79b9;

/// The name output and tools call the engine numbered `engine`: its words and its id, `adjective-noun:engine`.
#[must_use]
pub fn engine_name(engine: u32) -> String {
    format!("{}:{engine}", engine_words(engine))
}

/// The words of the engine numbered `engine`'s name, without its id, `adjective-noun`: what a transcript's tag shows.
#[must_use]
pub fn engine_words(engine: u32) -> String {
    let mixed = mix(engine ^ SEED);
    let adjective = ADJECTIVES[word(mixed >> 26)];
    let noun = NOUNS[word(mixed >> 20)];

    format!("{adjective}-{noun}")
}

/// The place in a list of 64 that the low six bits of `bits` name.
fn word(bits: u32) -> usize {
    usize::try_from(bits & 63).unwrap_or(0)
}

/// `value` hashed so that ids one apart share no word but by chance: a 32-bit integer hash of two multiplies, each
/// after its high bits are folded into its low.
const fn mix(value: u32) -> u32 {
    let folded = value ^ (value >> 16);
    let once = folded.wrapping_mul(0x7feb_352d);
    let refolded = once ^ (once >> 15);
    let twice = refolded.wrapping_mul(0x846c_a68b);

    twice ^ (twice >> 16)
}
