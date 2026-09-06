use super::AppSettings;

const KEYS: [&str; 3] = [
    "superwhisper_x_id",
    "superwhisper_x_license",
    "superwhisper_x_signature",
];

/// Opaque credentials imported from an authorized Superwhisper installation.
/// Deliberately has no Debug or Serialize implementation.
pub(crate) struct SuperwhisperCredentials<'a> {
    pub x_id: &'a str,
    pub x_license: &'a str,
    pub x_signature: &'a str,
}

impl<'a> SuperwhisperCredentials<'a> {
    pub fn new(x_id: &'a str, x_license: &'a str, x_signature: &'a str) -> Result<Self, String> {
        let x_id = x_id.trim();
        let x_license = x_license.trim();
        let x_signature = x_signature.trim();
        if [x_id, x_license, x_signature].contains(&"") {
            return Err("Superwhisper credentials are incomplete.".into());
        }
        let uuid = |value: &str| {
            value.len() == 36
                && value.bytes().enumerate().all(|(index, byte)| {
                    if [8, 13, 18, 23].contains(&index) {
                        byte == b'-'
                    } else {
                        byte.is_ascii_hexdigit()
                    }
                })
        };
        if !uuid(x_id)
            || !uuid(x_license)
            || x_signature.len() != 64
            || !x_signature
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("Superwhisper credentials are malformed.".into());
        }
        Ok(Self {
            x_id,
            x_license,
            x_signature,
        })
    }
}

impl AppSettings {
    pub(crate) fn superwhisper_credentials(&self) -> Result<SuperwhisperCredentials<'_>, String> {
        let values = KEYS.map(|key| {
            self.transcription_api_keys
                .get(key)
                .map(String::as_str)
                .unwrap_or_default()
        });
        SuperwhisperCredentials::new(values[0], values[1], values[2])
    }

    /// Validate before changing any entry. An entirely empty set clears the credentials.
    pub(crate) fn set_superwhisper_credentials(
        &mut self,
        x_id: &str,
        x_license: &str,
        x_signature: &str,
    ) -> Result<(), String> {
        let values = [x_id.trim(), x_license.trim(), x_signature.trim()];
        if values.iter().all(|value| value.is_empty()) {
            if self.selected_transcription_provider
                == super::TranscriptionProvider::SuperwhisperScribe
            {
                self.selected_transcription_provider = super::TranscriptionProvider::Local;
            }
            for key in KEYS {
                self.transcription_api_keys.remove(key);
            }
            return Ok(());
        }
        let credentials = SuperwhisperCredentials::new(values[0], values[1], values[2])?;
        for (key, value) in KEYS.into_iter().zip([
            credentials.x_id,
            credentials.x_license,
            credentials.x_signature,
        ]) {
            self.transcription_api_keys.insert(key.into(), value.into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const ID: &str = "00000000-0000-4000-8000-000000000001";
    const SIGNATURE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[test]
    fn validates_atomically_and_redacts_saved_credentials() {
        let mut settings = AppSettings::default();
        assert!(settings.superwhisper_credentials().is_err());
        settings
            .set_superwhisper_credentials(&format!(" {ID} "), ID, SIGNATURE)
            .unwrap();
        assert!(settings
            .set_superwhisper_credentials(ID, "", SIGNATURE)
            .is_err());
        assert!(settings
            .set_superwhisper_credentials(ID, ID, &SIGNATURE.to_uppercase())
            .is_err());
        assert!(settings
            .set_superwhisper_credentials("invalid", ID, SIGNATURE)
            .is_err());
        assert_eq!(settings.superwhisper_credentials().unwrap().x_id, ID);
        let debug = format!("{settings:?}");
        assert!(!debug.contains(ID));
        assert!(!debug.contains(SIGNATURE));
        settings.selected_transcription_provider =
            super::super::TranscriptionProvider::SuperwhisperScribe;
        settings.set_superwhisper_credentials("", " ", "").unwrap();
        assert_eq!(
            settings.selected_transcription_provider,
            super::super::TranscriptionProvider::Local
        );
        assert!(settings.superwhisper_credentials().is_err());
    }
}
