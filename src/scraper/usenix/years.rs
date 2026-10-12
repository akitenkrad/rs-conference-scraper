/// Returns the list of available USENIX Security years (2014-2025).
pub fn available_years() -> Vec<u16> {
    (2014..=crate::conference::current_year()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_available_years_range() {
        let years = available_years();
        assert_eq!(years.first(), Some(&2014));
        assert_eq!(years.last(), Some(&crate::conference::current_year()));
        assert_eq!(
            years.len(),
            (crate::conference::current_year() - 2014 + 1) as usize
        );
    }

    #[test]
    fn test_available_years_sorted() {
        let years = available_years();
        let mut sorted = years.clone();
        sorted.sort();
        assert_eq!(years, sorted);
    }
}
