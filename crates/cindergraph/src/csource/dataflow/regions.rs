//! Containment queries over source regions, without a scan per event.

use crate::syntax::ids::Span;

/// Sorted starts paired with prefix-maximum ends. Overlapping and nested
/// regions are supported without treating adjacent regions as one region.
pub(super) struct Regions(Vec<Span>);

impl Regions {
    pub(super) fn new(mut spans: Vec<Span>) -> Self {
        spans.sort_unstable_by_key(|span| span.lo);
        let mut hi = 0;
        for span in &mut spans {
            hi = hi.max(span.hi);
            span.hi = hi;
        }
        Self(spans)
    }

    pub(super) fn contains(&self, span: Span) -> bool {
        let index = self.0.partition_point(|outer| outer.lo <= span.lo);
        index > 0 && span.hi <= self.0[index - 1].hi
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexed_containment_matches_linear_reference() {
        let cases = [
            vec![],
            vec![Span { lo: 2, hi: 12 }, Span { lo: 4, hi: 6 }],
            vec![Span { lo: 7, hi: 12 }, Span { lo: 2, hi: 8 }],
            vec![Span { lo: 2, hi: 6 }, Span { lo: 6, hi: 10 }],
            vec![Span { lo: 2, hi: 4 }, Span { lo: 2, hi: 12 }],
        ];
        for original in cases {
            let indexed = Regions::new(original.clone());
            for lo in 0..16 {
                for hi in lo..16 {
                    let query = Span { lo, hi };
                    assert_eq!(
                        indexed.contains(query),
                        original.iter().any(|s| s.lo <= lo && hi <= s.hi),
                        "{original:?}: {query:?}"
                    );
                }
            }
        }
    }
}
