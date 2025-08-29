use std::collections::VecDeque;

pub struct Together<V> {
    series: Vec<VecDeque<V>>,
}

impl<V> Together<V> {
    pub fn new(series: Vec<Vec<V>>) -> Self {
        assert!(!series.is_empty(), "there must be at least one series");
        let l = series[0].len();
        for s in series.iter().skip(1) {
            assert!(s.len() == l, "series should all have the same size")
        }

        Self {
            series: series.into_iter().map(VecDeque::from).collect(),
        }
    }

    pub fn into_iter(self) -> impl Iterator<Item = Vec<V>> {
        let len = self.series.len();
        TogetherIterator { data: self, i: 0, len }
    }
}

struct TogetherIterator<V> {
    data: Together<V>,
    i: usize,
    len: usize,
}

impl<V> Iterator for TogetherIterator<V> {
    type Item = Vec<V>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.i >= self.len {
            return None;
        }

        let mut elements = Vec::with_capacity(self.len);
        for s in &mut self.data.series {
            elements.push(s.pop_front().unwrap());
        }
        self.i += 1;
        Some(elements)
    }
}
