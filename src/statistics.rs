//! Small integer distribution helpers shared by diagnostics modules.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Distribution {
    pub min: u64,
    pub p50: u64,
    pub p95: u64,
    pub max: u64,
}

pub fn distribution(mut values: Vec<u64>) -> Option<Distribution> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    Some(Distribution {
        min: values[0],
        p50: percentile(&values, 50),
        p95: percentile(&values, 95),
        max: values[values.len() - 1],
    })
}

fn percentile(values: &[u64], percent: usize) -> u64 {
    let rank = (values.len() * percent).div_ceil(100).max(1);
    values[rank - 1]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_rank_distribution_handles_small_samples() {
        assert_eq!(
            distribution(vec![4, 1, 3, 2]),
            Some(Distribution {
                min: 1,
                p50: 2,
                p95: 4,
                max: 4,
            })
        );
        assert_eq!(distribution(Vec::new()), None);
    }
}
