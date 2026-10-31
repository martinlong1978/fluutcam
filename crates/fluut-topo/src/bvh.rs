// SPDX-License-Identifier: MIT OR Apache-2.0

//! Bounding-volume hierarchy over face boxes (design D03 §4), median split
//! on the longest axis.

use fluut_geom::Aabb3;

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
        }
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
    fn empty() {
        assert!(Bvh::new(&[]).query(&unit_box(0.0, 0.0, 0.0)).is_empty());
    }
}
