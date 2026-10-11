/// AAMAS proceedings available on ifaamas.org (forms/contents.htm format)
/// Years 2013-2024 use a consistent HTML table-of-contents layout.
pub fn available_years() -> Vec<u16> {
    (2013..=crate::conference::current_year()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_available_years_range() {
        let years = available_years();
        assert_eq!(years.first(), Some(&2013));
        assert_eq!(years.last(), Some(&crate::conference::current_year()));
        assert_eq!(years.len(), (crate::conference::current_year() - 2013 + 1) as usize);
    }

    #[test]
    fn test_available_years_sorted() {
        let years = available_years();
        let mut sorted = years.clone();
        sorted.sort();
        assert_eq!(years, sorted);
    }
}
