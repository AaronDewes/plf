use slug::slugify;
use plf::{Kwargs, State};

/// Slugify the given value.
///
/// ```text
/// {{ title | slug }}
/// ```
pub fn slug(val: &str, _: Kwargs, _: &State) -> String {
    slugify(val)
}

#[cfg(test)]
mod tests {
    use super::*;
    use plf::{Context, Kwargs, State};

    #[test]
    fn test_slug() {
        let ctx = Context::new();
        let state = State::new(&ctx);
        assert_eq!(
            slug("Hello World", Kwargs::default(), &state),
            "hello-world"
        );
        assert_eq!(slug("Foo & Bar!", Kwargs::default(), &state), "foo-bar");
    }

    #[test]
    fn test_register() {
        let mut tera = plf::Tera::default();
        tera.register_filter("slug", slug);
    }
}
