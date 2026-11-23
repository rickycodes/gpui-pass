use fluent_bundle::{FluentArgs, FluentResource, concurrent::FluentBundle};
use std::sync::OnceLock;
use unic_langid::LanguageIdentifier;

const EN_US: &str = include_str!("../../../locales/en-US/main.ftl");

struct Localization {
    bundle: FluentBundle<FluentResource>,
}

impl Localization {
    fn english() -> Self {
        let language: LanguageIdentifier = "en-US".parse().expect("valid built-in locale");
        let mut bundle = FluentBundle::new_concurrent(vec![language]);
        let resource = FluentResource::try_new(EN_US.to_owned())
            .unwrap_or_else(|(_, error)| panic!("invalid built-in Fluent catalog: {error:?}"));
        bundle
            .add_resource(resource)
            .expect("built-in Fluent catalog has no duplicate messages");
        Self { bundle }
    }

    fn text(&self, id: &str) -> String {
        self.format(id, None)
    }

    fn format(&self, id: &str, args: Option<&FluentArgs<'_>>) -> String {
        let Some(message) = self.bundle.get_message(id) else {
            return id.to_owned();
        };
        let Some(pattern) = message.value() else {
            return id.to_owned();
        };
        let mut errors = Vec::new();
        self.bundle
            .format_pattern(pattern, args, &mut errors)
            .into_owned()
    }
}

static LOCALIZATION: OnceLock<Localization> = OnceLock::new();

pub fn text(id: &str) -> String {
    LOCALIZATION.get_or_init(Localization::english).text(id)
}

pub fn text_with_args(id: &str, args: &FluentArgs<'_>) -> String {
    LOCALIZATION
        .get_or_init(Localization::english)
        .format(id, Some(args))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_countdown_formats_the_number_and_plural() {
        let localization = Localization::english();
        fn plain(text: String) -> String {
            text.chars()
                .filter(|character| !matches!(character, '\u{2068}' | '\u{2069}'))
                .collect()
        }

        let mut one = FluentArgs::new();
        one.set("seconds", 1_i64);
        assert_eq!(
            plain(localization.format("clipboard-restores-in", Some(&one))),
            "Clipboard restores in 1 second"
        );

        let mut several = FluentArgs::new();
        several.set("seconds", 12_i64);
        assert_eq!(
            plain(localization.format("clipboard-restores-in", Some(&several))),
            "Clipboard restores in 12 seconds"
        );
    }
}
