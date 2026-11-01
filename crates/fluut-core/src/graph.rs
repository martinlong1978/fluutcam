// SPDX-License-Identifier: MIT OR Apache-2.0

//! The invalidation graph (D01 §4): every computed artefact is a node keyed
//! by a hash of its inputs. A node whose hash is not in the cache is stale.

use crate::ALGORITHM_VERSION;
use crate::store::ModelStore;
use fluut_model::hash::{setup_text, sha256_hex, toolpath_p_text, toolpath_r_text};
use fluut_model::project::{Project, Setup};
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
fn setup_sources<'a>(s: &Setup, p: &'a Project, store: &ModelStore) -> Vec<&'a str> {
    p.geometry
        .iter()
        .filter(|g| match &s.bodies {
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
        for s in &p.setups {
            let srcs = setup_sources(s, p, store);
            let mut text = format!("{ALGORITHM_VERSION}\n{}", setup_text(s));
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
            for t in &s.toolpaths {
                let tool = tools.get(&t.tool);
                let m = s.material.as_deref();
                let r =
                    sha256_hex(format!("{setup_hash}\n{}", toolpath_r_text(t, tool, m)).as_bytes());
                let rid = NodeId::Result(t.id.clone());
                let post = sha256_hex(format!("{r}\n{}", toolpath_p_text(t, tool, m)).as_bytes());
                nodes.insert(
                    rid.clone(),
                    Node {
                        hash: r,
                        deps: vec![setup.clone()],
                    },
                );
                nodes.insert(
                    NodeId::Post(t.id.clone()),
                    Node {
                        hash: post,
                        deps: vec![rid],
                    },
                );
            }
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
