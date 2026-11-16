use super::*;

#[test]
fn an_engine_s_name_is_its_id_mixed_into_two_fixed_words() {
    // Tripwire: an engine's name is a pure function of its id, which every engine and the hub compute alike; these
    // values change only when a word list, its order, the seed, or the mix changes, which renames every engine.
    let named: Vec<String> = [0, 1, 4, 37].into_iter().map(engine_name).collect();

    assert_eq!(named, ["amber-newt:0", "silent-grebe:1", "mellow-snipe:4", "rapid-owl:37"]);
}

#[test]
fn every_word_is_one_lowercase_token_and_none_is_listed_twice() {
    // Catches a word with a space or a capital, which would split a name printed as one token, and a word listed
    // twice, which makes two of a list's places one name.
    let words: Vec<&str> = ADJECTIVES.iter().chain(NOUNS.iter()).copied().collect();
    let malformed: Vec<&str> =
        words.iter().copied().filter(|word| !word.bytes().all(|byte| byte.is_ascii_lowercase())).collect();
    let mut sorted = words.clone();
    sorted.sort_unstable();
    sorted.dedup();

    assert!(malformed.is_empty(), "{malformed:?}");
    assert_eq!(sorted.len(), words.len());
}
