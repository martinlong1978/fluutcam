// SPDX-License-Identifier: MIT OR Apache-2.0

//! Built-in posts (spec 05 §5.2.1, D10 §7), embedded in the binary.
//!
//! Embedding makes them read-only; their integrity is covered by the signed
//! binary, so no separate hash check is needed at load (spec 05 §5.9).

use fluut_model::MemStore;

/// `(id, file text)` of every built-in post.
pub const BUILTIN_POSTS: [(&str, &str); 5] = [
    (
        "fluut/gcode",
        include_str!("../posts/fluut/gcode.post.toml"),
    ),
    ("fluut/grbl", include_str!("../posts/fluut/grbl.post.toml")),
    (
        "fluut/linuxcnc",
        include_str!("../posts/fluut/linuxcnc.post.toml"),
    ),
    (
        "fluut/linuxcnc-4x",
        include_str!("../posts/fluut/linuxcnc-4x.post.toml"),
    ),
    (
        "fluut/mixin/rotary-a",
        include_str!("../posts/fluut/mixin/rotary-a.post.toml"),
    ),
];

/// A store holding every built-in post.
pub fn builtin_store() -> MemStore {
    let mut s = MemStore::new();
    for (id, src) in BUILTIN_POSTS {
        s.insert_toml(src)
            .unwrap_or_else(|e| panic!("built-in post {id} does not parse: {e}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CompiledPost, Post, resolve_post};
    use fluut_model::{DefRef, DefStore};

    #[test]
    fn every_builtin_loads_and_compiles() {
        let s = builtin_store();
        for (id, _) in BUILTIN_POSTS {
            let def = s.lookup(&DefRef::parse(id).unwrap()).unwrap();
            assert_eq!(def.id, id);
            if id.contains("/mixin/") {
                continue;
            }
            let r = resolve_post(&s, &DefRef::parse(id).unwrap(), None).unwrap();
            let loaded = Post::load(&r).unwrap();
            assert!(loaded.warnings.is_empty(), "{id}: {:?}", loaded.warnings);
            CompiledPost::compile(loaded).unwrap();
        }
    }
}
