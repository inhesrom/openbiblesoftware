//! Block heights for one width and set of text settings: estimates until a block is laid out,
//! then real. Offsets come from a prefix sum rebuilt lazily.

#[derive(Debug, Clone)]
pub struct HeightIndex {
    heights: Vec<f32>,
    measured: Vec<bool>,
    prefix: Vec<f64>,
    stale_from: usize,
}

impl HeightIndex {
    pub fn new(estimates: Vec<f32>) -> Self {
        let n = estimates.len();
        Self {
            heights: estimates,
            measured: vec![false; n],
            prefix: vec![0.0; n + 1],
            stale_from: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.heights.len()
    }

    pub fn is_empty(&self) -> bool {
        self.heights.is_empty()
    }

    pub fn height(&self, i: usize) -> f32 {
        self.heights.get(i).copied().unwrap_or(0.0)
    }

    pub fn is_measured(&self, i: usize) -> bool {
        self.measured.get(i).copied().unwrap_or(false)
    }

    /// Record a block's real height.
    pub fn set(&mut self, i: usize, h: f32) {
        if let Some(slot) = self.heights.get_mut(i) {
            if *slot != h {
                *slot = h;
                self.stale_from = self.stale_from.min(i);
            }
            self.measured[i] = true;
        }
    }

    fn refresh(&mut self) {
        let n = self.heights.len();
        if self.stale_from > n {
            return;
        }
        for i in self.stale_from..n {
            self.prefix[i + 1] = self.prefix[i] + f64::from(self.heights[i]);
        }
        self.stale_from = n + 1;
    }

    /// Distance from the document top to block `i`'s top.
    pub fn offset(&mut self, i: usize) -> f64 {
        self.refresh();
        self.prefix[i.min(self.heights.len())]
    }

    pub fn total(&mut self) -> f64 {
        self.offset(self.heights.len())
    }

    /// The block at document position `y`, and the offset into it.
    pub fn at(&mut self, y: f64) -> (usize, f32) {
        self.refresh();
        if self.heights.is_empty() {
            return (0, 0.0);
        }
        let i = self.prefix.partition_point(|&p| p <= y).saturating_sub(1);
        let i = i.min(self.heights.len() - 1);
        (i, (y - self.prefix[i]).max(0.0) as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_follow_measurements() {
        let mut h = HeightIndex::new(vec![10.0, 20.0, 30.0]);
        assert_eq!(h.total(), 60.0);
        assert_eq!(h.at(25.0), (1, 15.0));
        h.set(0, 15.0);
        assert!(h.is_measured(0) && !h.is_measured(1));
        assert_eq!(h.offset(2), 35.0);
        assert_eq!(h.at(1000.0).0, 2);
        assert_eq!(h.at(-5.0), (0, 0.0));
    }
}
