fn sorted(xs: &[f64]) -> Vec<f64> {
    let mut v = xs.to_vec();
    v.sort_by(|a, b| a.total_cmp(b));
    v
}

pub fn median(xs: &[f64]) -> Option<f64> {
    if xs.is_empty() {
        return None;
    }
    let v = sorted(xs);
    let n = v.len();
    Some(if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 })
}

/// Nearest-rank 95th percentile: sorted value at index `ceil(0.95 * n) - 1`.
pub fn p95(xs: &[f64]) -> Option<f64> {
    if xs.is_empty() {
        return None;
    }
    let v = sorted(xs);
    let rank = (0.95 * v.len() as f64).ceil() as usize;
    Some(v[rank.clamp(1, v.len()) - 1])
}
