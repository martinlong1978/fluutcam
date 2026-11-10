// SPDX-License-Identifier: MIT OR Apache-2.0

//! The posts the ribbon offers: the built-in ones that load as a post
//! (mixins do not), with the extension of the files each writes.

use fluut_model::DefRef;
use fluut_post::builtin::{BUILTIN_POSTS, builtin_store};
use fluut_post::{Post, resolve_post};
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
    CHOICES.get_or_init(|| {
        let store = builtin_store();
        let mut v: Vec<PostChoice> = BUILTIN_POSTS
            .iter()
            .filter_map(|(id, _)| {
                let resolved = resolve_post(&store, &DefRef::parse(id).ok()?, None).ok()?;
                let loaded = Post::load(&resolved).ok()?;
                Some(PostChoice {
                    id: (*id).to_owned(),
                    extension: loaded.post.output.extension,
                })
            })
            .collect();
        v.sort_by(|a, b| a.id.cmp(&b.id));
        v
    })
}

/// The extension of the files post `id` writes; `nc` for an unknown post.
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
