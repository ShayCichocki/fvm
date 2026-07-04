use crate::artifact::{
    BenchmarkIteration, BenchmarkReport, BenchmarkSummary, HostBenchmarkInfo, now_unix_seconds,
};

pub fn build_report(
    name: impl Into<String>,
    iterations: Vec<BenchmarkIteration>,
    host: HostBenchmarkInfo,
) -> BenchmarkReport {
    let summary = summarize(&iterations);
    BenchmarkReport {
        name: name.into(),
        created_unix_seconds: now_unix_seconds(),
        iterations,
        summary,
        host,
    }
}

pub fn summarize(iterations: &[BenchmarkIteration]) -> BenchmarkSummary {
    let mut boot_times: Vec<u128> = iterations
        .iter()
        .filter_map(|iteration| iteration.boot_to_listen_ms)
        .collect();
    boot_times.sort_unstable();

    BenchmarkSummary {
        boot_to_listen_ms_median: percentile(&boot_times, 50.0),
        boot_to_listen_ms_p90: percentile(&boot_times, 90.0),
        boot_to_listen_ms_p99: percentile(&boot_times, 99.0),
        host_rss_kib_max: iterations
            .iter()
            .filter_map(|iteration| iteration.host_rss_kib)
            .max(),
        guest_rss_kib_max: iterations
            .iter()
            .filter_map(|iteration| iteration.guest_rss_kib)
            .max(),
    }
}

fn percentile(sorted_values: &[u128], percentile: f64) -> Option<u128> {
    if sorted_values.is_empty() {
        return None;
    }
    let rank = (percentile / 100.0) * ((sorted_values.len() - 1) as f64);
    let index = rank.ceil() as usize;
    sorted_values.get(index).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarizes_iterations() {
        let summary = summarize(&[
            BenchmarkIteration {
                boot_to_listen_ms: Some(10),
                host_rss_kib: Some(100),
                guest_rss_kib: Some(50),
            },
            BenchmarkIteration {
                boot_to_listen_ms: Some(20),
                host_rss_kib: Some(200),
                guest_rss_kib: Some(70),
            },
            BenchmarkIteration {
                boot_to_listen_ms: Some(30),
                host_rss_kib: Some(150),
                guest_rss_kib: Some(60),
            },
        ]);

        assert_eq!(summary.boot_to_listen_ms_median, Some(20));
        assert_eq!(summary.boot_to_listen_ms_p90, Some(30));
        assert_eq!(summary.host_rss_kib_max, Some(200));
        assert_eq!(summary.guest_rss_kib_max, Some(70));
    }
}
