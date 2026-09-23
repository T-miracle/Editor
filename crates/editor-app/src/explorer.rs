//! How the explorer orders the entries of a directory.
//!
//! Three rules, in order:
//!
//! 1. Directories before files.
//! 2. Alphabetical, ignoring capitalisation, so `README.md` files and
//!    `Cargo.toml` files do not get split apart.
//! 3. Chinese names sort by pronunciation, not by codepoint — `北京` goes with
//!    the `b`s even though its first codepoint is far past the Latin range.
//!    Everything else compares by codepoint, which is already what ASCII and
//!    the accented Latin letters want.

use pinyin::ToPinyin;
use std::cmp::Ordering;

/// How many leading alphabetic characters of each name decide rule 2.
///
/// `Cargo.toml` and `Cargo.lock` first differ after this many, and comparing
/// them whole is what would put `lock` before `toml`. The bound is what keeps
/// the key a fixed length instead of a transformed copy of every file name.
const ALPHABETICAL_KEY_DEPTH: usize = 4;

/// Separates a Chinese syllable from whatever follows it in the collation key.
///
/// `\u{1}` is below every printable character, so `张伟` keys as
/// `zhang\u{1}wei\u{1}` and a syllable cannot run into its neighbour. It is
/// dropped again by [`without_boundaries`] before any comparison: the separator
/// exists to keep the syllables apart, and comparing it would sort `b.rs` after
/// `北京.rs` purely because the marker is below `e`.
const SYLLABLE_BOUNDARY: char = '\u{1}';

/// A directory entry's position among its siblings.
///
/// The rule, in the order the fields are compared:
///
/// 1. `is_file` — `false` sorts first, putting directories above files.
/// 2. `alphabetical` — the collation key, so Chinese sorts by reading.
/// 3. `name` — the name itself, which separates names the collation cannot tell
///    apart and keeps this a total order over distinct names.
///
/// The third field has to stay, and stay last: toneless pinyin makes homophones
/// identical — `北京` and `背景` both collate to `beijing` — and without a final
/// tiebreak on the name, two such siblings would compare equal and a map keyed
/// by this type would silently drop one of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortKey {
    /// `false` sorts first, putting directories above files.
    is_file: bool,
    /// The collation key: lowercased, with Chinese runs replaced by their pinyin.
    alphabetical: String,
    /// The name as written, for the final tiebreak.
    name: String,
}

impl SortKey {
    pub fn new(name: &str, is_file: bool) -> Self {
        Self {
            is_file,
            alphabetical: collation_key(name),
            name: name.to_string(),
        }
    }
}

impl Ord for SortKey {
    fn cmp(&self, other: &Self) -> Ordering {
        // Syllable boundaries are separators, not characters. Comparing them
        // would order `b.rs` and `北京.rs` by the marker instead of by "b", so
        // the keys are reduced to what they actually read as first.
        let this = without_boundaries(&self.alphabetical);
        let other_key = without_boundaries(&other.alphabetical);

        self.is_file
            .cmp(&other.is_file)
            .then_with(|| alphabetical_prefix(&this).cmp(&alphabetical_prefix(&other_key)))
            .then_with(|| this.cmp(&other_key))
            .then_with(|| self.name.cmp(&other.name))
    }
}

impl PartialOrd for SortKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Strips the syllable separators that only exist to build the key.
fn without_boundaries(key: &str) -> String {
    key.chars()
        .filter(|character| *character != SYLLABLE_BOUNDARY)
        .collect()
}

/// Orders `name` against `other` at the same level of the tree.
///
/// Returns [`Ordering::Equal`] only for the same name, so this is a total order
/// over distinct names. The tree sorts by inserting into a map keyed by
/// [`SortKey`], so the pairwise form is only needed by tests.
#[cfg(test)]
pub fn compare(name: &str, is_file: bool, other: &str, other_is_file: bool) -> Ordering {
    SortKey::new(name, is_file).cmp(&SortKey::new(other, other_is_file))
}

/// Builds the case-folded, pinyin-substituted form of `name` that decides rule 2.
///
/// Names that are entirely ASCII take a cheap path with no collation work.
fn collation_key(name: &str) -> String {
    let mut key = String::with_capacity(name.len());

    for character in name.chars() {
        if character.is_ascii() {
            key.extend(character.to_lowercase());
            continue;
        }

        match character.to_pinyin() {
            Some(pinyin) => {
                key.push_str(pinyin.plain());
                key.push(SYLLABLE_BOUNDARY);
            }
            // Not Chinese (a non-ASCII Latin letter, punctuation, an emoji), or
            // Chinese with no reading in this table: fold its case and let it
            // compare by codepoint.
            None => key.extend(character.to_lowercase()),
        }
    }

    key
}

/// Reduces a collation key to the leading alphabetic run that decides rule 2.
///
/// Stops at the first digit or punctuation, so `Cargo.lock` and `Cargo.toml`
/// share the prefix `carg` and are then separated by their full collation key.
/// Letting the extension into the prefix would instead sort them as `cargol`
/// against `cargot`, which is a different rule.
fn alphabetical_prefix(key: &str) -> String {
    key.chars()
        .take_while(|character| character.is_alphabetic())
        .take(ALPHABETICAL_KEY_DEPTH)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Orders a directory listing the way the tree does: insert every entry into
    /// a map keyed by its [`SortKey`] and read it back.
    fn order(entries: &[(&str, bool)]) -> Vec<String> {
        let mut ordered = std::collections::BTreeMap::new();
        for (name, is_file) in entries {
            ordered.insert(SortKey::new(name, *is_file), (*name).to_string());
        }

        ordered.into_values().collect()
    }

    #[test]
    fn directories_come_before_files_whatever_their_names() {
        let ordered = order(&[
            ("zebra.txt", true),
            ("alpha.txt", true),
            ("zzz-folder", false),
            ("aaa-folder", false),
        ]);

        assert_eq!(
            ordered,
            ["aaa-folder", "zzz-folder", "alpha.txt", "zebra.txt"]
        );
    }

    #[test]
    fn names_are_ordered_ignoring_capitalisation() {
        let ordered = order(&[("apple.rs", true), ("Zebra.rs", true), ("Banana.rs", true)]);

        assert_eq!(ordered, ["apple.rs", "Banana.rs", "Zebra.rs"]);
    }

    #[test]
    fn chinese_names_are_ordered_by_pronunciation() {
        // bei, shang, zhong — codepoint order would put 上 (U+4E0A) first.
        let ordered = order(&[("中国.rs", true), ("北京.rs", true), ("上海.rs", true)]);

        assert_eq!(ordered, ["北京.rs", "上海.rs", "中国.rs"]);
    }

    #[test]
    fn chinese_and_latin_names_interleave_by_reading() {
        let ordered = order(&[("zebra.rs", true), ("北京.rs", true), ("apple.rs", true)]);

        assert_eq!(ordered, ["apple.rs", "北京.rs", "zebra.rs"]);
    }

    /// Siblings usually differ by what comes after the dot, so the extension has
    /// to be part of the decision.
    #[test]
    fn equal_alpha_prefixes_fall_through_to_the_whole_name() {
        let ordered = order(&[("Cargo.toml", true), ("Cargo.lock", true)]);

        assert_eq!(ordered, ["Cargo.lock", "Cargo.toml"]);
    }

    #[test]
    fn an_entry_never_compares_equal_to_a_differently_named_one() {
        assert_eq!(compare("a.txt", true, "a.txt", true), Ordering::Equal);
        assert_ne!(compare("a.txt", true, "a.md", true), Ordering::Equal);
        assert_ne!(compare("a", true, "a", false), Ordering::Equal);
        assert_ne!(compare("北京", true, "上海", true), Ordering::Equal);
    }

    /// Toneless pinyin cannot separate homophones: `北京` and `背景` both read
    /// `beijing`. They must still be distinct keys, or a map keyed by the sort
    /// key would keep only one of them.
    #[test]
    fn homophones_stay_distinct_entries() {
        assert_eq!(collation_key("北京"), collation_key("背景"));
        assert_ne!(compare("北京", true, "背景", true), Ordering::Equal);

        let ordered = order(&[("背景.rs", true), ("北京.rs", true)]);
        assert_eq!(
            ordered.len(),
            2,
            "a homophone pair collapsed into one entry"
        );
    }

    #[test]
    fn collation_keys_fold_case_and_keep_non_chinese_intact() {
        assert_eq!(collation_key("README.md"), "readme.md");
        assert_eq!(collation_key("北京"), "bei\u{1}jing\u{1}");
        // An accented Latin letter has no pinyin and must survive as itself.
        assert_eq!(collation_key("Café.md"), "café.md");
    }
}
