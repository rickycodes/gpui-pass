use super::*;
use crate::app_state::OnboardingState;

impl PassApp {
    pub(super) fn open_onboarding(&mut self, path: PathBuf, preview: bool, cx: &mut Context<Self>) {
        self.clear_create_entry();
        self.state.onboarding = Some(OnboardingState { path, preview });
        if preview {
            self.state.custom_entry_name = "DEMO-RECIPIENT".into();
            self.state.custom_entry_name_selection =
                self.state.custom_entry_name.len()..self.state.custom_entry_name.len();
            self.state.custom_entry_name_cursor = self.state.custom_entry_name.len();
        }
        self.state.onboarding_pending = None;
        self.clear_message();
        cx.notify();
    }
    pub(super) fn finish_onboarding_preview(&mut self, cx: &mut Context<Self>) {
        self.state.onboarding = None;
        self.state.onboarding_pending = None;
        self.state.onboarding_demo = false;
        self.state.demo = true;
        self.state.demo_empty_store = true;
        self.state.path = None;
        self.clear_create_entry();
        self.refresh(cx);
    }

    pub(super) fn initialize_onboarding(&mut self, cx: &mut Context<Self>) {
        if self.state.onboarding_pending.is_some() {
            return;
        }
        let Some(onboarding) = self.state.onboarding.as_ref() else {
            return;
        };
        let preview = onboarding.preview;
        let recipients: Vec<_> = self
            .state
            .custom_entry_name
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        if recipients.is_empty() {
            self.set_error("Enter at least one GPG recipient.".into());
            cx.notify();
            return;
        }
        if preview {
            self.finish_onboarding_preview(cx);
            return;
        }
        let path = onboarding.path.clone();
        let executable = self.state.gpg.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let result = (|| {
                let gpg = Gpg::new(executable)?;
                gpg.validate_recipients(&recipients)?;
                let signing_keys = configured_signing_keys()?;
                let contents = recipients.join("\n") + "\n";
                let signature = if signing_keys.is_empty() {
                    None
                } else {
                    Some(gpg.sign_detached(contents.as_bytes(), &signing_keys)?)
                };
                Store::initialize(&path, &recipients, signature.as_deref()).map(|_| ())
            })();
            let _ = sender.send(result);
        });
        self.clear_message();
        self.state.onboarding_pending = Some(receiver);
        cx.notify();
    }

    pub(super) fn poll_onboarding(&mut self, cx: &mut Context<Self>) {
        let Some(result) = self
            .state
            .onboarding_pending
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok())
        else {
            return;
        };
        self.state.onboarding_pending = None;
        match result {
            Ok(()) => {
                let path = self
                    .state
                    .onboarding
                    .as_ref()
                    .map(|onboarding| onboarding.path.clone());
                self.state.onboarding = None;
                self.state.custom_entry_name.clear();
                self.state.custom_entry_name_selection = 0..0;
                self.state.custom_entry_name_cursor = 0;
                self.state.path = path;
                self.refresh(cx);
            }
            Err(error) => self.set_error(error.to_string()),
        }
        cx.notify();
    }
}
