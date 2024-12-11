use std::collections::HashMap;

// Damerau-Levenshtein distance
pub fn distance_with_adjacent_transposition(a: String, b: String) -> usize {
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
    use super::distance_with_adjacent_transposition;

    #[test]
    fn basic_cases() {
        assert_eq!(0, distance_with_adjacent_transposition("".to_string(), "".to_string()));
        assert_eq!(1, distance_with_adjacent_transposition("".to_string(), " ".to_string()));
        assert_eq!(
            5,
            distance_with_adjacent_transposition("".to_string(), "Neron".to_string())
        );
        assert_eq!(
            5,
            distance_with_adjacent_transposition("Neron".to_string(), "".to_string())
        );
        assert_eq!(
            1,
            distance_with_adjacent_transposition("Neron".to_string(), "Necron".to_string())
        );
        assert_eq!(
            1,
            distance_with_adjacent_transposition("necron".to_string(), "neron".to_string())
        );
        assert_eq!(
            5,
            distance_with_adjacent_transposition("bread".to_string(), "butter".to_string())
        );
        assert_eq!(
            1,
            distance_with_adjacent_transposition("giggle".to_string(), "wiggle".to_string())
        );
        assert_eq!(
            2,
            distance_with_adjacent_transposition("sparkle".to_string(), "darkle".to_string())
        );
        assert_eq!(
            6,
            distance_with_adjacent_transposition("Amelia".to_string(), "Pond".to_string())
        );
        assert_eq!(
            2,
            distance_with_adjacent_transposition("Song".to_string(), "Pond".to_string())
        );
        assert_eq!(
            4,
            distance_with_adjacent_transposition("Donna".to_string(), "noble".to_string())
        );
        assert_eq!(
            7,
            distance_with_adjacent_transposition("Tweety".to_string(), "Sylvester".to_string())
        );
        assert_eq!(
            6,
            distance_with_adjacent_transposition("abacus".to_string(), "flower".to_string())
        );
    }

    #[test]
    fn intermediate_cases() {
        assert_eq!(
            1,
            distance_with_adjacent_transposition("Toto".to_string(), "toto".to_string())
        );
        assert_eq!(
            1,
            distance_with_adjacent_transposition("Peter".to_string(), "peter".to_string())
        );
        assert_eq!(
            1,
            distance_with_adjacent_transposition("capaldi".to_string(), "Capaldi".to_string())
        );
        assert_eq!(
            0,
            distance_with_adjacent_transposition("tutu".to_string(), "tutu".to_string())
        );
        assert_eq!(
            1,
            distance_with_adjacent_transposition("tuut".to_string(), "tutu".to_string())
        );
        assert_eq!(
            1,
            distance_with_adjacent_transposition("tutu".to_string(), "tuut".to_string())
        );
        assert_eq!(
            1,
            distance_with_adjacent_transposition("tutu".to_string(), "tuut".to_string())
        );
        assert_eq!(
            4,
            distance_with_adjacent_transposition("hello".to_string(), "olleH".to_string())
        );
    }

    #[test]
    fn complexe_cases() {
        assert_eq!(
            0,
            distance_with_adjacent_transposition(
                "Hello, is it me you're looking for? I can see it in your eyes I can see it in your smile".to_string(),
                "Hello, is it me you're looking for? I can see it in your eyes I can see it in your smile".to_string()
            )
        );
        assert_eq!(
            0,
            distance_with_adjacent_transposition("&é'(-è_çà)=".to_string(), "&é'(-è_çà)=".to_string())
        );
        assert_eq!(
            10,
            distance_with_adjacent_transposition("&é'(-è_çà)=".to_string(), "=)àç_è-('é&".to_string())
        );
        assert_eq!(
            0,
            distance_with_adjacent_transposition("Привет, это круто".to_string(), "Привет, это круто".to_string())
        );
        assert_eq!(
            6,
            distance_with_adjacent_transposition("Привет, круто это".to_string(), "Привет, это круто".to_string())
        );
    }
}
