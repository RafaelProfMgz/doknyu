//! Translation coverage checks: locale parity and no hardcoded UI strings.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Parses the flat `key: "value"` locale files (keys use dots, values are quoted).
fn parse_locale(path: &str) -> BTreeMap<String, String> {
    let content = fs::read_to_string(path).unwrap();
    let mut map = BTreeMap::new();
    for line in content.lines() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') || line.starts_with('_') {
            continue;
        }
        let (key, value) = line
            .split_once(": ")
            .unwrap_or_else(|| panic!("{path}: unparseable line: {line}"));
        let value = value
            .trim()
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .unwrap_or_else(|| panic!("{path}: value of {key} is not quoted"));
        assert!(
            map.insert(key.to_string(), value.to_string()).is_none(),
            "{path}: duplicate key {key}"
        );
    }
    map
}

fn placeholders(value: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = value;
    while let Some(start) = rest.find("%{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else { break };
        found.push(after[..end].to_string());
        rest = &after[end + 1..];
    }
    found.sort();
    found
}

#[test]
fn locales_have_same_keys() {
    let en = parse_locale("locales/en.yml");
    let pt = parse_locale("locales/pt-BR.yml");
    let missing_pt: Vec<_> = en.keys().filter(|k| !pt.contains_key(*k)).collect();
    let missing_en: Vec<_> = pt.keys().filter(|k| !en.contains_key(*k)).collect();
    assert!(missing_pt.is_empty(), "missing in pt-BR: {missing_pt:?}");
    assert!(missing_en.is_empty(), "missing in en: {missing_en:?}");
    for (key, en_value) in &en {
        assert_eq!(
            placeholders(en_value),
            placeholders(&pt[key]),
            "placeholder mismatch for {key}"
        );
    }
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_hardcoded_ui_strings() {
    // Literals allowed without translation (symbols, not words).
    const ALLOWED: &[&str] = &["—", "…"];
    let mut files = Vec::new();
    rust_files(Path::new("src/views"), &mut files);
    rust_files(Path::new("src/ui/components"), &mut files);
    assert!(!files.is_empty());

    let mut offenders = Vec::new();
    for file in files {
        let content = fs::read_to_string(&file).unwrap();
        // Everything from the test module onwards is ignored.
        let end = content
            .find("#[cfg(test)]")
            .or_else(|| content.find("mod tests"))
            .unwrap_or(content.len());
        let code = &content[..end];
        let mut from = 0;
        while let Some(pos) = code[from..].find("text(") {
            let start = from + pos + 5;
            from = start;
            // Allow `text(` followed by whitespace/newlines before the literal.
            let Some(literal) = code[start..].trim_start().strip_prefix('"') else {
                continue;
            };
            let first_is_letter = literal.chars().next().is_some_and(char::is_alphabetic);
            let value = literal.split('"').next().unwrap_or("");
            if first_is_letter && !ALLOWED.contains(&value) {
                let line = code[..start].matches('\n').count() + 1;
                offenders.push(format!("{}:{line}: text(\"{value}\"", file.display()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "hardcoded UI strings:\n{}",
        offenders.join("\n")
    );
}
