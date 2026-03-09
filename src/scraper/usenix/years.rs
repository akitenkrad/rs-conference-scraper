/// Returns the list of available USENIX Security years (2014-2025).
pub fn available_years() -> Vec<u16> {
    (2014..=2025).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_available_years_range() {
        let years = available_years();
        assert_eq!(years.first(), Some(&2014));
        assert_eq!(years.last(), Some(&2025));
        assert_eq!(years.len(), 12);
    }

    #[test]
    fn test_available_years_sorted() {
        let years = available_years();
        let mut sorted = years.clone();
        sorted.sort();
        assert_eq!(years, sorted);
    }
}
