// SPDX-License-Identifier: MIT OR Apache-2.0

//! Bounding-volume hierarchy over face boxes (design D03 §4), median split
//! on the longest axis.

use fluut_geom::Aabb3;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

#[derive(Debug, Clone)]
enum Node {
    Leaf { bounds: Aabb3, items: Vec<u32> },
    Inner { bounds: Aabb3, kids: Box<[Node; 2]> },
}

impl Node {
    fn bounds(&self) -> &Aabb3 {
        match self {
            Node::Leaf { bounds, .. } | Node::Inner { bounds, .. } => bounds,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Bvh {
    root: Option<Node>,
    boxes: Vec<Aabb3>,
}

/// A node or leaf item waiting in the best-first queue, ordered by bound,
/// then by sequence number so that the order is deterministic.
struct Pending<'a> {
    bound: f64,
    seq: u64,
    what: Waiting<'a>,
}

enum Waiting<'a> {
    Node(&'a Node),
    Item(u32),
}

impl PartialEq for Pending<'_> {
    fn eq(&self, o: &Self) -> bool {
        self.cmp(o) == Ordering::Equal
    }
}
impl Eq for Pending<'_> {}
impl PartialOrd for Pending<'_> {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Pending<'_> {
    fn cmp(&self, o: &Self) -> Ordering {
        self.bound
            .total_cmp(&o.bound)
            .then_with(|| o.seq.cmp(&self.seq))
    }
}

/// Items per leaf.
const LEAF: usize = 4;

fn overlaps(a: &Aabb3, b: &Aabb3) -> bool {
    a.min.x() <= b.max.x()
        && b.min.x() <= a.max.x()
        && a.min.y() <= b.max.y()
        && b.min.y() <= a.max.y()
        && a.min.z() <= b.max.z()
        && b.min.z() <= a.max.z()
}

fn centre(b: &Aabb3, axis: usize) -> f64 {
    let (lo, hi) = match axis {
        0 => (b.min.x(), b.max.x()),
        1 => (b.min.y(), b.max.y()),
        _ => (b.min.z(), b.max.z()),
    };
    0.5 * (lo + hi)
}

fn build(boxes: &[Aabb3], mut items: Vec<u32>) -> Node {
    let bounds = items
        .iter()
        .fold(Aabb3::empty(), |a, &i| a.union(&boxes[i as usize]));
    if items.len() <= LEAF {
        return Node::Leaf { bounds, items };
    }
    let ext = bounds.max - bounds.min;
    let axis = if ext.x() >= ext.y() && ext.x() >= ext.z() {
        0
    } else if ext.y() >= ext.z() {
        1
    } else {
        2
    };
    items.sort_by(|&a, &b| {
        centre(&boxes[a as usize], axis)
            .total_cmp(&centre(&boxes[b as usize], axis))
            .then(a.cmp(&b))
    });
    let right = items.split_off(items.len() / 2);
    Node::Inner {
        bounds,
        kids: Box::new([build(boxes, items), build(boxes, right)]),
    }
}

impl Bvh {
    /// A hierarchy over `boxes`; item `i` is `boxes[i]`. Empty boxes are left
    /// out.
    pub fn new(boxes: &[Aabb3]) -> Bvh {
        let items: Vec<u32> = (0..boxes.len() as u32)
            .filter(|&i| !boxes[i as usize].is_empty())
            .collect();
        Bvh {
            root: (!items.is_empty()).then(|| build(boxes, items)),
            boxes: boxes.to_vec(),
        }
    }

    /// The box of item `i`.
    pub fn item_box(&self, i: u32) -> &Aabb3 {
        &self.boxes[i as usize]
    }

    /// Best-first branch and bound for a maximum.
    ///
    /// `bound(b)` must be an upper bound of the objective over everything in
    /// box `b` (`−∞` prunes the box). Items are passed to `visit` in
    /// decreasing order of their own box's bound; `visit` returns the
    /// objective it found for the item, if any. The search stops as soon as
    /// no remaining bound exceeds the best value. Returns the best value,
    /// starting from `best`.
    pub fn best_first(
        &self,
        mut best: f64,
        mut bound: impl FnMut(&Aabb3) -> f64,
        mut visit: impl FnMut(u32, f64) -> Option<f64>,
    ) -> f64 {
        let mut heap = BinaryHeap::new();
        let mut seq = 0u64;
        if let Some(r) = &self.root {
            heap.push(Pending {
                bound: bound(r.bounds()),
                seq,
                what: Waiting::Node(r),
            });
        }
        while let Some(p) = heap.pop() {
            if p.bound.partial_cmp(&best) != Some(Ordering::Greater) {
                break;
            }
            match p.what {
                Waiting::Item(i) => {
                    if let Some(v) = visit(i, p.bound) {
                        best = best.max(v);
                    }
                }
                Waiting::Node(n) => {
                    let mut push = |b: f64, what| {
                        if b > best {
                            seq += 1;
                            heap.push(Pending {
                                bound: b,
                                seq,
                                what,
                            });
                        }
                    };
                    match n {
                        Node::Leaf { items, .. } => {
                            for &i in items {
                                push(bound(&self.boxes[i as usize]), Waiting::Item(i));
                            }
                        }
                        Node::Inner { kids, .. } => {
                            for k in kids.iter() {
                                push(bound(k.bounds()), Waiting::Node(k));
                            }
                        }
                    }
                }
            }
        }
        best
    }

    /// Items whose boxes overlap `q`, in increasing order.
    pub fn query(&self, q: &Aabb3) -> Vec<u32> {
        let mut out = Vec::new();
        let mut stack: Vec<&Node> = self.root.iter().collect();
        while let Some(n) = stack.pop() {
            if !overlaps(n.bounds(), q) {
                continue;
            }
            match n {
                Node::Leaf { items, .. } => out.extend(items),
                Node::Inner { kids, .. } => stack.extend(kids.iter()),
            }
        }
        out.sort_unstable();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fluut_geom::Point3;

    fn unit_box(x: f64, y: f64, z: f64) -> Aabb3 {
        Aabb3::from_points(&[Point3::new(x, y, z), Point3::new(x + 1.0, y + 1.0, z + 1.0)])
    }

    #[test]
    fn query_matches_brute_force() {
        let boxes: Vec<Aabb3> = (0..300)
            .map(|i| {
                let f = i as f64;
                unit_box((f * 7.3) % 50.0, (f * 3.1) % 40.0, (f * 1.7) % 10.0)
            })
            .collect();
        let bvh = Bvh::new(&boxes);
        for q in [
            unit_box(10.0, 10.0, 2.0),
            unit_box(-5.0, -5.0, -5.0),
            Aabb3::from_points(&[Point3::new(0.0, 0.0, 0.0), Point3::new(50.0, 40.0, 10.0)]),
        ] {
            let want: Vec<u32> = (0..boxes.len() as u32)
                .filter(|&i| overlaps(&boxes[i as usize], &q))
                .collect();
            assert_eq!(bvh.query(&q), want);
        }
    }

    #[test]
    fn best_first_finds_the_maximum() {
        // Objective: the top of each box, minus its distance from x = 20.
        let boxes: Vec<Aabb3> = (0..300)
            .map(|i| {
                let f = i as f64;
                unit_box((f * 7.3) % 50.0, (f * 3.1) % 40.0, (f * 1.7) % 10.0)
            })
            .collect();
        let value = |b: &Aabb3| {
            let dx = (b.min.x() - 20.0).max(20.0 - b.max.x()).max(0.0);
            b.max.z() - dx
        };
        let want = boxes.iter().map(value).fold(f64::NEG_INFINITY, f64::max);
        let bvh = Bvh::new(&boxes);
        let mut visited = 0;
        let got = bvh.best_first(f64::NEG_INFINITY, value, |i, _| {
            visited += 1;
            Some(value(&boxes[i as usize]))
        });
        assert_eq!(got, want);
        assert!(visited < 30, "{visited} items visited");
    }

    #[test]
    fn best_first_prunes() {
        let boxes = [unit_box(0.0, 0.0, 0.0), unit_box(5.0, 0.0, 0.0)];
        let bvh = Bvh::new(&boxes);
        let got = bvh.best_first(
            f64::NEG_INFINITY,
            |b| {
                if b.min.x() > 3.0 {
                    f64::NEG_INFINITY
                } else {
                    b.max.z()
                }
            },
            |i, _| {
                assert_eq!(i, 0);
                Some(0.5)
            },
        );
        assert_eq!(got, 0.5);
    }

    #[test]
    fn empty() {
        assert!(Bvh::new(&[]).query(&unit_box(0.0, 0.0, 0.0)).is_empty());
        assert_eq!(
            Bvh::new(&[]).best_first(1.0, |_| 2.0, |_, _| Some(3.0)),
            1.0
        );
    }
}
