/// CVPR/ICCV の利用可能な年度リストを返す
pub fn available_years(conf: &str) -> Vec<u16> {
    match conf.to_lowercase().as_str() {
        "cvpr" => (2013..=crate::conference::current_year()).collect(),
        // ICCV は奇数年のみ開催
        "iccv" => (2013..=crate::conference::current_year())
            .filter(|y| y % 2 == 1)
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cvpr_years() {
        let years = available_years("cvpr");
        assert_eq!(
            years.len(),
            (crate::conference::current_year() - 2013 + 1) as usize
        );
        assert_eq!(years[0], 2013);
        assert_eq!(*years.last().unwrap(), crate::conference::current_year());
        // All years should be consecutive from 2013 to the current year
        for (i, &year) in years.iter().enumerate() {
            assert_eq!(year, 2013 + i as u16);
        }
    }

    #[test]
    fn test_iccv_years() {
        let years = available_years("iccv");
        assert!(years.contains(&2025));
        // ICCV is biennial (odd years only)
        for &year in &years {
            assert!(year % 2 == 1, "ICCV year {} should be odd", year);
        }
        assert_eq!(years[0], 2013);
        assert!(*years.last().unwrap() >= 2025);
    }

    #[test]
    fn test_unknown_conference() {
        let years = available_years("unknown");
        assert!(years.is_empty());
    }

    #[test]
    fn test_case_insensitive() {
        assert_eq!(available_years("CVPR"), available_years("cvpr"));
        assert_eq!(available_years("ICCV"), available_years("iccv"));
    }
}
