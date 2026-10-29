//! Fuzzy matching for the command palette.

const LETTER: u32 = 1;
const WORD_START: u32 = 8;
const CONSECUTIVE: u32 = 4;
const PREFIX: u32 = 10;

/// Lowercase letter without the accents of Portuguese.
fn fold(c: char) -> impl Iterator<Item = char> {
    c.to_lowercase().map(|c| match c {
        'á' | 'à' | 'â' | 'ã' => 'a',
        'é' | 'ê' => 'e',
        'í' => 'i',
        'ó' | 'ô' | 'õ' => 'o',
        'ú' => 'u',
        'ç' => 'c',
        other => other,
    })
}

fn normalize(text: &str) -> Vec<char> {
    text.chars().flat_map(fold).collect()
}

/// How well `query` matches `candidate`; `None` when it does not match at all.
///
/// It matches when every letter of the query shows up in the candidate, in order, ignoring case
/// and accents. Each letter is worth 1, +8 when it starts a word, +4 when it follows the
/// previous letter directly; +10 when the candidate starts with the whole query. Of all the ways
/// to place the letters, the one with the most points counts. An empty query matches everything
/// with 0.
pub fn score(query: &str, candidate: &str) -> Option<u32> {
    let query = normalize(query);
    let candidate = normalize(candidate);
    if query.is_empty() {
        return Some(0);
    }
    let word_start = |j: usize| j == 0 || !candidate[j - 1].is_alphanumeric();
    // `best[j]`: the most points with the letters seen so far, the last one placed at `j`.
    let mut best: Vec<Option<u32>> = vec![None; candidate.len()];
    for (i, q) in query.iter().enumerate() {
        let mut next: Vec<Option<u32>> = vec![None; candidate.len()];
        // Best score of the previous letter placed strictly before the current position.
        let mut before: Option<u32> = None;
        for (j, c) in candidate.iter().enumerate() {
            if c == q {
                let own = LETTER + if word_start(j) { WORD_START } else { 0 };
                let from_before = if i == 0 { Some(0) } else { before };
                let consecutive = (i > 0 && j > 0)
                    .then(|| best[j - 1].map(|b| b + CONSECUTIVE))
                    .flatten();
                next[j] = from_before
                    .into_iter()
                    .chain(consecutive)
                    .max()
                    .map(|b| b + own);
            }
            // Letters placed at `j` can precede a letter at `j + 1` or later.
            before = before.max(best[j]);
        }
        best = next;
    }
    let total = best.into_iter().flatten().max()?;
    let prefix = if candidate.starts_with(&query) {
        PREFIX
    } else {
        0
    };
    Some(total + prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_basics() {
        assert_eq!(score("stp", "Parar container"), None);
        assert!(score("par", "Parar container") > score("par", "Abrir paleta"));
        assert!(score("rei", "Reiniciar container").is_some());
        assert!(score("acao", "Ação").is_some());
        assert_eq!(score("", "x"), Some(0));
    }

    #[test]
    fn fuzzy_prefers_word_starts() {
        assert!(score("rc", "Reiniciar container") > score("rc", "Abrir console"));
    }

    #[test]
    fn fuzzy_ignores_case_and_accents_in_both_texts() {
        assert!(score("AÇÃO", "acao").is_some());
        assert!(score("õ", "Opções").is_some());
        assert_eq!(score("x", "Ação"), None);
    }

    #[test]
    fn fuzzy_scores_follow_the_rules() {
        // "a" alone: +1, and +8 as the start of a word, +10 as a prefix.
        assert_eq!(score("a", "abc"), Some(1 + 8 + 10));
        // Not a prefix, in the middle of a word: just +1.
        assert_eq!(score("b", "abc"), Some(1));
        // "ab": a(1+8) b(1+4 consecutive), prefix +10.
        assert_eq!(score("ab", "abc"), Some(9 + 5 + 10));
        // Start of the second word.
        assert_eq!(score("c", "ab cd"), Some(1 + 8));
    }

    #[test]
    fn fuzzy_needs_the_letters_in_order() {
        assert_eq!(score("ba", "ab"), None);
        assert_eq!(score("aa", "a"), None);
    }
}
