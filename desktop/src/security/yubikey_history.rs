use super::*;

impl PassApp {
    pub(crate) fn capture_yubikey_status(&mut self, cx: &mut Context<Self>) {
        let observation = self
            .state
            .reveal
            .as_ref()
            .and_then(|(request, _)| {
                request
                    .progress
                    .card_observation()
                    .map(|observation| (observation, YubiOperation::Reveal))
            })
            .or_else(|| {
                self.state.copy_pending.as_ref().and_then(|pending| {
                    pending
                        .progress
                        .card_observation()
                        .map(|observation| (observation, YubiOperation::Copy))
                })
            })
            .or_else(|| {
                self.state.saving.as_ref().and_then(|(_, progress, _)| {
                    progress.card_observation().map(|observation| {
                        let operation = if self.state.saving_edit {
                            YubiOperation::SaveEdited
                        } else {
                            YubiOperation::SaveGenerated
                        };
                        (observation, operation)
                    })
                })
            });
        let Some((observation, operation)) = observation else {
            return;
        };
        if self
            .state
            .last_yubikey_observation
            .is_some_and(|last| observation.observed_at <= last)
        {
            return;
        }
        self.state.last_yubikey_observation = Some(observation.observed_at);
        self.state.yubikey_history.push_front(YubiRecord {
            observation,
            operation,
        });
        self.state.yubikey_history.truncate(12);
        cx.notify();
    }
}
