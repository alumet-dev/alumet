use std::collections::HashMap;

/// Computes the Damerau-Levenshtein distance between two strings.
///
/// The Damerau-Levenshtein distance is a measure of the similarity between two strings,
/// which is defined as the minimum number of operations needed to transform one string
/// into the other. The allowed operations are:
///
/// - Insertion of a single character
/// - Deletion of a single character
/// - Substitution of a single character
/// - Transposition of two adjacent characters
///
/// # Arguments
///
/// * `a` - The first string.
/// * `b` - The second string.
///
/// # Returns
///
/// The Damerau-Levenshtein distance between the two strings.
///
/// # Examples
///
/// ```
/// use app_agent::word_distance::distance_with_adjacent_transposition;
/// let distance = distance_with_adjacent_transposition("kitten", "sitting");
/// assert_eq!(distance, 3);
///
/// let distance = distance_with_adjacent_transposition("flaw", "lawn");
/// assert_eq!(distance, 2);
///
/// let distance = distance_with_adjacent_transposition("ca", "abc");
/// assert_eq!(distance, 2);
/// ```
pub fn distance_with_adjacent_transposition(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let a_len = a_chars.len();
    let b_len = b_chars.len();

    let mut da: HashMap<char, usize> = HashMap::new();
    let mut d: Vec<Vec<usize>> = vec![vec![0; b_len + 2]; a_len + 2];

    let max_dist = a_len + b_len;
    d[0][0] = max_dist;

    for i in 0..(a_len + 1) {
        d[i + 1][0] = max_dist;
        d[i + 1][1] = i;
    }
    for j in 0..(b_len + 1) {
        d[0][j + 1] = max_dist;
        d[1][j + 1] = j;
    }

    for i in 1..(a_len + 1) {
        let mut db = 0;
        for j in 1..(b_len + 1) {
            let k = *da.get(&b_chars[j - 1]).unwrap_or(&0);
            let l = db;
            let cost: usize;
            if a_chars[i - 1] == b_chars[j - 1] {
                cost = 0;
                db = j;
            } else {
                cost = 1;
            }
            let operations = [
                d[i][j] + cost,                          // substitution
                d[i + 1][j] + 1,                         // insertion
                d[i][j + 1] + 1,                         // deletion
                d[k][l] + (i - k - 1) + 1 + (j - l - 1), // transposition
            ];
            d[i + 1][j + 1] = *operations.iter().min().unwrap();
        }
        da.insert(a_chars[i - 1], i);
    }
    d[a_len + 1][b_len + 1]
}

#[cfg(test)]
mod tests {
    // cspell: disable
    use super::distance_with_adjacent_transposition;

    #[test]
    fn basic_cases() {
        assert_eq!(0, distance_with_adjacent_transposition("", ""));
        assert_eq!(1, distance_with_adjacent_transposition("", " "));
        assert_eq!(5, distance_with_adjacent_transposition("", "Neron"));
        assert_eq!(5, distance_with_adjacent_transposition("Neron", ""));
        assert_eq!(1, distance_with_adjacent_transposition("Neron", "Necron"));
        assert_eq!(1, distance_with_adjacent_transposition("necron", "neron"));
        assert_eq!(5, distance_with_adjacent_transposition("bread", "butter"));
        assert_eq!(1, distance_with_adjacent_transposition("giggle", "wiggle"));
        assert_eq!(2, distance_with_adjacent_transposition("sparkle", "darkle"));
        assert_eq!(6, distance_with_adjacent_transposition("Amelia", "Pond"));
        assert_eq!(2, distance_with_adjacent_transposition("Song", "Pond"));
        assert_eq!(4, distance_with_adjacent_transposition("Donna", "noble"));
        assert_eq!(7, distance_with_adjacent_transposition("Tweety", "Sylvester"));
        assert_eq!(6, distance_with_adjacent_transposition("abacus", "flower"));
    }

    #[test]
    fn intermediate_cases() {
        assert_eq!(1, distance_with_adjacent_transposition("Toto", "toto"));
        assert_eq!(1, distance_with_adjacent_transposition("Peter", "peter"));
        assert_eq!(1, distance_with_adjacent_transposition("capaldi", "Capaldi"));
        assert_eq!(0, distance_with_adjacent_transposition("tutu", "tutu"));
        assert_eq!(1, distance_with_adjacent_transposition("tuut", "tutu"));
        assert_eq!(1, distance_with_adjacent_transposition("tutu", "tuut"));
        assert_eq!(1, distance_with_adjacent_transposition("tutu", "tuut"));
        assert_eq!(4, distance_with_adjacent_transposition("hello", "olleH"));
    }

    #[test]
    fn complexe_cases() {
        assert_eq!(
            0,
            distance_with_adjacent_transposition(
                "Hello, is it me you're looking for? I can see it in your eyes I can see it in your smile",
                "Hello, is it me you're looking for? I can see it in your eyes I can see it in your smile"
            )
        );
        assert_eq!(0, distance_with_adjacent_transposition("&é'(-è_çà)=", "&é'(-è_çà)="));
        assert_eq!(10, distance_with_adjacent_transposition("&é'(-è_çà)=", "=)àç_è-('é&"));
        assert_eq!(
            0,
            distance_with_adjacent_transposition("Привет, это круто", "Привет, это круто")
        );
        assert_eq!(
            6,
            distance_with_adjacent_transposition("Привет, круто это", "Привет, это круто")
        );
    }
    // cspell: enable
}
