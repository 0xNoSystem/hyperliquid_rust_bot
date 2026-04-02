use super::types::{EquityPoint, PositionSnapshot};

/// Largest-Triangle-Three-Buckets downsampling for equity curves.
/// Preserves visual fidelity (peaks, valleys, inflections) far better than
/// naive every-Nth sampling.
pub fn lttb_equity(points: &[EquityPoint], target: usize) -> Vec<EquityPoint> {
    let n = points.len();
    if n <= target || target < 3 {
        return points.to_vec();
    }

    let mut out = Vec::with_capacity(target);
    out.push(points[0]); // always keep first

    let bucket_size = (n - 2) as f64 / (target - 2) as f64;

    let mut prev_idx: usize = 0;

    for i in 0..(target - 2) {
        let bucket_start = ((i as f64 * bucket_size) + 1.0) as usize;
        let bucket_end = (((i + 1) as f64 * bucket_size) + 1.0).min(n as f64 - 1.0) as usize;

        // Average of *next* bucket (look-ahead)
        let next_start = (((i + 1) as f64 * bucket_size) + 1.0) as usize;
        let next_end = (((i + 2) as f64 * bucket_size) + 1.0).min(n as f64 - 1.0) as usize;
        let next_count = (next_end - next_start + 1) as f64;
        let (avg_ts, avg_eq) = points[next_start..=next_end]
            .iter()
            .fold((0.0_f64, 0.0_f64), |(at, ae), p| {
                (at + p.ts as f64, ae + p.equity)
            });
        let avg_ts = avg_ts / next_count;
        let avg_eq = avg_eq / next_count;

        // Pick the point in current bucket that forms the largest triangle
        let prev = &points[prev_idx];
        let mut best_area = -1.0_f64;
        let mut best_idx = bucket_start;

        for (j, p) in points
            .iter()
            .enumerate()
            .take(bucket_end + 1)
            .skip(bucket_start)
        {
            let area = ((prev.ts as f64 - avg_ts) * (p.equity - prev.equity)
                - (prev.ts as f64 - p.ts as f64) * (avg_eq - prev.equity))
                .abs();
            if area > best_area {
                best_area = area;
                best_idx = j;
            }
        }

        out.push(points[best_idx]);
        prev_idx = best_idx;
    }

    out.push(points[n - 1]); // always keep last
    out
}

/// Cap snapshots to `max` by keeping first half and last half with even
/// sampling in between.
pub fn cap_snapshots(snapshots: &[PositionSnapshot], max: usize) -> Vec<PositionSnapshot> {
    let n = snapshots.len();
    if n <= max || max < 4 {
        return snapshots.to_vec();
    }

    let keep_ends = max / 4; // 25% from each end
    let middle_budget = max - (keep_ends * 2);

    let mut out = Vec::with_capacity(max);

    // First N
    out.extend_from_slice(&snapshots[..keep_ends]);

    // Evenly sampled middle
    let middle = &snapshots[keep_ends..n - keep_ends];
    let step = middle.len() as f64 / middle_budget as f64;
    for i in 0..middle_budget {
        let idx = (i as f64 * step) as usize;
        out.push(middle[idx].clone());
    }

    // Last N
    out.extend_from_slice(&snapshots[n - keep_ends..]);

    out
}
