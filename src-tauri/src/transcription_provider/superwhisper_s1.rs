use crate::settings::SuperwhisperCredentials;
use anyhow::{anyhow, Result};
use reqwest::multipart::{Form, Part};
use serde::Deserialize;

#[derive(Deserialize)]
struct Region {
    id: String,
    host: String,
}

#[derive(Deserialize)]
struct Regions {
    regions: Vec<Region>,
    default: String,
}

// Do not derive Debug: this response contains a short-lived bearer credential.
#[derive(Deserialize)]
struct InferenceKey {
    key: String,
}

#[derive(Deserialize)]
struct RunResponse {
    text: String,
}

async fn response_json<T: serde::de::DeserializeOwned>(
    request: reqwest::RequestBuilder,
) -> Result<T> {
    let response = request
        .send()
        .await
        .map_err(|_| anyhow!("Could not reach Superwhisper S1-Voice."))?;
    if !response.status().is_success() {
        return Err(anyhow!(
            "Superwhisper S1-Voice returned HTTP {}.",
            response.status().as_u16()
        ));
    }
    response
        .json()
        .await
        .map_err(|_| anyhow!("Superwhisper S1-Voice returned an invalid response."))
}

fn region_url(host: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(host)
        .map_err(|_| anyhow!("Superwhisper returned an invalid inference host."))?;
    // The discovered host receives a bearer token, so constrain its destination.
    if url.scheme() != "https"
        || !url
            .host_str()
            .is_some_and(|host| host.ends_with(".superwhisper.com"))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(anyhow!("Superwhisper returned an invalid inference host."));
    }
    Ok(url)
}

async fn bootstrap(
    api: &str,
    credentials: SuperwhisperCredentials<'_>,
) -> Result<(reqwest::Url, InferenceKey)> {
    let credentials = SuperwhisperCredentials::new(
        credentials.x_id,
        credentials.x_license,
        credentials.x_signature,
    )
    .map_err(|error| anyhow!(error))?;
    let client = super::http_client()?;
    let request = |path: &str| {
        client
            .get(format!("{api}/{path}"))
            .header("Accept", "application/json")
            .header("X-Platform", "macos")
            .header("User-Agent", super::superwhisper::USER_AGENT)
    };
    let regions: Regions = response_json(request("v2/inference/regions"))
        .await
        .map_err(|error| anyhow!("Could not discover the S1 region: {error}"))?;
    let region = regions
        .regions
        .into_iter()
        .find(|region| region.id == regions.default)
        .ok_or_else(|| anyhow!("Superwhisper returned no default inference region."))?;
    let host = region_url(&region.host)?;
    let key: InferenceKey = response_json(
        client
            .post(format!("{api}/v2/inference/key"))
            .header("Accept", "application/json")
            .header("X-Platform", "macos")
            .header("User-Agent", super::superwhisper::USER_AGENT)
            .json(&serde_json::json!({}))
            .query(&[("region", region.id)])
            .header("X-ID", credentials.x_id)
            .header("X-License", credentials.x_license)
            .header("X-Signature", credentials.x_signature),
    )
    .await
    .map_err(|error| anyhow!("Could not obtain the S1 inference key: {error}"))?;
    if key.key.trim().is_empty() {
        return Err(anyhow!("Superwhisper returned an empty inference key."));
    }
    Ok((host, key))
}

pub async fn transcribe(
    credentials: SuperwhisperCredentials<'_>,
    samples: &[f32],
    language: Option<&str>,
    keyterms: &[String],
) -> Result<super::ProviderTranscript> {
    let audio = super::opus_audio_part(samples).await?;
    // Request a fresh token for each recording; never persist temporary tokens.
    let (host, key) = bootstrap("https://api.superwhisper.com", credentials).await?;
    run_at(host, key, audio, language, keyterms).await
}

async fn run_at(
    host: reqwest::Url,
    key: InferenceKey,
    audio: Part,
    language: Option<&str>,
    keyterms: &[String],
) -> Result<super::ProviderTranscript> {
    let prompt = keyterms
        .iter()
        .map(|term| term.trim())
        .filter(|term| !term.is_empty() && term.chars().count() <= 100)
        .take(100)
        .collect::<Vec<_>>()
        .join(", ");
    let form = Form::new()
        .part("audio", audio)
        .text("language", language.unwrap_or("auto").to_string())
        .text("asr_prompt", prompt)
        .text("enable_word_timestamps", "true")
        .text("enable_audio_vocab", "false");
    let endpoint = host.join("generate")?;
    let response: RunResponse = response_json(
        super::http_client()?
            .post(endpoint)
            .bearer_auth(key.key)
            .multipart(form),
    )
    .await
    .map_err(|error| anyhow!("S1 transcription failed: {error}"))?;
    Ok(super::ProviderTranscript::plain(response.text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn exchanges_credentials_and_decodes_s1_transcript() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v2/inference/regions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "regions": [{"id": "use1", "host": "https://us.aws.superwhisper.com"}],
                "default": "use1"
            })))
            .expect(1)
            .mount(&server)
            .await;
        let id = "00000000-0000-4000-8000-000000000001";
        let signature = "a".repeat(64);
        Mock::given(method("POST"))
            .and(path("/v2/inference/key"))
            .and(query_param("region", "use1"))
            .and(header("X-ID", id))
            .and(header("X-License", id))
            .and(header("X-Signature", signature.as_str()))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"key": "test-token"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let (host, key) = bootstrap(
            &server.uri(),
            SuperwhisperCredentials::new(id, id, &signature).unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(host.as_str(), "https://us.aws.superwhisper.com/");
        Mock::given(method("POST"))
            .and(path("/generate"))
            .and(header("Authorization", "Bearer test-token"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"text": "Hello S1."})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let audio = super::super::opus_audio_part(&[0.0; 160]).await.unwrap();
        let transcript = run_at(
            server.uri().parse().unwrap(),
            key,
            audio,
            Some("en"),
            &[" Handy ".into()],
        )
        .await
        .unwrap();
        assert_eq!(transcript.text, "Hello S1.");
        let requests = server.received_requests().await.unwrap();
        let run = requests.last().unwrap();
        assert!(!run.headers.contains_key("X-License"));
        let body = String::from_utf8_lossy(&run.body);
        assert!(body.contains("OpusHead"));
        assert!(body.contains("audio/ogg"));
        assert!(body.contains("name=\"audio\"; filename=\"recording.ogg\""));
        assert!(body.contains("name=\"asr_prompt\"\r\n\r\nHandy"));
        assert!(body.contains("name=\"language\"\r\n\r\nen"));
        assert!(body.contains("name=\"enable_audio_vocab\"\r\n\r\nfalse"));
        assert!(!body.contains("tag_audio_events"));
    }

    #[tokio::test]
    async fn omits_private_error_response_bodies() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(401).set_body_string("private-token-and-transcript"),
            )
            .mount(&server)
            .await;
        let error = run_at(
            server.uri().parse().unwrap(),
            InferenceKey {
                key: "secret-token".into(),
            },
            Part::bytes(Vec::new()),
            None,
            &[],
        )
        .await
        .err()
        .unwrap()
        .to_string();
        assert!(error.contains("401"));
        assert!(!error.contains("private-token"));
        assert!(!error.contains("secret-token"));
    }

    #[test]
    fn rejects_untrusted_inference_hosts() {
        for host in [
            "http://us.aws.superwhisper.com",
            "https://superwhisper.com.attacker.test",
            "https://user@us.aws.superwhisper.com",
            "https://us.aws.superwhisper.com/other",
        ] {
            assert!(region_url(host).is_err());
        }
    }
}
