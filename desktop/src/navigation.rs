#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Page {
    #[default]
    Entries,
    CardDetails,
}

/// The generator is an overlay; opening it preserves the underlying page.
#[derive(Default)]
pub(crate) struct Navigation {
    page: Page,
    generator_open: bool,
}

impl Navigation {
    pub fn page(&self) -> Page {
        self.page
    }

    pub fn go_to(&mut self, page: Page) {
        self.page = page;
    }

    pub fn generator_open(&self) -> bool {
        self.generator_open
    }

    pub fn open_generator(&mut self) {
        self.generator_open = true;
    }

    pub fn close_generator(&mut self) {
        self.generator_open = false;
    }

    pub fn card_unavailable(&mut self) {
        if self.page == Page::CardDetails {
            self.page = Page::Entries;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generator_returns_to_the_page_it_was_opened_from() {
        for page in [Page::Entries, Page::CardDetails] {
            let mut navigation = Navigation::default();
            navigation.go_to(page);
            navigation.open_generator();
            navigation.open_generator();
            assert!(navigation.generator_open());
            navigation.close_generator();
            assert!(!navigation.generator_open());
            assert_eq!(navigation.page(), page);
        }
    }

    #[test]
    fn removal_while_generating_returns_to_entries_on_close() {
        let mut navigation = Navigation::default();
        navigation.go_to(Page::CardDetails);
        navigation.open_generator();
        navigation.card_unavailable();
        assert!(navigation.generator_open());
        navigation.close_generator();
        assert_eq!(navigation.page(), Page::Entries);
    }

    #[test]
    fn removal_leaves_card_details_immediately() {
        let mut navigation = Navigation::default();
        navigation.go_to(Page::CardDetails);
        navigation.card_unavailable();
        assert_eq!(navigation.page(), Page::Entries);
    }
}
