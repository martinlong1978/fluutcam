// SPDX-License-Identifier: MIT OR Apache-2.0

//! Playback of a simulation (D11 §4): the stock and its mesh at a chord
//! count, kept from one frame to the next.
//!
//! A [`Player`] keeps the stock it shows. Seeking forward within the same
//! checkpoint interval cuts on from that stock instead of from the
//! checkpoint, and the mesh rebuilds only the blocks whose rays changed.
//! [`SimPaint`] paints the stock's columns by their rest class against the
//! part and by the toolpath that last cut them.

use crate::dexel::TriDexel;
use crate::mesh::{Column, MeshTiles, OFF, Paint};
use crate::run::Simulation;

/// The paint of the stock after `n` chords of a simulation.
#[derive(Debug)]
pub struct SimPaint<'a> {
    sim: &'a Simulation,
    stock: &'a TriDexel,
    /// The stock before each toolpath up to the current one.
    before: Vec<&'a TriDexel>,
}

impl<'a> SimPaint<'a> {
    /// `stock` must be the stock after `n` chords of `sim`.
    pub fn new(sim: &'a Simulation, stock: &'a TriDexel, n: usize) -> SimPaint<'a> {
        let current = n
            .checked_sub(1)
            .and_then(|i| sim.chords.get(i))
            .map_or(0, |c| c.toolpath as usize + 1);
        SimPaint {
            sim,
            stock,
            before: (0..current).map(|k| sim.checkpoint(k)).collect(),
        }
    }
}

impl Paint for SimPaint<'_> {
    fn column(&self, i: usize, j: usize) -> Column {
        let g = &self.stock.grids[2];
        let ray = g.ray(i, j);
        let class = self.sim.part.as_ref().map_or(OFF, |p| {
            let z = self.stock.origin.z;
            let top = ray.last().map_or(z, |s| z + f64::from(s.hi));
            p.class(j * g.nu + i, top)
        });
        // The current toolpath if it changed the ray, else the latest one
        // before it that did.
        let b = &self.before;
        let toolpath = b.len().checked_sub(1).and_then(|m| {
            if b[m].grids[2].ray(i, j) != ray {
                return Some(m);
            }
            (0..m)
                .rev()
                .find(|&k| b[k].grids[2].ray(i, j) != b[k + 1].grids[2].ray(i, j))
        });
        Column {
            class,
            toolpath: toolpath.map(|k| k as u16),
        }
    }
}

/// The stock and its mesh at a chord count.
#[derive(Clone, Debug)]
pub struct Player {
    n: usize,
    mesh: MeshTiles,
}

impl Player {
    /// The stock after `n` chords of `sim`, meshed whole.
    pub fn new(sim: &Simulation, n: usize) -> Player {
        let n = n.min(sim.chords.len());
        let d = sim.stock_after(n);
        let mesh = MeshTiles::new(&d, &SimPaint::new(sim, &d, n));
        Player { n, mesh }
    }

    /// Moves to the stock after `n` chords of `sim`, the simulation this
    /// player was made for. Returns the blocks rebuilt.
    pub fn seek(&mut self, sim: &Simulation, n: usize) -> Vec<usize> {
        let n = n.min(sim.chords.len());
        let d = if n >= self.n && sim.checkpoint_index(n) == sim.checkpoint_index(self.n) {
            let mut d = self.mesh.stock().clone();
            sim.cut(&mut d, self.n..n);
            d
        } else {
            sim.stock_after(n)
        };
        let changed = self.mesh.update(&d, &SimPaint::new(sim, &d, n));
        self.n = n;
        changed
    }

    /// Chords cut in the stock shown.
    pub fn chords(&self) -> usize {
        self.n
    }

    pub fn stock(&self) -> &TriDexel {
        self.mesh.stock()
    }

    pub fn mesh(&self) -> &MeshTiles {
        &self.mesh
    }
}

#[cfg(test)]
mod tests;
