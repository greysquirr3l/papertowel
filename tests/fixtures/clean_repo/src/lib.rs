// CSV parsing and linear search helpers.

/// Splits a string by a delimiter and returns trimmed, non-empty segments.
pub fn split_and_trim(input: &str, delimiter: char) -> Vec<&str> {
    input
        .split(delimiter)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect()
}

/// Returns the position of the first element satisfying the predicate.
pub fn linear_find<T, F>(items: &[T], pred: F) -> Option<usize>
where
    F: Fn(&T) -> bool,
{
    items.iter().position(|x| pred(x))
}

/// Clamps a value to the closed interval [lo, hi].
pub fn clamp<T: PartialOrd>(val: T, lo: T, hi: T) -> T {
    if val < lo {
        lo
    } else if val > hi {
        hi
    } else {
        val
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_gives_empty_split() {
        assert!(split_and_trim("", ',').is_empty());
    }

    #[test]
    fn whitespace_trimmed_on_split() {
        assert_eq!(split_and_trim("a , b", ','), ["a", "b"]);
    }

    #[test]
    fn empty_segments_dropped() {
        assert_eq!(split_and_trim("a,,b", ','), ["a", "b"]);
    }

    #[test]
    fn linear_find_missing_returns_none() {
        assert_eq!(linear_find(&[1_i32, 2, 3], |x| *x > 10), None);
    }

    #[test]
    fn linear_find_first_match() {
        assert_eq!(linear_find(&[10_i32, 20, 30], |x| *x > 15), Some(1));
    }

    #[test]
    fn clamp_within_range_unchanged() {
        assert_eq!(clamp(5_i32, 1, 10), 5);
    }

    #[test]
    fn clamp_below_lo_snaps_to_lo() {
        assert_eq!(clamp(-1_i32, 0, 10), 0);
    }

    #[test]
    fn clamp_above_hi_snaps_to_hi() {
        assert_eq!(clamp(99_i32, 0, 10), 10);
    }
}
