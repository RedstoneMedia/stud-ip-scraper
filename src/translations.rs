use std::collections::HashMap;
use itertools::Itertools;

pub static LOCAL_TO_KEY: once_cell::sync::Lazy<HashMap<&'static str, &'static str>> = once_cell::sync::Lazy::new(|| {
    get_local_to_key_mapping()
});

/// Converts a localized string to a translation key (if possible)
///
/// Ignores case, whitespace at ends and ':' at end
pub fn local_to_key(localized: &str) -> &str {
    let local = localized.trim().trim_end_matches(':').trim().to_lowercase();
    LOCAL_TO_KEY.get(local.as_str()).map_or(localized, |s| *s)
}

fn get_local_to_key_mapping() -> HashMap<&'static str, &'static str> {
    let parsed = parse_translations();
    let mut local_to_key = HashMap::with_capacity(parsed.languages.len() * parsed.translations.len());
    for Translation {key, translations} in parsed.translations {
        for translation in translations {
            local_to_key.insert(translation, key);
        }
    }
    local_to_key
}

#[derive(Debug)]
struct Translation {
    key: &'static str,
    translations: Vec<&'static str>,
}

#[derive(Debug)]
struct ParsedTranslations {
    languages: Vec<&'static str>,
    translations: Vec<Translation>
}

fn parse_translations() -> ParsedTranslations {
    let keys = include_str!("keys.csv");
    let header = keys.lines().next().expect("Expected translation header");
    let mut header_split = header.split(",");
    assert_eq!(header_split.next(), Some("key"), "First column should always be the key");
    let languages = header_split.collect_vec();
    let translations = keys.lines().skip(1).map(|row| {
        let mut split = row.split(',');
        let key = split.next().expect("Expected translation key");
        let translations = split.collect_vec();
        Translation {
            key,
            translations,
        }
    }).collect();
    ParsedTranslations { languages, translations }
}


#[cfg(test)]
mod tests {
    use super::*; // gregory, you are my

    #[test]
    fn test_local_to_key() {
        assert_eq!(local_to_key("untertitel:"), "SUBTITLE");
        assert_eq!(local_to_key("subtitle  :  "), "SUBTITLE");
        assert_eq!(local_to_key("this is a unknown localization"), "this is a unknown localization");
    }
}