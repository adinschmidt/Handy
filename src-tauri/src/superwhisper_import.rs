use serde::Serialize;
use specta::Type;

#[derive(Debug, Serialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SuperwhisperImportError {
    UnsupportedPlatform,
    CacheUnavailable,
    NoCredentials,
}

#[cfg(target_os = "macos")]
pub(crate) struct ImportedCredentials {
    pub x_id: String,
    pub x_license: String,
    pub x_signature: String,
}

#[cfg(target_os = "macos")]
fn parse_request(bytes: &[u8]) -> Option<ImportedCredentials> {
    let request = plist::Value::from_reader(std::io::Cursor::new(bytes)).ok()?;
    let parts = request.as_dictionary()?.get("Array")?.as_array()?;
    // CFURLCache stores headers as a top-level dictionary in its request array.
    // Do not descend into request bodies, which may contain private audio or text.
    for headers in parts.iter().filter_map(plist::Value::as_dictionary) {
        let header = |name: &str| {
            headers
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(name))?
                .1
                .as_string()
        };
        let Some((x_id, x_license, x_signature)) = header("X-ID")
            .zip(header("X-License"))
            .zip(header("X-Signature"))
            .map(|((id, license), signature)| (id, license, signature))
        else {
            continue;
        };
        let Ok(credentials) =
            crate::settings::SuperwhisperCredentials::new(x_id, x_license, x_signature)
        else {
            continue;
        };
        return Some(ImportedCredentials {
            x_id: credentials.x_id.into(),
            x_license: credentials.x_license.into(),
            x_signature: credentials.x_signature.into(),
        });
    }
    None
}

#[cfg(target_os = "macos")]
pub(crate) fn read_credentials(
    path: &std::path::Path,
) -> Result<ImportedCredentials, SuperwhisperImportError> {
    use rusqlite::{Connection, OpenFlags};
    use SuperwhisperImportError::{CacheUnavailable, NoCredentials};
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| CacheUnavailable)?;
    connection
        .busy_timeout(std::time::Duration::from_millis(250))
        .map_err(|_| CacheUnavailable)?;
    let mut statement = connection
        .prepare(
            "SELECT b.request_object FROM cfurl_cache_response r
         JOIN cfurl_cache_blob_data b USING(entry_ID)
         WHERE r.request_key IN (
             'https://api.superwhisper.com/elevenlabs/v1/transcribe',
             'https://api.superwhisper.com/models/language/cloud',
             'https://api.superwhisper.com/v1/chat/completions'
         ) AND length(b.request_object) <= 8388608
         ORDER BY r.time_stamp DESC, r.entry_ID DESC LIMIT 32",
        )
        .map_err(|_| CacheUnavailable)?;
    let requests = statement
        .query_map([], |row| row.get::<_, Vec<u8>>(0))
        .map_err(|_| CacheUnavailable)?;
    for request in requests {
        if let Some(credentials) = parse_request(&request.map_err(|_| CacheUnavailable)?) {
            return Ok(credentials);
        }
    }
    Err(NoCredentials)
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use rusqlite::Connection;
    const ID: &str = "00000000-0000-4000-8000-000000000001";
    const LICENSE: &str = "00000000-0000-4000-8000-000000000002";
    const SIGNATURE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn request(id: &str) -> Vec<u8> {
        let mut headers = plist::Dictionary::new();
        for (key, value) in [
            ("x-id", id),
            ("X-License", LICENSE),
            ("X-Signature", SIGNATURE),
        ] {
            headers.insert(key.into(), plist::Value::String(value.into()));
        }
        let mut root = plist::Dictionary::new();
        root.insert(
            "Array".into(),
            plist::Value::Array(vec![plist::Value::Dictionary(headers)]),
        );
        let mut bytes = Vec::new();
        plist::Value::Dictionary(root)
            .to_writer_binary(&mut bytes)
            .unwrap();
        bytes
    }

    #[test]
    fn reads_newest_valid_set_without_changing_cache() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Cache.db");
        let db = Connection::open(&path).unwrap();
        db.execute_batch("CREATE TABLE cfurl_cache_response(entry_ID INTEGER, request_key TEXT, time_stamp TEXT);
            CREATE TABLE cfurl_cache_blob_data(entry_ID INTEGER, request_object BLOB);").unwrap();
        for (entry, url, id) in [
            (
                1,
                "https://api.superwhisper.com/elevenlabs/v1/transcribe",
                LICENSE,
            ),
            (2, "https://api.superwhisper.com/models/language/cloud", ID),
            (
                3,
                "https://api.superwhisper.com/v1/chat/completions",
                "invalid",
            ),
            (
                4,
                "https://unrelated.example/elevenlabs/v1/transcribe",
                LICENSE,
            ),
        ] {
            db.execute(
                "INSERT INTO cfurl_cache_response VALUES (?1,?2,'2026-09-06')",
                rusqlite::params![entry, url],
            )
            .unwrap();
            db.execute(
                "INSERT INTO cfurl_cache_blob_data VALUES (?1,?2)",
                rusqlite::params![entry, request(id)],
            )
            .unwrap();
        }
        drop(db);
        let before = std::fs::read(&path).unwrap();
        let credentials = read_credentials(&path).unwrap();
        assert_eq!(credentials.x_id, ID);
        assert_eq!(credentials.x_license, LICENSE);
        assert_eq!(credentials.x_signature, SIGNATURE);
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn missing_or_changed_cache_and_invalid_requests_fail_without_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.db");
        assert!(matches!(
            read_credentials(&path),
            Err(SuperwhisperImportError::CacheUnavailable)
        ));
        assert!(!path.exists());
        assert!(parse_request(b"bad plist").is_none());
        assert!(parse_request(&request("malformed-private-value")).is_none());
        let db = Connection::open(&path).unwrap();
        assert!(matches!(
            read_credentials(&path),
            Err(SuperwhisperImportError::CacheUnavailable)
        ));
        db.execute_batch("CREATE TABLE cfurl_cache_response(entry_ID INTEGER, request_key TEXT, time_stamp TEXT);
            CREATE TABLE cfurl_cache_blob_data(entry_ID INTEGER, request_object BLOB);").unwrap();
        assert!(matches!(
            read_credentials(&path),
            Err(SuperwhisperImportError::NoCredentials)
        ));
    }

    #[test]
    #[ignore = "reads credentials from the local Superwhisper installation without saving them"]
    fn installed_cache_contains_valid_credentials() {
        let path = std::path::PathBuf::from(std::env::var_os("HOME").unwrap())
            .join("Library/Caches/com.superduper.superwhisper/Cache.db");
        assert!(
            read_credentials(&path).is_ok(),
            "No usable credential set in installed cache"
        );
    }
}
