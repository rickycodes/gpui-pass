use super::*;

impl Gpg {
    pub fn decrypt(
        &self,
        store: &Store,
        entry: &Entry,
        cancel: &Cancellation,
    ) -> Result<Secret, Error> {
        self.decrypt_with_progress(store, entry, cancel, &OperationProgress::new(UNLOCKING))
    }

    pub fn decrypt_with_progress(
        &self,
        store: &Store,
        entry: &Entry,
        cancel: &Cancellation,
        progress: &OperationProgress,
    ) -> Result<Secret, Error> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        progress.update(UNLOCKING);
        self.decrypt_file(store.open_entry(entry)?, cancel, progress)
    }

    /// Capture the revision of the file actually decrypted, rejecting changes during unlock.
    pub fn decrypt_with_revision(
        &self,
        store: &Store,
        entry: &Entry,
        cancel: &Cancellation,
        progress: &OperationProgress,
    ) -> Result<(Secret, EntryRevision), Error> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let file = store.open_entry(entry)?;
        let revision = EntryRevision::from_file(&file)?;
        progress.update(UNLOCKING);
        let secret = self.decrypt_file(file, cancel, progress)?;
        store.check_revision(entry, &revision)?;
        Ok((secret, revision))
    }

    pub fn generate_entry(
        &self,
        store: &Store,
        entry: &Entry,
        options: GeneratorOptions,
        cancel: &Cancellation,
        progress: &OperationProgress,
    ) -> Result<Secret, Error> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let secret = generate_memorable_password(options)?;
        self.encrypt_new_entry(store, entry, &secret, cancel, progress)?;
        Ok(secret)
    }

    pub fn generate_pass_default_entry(
        &self,
        store: &Store,
        entry: &Entry,
        cancel: &Cancellation,
        progress: &OperationProgress,
    ) -> Result<Secret, Error> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let secret = crate::generate_pass_default_password()?;
        self.encrypt_new_entry(store, entry, &secret, cancel, progress)?;
        Ok(secret)
    }

    /// Encrypt caller-provided text exactly as supplied and create a new entry.
    pub fn create_entry(
        &self,
        store: &Store,
        entry: &Entry,
        secret: &Secret,
        cancel: &Cancellation,
        progress: &OperationProgress,
    ) -> Result<(), Error> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let policy = self.recipient_policy(store, entry)?;
        let ciphertext =
            self.encrypt_secret(secret, &policy.recipients, cancel, progress, false)?;
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        store.create_entry(entry, &ciphertext, &policy)
    }

    pub fn update_entry(
        &self,
        store: &Store,
        entry: &Entry,
        secret: &Secret,
        cancel: &Cancellation,
        progress: &OperationProgress,
    ) -> Result<(), Error> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let revision = store.entry_revision(entry)?;
        self.update_entry_if_unchanged(store, entry, secret, &revision, cancel, progress)
    }

    pub fn update_entry_if_unchanged(
        &self,
        store: &Store,
        entry: &Entry,
        secret: &Secret,
        expected: &EntryRevision,
        cancel: &Cancellation,
        progress: &OperationProgress,
    ) -> Result<(), Error> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        store.check_revision(entry, expected)?;
        let policy = self.recipient_policy(store, entry)?;
        let ciphertext =
            self.encrypt_secret(secret, &policy.recipients, cancel, progress, false)?;
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        store.replace_entry(entry, &ciphertext, expected, &policy)
    }

    /// Replace an entry with a generated password after verifying its revision.
    /// Generated entries retain pass's trailing-newline format.
    pub fn update_generated_entry_if_unchanged(
        &self,
        store: &Store,
        entry: &Entry,
        secret: &Secret,
        expected: &EntryRevision,
        cancel: &Cancellation,
        progress: &OperationProgress,
    ) -> Result<(), Error> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        store.check_revision(entry, expected)?;
        let policy = self.recipient_policy(store, entry)?;
        let ciphertext = self.encrypt_secret(secret, &policy.recipients, cancel, progress, true)?;
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        store.replace_entry(entry, &ciphertext, expected, &policy)
    }

    pub(crate) fn encrypt_new_entry(
        &self,
        store: &Store,
        entry: &Entry,
        secret: &Secret,
        cancel: &Cancellation,
        progress: &OperationProgress,
    ) -> Result<(), Error> {
        let policy = self.recipient_policy(store, entry)?;
        let ciphertext = self.encrypt_secret(secret, &policy.recipients, cancel, progress, true)?;
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        store.create_entry(entry, &ciphertext, &policy)
    }
}
