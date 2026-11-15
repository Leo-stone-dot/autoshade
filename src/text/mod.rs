//! Text resources — the Android/iOS resource model for the app's strings.
//!
//! Every user-facing string is a RESOURCE: a stable dotted key (like
//! `R.string.develop_exposure` / an `NSLocalizedString` key) whose text lives
//! in one TOML file per locale (`resources/strings/en.toml`,
//! `resources/strings/zh-Hans.toml`). `scripts/gen_text.py` compiles those
//! files into [`generated`]: one `Key` constant per resource, the locale
//! tables, and the placeholder metadata `scripts/check_text.py` gates on.
//!
//! A [`Key`] is either a resource row (the normal case) or a RAW literal such
//! as an aspect ratio, a number or a glyph the catalogue deliberately does not
//! translate. `tr` answers a raw literal unchanged in every locale — the
//! behaviour the old English-fallback catalogue had, made explicit.
//!
//! The GUI, the local web page and the CLI all resolve through this one
//! module, so a string cannot have two spellings and a locale cannot drift
//! from its keys silently. The English text is NOT the key: editing copy
//! edits a resource, not the identity of a translation.

pub mod generated;

pub use generated::Key;

/// The UI language. `En` is the default and the skeleton locale (every
/// resource has an `en` row); a missing translation falls back to it at
/// runtime, though [`scripts/check_text.py`] forbids one from shipping.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Lang {
    #[default]
    En,
    Zh,
}

impl Lang {
    /// The locale's BCP-47-ish tag, as spelled by the resource file names.
    pub fn tag(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Zh => "zh-Hans",
        }
    }

    /// Native display name for the language picker (never translated).
    pub fn label(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Zh => "中文",
        }
    }

    fn table(self) -> &'static [&'static str] {
        match self {
            Lang::En => &generated::EN,
            Lang::Zh => &generated::ZH_HANS,
        }
    }
}

/// Every locale this build carries.
pub const LANGS: [Lang; 2] = [Lang::En, Lang::Zh];

/// The truncated-note poison pill: it names no resource and renders to text no
/// rationale string contains, so a truncated note vector can never strip-match
/// the persisted prose.
pub const TRUNCATED: Key = Key::Raw("\u{0}notes-truncated");

/// The text of one resource. A raw literal answers itself in every locale.
/// Falls back to the default locale when a locale row is empty (the gate
/// forbids it; the fallback keeps a broken build readable rather than blank).
pub fn tr(lang: Lang, key: impl Into<Key>) -> &'static str {
    match key.into() {
        Key::Raw(text) => text,
        Key::Resource(index) => {
            let text = lang.table()[index as usize];
            if text.is_empty() && lang != Lang::En {
                generated::EN[index as usize]
            } else {
                text
            }
        }
    }
}

/// The default locale's text of a key — for the surfaces that inspect a key's
/// spelling (the leading-separator convention, tests pinning wording). UI
/// rendering goes through [`tr`] with the session locale instead.
pub fn english(key: Key) -> &'static str {
    match key {
        Key::Raw(text) => text,
        Key::Resource(index) => generated::EN[index as usize],
    }
}

/// Look a resource up by its DEFAULT text. This is a migration scaffold for
/// the dynamic label sources (`kind.label()`, enum label tables) that have not
/// been moved to a `Key` yet; a moved table calls [`tr`] with the constant.
/// A text with no row answers a raw key, so the old fallback behaviour holds.
pub fn key_of_text(text: &'static str) -> Option<Key> {
    static BY_TEXT: std::sync::OnceLock<std::collections::HashMap<&'static str, Key>> =
        std::sync::OnceLock::new();
    BY_TEXT
        .get_or_init(|| {
            generated::EN
                .iter()
                .enumerate()
                .map(|(i, text)| (*text, Key::Resource(i as u16)))
                .collect()
        })
        .get(text)
        .copied()
}

impl From<&'static str> for Key {
    fn from(text: &'static str) -> Self {
        key_of_text(text).unwrap_or(Key::Raw(text))
    }
}

/// The same bridge for the label tables iterated by reference (`&&'static str`
/// from a `map`/`for` over `&[(&str, &str)]`).
impl From<&&'static str> for Key {
    fn from(text: &&'static str) -> Self {
        (*text).into()
    }
}

/// Text identity comparisons, for the migration's test surfaces: a key names
/// the same text as its default spelling. Production code compares `Key`s.
impl PartialEq<&str> for Key {
    fn eq(&self, other: &&str) -> bool {
        english(*self) == *other
    }
}

impl PartialEq<Key> for &str {
    fn eq(&self, other: &Key) -> bool {
        *self == english(*other)
    }
}

/// Migration bridge: tests (and the separator convention) read a key's default
/// text through `str` methods. Production UI text goes through [`tr`], so this
/// only ever answers the English skeleton; `scripts/check_text.py` reports the
/// call sites that still use it.
impl std::ops::Deref for Key {
    type Target = str;

    fn deref(&self) -> &str {
        english(*self)
    }
}

impl std::fmt::Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(english(*self))
    }
}

/// The resource's key name (diagnostics, audits, the test that pins the table).
pub fn name(key: Key) -> &'static str {
    match key {
        Key::Raw(text) => text,
        Key::Resource(index) => generated::NAMES[index as usize],
    }
}

/// The placeholders the resource's default text declares, sorted. A raw
/// literal declares none (it is not interpolated through the catalogue).
pub fn placeholders(key: Key) -> &'static [&'static str] {
    match key {
        Key::Raw(_) => &[],
        Key::Resource(index) => generated::PLACEHOLDERS[index as usize],
    }
}

/// How many resource rows the generated tables carry.
pub fn key_count() -> usize {
    generated::NAMES.len()
}

/// The `rationale::values::ALL` entry at `value`, translated. The enumerated
/// engine values are arguments to rationale templates, and this is the one
/// place an argument VALUE can be a resource too.
pub fn value_text(lang: Lang, value: &str) -> &str {
    match crate::rationale::values::ALL.iter().position(|v| *v == value) {
        Some(index) => tr(lang, generated::VALUE_KEYS[index]),
        None => value,
    }
}

/// Render a resource with named `{placeholder}` arguments.
///
/// One pass over the TEMPLATE, never over inserted values: a value that
/// contains placeholder syntax (a directory literally named `{count}`) must
/// not be reinterpreted as markup. Arguments whose value is an enumerated
/// engine value are translated through [`value_text`], so a rationale sentence
/// never carries an English fragment inside a translated sentence.
pub fn trf(lang: Lang, key: impl Into<Key>, args: &[(&str, &str)]) -> String {
    let key = key.into();
    let template = tr(lang, key);
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open..];
        match after.find('}') {
            Some(close) => {
                let field = &after[1..close];
                match args.iter().find(|(n, _)| *n == field) {
                    Some((_, value)) => out.push_str(value_text(lang, value)),
                    None => out.push_str(&after[..=close]),
                }
                rest = &after[close + 1..];
            }
            None => {
                out.push_str(after);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_resolves_in_both_locales() {
        for i in 0..key_count() {
            let key = Key::Resource(i as u16);
            assert!(!name(key).is_empty(), "key {i} has no name");
            assert!(!tr(Lang::En, key).is_empty(), "{} has no English", name(key));
            assert!(!tr(Lang::Zh, key).is_empty(), "{} has no Chinese", name(key));
        }
    }

    #[test]
    fn the_placeholder_metadata_is_the_default_texts_own() {
        for i in 0..key_count() {
            let key = Key::Resource(i as u16);
            let mut declared: Vec<&str> = placeholders(key).to_vec();
            declared.sort_unstable();
            let mut expected: Vec<&str> = Vec::new();
            let text = tr(Lang::En, key);
            let mut rest = text;
            while let Some(open) = rest.find('{') {
                let after = &rest[open..];
                let Some(close) = after.find('}') else { break };
                expected.push(&after[1..close]);
                rest = &after[close + 1..];
            }
            expected.sort_unstable();
            expected.dedup();
            assert_eq!(declared, expected, "{} placeholder metadata drifted", name(key));
        }
    }

    #[test]
    fn a_formatted_resource_and_its_arguments_resolve() {
        let key = (0..key_count())
            .map(|i| Key::Resource(i as u16))
            .find(|k| !placeholders(*k).is_empty())
            .expect("the catalogue has a formatted resource");
        let arg = placeholders(key)[0];
        let rendered = trf(Lang::En, key, &[(arg, "3")]);
        assert!(rendered.contains('3'), "{rendered}");
        let zh = trf(Lang::Zh, key, &[(arg, "3")]);
        assert!(zh.contains('3'), "{zh}");
    }

    #[test]
    fn a_raw_literal_answers_itself_in_every_locale() {
        let key: Key = "1:1".into();
        assert_eq!(key, Key::Raw("1:1"));
        assert_eq!(tr(Lang::En, key), "1:1");
        assert_eq!(tr(Lang::Zh, key), "1:1");
        assert_eq!(english(key), "1:1");
    }

    #[test]
    fn an_enumerated_argument_value_is_translated() {
        let value = crate::rationale::values::ALL[0];
        assert_eq!(value_text(Lang::En, value), value);
        assert_ne!(value_text(Lang::Zh, value), value, "the value table is not translated");
        assert_eq!(value_text(Lang::En, "not-an-enum-value"), "not-an-enum-value");
    }

    #[test]
    fn every_enumerated_value_has_a_resource() {
        assert_eq!(generated::VALUE_KEYS.len(), crate::rationale::values::ALL.len());
        for (i, value) in crate::rationale::values::ALL.iter().enumerate() {
            assert_eq!(tr(Lang::En, generated::VALUE_KEYS[i]), *value);
        }
    }

    /// A value that LOOKS like placeholder syntax must not be reinterpreted by
    /// the next argument's substitution (the old sequential replace did).
    #[test]
    fn a_value_containing_placeholder_syntax_is_not_reinterpreted() {
        let key = Key::Raw("{dir} · {count} photos");
        let s = trf(Lang::En, key, &[("dir", "packs/{count}"), ("count", "3")]);
        assert_eq!(s, "packs/{count} · 3 photos");
    }

    /// A template placeholder with no argument (or one the translation
    /// dropped) stays visible, and a lone `{` is text.
    #[test]
    fn a_dropped_or_unknown_placeholder_stays_visible() {
        let s = trf(Lang::En, Key::Raw("saved → {path}"), &[]);
        assert_eq!(s, "saved → {path}");
        let s = trf(Lang::En, Key::Raw("brace {"), &[]);
        assert_eq!(s, "brace {");
    }

    /// The same placeholder repeated expands at every position.
    #[test]
    fn repeated_placeholders_all_expand() {
        let s = trf(Lang::En, Key::Raw("{n} of {n}"), &[("n", "2")]);
        assert_eq!(s, "2 of 2");
    }

    /// The sentinel names no resource and renders to text no rationale string
    /// contains (the truncated-vec poison pill).
    #[test]
    fn the_truncated_sentinel_is_not_a_resource() {
        assert!(matches!(TRUNCATED, Key::Raw(_)));
        assert!(tr(Lang::En, TRUNCATED).starts_with('\u{0}'));
        assert!(tr(Lang::Zh, TRUNCATED).starts_with('\u{0}'));
    }
}
