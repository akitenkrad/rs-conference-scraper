/// CVPR/ICCV の利用可能な年度リストを返す
pub fn available_years(conf: &str) -> Vec<u16> {
    match conf.to_lowercase().as_str() {
        "cvpr" => vec![
            2013, 2014, 2015, 2016, 2017, 2018, 2019, 2020, 2021, 2022, 2023, 2024, 2025,
        ],
        "iccv" => vec![2013, 2015, 2017, 2019, 2021, 2023],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cvpr_years() {
        let years = available_years("cvpr");
        assert_eq!(years.len(), 13);
        assert_eq!(years[0], 2013);
        assert_eq!(*years.last().unwrap(), 2025);
        // All years should be consecutive from 2013 to 2025
        for (i, &year) in years.iter().enumerate() {
            assert_eq!(year, 2013 + i as u16);
        }
    }

    #[test]
    fn test_iccv_years() {
        let years = available_years("iccv");
        assert_eq!(years.len(), 6);
        // ICCV is biennial (odd years only)
        for &year in &years {
            assert!(year % 2 == 1, "ICCV year {} should be odd", year);
        }
        assert_eq!(years[0], 2013);
        assert_eq!(*years.last().unwrap(), 2023);
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
