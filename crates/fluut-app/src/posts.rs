// SPDX-License-Identifier: MIT OR Apache-2.0

//! The posts the ribbon offers: the built-in ones that load as a post
//! (mixins do not), with the extension of the files each writes.

use fluut_model::library::{DefKind, Library};
use fluut_model::{DefRef, MemStore};
use fluut_post::builtin::{BUILTIN_POSTS, builtin_store};
use fluut_post::{Post, resolve_post};
use std::collections::BTreeSet;
use std::sync::OnceLock;

/// A post to choose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PostChoice {
    pub id: String,
    /// File extension without the dot.
    pub extension: String,
}

/// The built-in posts, ordered by id.
pub fn choices() -> &'static [PostChoice] {
    static CHOICES: OnceLock<Vec<PostChoice>> = OnceLock::new();
    CHOICES.get_or_init(|| from_store(&builtin_store(), BUILTIN_POSTS.iter().map(|(id, _)| *id)))
}

/// The posts of `library` that load as a post (mixins do not), built-in and
/// the project's own, ordered by id.
pub fn library_choices(library: &Library) -> Vec<PostChoice> {
    let store = library.mem_store(DefKind::Post);
    let ids: BTreeSet<&str> = library
        .entries()
        .iter()
        .filter(|e| e.kind == DefKind::Post)
        .map(|e| e.def.id.as_str())
        .collect();
    from_store(&store, ids.into_iter())
}

fn from_store<'a>(store: &MemStore, ids: impl Iterator<Item = &'a str>) -> Vec<PostChoice> {
    let mut v: Vec<PostChoice> = ids
        .filter_map(|id| {
            let resolved = resolve_post(store, &DefRef::parse(id).ok()?, None).ok()?;
            let loaded = Post::load(&resolved).ok()?;
            Some(PostChoice {
                id: id.to_owned(),
                extension: loaded.post.output.extension,
            })
        })
        .collect();
    v.sort_by(|a, b| a.id.cmp(&b.id));
    v.dedup_by(|a, b| a.id == b.id);
    v
}

/// The extension of the files post `id` writes, among `posts`; `nc` for an
/// unknown post.
pub fn extension_in(posts: &[PostChoice], id: &str) -> String {
    posts
        .iter()
        .find(|c| c.id == id)
        .map_or_else(|| "nc".to_owned(), |c| c.extension.clone())
}

/// The extension of the files built-in post `id` writes; `nc` for an
/// unknown post.
pub fn extension(id: &str) -> &'static str {
    choices()
        .iter()
        .find(|c| c.id == id)
        .map_or("nc", |c| c.extension.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_choices_are_the_posts_and_not_the_mixins() {
        let ids: Vec<_> = choices().iter().map(|c| c.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "fluut/gcode",
                "fluut/grbl",
                "fluut/linuxcnc",
                "fluut/linuxcnc-4x"
            ]
        );
    }

    #[test]
    fn the_extension_comes_from_the_post() {
        assert_eq!(extension("fluut/linuxcnc"), "ngc");
        assert_eq!(extension("fluut/grbl"), "gcode");
        assert_eq!(extension("fluut/gcode"), "nc");
        assert_eq!(extension("fluut/linuxcnc-4x"), "ngc");
        assert_eq!(extension("no/such"), "nc");
    }
}
