// SPDX-License-Identifier: MIT OR Apache-2.0

//! The invalidation graph (D01 §4): every computed artefact is a node keyed
//! by a hash of its inputs. A node whose hash is not in the cache is stale.

use crate::ALGORITHM_VERSION;
use crate::store::ModelStore;
use fluut_model::hash::{setup_text, sha256_hex, toolpath_p_text, toolpath_r_text};
use fluut_model::project::Project;
use fluut_model::setup::{ResolvedSetup, resolve_all};
use fluut_model::tools::ToolStore;
use std::collections::BTreeMap;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum NodeId {
    /// A geometry source file.
    Geometry(String),
    /// A Setup frame: bodies in W, stock, zero.
    Setup(String),
    /// A toolpath's `ToolpathResult`.
    Result(String),
    /// A toolpath's post output (feeds, speed, names).
    Post(String),
    /// A Setup's simulation (D11): its enabled toolpaths' results and
    /// feeds, and their tools.
    Simulation(String),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Node {
    pub hash: String,
    pub deps: Vec<NodeId>,
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Graph {
    pub nodes: BTreeMap<NodeId, Node>,
}

/// Sources holding the Setup's bodies (all sources when the Setup names
/// none), by id.
fn setup_sources<'a>(s: &ResolvedSetup, p: &'a Project, store: &ModelStore) -> Vec<&'a str> {
    p.geometry
        .iter()
        .filter(|g| match &s.geometry.bodies {
            None => true,
            Some(ids) => store
                .source(&g.id)
                .is_some_and(|src| src.bodies.iter().any(|b| ids.contains(&b.id))),
        })
        .map(|g| g.id.as_str())
        .collect()
}

impl Graph {
    pub fn build(p: &Project, store: &ModelStore, tools: &ToolStore) -> Graph {
        let mut nodes = BTreeMap::new();
        for g in &p.geometry {
            let hash = store
                .source(&g.id)
                .map_or_else(|| "missing".to_owned(), |s| s.hash.clone());
            nodes.insert(
                NodeId::Geometry(g.id.clone()),
                Node {
                    hash,
                    deps: Vec::new(),
                },
            );
        }
        for (s, r) in p.setups.iter().zip(resolve_all(p)) {
            // A Setup whose chain is broken hashes its error, so it is
            // stale until fixed.
            let (srcs, body) = match &r {
                Ok(r) => (setup_sources(r, p, store), setup_text(r)),
                Err(d) => (Vec::new(), format!("error = {}\n", d.code)),
            };
            let material = r.as_ref().ok().and_then(|r| r.material.clone());
            let mut text = format!("{ALGORITHM_VERSION}\n{body}");
            let mut deps = Vec::new();
            for id in srcs {
                let d = NodeId::Geometry(id.to_owned());
                text.push_str(&format!("geometry {id} = {}\n", nodes[&d].hash));
                deps.push(d);
            }
            let setup = NodeId::Setup(s.id.clone());
            let setup_hash = sha256_hex(text.as_bytes());
            nodes.insert(
                setup.clone(),
                Node {
                    hash: setup_hash.clone(),
                    deps,
                },
            );
            let mut sim = format!("{ALGORITHM_VERSION} sim\n{setup_hash}\n");
            let mut sim_deps = vec![setup.clone()];
            // Earlier toolpaths that change the in-process stock: a
            // toolpath that reads it depends on their results (D15 §1.8).
            let mut stock_text = String::new();
            let mut stock_deps = Vec::new();
            for t in &s.toolpaths {
                let tool = tools.get(t.tool_ref());
                let m = material.as_deref();
                let mut text = format!("{setup_hash}\n{}", toolpath_r_text(t, tool, m));
                let mut deps = vec![setup.clone()];
                if t.params.reads_stock() {
                    text.push_str(&format!("in_process:\n{stock_text}"));
                    deps.extend(stock_deps.iter().cloned());
                }
                let r = sha256_hex(text.as_bytes());
                let rid = NodeId::Result(t.id.clone());
                if t.kind().updates_stock() {
                    stock_text.push_str(&format!("{} {} {r}\n", t.id, t.is_enabled()));
                    stock_deps.push(rid.clone());
                }
                let post = sha256_hex(format!("{r}\n{}", toolpath_p_text(t, tool, m)).as_bytes());
                nodes.insert(rid.clone(), Node { hash: r, deps });
                if t.is_enabled() {
                    // The whole tool: flute length, shank and holder are
                    // checked, and they need not change the result.
                    sim.push_str(&format!("{} = {post}\n{tool:?}\n", t.id));
                    sim_deps.push(NodeId::Post(t.id.clone()));
                }
                nodes.insert(
                    NodeId::Post(t.id.clone()),
                    Node {
                        hash: post,
                        deps: vec![rid],
                    },
                );
            }
            nodes.insert(
                NodeId::Simulation(s.id.clone()),
                Node {
                    hash: sha256_hex(sim.as_bytes()),
                    deps: sim_deps,
                },
            );
        }
        Graph { nodes }
    }

    pub fn hash(&self, id: &NodeId) -> Option<&str> {
        self.nodes.get(id).map(|n| n.hash.as_str())
    }

    /// Nodes whose hash differs from `other` (edited or newly added).
    pub fn changed(&self, other: &Graph) -> Vec<NodeId> {
        self.nodes
            .iter()
            .filter(|(k, n)| other.nodes.get(*k).is_none_or(|o| o.hash != n.hash))
            .map(|(k, _)| k.clone())
            .collect()
    }
}
